//! Putting model files on a machine whose binary carries none.
//!
//! Two different situations look the same in `svipall doctor` and are not. A build compiled without
//! any `onnx-*` feature cannot read a model file at all, and no download fixes that — it would
//! report `models_not_readable` instead, with 58 MB on disk answering nothing. A build that *can*
//! read one and has none is the case this module exists for, and `cargo install svipall` is exactly
//! that: the default features compile the ONNX path in, and the published crate carries no weights
//! because crates.io is not where 58 MB of them belong.
//!
//! Nothing here runs by itself. `docs/models.md` promises that nothing is downloaded at run time,
//! and that promise is kept: this is a command a person types, like `svipall browser install`.
//! Files land in `~/.svipall/models/`, where [`crate::model_source`] already prefers them over an
//! embedded copy, so an installation can also be *updated* this way.

use anyhow::{bail, Context, Result};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use svipall_http::{HttpFetcher, HttpRequest};

/// Where the release keeps the archive. The same host `install.sh` downloads from.
const RELEASES: &str = "https://github.com/ilien-dev/svipall/releases/download";

/// `SVIPALL_RELEASES_URL` replaces it: a mirror inside a network that cannot reach GitHub, and how
/// the tests serve an archive over a real socket instead of describing what one would look like.
fn releases_base() -> String {
    std::env::var("SVIPALL_RELEASES_URL").unwrap_or_else(|_| RELEASES.to_string())
}

/// The models a published archive is allowed to contain, with the sidecar each one needs. Anything
/// else in the archive is not a model this build knows how to read, and an entry that is not in
/// this list is not written: an archive is somebody else's file, and `../../.bashrc` is a name.
const ALLOWED: &[&str] = &["detect", "segment", "grid", "ocr", "audio"];

/// Where the archive comes from.
pub enum Source {
    /// The release matching this build's own version.
    Release,
    /// A file already on disk: an air-gapped install, and what the tests use.
    File(PathBuf),
}

/// What an install did, in the words `svipall models` prints.
#[derive(Debug)]
pub struct Report {
    /// Model names now on disk, in the order the archive listed them.
    pub installed: Vec<String>,
    /// Where they landed.
    pub dir: PathBuf,
    /// Whether the archive was checked against the release's own `sha256sums.txt`. A local file has
    /// nothing to check against, and a missing sums file is a warning rather than a refusal — the
    /// same rule `install.sh` follows, and the same reason: a successful exit is not by itself
    /// proof that anything was verified.
    pub verified: bool,
}

/// The asset a release publishes. One archive for every platform, because a model file is the same
/// bytes everywhere — it is the ONNX Runtime that has a target, not the weights.
pub fn asset_name(version: &str) -> String {
    format!("svipall-models-{version}.zip")
}

/// The digest `sha256sums.txt` lists for `name`, or `None`.
///
/// `sha256sum` writes one space and a mode character, and GNU coreutils writes two spaces for text
/// mode, so the separator is whitespace rather than a fixed string. The name is matched at the end
/// of the line: `svipall-models-1.0.0.zip` must not be found by looking for `models-1.0.0.zip`.
pub fn sha_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let digest = parts.next()?;
        let listed = parts.next()?;
        (listed.trim_start_matches('*') == name).then(|| digest.to_string())
    })
}

/// Write the models an archive carries into `dir`, and nothing else.
///
/// Returns the file names written. Both halves of a model have to be there: the build script
/// refuses to embed a model without its sidecar, and installing half of one would move the failure
/// from here to the first captcha.
pub fn unpack(bytes: &[u8], dir: &Path) -> Result<Vec<String>> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .context("the models archive is not a zip file")?;
    // Read everything first, write nothing yet: an archive that turns out to be missing a sidecar
    // must leave the models directory exactly as it was.
    let mut pending: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        // `enclosed_name` is None for an entry that would escape the destination, which is the
        // whole answer to `../`. A name with a directory in it is refused separately: a model lives
        // in the root of the archive, and accepting `nested/detect.onnx` would make two different
        // archives install the same model from different places.
        let Some(path) = entry.enclosed_name() else {
            continue;
        };
        if path.components().count() != 1 {
            continue;
        }
        let name = path.to_string_lossy().to_string();
        let Some((stem, ext)) = name.rsplit_once('.') else {
            continue;
        };
        if !ALLOWED.contains(&stem) || !matches!(ext, "onnx" | "json") {
            continue;
        }
        let mut body = Vec::new();
        entry.read_to_end(&mut body)?;
        pending.push((name, body));
    }

    for (name, _) in &pending {
        if let Some(stem) = name.strip_suffix(".onnx") {
            let sidecar = format!("{stem}.json");
            if !pending.iter().any(|(n, _)| *n == sidecar) {
                bail!("the archive has {name} but no {sidecar}; half a model is no model");
            }
        }
    }
    if pending.is_empty() {
        bail!("the archive contains no model files");
    }

    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let mut written = Vec::new();
    for (name, body) in pending {
        let path = dir.join(&name);
        // Replace rather than write in place, for the same reason the installers do: a half-copied
        // model that a running server picks up is worse than no model.
        let tmp = dir.join(format!(".{name}.new"));
        std::fs::write(&tmp, &body).with_context(|| format!("writing {}", path.display()))?;
        std::fs::rename(&tmp, &path).with_context(|| format!("replacing {}", path.display()))?;
        written.push(name);
    }
    Ok(written)
}

/// Install the models from `source` into `~/.svipall/models/`.
///
/// `progress` is called with a line per step, because a 58 MB download that says nothing looks like
/// a hang.
pub async fn install(source: Source, progress: &mut (dyn FnMut(String) + Send)) -> Result<Report> {
    let dir = crate::model_source::models_dir();
    let (bytes, verified) = match source {
        Source::File(path) => {
            progress(format!("reading {}", path.display()));
            let bytes =
                std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
            (bytes, false)
        }
        Source::Release => download(progress).await?,
    };
    let mut installed: Vec<String> = unpack(&bytes, &dir)?
        .iter()
        .filter_map(|f| f.strip_suffix(".onnx").map(String::from))
        .collect();
    installed.sort();
    Ok(Report {
        installed,
        dir,
        verified,
    })
}

/// The archive for this build's own version, and whether it was verified.
async fn download(progress: &mut (dyn FnMut(String) + Send)) -> Result<(Vec<u8>, bool)> {
    let version = env!("CARGO_PKG_VERSION");
    let name = asset_name(version);
    let base = format!("{}/v{version}", releases_base());
    let cfg = svipall_core::config::load_in(&svipall_core::config::home_dir()).unwrap_or_default();
    let identity = svipall_core::IdentityProfile::resolve(None, &cfg);
    let http: Arc<dyn HttpFetcher> =
        svipall_http::build(svipall_http::FetcherConfig::new(identity))?;

    progress(format!("downloading {name}"));
    let archive = get(&http, &format!("{base}/{name}"))
        .await
        .with_context(|| {
            format!("{name} is not in the v{version} release; pass --from-file if you have it")
        })?;

    // Verified, or the reason it could not be — never a silent skip.
    let verified = match get(&http, &format!("{base}/sha256sums.txt")).await {
        Ok(sums) => {
            let sums = String::from_utf8_lossy(&sums);
            match sha_for(&sums, &name) {
                Some(want) => {
                    let got = sha256(&archive);
                    if want.eq_ignore_ascii_case(&got) {
                        progress("checksum ok".into());
                        true
                    } else {
                        bail!("checksum mismatch for {name} (expected {want}, got {got})");
                    }
                }
                None => {
                    progress(format!("warning: {name} is not listed in sha256sums.txt"));
                    false
                }
            }
        }
        Err(e) => {
            progress(format!("warning: sha256sums.txt could not be read ({e})"));
            false
        }
    };
    Ok((archive, verified))
}

async fn get(http: &Arc<dyn HttpFetcher>, url: &str) -> Result<Vec<u8>> {
    let mut req = HttpRequest::get(url.to_string());
    req.headers = http.identity().nav_headers();
    let resp = http.send(req).await?;
    if resp.status >= 400 {
        bail!("HTTP {} for {url}", resp.status);
    }
    Ok(resp.body)
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// `svipall models <status|install|remove>`, as the CLI dispatches it.
///
/// `status` is deliberately the default: the first question anybody has here is what this
/// installation already has, and answering it costs nothing and downloads nothing.
pub async fn run(action: &str, from_file: Option<String>) -> Result<serde_json::Value> {
    match action {
        "status" => Ok(status()),
        "install" => {
            let source = match from_file {
                Some(path) => Source::File(PathBuf::from(path)),
                None => Source::Release,
            };
            let report = install(source, &mut |line| eprintln!("svipall: {line}")).await?;
            Ok(serde_json::json!({
                "installed": report.installed,
                "dir": report.dir,
                "verified": report.verified,
                "readable": cfg!(feature = "onnx"),
            }))
        }
        "remove" => {
            let dir = crate::model_source::models_dir();
            let mut removed = Vec::new();
            for name in ALLOWED {
                for ext in ["onnx", "json"] {
                    let path = dir.join(format!("{name}.{ext}"));
                    if path.is_file() && std::fs::remove_file(&path).is_ok() {
                        removed.push(format!("{name}.{ext}"));
                    }
                }
            }
            Ok(serde_json::json!({ "removed": removed, "dir": dir }))
        }
        other => bail!("unknown models action '{other}'; use status, install or remove"),
    }
}

/// What this installation has, and whether anything in it can read a model at all.
///
/// The last field is the difference this module exists to make visible: weights with no `onnx-*`
/// feature to read them answer no captcha, and installing more of them will not change that.
pub fn status() -> serde_json::Value {
    let dir = crate::model_source::models_dir();
    let installed: Vec<String> = ALLOWED
        .iter()
        .filter(|name| {
            dir.join(format!("{name}.onnx")).is_file() && dir.join(format!("{name}.json")).is_file()
        })
        .map(|n| n.to_string())
        .collect();
    serde_json::json!({
        "embedded": svipall_models::compiled_in(),
        "installed": installed,
        "dir": dir,
        "readable": cfg!(feature = "onnx"),
        "asset": asset_name(env!("CARGO_PKG_VERSION")),
    })
}
