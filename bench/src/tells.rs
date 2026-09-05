//! What a detector reads off the session, asserted offline.
//!
//! `fingerprint` asks public detectors what they see and needs the network to do it, so it cannot
//! sit in the gate. This does the same job against a page the benchmark serves itself, on
//! loopback, with no request leaving the machine — which means it *can* fail the build.
//!
//! The probes are not a wish list. Most of them were a real contradiction in this tree when they
//! were written: a named global on `window`, a window parked at -32000, a `DOMRect` whose `x` and
//! `left` disagree, host objects replaced by object literals, a frozen `navigator.languages`,
//! accessors named `get` instead of `get <prop>`, a worker realm nothing patches, touch emulation
//! on a desktop identity, a viewport with no scrollbar, an accessor that stringifies to its own
//! source when a second realm asks, a `PermissionStatus` that is an object literal, and a
//! `navigator.webdriver` deleted rather than answered. The rest are watchdogs, and they are worth
//! as much: a watchdog fails the build when a fix that already shipped silently stops applying.
//!
//! Every tier that runs a browser is measured, because the tiers do not share a configuration:
//! `browser` carries no stealth script, `real` and `warm` are headful and parked offscreen. A probe
//! that passes at three tiers and fails at one is the normal shape of a finding here.

use serde_json::{json, Map, Value};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::time::{Duration, Instant};
use svipall_mcp::browser::{BrowserPool, BrowserTier, PageOpts};

/// The probe page. Embedded rather than read at runtime so the benchmark has no working directory
/// to get wrong and no file to lose.
const PAGE: &str = include_str!("../fixtures/tells/index.html");

/// Every probe the page defines, by name.
///
/// `run` counts a probe that reported `ok: false`. It cannot count one that never reported at all,
/// and a JavaScript error thrown before a `put(...)` line takes every probe after it with it: the
/// run then prints a smaller `total` and passes. Naming the probes here rather than counting them
/// also catches one renamed away from its baseline. This is the guard `targets.rs` puts on the
/// frozen target lists, applied to the frozen probe list.
const PROBES: &[&str] = &[
    "residue",
    "screen_position",
    "dom_rect",
    "host_object_brands",
    "languages_fresh",
    "languages_shape",
    "screen_plausible",
    "getter_names",
    "worker_realm",
    "heap_limit_is_a_ceiling",
    "touch_matches_identity",
    "scrollbar_present",
    "stack_has_no_injected_url",
    "runtime_domain_unobservable",
    "navigator_webdriver",
    "no_duplicate_navigator_getters",
    "patched_functions_are_native",
    "brave_absent",
    "plugins_present",
    "user_agent_data_present",
    "window_chrome_height",
    "device_pixel_ratio",
    "text_geometry_stable",
    "connection_coherent",
    "canvas_noise_stable",
    "navigator_getters_are_native",
    "cross_realm_tostring",
    "iframe_realm_agrees",
    "focus_and_visibility",
    "input_modality",
    "ua_string_self_agreement",
    "permission_state_is_valid",
];

/// The names in `expected` that `seen` never reported.
fn missing<'a>(expected: &[&'a str], seen: &Map<String, Value>) -> Vec<&'a str> {
    expected
        .iter()
        .copied()
        .filter(|name| !seen.contains_key(*name))
        .collect()
}

/// A one-page HTTP server on loopback, in a thread of its own.
///
/// Deliberately not axum: the whole contract is "answer any path with one document", and a
/// dependency the workspace would otherwise not need is a dependency `cargo-machete` and the size
/// budget both have to carry. The thread is detached and dies with the process.
fn serve() -> std::io::Result<String> {
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
    let url = format!("http://127.0.0.1:{}/", listener.local_addr()?.port());
    std::thread::spawn(move || {
        let body = PAGE.as_bytes();
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
            body.len()
        );
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            // Read whatever the browser sent and drop it: there is only one document to serve, so
            // the request line never changes the answer.
            let mut scratch = [0u8; 2048];
            let _ = stream.read(&mut scratch);
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(body);
            let _ = stream.flush();
        }
    });
    Ok(url)
}

/// Open the probe page at one tier and return its verdicts.
/// `reuse` parks the page and probes it a second time through the parking lot, which is the only
/// way to catch a patch installed twice. `prepare` uses `evaluate_on_new_document`, so a reused
/// page that were re-prepared would carry two copies of the identity script — readable from the
/// page, and invisible to every probe that only ever looks at a fresh tab.
async fn probe(
    pool: &BrowserPool,
    tier: BrowserTier,
    url: &str,
    reuse: bool,
) -> Result<Map<String, Value>, String> {
    // A profile per tier. Without one the headless tiers fall back to a single shared directory
    // that Chrome locks, so the second browser never launches — which is a finding about the
    // default, not about the tier being measured, and it belongs in its own probe rather than
    // here.
    let profile_dir = std::env::temp_dir().join(format!("svipall-tells-{tier:?}").to_lowercase());
    let opts = PageOpts {
        identity_seed: None,
        mobile: false,
        tier,
        profile_dir: Some(profile_dir),
        proxy: None,
        visible: false,
    };
    let key = BrowserPool::kept_key(&opts, "tells.local", false);
    let (pooled, page) = if reuse {
        // Park a page and take it back, so what is probed is a tab that has been navigated more
        // than once — the state a held page is actually in when the next fetch gets it.
        let (p, first) = pool
            .page(&opts)
            .await
            .map_err(|e| format!("page could not be opened: {e}"))?;
        if let Err(e) = pool.navigate(&first, url).await {
            pool.close_page(first).await;
            return Err(format!("navigation failed: {e}"));
        }
        pool.keep_page(&key, p, first).await;
        let (p, page, was_reused) = pool
            .warm_page(&opts, &key)
            .await
            .map_err(|e| format!("kept page could not be taken back: {e}"))?;
        if !was_reused {
            pool.close_page(page).await;
            return Err("the parked page was not handed back, so nothing was reused".into());
        }
        (p, page)
    } else {
        pool.page(&opts)
            .await
            .map_err(|e| format!("page could not be opened: {e}"))?
    };
    let _ = pooled;
    if let Err(e) = pool.navigate(&page, url).await {
        pool.close_page(page).await;
        return Err(format!("navigation failed: {e}"));
    }
    // The worker probe is asynchronous, so the page says when it is finished rather than the
    // runner guessing at a sleep.
    let deadline = Instant::now() + Duration::from_secs(10);
    let value;
    loop {
        let seen = page
            .evaluate("window.__TELLS_DONE__ === true ? window.__TELLS__ : null")
            .await
            .ok()
            .and_then(|r| r.value().cloned())
            .unwrap_or(Value::Null);
        if seen.is_object() || Instant::now() >= deadline {
            value = seen;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    pool.close_page(page).await;
    match value {
        Value::Object(m) => Ok(m),
        _ => Err("the page never reported its probes".into()),
    }
}

pub async fn run(assert: bool) -> usize {
    let pool = BrowserPool::new(svipall_core::Config::default());
    if !pool.available() {
        eprintln!("skip no browser available (svipall browser install)");
        return 0;
    }
    let url = match serve() {
        Ok(u) => u,
        Err(e) => {
            eprintln!("FAIL could not open a loopback listener: {e}");
            return usize::from(assert);
        }
    };

    let mut failures = 0;
    let mut report: Vec<Value> = Vec::new();
    // The last row is the same tier again, on a page that was parked and taken back. A held page is
    // navigated more than once and never re-prepared; if either of those ever changed, this is the
    // row that says so — offline, on loopback, inside `qc`.
    for (tier, reuse) in [
        (BrowserTier::Browser, false),
        (BrowserTier::Stealth, false),
        (BrowserTier::Real, false),
        (BrowserTier::Warm, false),
        (BrowserTier::Browser, true),
    ] {
        let label = if reuse {
            format!("{tier:?} (reused)")
        } else {
            format!("{tier:?}")
        };
        eprintln!("\n== {label} ==");
        match probe(&pool, tier, &url, reuse).await {
            Ok(probes) => {
                for (name, v) in &probes {
                    let ok = v["ok"] == Value::Bool(true);
                    let detail = v["detail"].as_str().unwrap_or_default();
                    eprintln!("{} {name:<28} {detail}", if ok { "ok  " } else { "FAIL" });
                    if !ok {
                        failures += 1;
                    }
                    report.push(json!({
                        "tier": label.to_lowercase(),
                        "probe": name,
                        "ok": ok,
                        "detail": detail,
                    }));
                }
                // A probe that never reported is invisible to the loop above: it is not a row with
                // `ok: false`, it is no row at all. Name it here so a silent loss costs the same as
                // a contradiction.
                for name in missing(PROBES, &probes) {
                    eprintln!("FAIL {name:<28} the page never reported this probe");
                    failures += 1;
                    report.push(json!({
                        "tier": label.to_lowercase(),
                        "probe": name,
                        "ok": false,
                        "detail": "the page never reported this probe",
                    }));
                }
            }
            Err(e) => {
                eprintln!("FAIL {e}");
                failures += 1;
                report.push(json!({
                    "tier": label.to_lowercase(),
                    "probe": "-",
                    "ok": false,
                    "detail": e,
                }));
            }
        }
    }
    // Bounded: four browsers, two of them headful, and a shutdown that does not come back leaves a
    // process holding this binary open — which the next `cargo build` then cannot replace. The
    // numbers are already collected by this point, so a slow teardown must not cost them.
    if tokio::time::timeout(Duration::from_secs(15), pool.shutdown())
        .await
        .is_err()
    {
        eprintln!("note browsers did not all close within 15s; leaving them to the OS");
    }

    let passed = report.iter().filter(|r| r["ok"] == true).count();
    println!(
        "{}",
        json!({
            "passed": passed,
            "total": report.len(),
            "failures": failures,
            "probes": report,
        })
    );
    eprintln!("\n{passed}/{} probes clean", report.len());
    if assert {
        failures
    } else {
        // Reported, not asserted: the same rule the evasion runs follow, so a measurement pass
        // never turns a shell red on its own.
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `put('name', ...)` the page calls, deduplicated: `worker_realm` has two branches.
    fn names_in_the_page() -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        for (i, _) in PAGE.match_indices("put('") {
            let rest = &PAGE[i + "put('".len()..];
            let Some(end) = rest.find('\'') else { continue };
            let name = rest[..end].to_string();
            if !found.contains(&name) {
                found.push(name);
            }
        }
        found
    }

    /// The list and the page have to agree in both directions. A probe dropped from the page, or
    /// renamed without its entry here, is otherwise only visible as a smaller `total` at the end of
    /// a four-browser run — and `run` passes on a smaller total.
    #[test]
    fn the_probe_list_and_the_page_agree() {
        let page = names_in_the_page();
        let mut absent_from_page: Vec<&str> = PROBES
            .iter()
            .copied()
            .filter(|p| !page.iter().any(|n| n == p))
            .collect();
        absent_from_page.sort_unstable();
        assert!(
            absent_from_page.is_empty(),
            "named in PROBES but never put() by the page: {absent_from_page:?}"
        );
        let mut absent_from_list: Vec<&String> = page
            .iter()
            .filter(|n| !PROBES.contains(&n.as_str()))
            .collect();
        absent_from_list.sort();
        assert!(
            absent_from_list.is_empty(),
            "put() by the page but not named in PROBES: {absent_from_list:?}"
        );
    }

    #[test]
    fn a_probe_the_page_never_reported_is_named() {
        let mut seen = Map::new();
        for name in PROBES.iter().skip(1) {
            seen.insert((*name).to_string(), json!({ "ok": true, "detail": "" }));
        }
        assert_eq!(missing(PROBES, &seen), vec![PROBES[0]]);
        seen.insert(PROBES[0].to_string(), json!({ "ok": true, "detail": "" }));
        assert!(missing(PROBES, &seen).is_empty());
    }
}
