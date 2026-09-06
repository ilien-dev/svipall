//! End-to-end browser regressions served only on loopback.
mod support;
use serde_json::{json, Value};
use support::{Reply, Site};
use svipall_mcp::{
    browser::{BrowserPool, BrowserTier, PageOpts},
    server::SvipallServer,
    tools::WebFetchParams,
};

fn article(title: &str, script: &str) -> Reply {
    Reply::html(&format!("<html><head><title>{title}</title></head><body><main><h1>{title}</h1><p>{}</p></main><script>{script}</script></body></html>",
        "A detailed account of the local experiment, with independent observations and useful information for its readers. ".repeat(12)))
        .header("x-kpsdk-ct", "local-fixture")
}

#[tokio::test]
async fn saved_policy_is_applied_without_restarting_or_mutating_inflight_calls() {
    let home = support::isolate();
    let server =
        SvipallServer::new(None, svipall_core::Config::default(), None).with_live_configuration();
    let first = server.active().await.unwrap();
    let same = server.active().await.unwrap();
    assert!(std::sync::Arc::ptr_eq(&first, &same));
    let original = first.config().warm_wait_ms;
    first
        .status_json(svipall_mcp::tools::WebStatusParams {
            configure: Some(json!({"warm_wait_ms":original + 1})),
            ..Default::default()
        })
        .await
        .unwrap();
    let next = server.active().await.unwrap();
    assert_eq!(next.config().warm_wait_ms, original + 1);
    assert_eq!(
        first.config().warm_wait_ms,
        original,
        "existing calls retain their policy"
    );
    assert!(!std::sync::Arc::ptr_eq(&first, &next));
    assert_eq!(
        svipall_core::config::load_in(&home).unwrap().warm_wait_ms,
        original + 1
    );
    server.shutdown_configuration().await;
    svipall_core::config::update_in(&home, json!({"warm_wait_ms":original})).unwrap();
}

#[tokio::test]
async fn stable_profile_warms_once_and_reuses_sdk_without_returning_cached_content() {
    support::isolate();
    for named in [true, false] {
        session_case(named).await;
    }
}

async fn session_case(named: bool) {
    let site = Site::start(vec![
        ("/", article("Front page", "")),
        ("/article", article("First response", "const originalFetch = window.fetch; window.fetch = (u, o) => originalFetch('/fresh', o);")),
        ("/fresh", article("Fresh response from live SDK", "")),
    ]).await;
    let server = SvipallServer::new(None, svipall_core::Config::default(), None);
    if !server.pool().available() {
        eprintln!("SKIP: no browser");
        return;
    }
    let fetch = || WebFetchParams {
        url: site.url("/article"),
        mode: Some("warm".into()),
        profile: named.then(|| format!("session-{}", site.port)),
        cache: Some("bypass".into()),
        robots: Some("ignore".into()),
        ..Default::default()
    };
    let first = server.fetch_json(fetch()).await.value;
    assert!(first["blocked_reason"].is_null(), "{first}");
    assert_eq!(
        site.hits("/"),
        1,
        "new profile must visit the origin including its port"
    );
    let second = server.fetch_json(fetch()).await.value;
    server.pool().shutdown().await;
    assert!(second["blocked_reason"].is_null(), "{second}");
    assert!(
        second["content"]
            .as_str()
            .unwrap_or("")
            .contains("Fresh response from live SDK"),
        "{second}"
    );
    assert_eq!(second["warm"]["document_reused"], json!(true));
    assert_eq!(site.hits("/"), 1, "returning profile must skip warmup");
    assert_eq!(site.hits("/article"), 1, "re-navigation destroys the SDK");
    assert_eq!(
        site.hits("/fresh"),
        1,
        "cache bypass must perform a new network request"
    );
}

#[tokio::test]
async fn native_mode_keeps_real_apis_and_workers_even_after_an_emulated_pool() {
    support::isolate();
    let site = Site::start(vec![("/", article("Native hardware", ""))]).await;
    let mut reports = Vec::new();
    for (mode, locale, timezone) in [
        ("emulated", "", ""),
        ("native", "", ""),
        ("native", "fr-FR", "Europe/Paris"),
    ] {
        let cfg = svipall_core::Config {
            browser_identity: mode.into(),
            locale: locale.into(),
            timezone: timezone.into(),
            ..Default::default()
        };
        let pool = BrowserPool::new(cfg);
        if !pool.available() {
            eprintln!("SKIP: no browser");
            return;
        }
        let opts = PageOpts {
            tier: BrowserTier::Stealth,
            profile_dir: None,
            proxy: None,
            visible: false,
            mobile: false,
            identity_seed: Some(984),
        };
        let (_, page) = pool.page(&opts).await.unwrap();
        pool.navigate(&page, &site.url("/")).await.unwrap();
        let report: Value = page.evaluate(r#"(async () => {
            const src = URL.createObjectURL(new Blob(['postMessage({cores:navigator.hardwareConcurrency,language:navigator.language,locale:Intl.DateTimeFormat().resolvedOptions().locale,timezone:Intl.DateTimeFormat().resolvedOptions().timeZone})'], {type:'text/javascript'}));
            const worker = new Worker(src);
            const cores = await new Promise(resolve => worker.onmessage = e => resolve(e.data));
            worker.terminate(); URL.revokeObjectURL(src);
            const frame = document.createElement('iframe'); document.body.append(frame);
            return {cores:navigator.hardwareConcurrency, worker:cores,
                ownGpu:Object.hasOwn(WebGLRenderingContext.prototype, 'getParameter'),
                gpuSource:frame.contentWindow.Function.prototype.toString.call(WebGLRenderingContext.prototype.getParameter),
                ua:navigator.userAgent,language:navigator.language,
                locale:Intl.DateTimeFormat().resolvedOptions().locale,
                timezone:Intl.DateTimeFormat().resolvedOptions().timeZone};
        })()"#).await.unwrap().into_value().unwrap();
        pool.shutdown().await;
        assert_eq!(
            report["cores"], report["worker"]["cores"],
            "{mode}: {report}"
        );
        if mode == "native" {
            for field in ["language", "locale", "timezone"] {
                assert_eq!(report[field], report["worker"][field], "{mode}: {report}");
            }
            if !timezone.is_empty() {
                assert_eq!(report["timezone"], timezone, "{report}");
                assert_eq!(report["locale"], locale, "{report}");
            }
            assert!(
                report["gpuSource"]
                    .as_str()
                    .unwrap()
                    .contains("[native code]"),
                "{report}"
            );
        }
        reports.push(report);
    }
    assert!(reports[1]["timezone"].is_string());
}

fn scratch_dirs() -> Vec<String> {
    std::fs::read_dir(svipall_mcp::browser::sessions_dir())
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.starts_with("scratch-"))
                .collect()
        })
        .unwrap_or_default()
}

fn no_profile(seed: u64) -> PageOpts {
    PageOpts {
        tier: BrowserTier::Stealth,
        profile_dir: None,
        proxy: None,
        visible: false,
        mobile: false,
        identity_seed: Some(seed),
    }
}

/// A browser with no profile of its own used to land on one directory shared by every such
/// browser on the machine, and Chrome refuses to start on a directory another instance holds
/// (exit status 21). Two pools opened at the same moment must both come up, each on a directory
/// of its own, and neither directory may outlive its browser.
#[tokio::test]
async fn browsers_without_a_profile_get_a_directory_each_and_leave_none_behind() {
    support::isolate();
    let a = BrowserPool::new(svipall_core::Config::default());
    let b = BrowserPool::new(svipall_core::Config::default());
    if !a.available() {
        eprintln!("SKIP: no browser");
        return;
    }
    let before = scratch_dirs().len();
    let opts = no_profile(1);
    let (pa, pb) = tokio::join!(a.page(&opts), b.page(&opts));
    let (_, page_a) = pa.expect("first browser");
    let (_, page_b) = pb.expect("second browser, at the same time");
    assert_eq!(
        scratch_dirs().len(),
        before + 2,
        "one directory per browser"
    );
    // Both really are separate processes on separate directories: a cookie set in one is
    // invisible to the other.
    let site = Site::start(vec![("/", article("Two browsers", ""))]).await;
    a.navigate(&page_a, &site.url("/")).await.unwrap();
    b.navigate(&page_b, &site.url("/")).await.unwrap();
    page_a
        .evaluate("document.cookie = 'shared=no'; document.cookie")
        .await
        .unwrap();
    let seen: String = page_b
        .evaluate("document.cookie")
        .await
        .unwrap()
        .into_value()
        .unwrap_or_default();
    assert_eq!(seen, "");
    a.shutdown().await;
    b.shutdown().await;
    assert_eq!(
        scratch_dirs().len(),
        before,
        "a browser's scratch directory goes with it"
    );
}

/// Shutting a pool down returns only once its browser process has left. Until then the profile
/// directory is still locked, and the next launch on it — the very next line, in every test
/// that opens one pool after another — fails to create the process singleton.
#[tokio::test]
async fn a_profile_is_free_the_moment_its_pool_has_shut_down() {
    let home = support::isolate();
    let dir = home.join("profiles").join("relaunch-at-once");
    for round in 0..3 {
        let pool = BrowserPool::new(svipall_core::Config::default());
        if !pool.available() {
            eprintln!("SKIP: no browser");
            return;
        }
        let opts = PageOpts {
            profile_dir: Some(dir.clone()),
            ..no_profile(7)
        };
        pool.page(&opts)
            .await
            .unwrap_or_else(|e| panic!("round {round}: {e:#}"));
        pool.shutdown().await;
    }
}
