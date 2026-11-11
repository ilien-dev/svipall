//! The shell answers the way the tools do: `svipall fetch --out` writes the bytes as served, and
//! `svipall probe` checks addresses without reading them.
mod support;

use sha2::{Digest, Sha256};
use support::{Reply, Site};

fn svipall(home: &std::path::Path, args: &[&str]) -> serde_json::Value {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_svipall"))
        .args(args)
        .env("SVIPALL_HOME", home)
        .env_remove("SVIPALL_HTTP_ENGINE")
        .output()
        .expect("run svipall");
    assert!(
        out.status.success(),
        "svipall {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|_| panic!("not json: {}", String::from_utf8_lossy(&out.stdout)))
}

/// A home of its own, told never to reach for a browser.
fn home(name: &str) -> std::path::PathBuf {
    let h = support::isolate().join(name);
    std::fs::create_dir_all(&h).unwrap();
    std::fs::write(
        h.join("config.toml"),
        "max_tier = \"http\"\nrequest_min_interval_ms = 1\nupdate_check = false\n",
    )
    .unwrap();
    h
}

#[tokio::test]
async fn fetch_out_writes_the_bytes_as_served() {
    let body: Vec<u8> = (0..30_000u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 9) as u8)
        .collect();
    let site = Site::start(vec![("/pkg.nupkg", Reply::bytes(&body, "application/zip"))]).await;
    let h = home("cli-fetch");
    let target = h.join("pkg.nupkg");
    let url = site.url("/pkg.nupkg");
    let home = h.clone();
    let t = target.to_string_lossy().into_owned();
    let out = tokio::task::spawn_blocking(move || {
        svipall(&home, &["fetch", &url, "--out", &t, "--cache", "bypass"])
    })
    .await
    .unwrap();
    let written = std::fs::read(&target).unwrap_or_else(|e| panic!("{e}: {out}"));
    assert_eq!(
        Sha256::digest(&written)[..],
        Sha256::digest(&body)[..],
        "{out}"
    );
}

#[tokio::test]
async fn probe_answers_each_url_in_order() {
    let site = Site::start(vec![("/here", Reply::page("Here", &[]))]).await;
    let h = home("cli-probe");
    let (a, b) = (site.url("/here"), site.url("/gone"));
    let (a2, b2) = (a.clone(), b.clone());
    let out = tokio::task::spawn_blocking(move || svipall(&h, &["probe", &a2, &b2]))
        .await
        .unwrap();
    assert_eq!(out["results"][0]["url"], a.as_str(), "{out}");
    assert_eq!(out["results"][0]["status"], 200, "{out}");
    assert_eq!(out["results"][1]["url"], b.as_str(), "{out}");
    assert_eq!(out["results"][1]["status"], 404, "{out}");
    assert_eq!(out["results"][1]["exists"], false, "{out}");
}
