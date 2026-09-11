//! Installing the model files a build does not carry.
//!
//! Some builds can read a model and have none to read: `cargo install svipall` compiles the ONNX
//! path in through the default features and the published crate carries no weights, because
//! crates.io is not where 58 MB of them belong. `svipall models install` is the answer to that, and
//! everything here is asserted without a network: the release path and the archive are the same
//! code, and `--from-file` is what an air-gapped install and this file both use.

use std::io::Write;
use std::path::PathBuf;

use svipall::model_install::{self, Source};

/// The environment is process-wide; every test here that sets `SVIPALL_HOME` takes this first.
/// `tokio`'s mutex rather than `std`'s because these tests await while holding it.
static HOME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A directory of this test's own, emptied first so a previous run cannot answer for this one.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("svipall-model-install-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the scratch directory");
    dir
}

/// A zip with the entries a models archive is supposed to carry, plus whatever extra names a test
/// wants to try smuggling past the unpacker.
fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let opts: zip::write::FileOptions<()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, body) in entries {
            zip.start_file(*name, opts).expect("start entry");
            zip.write_all(body).expect("write entry");
        }
        zip.finish().expect("finish the archive");
    }
    buf
}

fn a_model_pair() -> Vec<(&'static str, &'static [u8])> {
    vec![
        ("detect.onnx", b"onnx-bytes" as &[u8]),
        ("detect.json", br#"{"height":320}"# as &[u8]),
        ("segment.onnx", b"onnx-bytes" as &[u8]),
        ("segment.json", br#"{"height":320}"# as &[u8]),
    ]
}

/// The asset the release publishes, named the way `sha256sums.txt` lists it. A name built two ways
/// is a download that 404s on the day the two disagree.
#[test]
fn the_asset_is_named_after_the_version_it_belongs_to() {
    assert_eq!(
        model_install::asset_name("1.2.3"),
        "svipall-models-1.2.3.zip"
    );
}

/// The checksum comes from the file the release already publishes, in the format `sha256sum` writes
/// — one or two spaces between the digest and the name, which is why this is not a `split(' ')`.
#[test]
fn the_checksum_is_read_from_the_published_sums_file() {
    let sums =
        "aaaa  svipall-1.0.0-x86_64-unknown-linux-gnu.tar.gz\nbbbb  svipall-models-1.0.0.zip\n";
    assert_eq!(
        model_install::sha_for(sums, "svipall-models-1.0.0.zip").as_deref(),
        Some("bbbb")
    );
    assert_eq!(
        model_install::sha_for(sums, "svipall-models-9.9.9.zip"),
        None
    );
}

/// Only the names a model can have, and only in the root of the archive. An archive is somebody
/// else's file: an entry called `../../.bashrc` must not be a write outside the models directory,
/// and an entry called `notes.txt` has no business being installed as a model either.
#[test]
fn an_archive_can_only_write_model_files_into_the_models_directory() {
    let dir = scratch("whitelist").join("models");
    std::fs::create_dir_all(&dir).expect("create the models directory");
    let mut entries = a_model_pair();
    entries.push(("../escaped.onnx", b"no" as &[u8]));
    entries.push(("nested/detect.onnx", b"no" as &[u8]));
    entries.push(("notes.txt", b"no" as &[u8]));
    let written = model_install::unpack(&archive(&entries), &dir).expect("unpack");

    let mut written = written;
    written.sort();
    assert_eq!(
        written,
        vec!["detect.json", "detect.onnx", "segment.json", "segment.onnx"]
    );
    assert!(!dir.join("notes.txt").exists());
    assert!(!dir.join("nested").exists());
    assert!(
        !dir.parent()
            .expect("a parent")
            .join("escaped.onnx")
            .exists(),
        "an entry with .. in it escaped the models directory"
    );
}

/// Half a model is no model: the build script refuses to embed one without its sidecar, and an
/// archive missing one is refused here for the same reason rather than installed to fail later.
#[test]
fn an_archive_missing_a_sidecar_is_refused() {
    let dir = scratch("sidecar");
    let err = model_install::unpack(&archive(&[("detect.onnx", b"onnx-bytes")]), &dir)
        .expect_err("an archive with no sidecar must be refused");
    assert!(
        err.to_string().contains("detect.json"),
        "the error must name what was missing: {err}"
    );
    assert!(!dir.join("detect.onnx").exists(), "nothing is left behind");
}

/// The end of the road: a file on disk becomes models this installation can find. `SVIPALL_HOME`
/// points the whole thing at a temporary directory, which is what makes this test something other
/// than a description of the machine it runs on.
#[tokio::test]
async fn a_local_archive_becomes_models_this_installation_reports() {
    let _guard = HOME.lock().await;
    let home = scratch("install");
    std::env::set_var("SVIPALL_HOME", &home);
    let file = home.join("models.zip");
    std::fs::write(&file, archive(&a_model_pair())).expect("write the archive");

    let report = model_install::install(Source::File(file), &mut |_| {})
        .await
        .expect("install from a file");
    assert_eq!(report.installed, vec!["detect", "segment"]);
    assert_eq!(report.dir, svipall::model_source::models_dir());
    for name in ["detect.onnx", "detect.json", "segment.onnx", "segment.json"] {
        assert!(report.dir.join(name).is_file(), "{name} is not on disk");
    }
    std::env::remove_var("SVIPALL_HOME");
}

/// One request, one response, one connection: enough to be a release server for the two tests
/// below, and small enough to need no dependency. Returns the base URL.
async fn serve(files: Vec<(String, Vec<u8>)>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a port");
    let port = listener.local_addr().expect("the address").port();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let files = files.clone();
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = vec![0u8; 8192];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                let body = files
                    .iter()
                    .find(|(name, _)| path.ends_with(name.as_str()))
                    .map(|(_, body)| body.clone());
                let resp = match body {
                    Some(body) => {
                        let mut out = format!(
                            "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                            body.len()
                        )
                        .into_bytes();
                        out.extend_from_slice(&body);
                        out
                    }
                    None => {
                        b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                            .to_vec()
                    }
                };
                let _ = socket.write_all(&resp).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    format!("http://127.0.0.1:{port}")
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// The release path, over a real socket: download the archive, check it against the same
/// `sha256sums.txt` the release publishes, and install what it carries.
#[tokio::test]
async fn the_release_archive_is_downloaded_and_checked_before_it_is_installed() {
    let _guard = HOME.lock().await;
    let home = scratch("download");
    std::env::set_var("SVIPALL_HOME", &home);

    let zip = archive(&a_model_pair());
    let name = model_install::asset_name(env!("CARGO_PKG_VERSION"));
    let sums = format!("{}  {name}\n", sha256_hex(&zip));
    let base = serve(vec![
        (name.clone(), zip),
        ("sha256sums.txt".into(), sums.into_bytes()),
    ])
    .await;
    std::env::set_var("SVIPALL_RELEASES_URL", &base);

    let report = model_install::install(Source::Release, &mut |_| {})
        .await
        .expect("install from the release");
    std::env::remove_var("SVIPALL_RELEASES_URL");
    std::env::remove_var("SVIPALL_HOME");

    assert_eq!(report.installed, vec!["detect", "segment"]);
    assert!(
        report.verified,
        "the archive was not checked against the sums file"
    );
    assert!(report.dir.join("segment.onnx").is_file());
}

/// A digest that does not match is a refusal, not a warning: a corrupt or swapped archive becomes a
/// model that answers wrong, and there is no later point at which that gets noticed.
#[tokio::test]
async fn a_checksum_that_does_not_match_installs_nothing() {
    let _guard = HOME.lock().await;
    let home = scratch("mismatch");
    std::env::set_var("SVIPALL_HOME", &home);

    let name = model_install::asset_name(env!("CARGO_PKG_VERSION"));
    let base = serve(vec![
        (name.clone(), archive(&a_model_pair())),
        (
            "sha256sums.txt".into(),
            format!("{}  {name}\n", "0".repeat(64)).into_bytes(),
        ),
    ])
    .await;
    std::env::set_var("SVIPALL_RELEASES_URL", &base);

    let err = model_install::install(Source::Release, &mut |_| {})
        .await
        .expect_err("a mismatch must stop the install");
    std::env::remove_var("SVIPALL_RELEASES_URL");
    std::env::remove_var("SVIPALL_HOME");

    assert!(
        err.to_string().contains("checksum mismatch"),
        "the error must say what happened: {err}"
    );
    assert!(
        !svipall::model_source::models_dir()
            .join("detect.onnx")
            .exists(),
        "a refused archive still installed something"
    );
}
