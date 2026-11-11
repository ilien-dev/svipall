//! A file is bytes, and comes back as bytes.
//!
//! `.tar.bz2`, `.crx`, `.nupkg`, `.woff2` and images asked for with `out_file` all arrived broken:
//! every non-page body went through a lossy UTF-8 decode, and `out_file` wrote that text. And a
//! multi-gigabyte download could not be done at all, because the body was held in memory and the
//! request died at the client's page timeout. Every check here is on the bytes on disk, by sha256.
mod support;

use sha2::{Digest, Sha256};
use std::path::PathBuf;
use support::{Reply, Site};
use svipall::{server::SvipallServer, tools::WebFetchParams};

fn server() -> SvipallServer {
    support::isolate();
    SvipallServer::new(
        None,
        svipall_core::Config {
            request_min_interval_ms: 1,
            max_tier: "stealth".into(),
            ..Default::default()
        },
        None,
    )
}

fn params(url: String, out_file: Option<PathBuf>) -> WebFetchParams {
    WebFetchParams {
        url,
        cache: Some("bypass".into()),
        robots: Some("ignore".into()),
        out_file: out_file.map(|p| p.to_string_lossy().into_owned()),
        ..Default::default()
    }
}

/// An archive's worth of bytes: every value 0..=255, NULs and invalid UTF-8 included.
fn archive(len: usize) -> Vec<u8> {
    let mut b = b"BZh91AY&SY".to_vec();
    b.extend((0..len as u32).map(|i| (i.wrapping_mul(2654435761) >> 11) as u8));
    b
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A directory of this test's own for the files it asks for.
fn dir(name: &str) -> PathBuf {
    let d = support::isolate().join("files").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[tokio::test]
async fn a_binary_file_without_out_file_is_described_never_decoded() {
    let body = archive(20_000);
    let site = Site::start(vec![(
        "/pkg.tar.bz2",
        Reply::bytes(&body, "application/x-bzip2"),
    )])
    .await;
    let out = server()
        .fetch_json(params(site.url("/pkg.tar.bz2"), None))
        .await
        .value;
    let content = out["content"].as_str().unwrap_or_default();
    assert!(!content.contains('\u{FFFD}'), "mojibake: {out}");
    assert!(content.contains("out_file"), "says how to get it: {out}");
    assert_eq!(out["file"]["sha256"], sha(&body), "{out}");
    assert_eq!(out["file"]["bytes"], body.len(), "{out}");
    assert_eq!(out["file"]["content_type"], "application/x-bzip2", "{out}");
    assert_eq!(out["tier_used"], "http", "{out}");
    assert_eq!(
        out["attempts"].as_array().map(|a| a.len()),
        Some(1),
        "a file is not climbed: {out}"
    );
}

#[tokio::test]
async fn out_file_gets_the_bytes_exactly_as_served() {
    let d = dir("exact");
    for (path, ct, name) in [
        ("/pkg.tar.bz2", "application/x-bzip2", "pkg.tar.bz2"),
        ("/font", "font/woff2", "font.woff2"),
        ("/ext.crx", "application/octet-stream", "ext.crx"),
        ("/scan.png", "image/png", "scan.png"),
    ] {
        let body = archive(50_000);
        let site = Site::start(vec![(path, Reply::bytes(&body, ct))]).await;
        let target = d.join(name);
        let out = server()
            .fetch_json(params(site.url(path), Some(target.clone())))
            .await
            .value;
        let written = std::fs::read(&target).unwrap_or_else(|e| panic!("{name}: {e}: {out}"));
        assert_eq!(sha(&written), sha(&body), "{name} was altered: {out}");
        assert_eq!(out["file"]["sha256"], sha(&body), "{name}: {out}");
        assert_eq!(out["out_file"], target.to_string_lossy().as_ref(), "{out}");
        assert_eq!(out["bytes"], body.len(), "{out}");
        assert!(out.get("content").is_none(), "nothing inline: {out}");
    }
}

#[tokio::test]
async fn a_json_answer_still_comes_back_as_itself() {
    let body = "{\"ok\":true,\"items\":[{\"name\":\"caf\u{e9}\",\"n\":1}]}";
    let site = Site::start(vec![(
        "/api",
        Reply::bytes(body.as_bytes(), "application/json"),
    )])
    .await;
    let out = server()
        .fetch_json(params(site.url("/api"), None))
        .await
        .value;
    assert_eq!(out["content"], body, "{out}");
    assert!(out.get("file").is_none(), "{out}");
}

#[tokio::test]
async fn a_cut_download_is_kept_and_the_next_call_finishes_it() {
    let d = dir("resume");
    let body = archive(400_000);
    let site = Site::start(vec![(
        "/model.bin",
        Reply::bytes(&body, "application/octet-stream")
            .header("ETag", "\"v1\"")
            .cut_first_at(150_000),
    )])
    .await;
    let target = d.join("model.bin");

    let first = server()
        .fetch_json(params(site.url("/model.bin"), Some(target.clone())))
        .await
        .value;
    assert_eq!(first["file"]["partial"], true, "{first}");
    assert_eq!(first["file"]["resumable"], true, "{first}");
    assert!(
        !target.exists(),
        "an unfinished file is never put in place: {first}"
    );
    let kept = first["file"]["bytes_written"].as_u64().unwrap();
    assert!(kept > 0 && kept <= 150_000, "{first}");
    assert_eq!(
        first["attempts"].as_array().map(|a| a.len()),
        Some(1),
        "{first}"
    );

    let second = server()
        .fetch_json(params(site.url("/model.bin"), Some(target.clone())))
        .await
        .value;
    assert_eq!(second["file"]["resumed_from"], kept, "{second}");
    assert_eq!(second["file"]["complete"], true, "{second}");
    assert_eq!(second["status"], 206, "{second}");
    let written = std::fs::read(&target).unwrap();
    assert_eq!(
        sha(&written),
        sha(&body),
        "the resumed file is not the file"
    );
    assert_eq!(
        second["file"]["sha256"],
        sha(&body),
        "the checksum covers every byte"
    );
    assert!(!PathBuf::from(format!("{}.part", target.display())).exists());
}

#[tokio::test]
async fn a_server_whose_copy_changed_sends_the_whole_file_and_it_starts_over() {
    let d = dir("changed");
    let body = archive(120_000);
    let site = Site::start(vec![(
        "/model.bin",
        Reply::bytes(&body, "application/octet-stream").header("ETag", "\"v2\""),
    )])
    .await;
    let target = d.join("model.bin");
    // What an earlier, interrupted transfer of an older copy left behind.
    std::fs::write(format!("{}.part", target.display()), b"stale bytes of v1").unwrap();
    std::fs::write(
        format!("{}.part.meta", target.display()),
        serde_json::json!({"url": site.url("/model.bin"), "etag": "\"v1\""}).to_string(),
    )
    .unwrap();

    let out = server()
        .fetch_json(params(site.url("/model.bin"), Some(target.clone())))
        .await
        .value;
    assert_eq!(out["status"], 200, "{out}");
    assert!(out["file"].get("resumed_from").is_none(), "{out}");
    assert_eq!(sha(&std::fs::read(&target).unwrap()), sha(&body), "{out}");
}

#[tokio::test]
async fn a_text_name_still_asks_for_the_text() {
    // `out_file: "notes.md"` on a PDF or an image is a request for what it says, as before.
    let d = dir("text");
    let body = archive(10_000);
    let site = Site::start(vec![(
        "/x.bin",
        Reply::bytes(&body, "application/octet-stream"),
    )])
    .await;
    let target = d.join("notes.md");
    let out = server()
        .fetch_json(params(site.url("/x.bin"), Some(target.clone())))
        .await
        .value;
    // A body that is bytes whatever it was asked as: the bytes, never mojibake in a .md.
    assert_eq!(sha(&std::fs::read(&target).unwrap()), sha(&body), "{out}");
}
