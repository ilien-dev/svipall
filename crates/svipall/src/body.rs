//! A response body kept as bytes: in memory when it is going to be read, straight to disk when the
//! caller asked for the file.
//!
//! Every non-page body used to go through `String::from_utf8_lossy` and come back as mojibake: a
//! `.tar.bz2`, a `.crx`, a `.nupkg`, a font, an image asked for with `out_file` — all of them
//! arrived broken, and multi-gigabyte model downloads went to curl because nothing here could hold
//! one. One sink now decides, from the headers, where the body goes:
//!
//! - **to a file**, when `out_file` names one that is not a text name (`.md`, `.txt`, …) and the
//!   body is not text: written as it arrives to `<out_file>.part`, hashed on the way, renamed into
//!   place when complete. A transfer cut short leaves the `.part` and a `.part.meta` naming the
//!   validator, and the next call with the same `out_file` asks for the rest with `Range` and
//!   `If-Range` — a server whose copy changed answers with the whole file, which starts it again.
//! - **to memory** otherwise, up to [`max_body_bytes`]. A body over that is stopped, not read,
//!   and said so; it is never handed to a browser tier, which would load it again.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// What is kept in memory for a body that is going to be read. A page is a few hundred kilobytes
/// and a long PDF tens of megabytes; past this, the body is a download and belongs on disk.
pub const DEFAULT_MAX_BODY: u64 = 100 * 1024 * 1024;

/// The in-memory cap: [`DEFAULT_MAX_BODY`], or `SVIPALL_MAX_BODY_BYTES`.
pub fn max_body_bytes() -> u64 {
    std::env::var("SVIPALL_MAX_BODY_BYTES")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_MAX_BODY)
}

/// An `out_file` whose extension asks for the text rather than the file.
pub fn wants_text(out_file: &str) -> bool {
    let ext = Path::new(out_file)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "md" | "markdown" | "txt" | "text" | "json" | "jsonl" | "csv" | "html" | "htm"
    )
}

/// A content type that says the body is text: every `text/*`, and the structured formats that
/// are text without saying `text` (JSON, XML, JavaScript, YAML and their `+json`/`+xml` kin).
pub fn texty(content_type: &str) -> bool {
    let ct = content_type.to_ascii_lowercase();
    let main = ct.split(';').next().unwrap_or("").trim();
    if main.starts_with("text/") {
        return true;
    }
    // By subtype, never by substring: an office document is `…openxmlformats…` and is not text.
    let sub = main.split_once('/').map(|(_, s)| s).unwrap_or("");
    let sub = sub.strip_prefix("x-").unwrap_or(sub);
    matches!(
        sub,
        "json"
            | "xml"
            | "javascript"
            | "ecmascript"
            | "yaml"
            | "www-form-urlencoded"
            | "csv"
            | "graphql"
            | "ndjson"
            | "jsonl"
            | "json-seq"
            | "sh"
            | "toml"
    ) || [".json", "+json", "+xml", "+yaml"]
        .iter()
        .any(|k| sub.ends_with(k))
}

/// Families that are never text whatever their first bytes look like.
fn binary_type(content_type: &str) -> bool {
    let ct = content_type.to_ascii_lowercase();
    let main = ct.split(';').next().unwrap_or("").trim();
    main.starts_with("font/")
        || main.starts_with("image/")
        || main.starts_with("audio/")
        || main.starts_with("video/")
        || [
            "zip",
            "gzip",
            "bzip",
            "x-xz",
            "x-7z",
            "x-tar",
            "x-rar",
            "wasm",
            "java-archive",
            "x-msdownload",
            "android.package",
            "x-chrome-extension",
            "x-apple-diskimage",
            "x-iso9660",
            "zstd",
        ]
        .iter()
        .any(|k| main.contains(k))
}

/// Is this body bytes rather than text? Text by its type is text; otherwise a binary family, a NUL
/// or invalid UTF-8 in the first 8 KB says it is not.
pub fn is_binary(content_type: &str, body: &[u8]) -> bool {
    if texty(content_type) {
        return false;
    }
    if binary_type(content_type) {
        return true;
    }
    let head = &body[..body.len().min(8192)];
    if head.contains(&0) {
        return true;
    }
    match std::str::from_utf8(head) {
        Ok(_) => false,
        // A character cut by the 8 KB boundary is not evidence.
        Err(e) => e.error_len().is_some(),
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(digest: &[u8]) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// What a binary body is, for a response that will not carry its bytes.
pub fn describe(content_type: &str, body: &[u8]) -> Value {
    json!({
        "content_type": content_type,
        "bytes": body.len(),
        "sha256": sha256_hex(body),
    })
}

/// Where the unfinished copy of `target` lives, and the note beside it.
pub fn part_of(target: &Path) -> (PathBuf, PathBuf) {
    let mut part = target.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    let mut meta = part.as_os_str().to_owned();
    meta.push(".meta");
    (part, PathBuf::from(meta))
}

/// Where a cut transfer of `url` into `target` stopped, and the validator to resume it with. `None`
/// when there is nothing to resume, or nothing to prove the server's copy is still the same one.
pub fn resume_point(target: &Path, url: &str) -> Option<(u64, String)> {
    let (part, meta) = part_of(target);
    let len = std::fs::metadata(&part).ok()?.len();
    if len == 0 {
        return None;
    }
    let note: Value = serde_json::from_slice(&std::fs::read(meta).ok()?).ok()?;
    if note["url"].as_str() != Some(url) {
        return None;
    }
    let validator = note["etag"]
        .as_str()
        .filter(|e| !e.starts_with("W/"))
        .or_else(|| note["last_modified"].as_str())?;
    Some((len, validator.to_string()))
}

/// A body on its way to disk.
pub struct Spill {
    target: PathBuf,
    part: PathBuf,
    meta: PathBuf,
    file: std::fs::File,
    hasher: Sha256,
    pub written: u64,
    pub resumed_from: u64,
    pub expected: Option<u64>,
    pub resumable: bool,
}

/// Where a response body goes. See the module doc.
pub struct Sink {
    url: String,
    cap: u64,
    /// The file the caller asked for, when it asked for the file rather than the text.
    raw: Option<PathBuf>,
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub memory: Vec<u8>,
    pub spill: Option<Spill>,
    /// Set when the in-memory cap stopped the transfer: the size it would have been, or reached.
    pub over_cap: Option<u64>,
    started: Instant,
    pub first_byte_ms: Option<u64>,
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// `bytes a-b/total` of a 206.
fn content_range(v: &str) -> Option<(u64, Option<u64>)> {
    let spec = v.trim().strip_prefix("bytes")?.trim();
    let (range, total) = spec.split_once('/')?;
    let start = range.split('-').next()?.trim().parse().ok()?;
    Some((start, total.trim().parse().ok()))
}

impl Sink {
    pub fn new(url: &str, raw: Option<PathBuf>, cap: u64) -> Self {
        Self {
            url: url.to_string(),
            cap,
            raw,
            status: 0,
            headers: Vec::new(),
            memory: Vec::new(),
            spill: None,
            over_cap: None,
            started: Instant::now(),
            first_byte_ms: None,
        }
    }

    pub fn content_type(&self) -> String {
        header(&self.headers, "content-type")
            .unwrap_or_default()
            .to_ascii_lowercase()
    }

    fn open_spill(&self, target: &Path) -> anyhow::Result<Spill> {
        let (part, meta) = part_of(target);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let length = header(&self.headers, "content-length").and_then(|v| v.trim().parse().ok());
        let mut hasher = Sha256::new();
        let (file, resumed_from, expected) = match (
            self.status,
            header(&self.headers, "content-range").and_then(content_range),
        ) {
            (206, Some((start, total))) => {
                let have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
                anyhow::ensure!(
                    start == have,
                    "the server resumed at byte {start}, but {} holds {have}",
                    part.display()
                );
                // The bytes already on disk are part of the file, and of its checksum.
                let mut old = std::fs::File::open(&part)?;
                let mut buf = vec![0u8; 1 << 16];
                loop {
                    let n = old.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    hasher.update(&buf[..n]);
                }
                let file = std::fs::OpenOptions::new().append(true).open(&part)?;
                (file, start, total.or(length.map(|l: u64| l + start)))
            }
            _ => (std::fs::File::create(&part)?, 0, length),
        };
        let etag = header(&self.headers, "etag");
        let last_modified = header(&self.headers, "last-modified");
        let resumable = etag.is_some_and(|e| !e.starts_with("W/")) || last_modified.is_some();
        std::fs::write(
            &meta,
            json!({"url": self.url, "etag": etag, "last_modified": last_modified,
                   "expected_bytes": expected})
            .to_string(),
        )?;
        Ok(Spill {
            target: target.to_path_buf(),
            part,
            meta,
            file,
            hasher,
            written: resumed_from,
            resumed_from,
            expected,
            resumable,
        })
    }

    /// The finished download moved into place, as the response describes it — or, when it did not
    /// all arrive, what [`Sink::partial`] says about it.
    pub fn finish(&mut self) -> Option<Value> {
        let mut spill = self.spill.take()?;
        let short = spill
            .expected
            .filter(|want| spill.written != *want)
            .map(|want| anyhow::anyhow!("the transfer ended at {} of {want} bytes", spill.written));
        let flushed = spill.file.flush().map_err(anyhow::Error::from);
        if let Some(e) = short.or(flushed.err()) {
            self.spill = Some(spill);
            return self.partial(&e);
        }
        let Spill {
            target,
            part,
            meta,
            file,
            hasher,
            written,
            resumed_from,
            ..
        } = spill;
        drop(file);
        if let Err(e) = std::fs::rename(&part, &target) {
            return Some(json!({
                "partial": true, "part_file": part.to_string_lossy(), "bytes_written": written,
                "error": format!("the download is complete in the .part file but could not be \
                                  moved to {}: {e}", target.display()),
            }));
        }
        let _ = std::fs::remove_file(&meta);
        let elapsed = self.started.elapsed().as_millis().max(1) as u64;
        let mut info = json!({
            "out_file": target.to_string_lossy(),
            "bytes": written,
            "sha256": hex(&hasher.finalize()),
            "content_type": self.content_type(),
            "complete": true,
            "elapsed_ms": elapsed,
            "bytes_per_sec": (written - resumed_from) * 1000 / elapsed,
        });
        if resumed_from > 0 {
            info["resumed_from"] = json!(resumed_from);
        }
        Some(info)
    }

    /// What a transfer that stopped part way left behind, when it was going to disk.
    pub fn partial(&mut self, cause: &anyhow::Error) -> Option<Value> {
        let spill = self.spill.take()?;
        let _ = spill.file.sync_all();
        let mut info = json!({
            "partial": true,
            "part_file": spill.part.to_string_lossy(),
            "bytes_written": spill.written,
            "expected_bytes": spill.expected,
            "content_type": self.content_type(),
            "resumable": spill.resumable,
            "error": format!("{cause:#}"),
        });
        info["next"] = json!(if spill.resumable {
            format!(
                "call again with the same url and out_file: it resumes from byte {}",
                spill.written
            )
        } else {
            "the server sent no validator (ETag or Last-Modified), so a resume could not prove the \
             file unchanged; calling again starts it over"
                .to_string()
        });
        Some(info)
    }

    pub fn take_memory(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.memory)
    }
}

impl svipall_http::BodySink for Sink {
    fn begin(
        &mut self,
        status: u16,
        _final_url: &str,
        headers: &[(String, String)],
    ) -> anyhow::Result<()> {
        self.status = status;
        self.headers = headers.to_vec();
        self.first_byte_ms = Some(self.started.elapsed().as_millis() as u64);
        let ct = self.content_type();
        if let (Some(target), true) = (self.raw.clone(), (200..300).contains(&status)) {
            if !texty(&ct) {
                self.spill = Some(self.open_spill(&target)?);
                return Ok(());
            }
        }
        if let Some(len) = header(headers, "content-length").and_then(|v| v.trim().parse::<u64>().ok())
        {
            if len > self.cap {
                self.over_cap = Some(len);
                anyhow::bail!("the body is {len} bytes, over the {} kept in memory", self.cap);
            }
        }
        Ok(())
    }

    fn write(&mut self, chunk: &[u8]) -> anyhow::Result<()> {
        if let Some(s) = self.spill.as_mut() {
            s.file.write_all(chunk)?;
            s.hasher.update(chunk);
            s.written += chunk.len() as u64;
            return Ok(());
        }
        if self.memory.len() as u64 + chunk.len() as u64 > self.cap {
            self.over_cap = Some(self.memory.len() as u64 + chunk.len() as u64);
            anyhow::bail!("the body passed the {} bytes kept in memory", self.cap);
        }
        self.memory.extend_from_slice(chunk);
        Ok(())
    }
}

/// The note that stands in for a body that was not returned as text.
pub fn binary_note(info: &Value, saved: bool) -> String {
    let ct = info["content_type"].as_str().unwrap_or("");
    let ct = if ct.is_empty() { "no content-type" } else { ct };
    if saved {
        format!(
            "Saved {} bytes ({ct}) to {}, unchanged; sha256 {}.",
            info["bytes"],
            info["out_file"].as_str().unwrap_or(""),
            info["sha256"].as_str().unwrap_or("")
        )
    } else {
        format!(
            "This is a binary file ({ct}, {} bytes, sha256 {}), not text, so its bytes are not \
             returned here. Pass out_file to save it to disk exactly as served.",
            info["bytes"],
            info["sha256"].as_str().unwrap_or("")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_and_its_kin_are_text_whatever_they_are_called() {
        for ct in [
            "application/json",
            "application/vnd.github+json; charset=utf-8",
            "application/xml",
            "application/rss+xml",
            "application/javascript",
            "text/plain",
            "application/x-ndjson",
            "image/svg+xml",
        ] {
            assert!(texty(ct), "{ct}");
            assert!(!is_binary(ct, b"\xff\xfe\x00"), "{ct}");
        }
        assert!(!texty(
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        ));
        assert!(!texty("application/octet-stream") && !texty("image/png"));
    }

    #[test]
    fn bytes_are_told_from_text_by_type_and_by_content() {
        assert!(is_binary("application/x-bzip2", b"BZh91AY"));
        assert!(is_binary("font/woff2", b"wOF2"));
        assert!(is_binary("application/octet-stream", b"PK\x03\x04\x00\x00"));
        assert!(is_binary("", b"\x89PNG\r\n\x1a\n"));
        assert!(!is_binary("application/octet-stream", b"just some words"));
        // A multi-byte character cut by the sniff window is not binary.
        let mut s = "a".repeat(8191).into_bytes();
        s.extend("é".as_bytes());
        assert!(!is_binary("", &s));
    }

    #[test]
    fn a_text_name_asks_for_the_text_and_any_other_for_the_file() {
        assert!(wants_text("notes.md") && wants_text("x.JSON") && wants_text("a/b.txt"));
        assert!(!wants_text("model.bin") && !wants_text("pkg.tar.bz2") && !wants_text("x.pdf"));
        assert!(!wants_text("noextension"));
    }

    #[test]
    fn a_content_range_names_where_the_bytes_start_and_how_many_there_are() {
        assert_eq!(content_range("bytes 100-199/200"), Some((100, Some(200))));
        assert_eq!(content_range("bytes 5-9/*"), Some((5, None)));
        assert_eq!(content_range("nonsense"), None);
    }
}
