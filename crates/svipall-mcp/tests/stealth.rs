//! Network checks that the browser tiers do not announce themselves.
//!
//! Ignored by default; run with `cargo test -p svipall-mcp --test stealth -- --ignored`.
//!
//! Every assertion here corresponds to something that was actually measured failing before the
//! stealth rewrite, against bot.sannysoft.com on the `stealth` tier:
//!   * `WebDriver (New): present (failed)`
//!   * `webdriver`, `hardwareConcurrency` and `deviceMemory` each listed twice in the prototype
//!   * `navigator.brave` present while the UA claimed Chrome
//!   * `screen.availHeight === screen.height` on Win32
//!   * `outerHeight - innerHeight == 1`

mod support;

use serde_json::Value;
use svipall_mcp::browser::{BrowserPool, BrowserTier, PageOpts};

async fn probe(js: &str) -> Value {
    let cfg = svipall_core::Config::default();
    let pool = BrowserPool::new(cfg);
    if !pool.available() {
        eprintln!("no browser available; skipping");
        return Value::Null;
    }
    let opts = PageOpts {
        identity_seed: None,
        mobile: false,
        tier: BrowserTier::Stealth,
        profile_dir: None,
        proxy: None,
        visible: false,
    };
    let (_pooled, page) = pool.page(&opts).await.expect("open page");
    // A real page, so the init script has run in a normal document context.
    pool.navigate(&page, "https://example.com/")
        .await
        .expect("navigate");
    let out = page
        .evaluate(js)
        .await
        .expect("evaluate")
        .value()
        .cloned()
        .unwrap_or(Value::Null);
    pool.close_page(page).await;
    pool.shutdown().await;
    out
}

// Four tests used to sit here: `navigator_does_not_announce_automation`,
// `patched_properties_appear_exactly_once`, `patched_functions_still_look_native` and
// `screen_geometry_is_self_consistent`. Each ran at one tier, needed the network to reach
// `example.com`, and was `#[ignore]`d out of the gate. `bench tells` asks all four questions at
// four tiers on a loopback page and fails the build, so keeping them here was duplication that
// asserted less. They are `navigator_webdriver`, `no_duplicate_navigator_getters`,
// `patched_functions_are_native`, `screen_plausible` and `window_chrome_height` now.

/// Canvas noise must be stable within an identity: a fingerprint that changes on every read is
/// itself the signal.
#[tokio::test]
#[ignore = "network + browser"]
async fn canvas_noise_is_deterministic_within_a_page() {
    let v = probe(
        r#"(() => {
            const draw = () => {
                const c = document.createElement('canvas');
                c.width = 60; c.height = 20;
                const ctx = c.getContext('2d');
                ctx.textBaseline = 'top';
                ctx.font = '14px Arial';
                ctx.fillText('svipall', 2, 2);
                return c.toDataURL();
            };
            const a = draw(), b = draw();
            return { same: a === b, len: a.length };
        })()"#,
    )
    .await;
    if v.is_null() {
        return;
    }
    assert!(
        v["len"].as_u64().unwrap_or(0) > 100,
        "canvas produced nothing usable"
    );
    // The test computed this and then asserted only that the canvas produced bytes, so it never
    // tested the thing it is named for. Noise reseeded per call is a louder signal than no noise:
    // a fingerprint that changes between two reads of the same drawing is one no real rasteriser
    // produces.
    assert_eq!(
        v["same"], true,
        "two draws of the same content produced different data URLs: the noise is reseeded per call"
    );
}

/// The accessibility snapshot against a real page. Ignored like the rest of this file: it needs a
/// browser. What it proves is the thing unit tests cannot — that the protocol call returns a tree
/// with the shape `prune` expects.
#[tokio::test]
#[ignore = "network + browser"]
async fn a_real_page_yields_a_usable_snapshot() {
    use svipall_mcp::server::SvipallServer;
    use svipall_mcp::tools::WebSnapshotParams;

    let s = SvipallServer::new(None, svipall_core::Config::default(), None);
    if !s.pool().available() {
        eprintln!("no browser available; skipping");
        return;
    }
    let out = s
        .snapshot_json(WebSnapshotParams {
            url: "https://example.com/".into(),
            find: None,
            max_depth: None,
            limit: Some(200),
            tier: Some("stealth".into()),
            profile: None,
            timeout: None,
        })
        .await
        .expect("snapshot");

    let text = out["snapshot"].as_str().unwrap_or_default();
    assert!(out["nodes"].as_u64().unwrap_or(0) > 0, "empty tree: {out}");
    assert!(
        text.contains("link") || text.contains("heading"),
        "no usable roles: {text}"
    );
    // The whole point: a link carries a reference the click tool can take.
    assert!(text.contains("[e"), "no references handed out: {text}");
}

/// What the snapshot actually looks like, printed. Not an assertion — a way to eyeball the shape
/// when the pruning rules change.
#[tokio::test]
#[ignore = "network + browser"]
async fn show_a_snapshot() {
    use svipall_mcp::server::SvipallServer;
    use svipall_mcp::tools::WebSnapshotParams;
    let s = SvipallServer::new(None, svipall_core::Config::default(), None);
    if !s.pool().available() {
        return;
    }
    let out = s
        .snapshot_json(WebSnapshotParams {
            url: "https://news.ycombinator.com/".into(),
            find: None,
            max_depth: Some(4),
            limit: Some(25),
            tier: Some("stealth".into()),
            profile: None,
            timeout: None,
        })
        .await
        .expect("snapshot");
    eprintln!("nodes={} tokens={}", out["nodes"], out["tokens_estimated"]);
    eprintln!("{}", out["snapshot"].as_str().unwrap_or_default());
}

/// Traffic capture against a page that really does render from an API.
#[tokio::test]
#[ignore = "network + browser"]
async fn a_page_that_renders_from_json_gives_up_its_endpoint() {
    use svipall_mcp::server::SvipallServer;
    let s = SvipallServer::new(None, svipall_core::Config::default(), None);
    if !s.pool().available() {
        return;
    }
    let out = s
        .capture_json(svipall_mcp::tools::WebCaptureParams {
            // A page that deliberately renders from AJAX, so a zero here is a real failure rather
            // than a server-rendered site having nothing to capture.
            url: "https://quotes.toscrape.com/scroll".into(),
            pattern: None,
            bodies: Some(false),
            max_body: None,
            settle_ms: Some(2500),
            tier: Some("stealth".into()),
            profile: None,
        })
        .await
        .expect("capture");
    eprintln!(
        "captured={} endpoints={}",
        out["captured"], out["endpoints"]
    );
    assert!(
        out["captured"].as_u64().unwrap_or(0) > 0,
        "a page built from AJAX gave up no traffic: {out}"
    );
}

/// An isolated fetch must leave nothing on disk. Needs a real browser, so it is `#[ignore]`d with
/// the rest of this file.
#[tokio::test]
#[ignore = "needs a real browser"]
async fn an_isolated_fetch_leaves_no_profile_behind() {
    let before = count_once_profiles();
    let server = svipall_mcp::server::SvipallServer::with_store(
        None,
        svipall_core::Config::default(),
        None,
        None,
    );
    let _ = server
        .fetch_json(svipall_mcp::tools::WebFetchParams {
            url: "https://example.com/".into(),
            mode: Some("browser".into()),
            isolated: Some(true),
            ..Default::default()
        })
        .await;
    assert_eq!(
        count_once_profiles(),
        before,
        "an isolated fetch left its profile on disk"
    );
}

fn count_once_profiles() -> usize {
    std::fs::read_dir(svipall_mcp::browser::sessions_dir())
        .map(|d| {
            d.flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("once-"))
                .count()
        })
        .unwrap_or(0)
}

/// A listing that renders twenty rows per scroll event, read after `scroll: "auto"`, comes back
/// whole. Needs a browser, like everything else in this file.
#[tokio::test]
#[ignore = "network + browser"]
async fn a_feed_that_loads_on_scroll_is_read_whole_when_asked() {
    use support::{Reply, Site};
    let page = r#"<!doctype html><html><body><main id="feed"></main>
<script>
  let n = 0;
  function more(k) { for (let i = 0; i < k; i++) { const d = document.createElement('p');
    d.style.height = '120px'; d.textContent = 'row ' + (++n); document.getElementById('feed').appendChild(d); } }
  more(20);
  window.addEventListener('scroll', () => {
    if (n < 100 && window.innerHeight + window.scrollY > document.body.scrollHeight - 800) setTimeout(() => more(20), 150);
  });
</script></body></html>"#;
    let site = Site::start(vec![("/", Reply::html(page))]).await;
    support::isolate();
    let s = svipall_mcp::server::SvipallServer::with_store(
        None,
        svipall_core::Config::default(),
        None,
        svipall_core::cache::Store::open_memory()
            .ok()
            .map(std::sync::Arc::new),
    );
    let out = s
        .fetch_json(svipall_mcp::tools::WebFetchParams {
            url: site.url("/"),
            scroll: Some("auto".into()),
            max_tier: Some("browser".into()),
            ..Default::default()
        })
        .await;
    let text = out.value["content"].as_str().unwrap_or_default();
    assert!(
        out.value["scrolled"].as_u64().unwrap_or(0) >= 3,
        "{:?}",
        out.value
    );
    assert!(
        text.contains("row 100"),
        "only {} rows: {:?}",
        text.matches("row ").count(),
        out.value
    );
    s.pool().shutdown().await;
}

/// The browser tiers reported no response headers at all, so a vendor whose only give-away is one
/// was invisible at exactly the tiers that hold a live browser.
///
/// Driven against the pool rather than through `fetch_json`: a loopback URL is pinned to the http
/// tier by the ladder, so a fetch could never exercise this path. What is asserted here is the part
/// that was untestable offline — the CDP subscription, and that it picks the document out of
/// everything else the page asked for.
#[tokio::test]
#[ignore = "needs a real browser"]
async fn a_browser_page_reports_the_documents_response_headers() {
    let cfg = svipall_core::Config::default();
    let pool = BrowserPool::new(cfg);
    if !pool.available() {
        eprintln!("no browser available; skipping");
        return;
    }
    // The document carries a header no page would have by accident, and pulls in a sub-resource
    // carrying a different one. Only the first may be reported.
    let site = support::Site::start(vec![
        (
            "/",
            support::Reply::html(
                "<!doctype html><html><head><script src=\"/s.js\"></script></head>\
                 <body><div id=\"root\"></div></body></html>",
            )
            .header("x-kpsdk-ct", "2|abc|def"),
        ),
        (
            "/s.js",
            support::Reply::html("/* not the document */").header("x-sub-resource", "yes"),
        ),
    ])
    .await;
    let opts = PageOpts {
        identity_seed: None,
        mobile: false,
        tier: BrowserTier::Stealth,
        profile_dir: None,
        proxy: None,
        visible: false,
    };
    let (_pooled, page) = pool.page(&opts).await.expect("open page");
    let mut watch = svipall_mcp::wire::DocumentWatch::start(&page)
        .await
        .expect("start the document watch");
    pool.navigate(&page, &site.url("/"))
        .await
        .expect("navigate");
    pool.settle(&page, std::time::Duration::from_millis(800))
        .await;
    watch.drain();
    let names: Vec<String> = watch
        .headers()
        .iter()
        .map(|(k, _)| k.to_ascii_lowercase())
        .collect();
    pool.close_page(page).await;
    pool.shutdown().await;

    assert!(
        names.iter().any(|k| k == "x-kpsdk-ct"),
        "the document's own header never arrived: {names:?}"
    );
    assert!(
        !names.iter().any(|k| k == "x-sub-resource"),
        "a sub-resource's headers were reported as the page's: {names:?}"
    );
}

/// A clearance that lives in the page's runtime dies with the tab. Holding the tab is the whole
/// point, and "the same tab" is only provable from inside it: a value written to `window` survives
/// a navigation on a reused page and cannot exist on a fresh one.
#[tokio::test]
#[ignore = "needs a real browser"]
async fn a_page_the_pool_parked_comes_back_to_the_next_fetch_that_wants_it() {
    let cfg = svipall_core::Config::default();
    let pool = BrowserPool::new(cfg);
    if !pool.available() {
        eprintln!("no browser available; skipping");
        return;
    }
    let opts = PageOpts {
        identity_seed: None,
        mobile: false,
        tier: BrowserTier::Stealth,
        profile_dir: None,
        proxy: None,
        visible: false,
    };
    let key = BrowserPool::kept_key(&opts, "shop.example", false);

    let (pooled, page, reused) = pool.warm_page(&opts, &key).await.expect("first page");
    assert!(!reused, "nothing was parked yet");
    let id = page.target_id().inner().clone();
    pool.keep_page(&key, pooled, page).await;

    let (pooled2, page2, reused2) = pool.warm_page(&opts, &key).await.expect("second page");
    assert!(reused2, "the parked page was not handed back");
    assert_eq!(page2.target_id().inner(), &id, "a different tab came back");
    pool.keep_page(&key, pooled2, page2).await;

    // A different form factor is a different page: the viewport and user agent are set once, when
    // the tab is created, and a reused tab cannot be re-prepared.
    let phone = PageOpts {
        mobile: true,
        ..opts.clone()
    };
    let phone_key = BrowserPool::kept_key(&phone, "shop.example", false);
    assert_ne!(phone_key, key);

    // Taking it removes it: two fetches must never drive one tab.
    let (_p3, page3, reused3) = pool.warm_page(&opts, &key).await.expect("third page");
    assert!(reused3);
    let (_p4, page4, reused4) = pool.warm_page(&opts, &key).await.expect("fourth page");
    assert!(!reused4, "one tab was handed to two fetches at once");
    assert_ne!(page4.target_id().inner(), page3.target_id().inner());

    pool.shutdown().await;
}
