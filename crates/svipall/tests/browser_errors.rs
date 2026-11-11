//! What a browser tool says when it fails: the cause, not the outermost word of it.
//!
//! The offline tests need no browser: a verb that does not exist is refused before one starts, a
//! profile another process holds is refused before one starts, and a "browser" that is really a
//! program which exits at once fails inside the launch, where the cause used to be dropped.
mod support;
use serde_json::json;
use svipall::server::SvipallServer;
use svipall::tools::{BrowserDoParams, BrowserOpenParams, WebActParams, WebScreenshotParams};

fn server(cfg: svipall_core::Config) -> SvipallServer {
    support::isolate();
    SvipallServer::new(None, cfg, None)
}

#[tokio::test]
async fn an_unknown_action_is_answered_with_every_verb_and_alias_before_a_browser_starts() {
    let s = server(svipall_core::Config::default());
    let out = s
        .act_json(WebActParams {
            url: "http://127.0.0.1:9/".into(),
            actions: vec![
                json!({"do": "click", "selector": "a"}),
                json!({"do": "tap"}),
            ],
            extraction: None,
            tier: None,
            profile: None,
            proxy: None,
            timeout: Some(5_000),
        })
        .await;
    let e = out["error"].as_str().unwrap_or_default();
    assert!(e.contains("'tap'"), "names the verb: {out}");
    for (verb, aliases) in svipall::tools::ACTIONS {
        assert!(e.contains(verb), "lists {verb}: {e}");
        for a in *aliases {
            assert!(e.contains(a), "lists the alias {a}: {e}");
        }
    }
    for must in [
        "verify",
        "console",
        "hold",
        "press_and_hold",
        "evaluate",
        "navigate",
    ] {
        assert!(
            e.contains(must),
            "{must} was missing from the old list: {e}"
        );
    }
}

#[test]
fn the_schema_enum_is_the_tables_first_column() {
    let schema = schemars::schema_for!(svipall::tools::WebActParams);
    let text = serde_json::to_string(&schema).unwrap();
    let verbs: Vec<&str> = svipall::tools::ACTIONS.iter().map(|(v, _)| *v).collect();
    assert!(
        text.contains(&serde_json::to_string(&verbs).unwrap()),
        "the enum is built from ACTIONS: {text}"
    );
}

/// Hold a profile's lock the way a running browser does.
struct Held {
    #[allow(dead_code)]
    file: Option<std::fs::File>,
}

fn hold_lock(dir: &std::path::Path) -> Held {
    std::fs::create_dir_all(dir).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .share_mode(0)
            .open(dir.join("lockfile"))
            .unwrap();
        Held { file: Some(file) }
    }
    #[cfg(not(windows))]
    {
        let _ = std::fs::remove_file(dir.join("SingletonLock"));
        std::os::unix::fs::symlink(
            format!("somehost-{}", std::process::id()),
            dir.join("SingletonLock"),
        )
        .unwrap();
        Held { file: None }
    }
}

#[tokio::test]
async fn a_profile_another_process_holds_says_so_and_what_to_do() {
    let s = server(svipall_core::Config::default());
    let dir = svipall::browser::named_profile("held-elsewhere");
    let _held = hold_lock(&dir);
    let out = s
        .browser_open_json(BrowserOpenParams {
            profile: Some("held-elsewhere".into()),
            proxy: None,
            visible: None,
        })
        .await;
    let e = out["error"].as_str().unwrap_or_default();
    assert!(e.contains("in use by another browser process"), "{out}");
    assert!(e.contains("held-elsewhere"), "names the directory: {out}");
    assert!(
        e.contains("close") && e.contains("different profile"),
        "names the remedy: {out}"
    );
}

/// A program that is not a browser and exits as soon as it starts.
fn not_a_browser() -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    {
        let src = std::path::PathBuf::from(std::env::var("SystemRoot").ok()?)
            .join("System32")
            .join("whoami.exe");
        // Copied somewhere with no version-shaped directories beside it.
        let dir = support::isolate().join("notabrowser");
        std::fs::create_dir_all(&dir).ok()?;
        let dst = dir.join("whoami.exe");
        std::fs::copy(&src, &dst).ok()?;
        Some(dst)
    }
    #[cfg(not(windows))]
    {
        ["/bin/true", "/usr/bin/true"]
            .iter()
            .map(std::path::PathBuf::from)
            .find(|p| p.is_file())
    }
}

#[tokio::test]
async fn a_browser_that_will_not_launch_reports_why_not_just_where() {
    let Some(exe) = not_a_browser() else {
        eprintln!("no stand-in executable on this machine; skipped");
        return;
    };
    let s = server(svipall_core::Config {
        browser_path: exe.to_string_lossy().to_string(),
        ..Default::default()
    });
    let shot = s
        .screenshot_json(WebScreenshotParams {
            url: "http://127.0.0.1:9/".into(),
            full_page: None,
            tier: Some("stealth".into()),
            profile: None,
            proxy: None,
            inline: Some(false),
            mobile: None,
            timeout: Some(45_000),
        })
        .await
        .unwrap();
    let e = shot.value["error"].as_str().unwrap_or_default().to_string();
    assert!(e.starts_with("launching browser"), "{}", shot.value);
    assert!(
        e.len() > "launching browser".len() + 5,
        "the cause comes with it: {}",
        shot.value
    );
}

/// The actions already run are kept when the page cannot be read afterwards. Needs a browser.
#[tokio::test]
#[ignore = "needs a real browser"]
async fn browser_do_keeps_its_action_results_when_the_page_read_fails() {
    let s = server(svipall_core::Config::default());
    let open = s
        .browser_open_json(BrowserOpenParams {
            profile: None,
            proxy: None,
            visible: None,
        })
        .await;
    let id = open["session_id"].as_str().expect("a session").to_string();
    let out = s
        .browser_do_json(BrowserDoParams {
            session_id: id,
            url: None,
            actions: Some(vec![
                json!({"do": "eval", "script": "1 + 1"}),
                // The renderer goes away, so the content read after the actions fails.
                json!({"do": "goto", "url": "chrome://crash"}),
            ]),
            extraction: None,
            query: None,
            timeout: Some(30_000),
        })
        .await;
    eprintln!("{out}");
    let actions = out["actions"].as_array().expect("the actions are kept");
    assert_eq!(actions[0]["ok"], true, "{out}");
    assert_eq!(actions[0]["value"], 2, "{out}");
    assert!(
        out["content_error"].is_string() || out["content"].is_string(),
        "{out}"
    );
}
