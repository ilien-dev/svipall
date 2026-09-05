//! Fetching a browser when the machine has none.
//!
//! Nothing here runs on its own. The ladder reports an actionable error and the operator (or the
//! model) calls `browser_setup` — downloading 190 MB is not something to do behind someone's back.
//!
//! Chrome for Testing is used rather than a Chromium snapshot: it is a real Chrome build with a
//! real version number, and the whole identity design depends on the User-Agent not lying about
//! which browser is underneath. A Chromium snapshot would announce `Chromium/…` and carry a
//! different `userAgentData.brands`.

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use svipall_http::{HttpFetcher, HttpRequest};

const VERSIONS_URL: &str =
    "https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json";

/// Verified against the live endpoint: `chrome-win64.zip` is 202,789,024 bytes, and the response
/// carries `Accept-Ranges: bytes` plus `x-goog-hash: md5=…`, which is what makes resume and
/// integrity checking possible rather than aspirational.
const CHROME_ARTIFACT: &str = "chrome";

#[derive(Debug, Deserialize)]
struct VersionsFile {
    channels: Channels,
}

#[derive(Debug, Deserialize)]
struct Channels {
    #[serde(rename = "Stable")]
    stable: Channel,
}

#[derive(Debug, Deserialize)]
struct Channel {
    version: String,
    downloads: Downloads,
}

#[derive(Debug, Deserialize)]
struct Downloads {
    #[serde(default)]
    chrome: Vec<Download>,
    #[serde(default, rename = "chrome-headless-shell")]
    headless_shell: Vec<Download>,
}

#[derive(Debug, Deserialize, Clone)]
struct Download {
    platform: String,
    url: String,
}

#[derive(Debug, Clone)]
pub struct Release {
    pub version: String,
    pub platform: String,
    pub url: String,
    pub artifact: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Installed {
    pub version: String,
    pub exe: String,
    pub bytes_on_disk: u64,
}

/// Chrome for Testing's platform token for the host, or None on a platform it does not build for.
pub fn platform() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("win64"),
        ("windows", "x86") => Some("win32"),
        ("macos", "aarch64") => Some("mac-arm64"),
        ("macos", "x86_64") => Some("mac-x64"),
        ("linux", "x86_64") => Some("linux64"),
        _ => None,
    }
}

fn parse_release(body: &[u8], platform: &str, artifact: &str) -> Result<Release> {
    let file: VersionsFile = serde_json::from_slice(body).context("parsing versions JSON")?;
    let list = if artifact == CHROME_ARTIFACT {
        &file.channels.stable.downloads.chrome
    } else {
        &file.channels.stable.downloads.headless_shell
    };
    let hit = list
        .iter()
        .find(|d| d.platform == platform)
        .ok_or_else(|| anyhow!("Chrome for Testing has no {artifact} build for {platform}"))?;
    Ok(Release {
        version: file.channels.stable.version.clone(),
        platform: platform.to_string(),
        url: hit.url.clone(),
        artifact: artifact.to_string(),
    })
}

pub struct Provisioner {
    http: Arc<dyn HttpFetcher>,
    root: PathBuf,
    artifact: String,
}

impl Provisioner {
    pub fn new(http: Arc<dyn HttpFetcher>, artifact: Option<&str>) -> Self {
        Self {
            http,
            root: crate::browser::managed_browser_dir(),
            artifact: artifact.unwrap_or(CHROME_ARTIFACT).to_string(),
        }
    }

    pub fn installed(&self) -> Option<Installed> {
        let exe = crate::browser::managed_browser()?;
        let version = exe
            .ancestors()
            .find_map(|a| {
                let name = a.file_name()?.to_string_lossy().to_string();
                name.chars().next()?.is_ascii_digit().then_some(name)
            })
            .unwrap_or_default();
        Some(Installed {
            version,
            exe: exe.to_string_lossy().to_string(),
            bytes_on_disk: dir_size(&self.root),
        })
    }

    pub async fn latest_stable(&self) -> Result<Release> {
        let platform = platform().ok_or_else(|| {
            anyhow!(
                "Chrome for Testing publishes no build for {}/{}; install Chrome or Edge and set \
                 browser_path in ~/.svipall/config.toml",
                std::env::consts::OS,
                std::env::consts::ARCH
            )
        })?;
        let resp = self
            .http
            .send(HttpRequest::get(VERSIONS_URL))
            .await
            .context("asking Chrome for Testing which version is current")?;
        if resp.status >= 400 {
            bail!("version index returned HTTP {}", resp.status);
        }
        parse_release(&resp.body, platform, &self.artifact)
    }

    /// Download, verify and unpack. `progress` receives short human-readable lines that end up in
    /// the tool result, so the caller can see what a multi-minute download is doing.
    pub async fn install(
        &self,
        release: &Release,
        progress: &mut (dyn FnMut(String) + Send),
    ) -> Result<Installed> {
        let target = self.root.join("cft").join(&release.version);
        if let Some(existing) = self.installed() {
            if existing.version == release.version {
                progress(format!("already installed: {}", release.version));
                return Ok(existing);
            }
        }
        std::fs::create_dir_all(self.root.join("downloads"))?;
        let zip_path = self
            .root
            .join("downloads")
            .join(format!("{}-{}.zip", release.version, release.platform));

        progress(format!(
            "downloading Chrome for Testing {} ({})",
            release.version, release.platform
        ));
        let bytes = self.download(&release.url, &zip_path, progress).await?;

        progress("extracting".into());
        let tmp = target.with_extension("tmp");
        let _ = std::fs::remove_dir_all(&tmp);
        extract_zip(&bytes, &tmp).context("unpacking the archive")?;
        let _ = std::fs::remove_dir_all(&target);
        std::fs::rename(&tmp, &target).context("moving the unpacked build into place")?;
        let _ = std::fs::remove_file(&zip_path);

        let exe = crate::browser::managed_browser()
            .ok_or_else(|| anyhow!("unpacked the archive but found no chrome executable in it"))?;
        // Proof it actually runs, rather than trusting that the bytes arrived.
        let major = crate::browser::browser_version(&exe)
            .ok_or_else(|| anyhow!("the unpacked binary did not report a version"))?;
        progress(format!("installed Chrome {major} at {}", exe.display()));
        self.prune(1);
        Ok(Installed {
            version: release.version.clone(),
            exe: exe.to_string_lossy().to_string(),
            bytes_on_disk: dir_size(&self.root),
        })
    }

    /// Resumable download. A 190 MB transfer over a flaky link should not start from zero.
    async fn download(
        &self,
        url: &str,
        path: &Path,
        progress: &mut (dyn FnMut(String) + Send),
    ) -> Result<Vec<u8>> {
        let head = self
            .http
            .send(HttpRequest {
                url: url.to_string(),
                method: "HEAD".into(),
                headers: self.http.identity().nav_headers(),
                body: None,
            })
            .await
            .ok();
        let total: u64 = head
            .as_ref()
            .and_then(|r| r.header("content-length"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        // The reference digest comes from the HEAD response, independent of the body we are about
        // to read. It guards against truncation and corruption — HTTPS to Google is what guards
        // against tampering, and this is not a substitute for that.
        let expected_md5 = head
            .as_ref()
            .and_then(|r| r.header("x-goog-hash"))
            .and_then(parse_goog_md5);

        let have: u64 = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let mut body = if have > 0 && total > 0 && have < total {
            progress(format!(
                "resuming at {} of {} MB",
                have / 1_048_576,
                total / 1_048_576
            ));
            let mut req = HttpRequest::get(url.to_string());
            req.headers = self.http.identity().nav_headers();
            req.set_header("range", &format!("bytes={have}-"));
            let resp = self.http.send(req).await?;
            let mut existing = std::fs::read(path)?;
            if resp.status == 206 {
                existing.extend_from_slice(&resp.body);
                existing
            } else {
                // Server ignored the range and sent the whole file; take it and drop the partial.
                resp.body
            }
        } else {
            let mut req = HttpRequest::get(url.to_string());
            req.headers = self.http.identity().nav_headers();
            let resp = self.http.send(req).await?;
            if resp.status >= 400 {
                bail!("download returned HTTP {}", resp.status);
            }
            resp.body
        };

        if total > 0 && body.len() as u64 != total {
            let _ = std::fs::write(path, &body);
            bail!(
                "download is {} bytes but the server said {total}; run browser_setup again to resume",
                body.len()
            );
        }
        if let Some(expected) = expected_md5 {
            progress("verifying checksum".into());
            let got = md5_base64(&body);
            if got != expected {
                let _ = std::fs::remove_file(path);
                bail!(
                    "checksum mismatch (expected {expected}, got {got}); the download was corrupt"
                );
            }
        }
        let _ = std::fs::remove_file(path);
        body.shrink_to_fit();
        Ok(body)
    }

    /// Keep the newest `keep` versions, delete the rest. A stale 400 MB build is pure waste.
    pub fn prune(&self, keep: usize) -> Vec<String> {
        let cft = self.root.join("cft");
        let mut versions: Vec<(u16, PathBuf)> = std::fs::read_dir(&cft)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                crate::browser::major_of_public(&name).map(|m| (m, e.path()))
            })
            .collect();
        versions.sort_by_key(|(major, _)| std::cmp::Reverse(*major));
        let mut removed = Vec::new();
        for (_, dir) in versions.into_iter().skip(keep) {
            if std::fs::remove_dir_all(&dir).is_ok() {
                removed.push(dir.to_string_lossy().to_string());
            }
        }
        // Abandoned partial downloads older than a week are not going to be resumed.
        if let Ok(rd) = std::fs::read_dir(self.root.join("downloads")) {
            for e in rd.flatten() {
                let stale = e
                    .metadata()
                    .and_then(|m| m.modified())
                    .map(|t| {
                        t.elapsed()
                            .map(|d| d.as_secs() > 7 * 86_400)
                            .unwrap_or(false)
                    })
                    .unwrap_or(false);
                if stale {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        removed
    }

    pub fn remove_all(&self) -> Result<u64> {
        let bytes = dir_size(&self.root);
        std::fs::remove_dir_all(&self.root).context("removing the managed browser")?;
        Ok(bytes)
    }
}

/// `x-goog-hash: crc32c=…,md5=<base64>`
fn parse_goog_md5(header: &str) -> Option<String> {
    header
        .split(',')
        .map(str::trim)
        .find_map(|p| p.strip_prefix("md5=").map(str::to_string))
}

fn md5_base64(data: &[u8]) -> String {
    use base64::Engine as _;
    let digest = md5::compute(data);
    base64::engine::general_purpose::STANDARD.encode(digest.0)
}

fn dir_size(path: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(path) else {
        return 0;
    };
    rd.flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_file() => m.len(),
            Ok(m) if m.is_dir() => dir_size(&e.path()),
            _ => 0,
        })
        .sum()
}

/// Unpack, refusing anything that tries to escape the destination.
fn extract_zip(bytes: &[u8], dest: &Path) -> Result<()> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
    std::fs::create_dir_all(dest)?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        // `enclosed_name` returns None for absolute paths and anything containing `..`.
        let Some(rel) = entry.enclosed_name() else {
            bail!("archive entry {:?} escapes the destination", entry.name());
        };
        let out = dest.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut f = std::fs::File::create(&out)?;
        std::io::copy(&mut entry, &mut f)?;
        f.flush()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = entry.unix_mode() {
                let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "timestamp": "2026-09-01T00:00:00.000Z",
      "channels": {
        "Stable": {
          "channel": "Stable",
          "version": "152.0.7977.75",
          "revision": "1",
          "downloads": {
            "chrome": [
              {"platform": "linux64", "url": "https://example.test/linux64/chrome-linux64.zip"},
              {"platform": "win64", "url": "https://example.test/win64/chrome-win64.zip"}
            ],
            "chrome-headless-shell": [
              {"platform": "win64", "url": "https://example.test/win64/chrome-headless-shell-win64.zip"}
            ]
          }
        }
      }
    }"#;

    #[test]
    fn release_is_picked_for_the_requested_platform_and_artifact() {
        let r = parse_release(SAMPLE.as_bytes(), "win64", "chrome").unwrap();
        assert_eq!(r.version, "152.0.7977.75");
        assert!(r.url.ends_with("chrome-win64.zip"));

        let hs = parse_release(SAMPLE.as_bytes(), "win64", "chrome-headless-shell").unwrap();
        assert!(hs.url.contains("headless-shell"));
    }

    #[test]
    fn an_unavailable_platform_is_a_clear_error_not_a_panic() {
        let e = parse_release(SAMPLE.as_bytes(), "mac-arm64", "chrome").unwrap_err();
        assert!(
            e.to_string().contains("mac-arm64"),
            "the error should name the platform: {e}"
        );
    }

    #[test]
    fn md5_is_read_out_of_the_google_hash_header() {
        assert_eq!(
            parse_goog_md5("crc32c=ERQkpA==,md5=qXzriYHeSN9aiznL4gcEVg==").as_deref(),
            Some("qXzriYHeSN9aiznL4gcEVg==")
        );
        assert_eq!(parse_goog_md5("crc32c=ERQkpA==").as_deref(), None);
    }

    #[test]
    fn md5_matches_the_known_digest_of_an_empty_input() {
        // d41d8cd98f00b204e9800998ecf8427e, base64-encoded.
        assert_eq!(md5_base64(b""), "1B2M2Y8AsgTpgAmY7PhCfg==");
    }

    #[test]
    fn platform_token_is_known_for_this_host() {
        // Every platform svipall builds for must map to a Chrome for Testing token.
        assert!(
            platform().is_some(),
            "no Chrome for Testing token for {}/{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
    }

    #[test]
    fn a_zip_entry_cannot_escape_the_destination() {
        // Hand-built archive containing `../escaped.txt`.
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            w.start_file::<_, ()>("../escaped.txt", zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(b"nope").unwrap();
            w.finish().unwrap();
        }
        let dest = std::env::temp_dir().join(format!("svipall-zip-{}", std::process::id()));
        let err = extract_zip(&buf, &dest).unwrap_err();
        assert!(
            err.to_string().contains("escapes"),
            "path traversal must be refused: {err}"
        );
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn a_normal_entry_extracts() {
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            w.start_file::<_, ()>(
                "chrome-win64/chrome.exe",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            w.write_all(b"binary").unwrap();
            w.finish().unwrap();
        }
        let dest = std::env::temp_dir().join(format!("svipall-zip-ok-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dest);
        extract_zip(&buf, &dest).unwrap();
        assert_eq!(
            std::fs::read(dest.join("chrome-win64/chrome.exe")).unwrap(),
            b"binary"
        );
        let _ = std::fs::remove_dir_all(&dest);
    }
}
