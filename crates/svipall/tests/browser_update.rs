//! The managed browser is kept current against the stable channel, and only against it.
//!
//! Offline: the channel's version index and the "download" are served by a local fixture, and the
//! archive holds an executable that is a few bytes of nothing. Nothing here runs a browser.
mod support;
use std::io::Write as _;
use std::process::Command;
use support::{Reply, Site};
use svipall::provision::{auto_update, Provisioner};
use svipall::server::SvipallServer;

/// Where the executable sits inside a Chrome for Testing archive on this platform.
fn exe_in_archive() -> &'static str {
    match svipall::provision::platform().expect("a supported platform") {
        "win64" => "chrome-win64/chrome.exe",
        "win32" => "chrome-win32/chrome.exe",
        "linux64" => "chrome-linux64/chrome",
        "mac-x64" => {
            "chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"
        }
        _ => "chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
    }
}

fn archive() -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        w.start_file::<_, ()>(exe_in_archive(), zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(b"not a browser").unwrap();
        w.finish().unwrap();
    }
    buf
}

fn index(version: &str, download: &str) -> String {
    let platform = svipall::provision::platform().unwrap();
    format!(
        r#"{{"channels":{{"Stable":{{"version":"{version}","downloads":{{
            "chrome":[{{"platform":"{platform}","url":"{download}"}}]}}}}}}}}"#
    )
}

/// A managed install of `version` already on disk, as an older release of svipall left it.
fn install_old(home: &std::path::Path, version: &str) -> std::path::PathBuf {
    let exe = home
        .join("browser")
        .join("cft")
        .join(version)
        .join(exe_in_archive());
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::write(&exe, b"old").unwrap();
    exe
}

/// One test, because every step shares one `SVIPALL_HOME` and the environment is process-wide.
#[tokio::test]
async fn a_managed_browser_two_majors_behind_is_replaced_and_only_the_channel_decides() {
    let home = support::isolate();
    let old = install_old(&home, "150.0.7000.1");
    let zip = archive();
    // The archive first, because the index has to name its address.
    let files = Site::start(vec![("/chrome.zip", Reply::bytes(&zip, "application/zip"))]).await;
    let site = Site::start(vec![(
        "/versions.json",
        Reply::bytes(
            index("157.0.7400.5", &files.url("/chrome.zip")).as_bytes(),
            "application/json",
        ),
    )])
    .await;
    let server = SvipallServer::new(None, svipall_core::Config::default(), None);
    let store = server.store().map(|s| s.as_ref());
    let prov = Provisioner::with_index(server.fetcher(), None, &site.url("/versions.json"));

    // Opted out: nothing is asked and nothing changes.
    let cfg = svipall_core::Config {
        browser_auto_update: false,
        ..Default::default()
    };
    let v = auto_update(&cfg, &home, &prov, store, true).await.unwrap();
    assert_eq!(v["plan"]["plan"], "disabled", "{v}");
    assert_eq!(site.hits("/versions.json"), 0, "an opt-out asks nothing");

    // A browser the operator named is theirs, whatever its age.
    let theirs = home.join("their-chrome.exe");
    std::fs::write(&theirs, b"x").unwrap();
    let cfg = svipall_core::Config {
        browser_path: theirs.to_string_lossy().to_string(),
        ..Default::default()
    };
    let v = auto_update(&cfg, &home, &prov, store, true).await.unwrap();
    assert_eq!(v["plan"]["plan"], "not_managed", "{v}");
    assert_eq!(files.hits("/chrome.zip"), 0);

    // The default: 150 against a stable 157 is replaced.
    let cfg = svipall_core::Config {
        browser_path: old.to_string_lossy().to_string(),
        ..Default::default()
    };
    let v = auto_update(&cfg, &home, &prov, store, true).await.unwrap();
    assert_eq!(v["plan"]["plan"], "update", "{v}");
    assert_eq!(v["plan"]["from"], 150, "{v}");
    assert_eq!(v["plan"]["to"], 157, "{v}");
    assert_eq!(v["installed"]["version"], "157.0.7400.5", "{v}");
    let new_exe = v["installed"]["exe"].as_str().unwrap().to_string();
    assert!(std::path::Path::new(&new_exe).is_file(), "{new_exe}");
    assert!(new_exe.contains("157.0.7400.5"), "{new_exe}");
    // browser_path followed it: the old build is pruned, and a path left pointing at it would make
    // the next start fail with "configured browser_path does not exist".
    let after = svipall_core::config::load_in(&home).unwrap();
    assert_eq!(after.browser_path, new_exe);
    assert!(!old.exists(), "the old build is pruned");
    // What the channel said is remembered, for every warning and status call that follows.
    assert_eq!(svipall::provision::known_stable_major(store), Some(157));

    // Asked again soon after, it does not ask the channel again, and there is nothing to do.
    let hits = site.hits("/versions.json");
    let v = auto_update(&after, &home, &prov, store, false)
        .await
        .unwrap();
    assert_eq!(v["plan"]["plan"], "current", "{v}");
    assert_eq!(v["checked"], false, "{v}");
    assert_eq!(
        site.hits("/versions.json"),
        hits,
        "no second look within the day"
    );
}

/// The stale warning names the stable channel, and is printed once per process however many pools
/// the command builds. It used to be printed twice by every CLI command, and measured against
/// whatever other browser happened to be installed.
#[test]
fn the_stale_warning_is_said_once_and_against_the_stable_channel() {
    let home = std::env::temp_dir().join(format!(
        "svipall-stale-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("config.toml"),
        "browser_auto_install = false\nbrowser_auto_update = false\n",
    )
    .unwrap();
    install_old(&home, "150.0.7000.1");
    {
        let store = svipall_core::cache::Store::open_at(&home.join("svipall.db")).unwrap();
        store
            .kv_set(svipall::provision::LATEST_STABLE_KEY, "157")
            .unwrap();
    }
    let o = Command::new(env!("CARGO_BIN_EXE_svipall"))
        .arg("status")
        .env("SVIPALL_HOME", &home)
        .env_remove("SVIPALL_BROWSER")
        .env_remove("CHROME_PATH")
        .env_remove("CHROME_BIN")
        .env_remove("PUPPETEER_EXECUTABLE_PATH")
        .output()
        .expect("runs");
    let err = String::from_utf8_lossy(&o.stderr);
    let said = err.matches("majors behind").count();
    assert_eq!(said, 1, "said exactly once:\n{err}");
    assert!(
        err.contains("Chrome 150") && err.contains("stable channel (157)"),
        "measured against the channel: {err}"
    );
}
