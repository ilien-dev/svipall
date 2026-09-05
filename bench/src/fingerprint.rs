//! What public detectors actually see.
//!
//! This is the regression guard for every stealth change. The baseline it replaced, measured
//! before the work started:
//!
//! | check                         | before                       |
//! |-------------------------------|------------------------------|
//! | TLS / JA4                     | `t13d2011_…`, no `h2` marker |
//! | negotiated protocol           | HTTP/1.1                     |
//! | TLS extensions                | 11                           |
//! | GREASE                        | absent                       |
//! | sannysoft `WebDriver (New)`   | present (failed)             |
//! | duplicate navigator getters   | 3                            |
//! | `screen.availHeight` on Win32 | equal to `screen.height`     |
//! | `outerHeight - innerHeight`   | 1px                          |

use serde_json::Value;
use svipall_core::{coherence, IdentityProfile, Os, MAX_EMULATED_CHROME, MAX_EMULATED_FIREFOX};
use svipall_http::{build, Engine, FetcherConfig, HttpRequest};

/// The offline coherence pass: every identity svipall would wear, checked against itself. Returns
/// the number of contradictions, so the build fails on one. `engine` limits it to Firefox when
/// asked; `None` checks Chrome, Firefox and the phone together.
pub fn coherence(engine: Option<&str>) -> usize {
    let firefox_only = matches!(engine, Some(e) if e.eq_ignore_ascii_case("firefox"));
    let mut ids: Vec<(String, IdentityProfile)> = Vec::new();
    for os in [Os::Windows, Os::MacOs, Os::Linux] {
        if !firefox_only {
            ids.push((
                format!("chrome/{os:?}"),
                IdentityProfile::for_major(MAX_EMULATED_CHROME, os).as_machine(0xBEEF ^ os as u64),
            ));
        }
        ids.push((
            format!("firefox/{os:?}"),
            IdentityProfile::firefox(MAX_EMULATED_FIREFOX, os),
        ));
    }
    if !firefox_only {
        ids.push((
            "chrome/phone".into(),
            IdentityProfile::for_major(MAX_EMULATED_CHROME, Os::Linux).as_phone(),
        ));
    }
    let mut failures = 0;
    for (name, id) in &ids {
        let violations = coherence::violations(id);
        let token_ok = coherence::os_token_matches(id);
        let ok = violations.is_empty() && token_ok;
        let detail = if ok {
            "coherent".to_string()
        } else {
            let mut parts: Vec<String> = violations.iter().map(|v| v.rule.to_string()).collect();
            if !token_ok {
                parts.push("os-token".into());
            }
            parts.join(", ")
        };
        failures += check(&format!("coherence {name}"), ok, detail);
    }
    failures += sweep(firefox_only);
    failures
}

/// How many machines to draw per platform. Enough to reach the thin rows of every weighted table
/// in `fleet` — the 2 % ones are where the impossible combinations hide.
const SWEEP: u64 = 500;

/// The same linter, over the space the sampler actually draws from.
///
/// The fixtures above are seven points. `fleet` draws screens, core counts, memory and GPUs in the
/// proportions real traffic has them, which is thousands of combinations, and a linter that never
/// visits them says nothing about what svipall wears in the field. Reported as one aggregate line
/// so the baseline stays readable; the first contradiction found is named in full.
fn sweep(firefox_only: bool) -> usize {
    if firefox_only {
        // Firefox identities do not draw a machine: their renderer is masked and the http tier,
        // which is where they are used, has no WebGL to report.
        return 0;
    }
    let mut drawn = 0usize;
    let mut first_bad: Option<String> = None;
    for os in [Os::Windows, Os::MacOs, Os::Linux] {
        for seed in 0..SWEEP {
            let id = IdentityProfile::for_major(MAX_EMULATED_CHROME, os)
                .as_machine(seed.wrapping_mul(0x9E37_79B9));
            drawn += 1;
            let violations = coherence::violations(&id);
            let ok = violations.is_empty() && coherence::os_token_matches(&id);
            if !ok && first_bad.is_none() {
                let rules: Vec<&str> = violations.iter().map(|v| v.rule).collect();
                first_bad = Some(format!("{os:?} seed {seed}: {}", rules.join(", ")));
            }
        }
    }
    let ok = first_bad.is_none();
    check(
        &format!("coherence sweep ({drawn} drawn machines)"),
        ok,
        first_bad.unwrap_or_else(|| "coherent".to_string()),
    )
}

/// Every check the run produced, so the whole thing can be diffed against a stored baseline.
///
/// A stealth change is only safe if it moves these in one direction. Reading the numbers off the
/// terminal by eye is how a regression slips through.
static RESULTS: std::sync::Mutex<Vec<Value>> = std::sync::Mutex::new(Vec::new());

fn check(label: &str, ok: bool, detail: impl std::fmt::Display) -> usize {
    let detail = detail.to_string();
    eprintln!(
        "{} {:<38} {}",
        if ok { "ok  " } else { "FAIL" },
        label,
        detail
    );
    if let Ok(mut r) = RESULTS.lock() {
        r.push(serde_json::json!({"check": label, "ok": ok, "detail": detail}));
    }
    usize::from(!ok)
}

pub async fn run() -> usize {
    let mut failures = 0;
    eprintln!("== http tier (tls.peet.ws) ==");
    failures += tls().await;
    // The browser half used to live here: sixteen checks against a real navigation, at the
    // `stealth` tier only, reported and never asserted. Every one of them reads `navigator`,
    // `screen` or `window` and needs no network, so they are `bench tells` probes now — four
    // tiers, on loopback, failing the build. What is left here is what genuinely needs the
    // network.

    // stdout carries the machine-readable copy; stderr stays the human one.
    let checks = RESULTS.lock().map(|r| r.clone()).unwrap_or_default();
    let passed = checks.iter().filter(|c| c["ok"] == true).count();
    println!(
        "{}",
        serde_json::json!({
            "passed": passed,
            "total": checks.len(),
            "failures": failures,
            "checks": checks,
        })
    );
    failures
}

async fn tls() -> usize {
    let mut cfg = FetcherConfig::new(IdentityProfile::for_major(MAX_EMULATED_CHROME, Os::host()));
    cfg.engine = Engine::Auto;
    let Ok(f) = build(cfg) else {
        return check("build fetcher", false, "could not build the http engine");
    };
    let resp = match f
        .send(HttpRequest::get("https://tls.peet.ws/api/all"))
        .await
    {
        Ok(r) => r,
        Err(e) => return check("reach tls.peet.ws", false, e),
    };
    let Ok(v) = serde_json::from_slice::<Value>(&resp.body) else {
        return check("parse tls.peet.ws", false, "response was not JSON");
    };

    let ja4 = v["tls"]["ja4"].as_str().unwrap_or_default().to_string();
    let mut failures = 0;
    failures += check("engine", f.engine() == "wreq", f.engine());
    failures += check(
        "negotiated h2",
        v["http_version"].as_str().unwrap_or_default().contains('2'),
        &v["http_version"],
    );
    // JA4_a is t13d + cipher count + extension count + ALPN.
    failures += check(
        "JA4 carries the h2 marker",
        ja4.len() >= 10 && &ja4[8..10] == "h2",
        &ja4,
    );
    let ciphers: u32 = ja4.get(4..6).and_then(|s| s.parse().ok()).unwrap_or(0);
    let exts: u32 = ja4.get(6..8).and_then(|s| s.parse().ok()).unwrap_or(0);
    failures += check(
        "cipher count is Chrome-shaped",
        (13..=17).contains(&ciphers),
        format!("{ciphers} (rustls sends 20)"),
    );
    failures += check(
        "extension count is Chrome-shaped",
        (14..=20).contains(&exts),
        format!("{exts} (rustls sends 11)"),
    );
    let grease = v["tls"]["extensions"]
        .as_array()
        .map(|a| {
            a.iter().any(|e| {
                e["name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .contains("grease")
            })
        })
        .unwrap_or(false);
    failures += check("GREASE values present", grease, grease);
    failures += check(
        "User-Agent matches the emulation",
        v["user_agent"].as_str() == Some(f.identity().user_agent.as_str()),
        v["user_agent"].as_str().unwrap_or_default(),
    );
    // JA4_r lists ciphers, extensions and signature algorithms — never supported groups — so an
    // earlier version of this check looked for the group in `ja4_r`, could not pass, and reported
    // a gap the engine did not have. Read the two extensions that actually carry it.
    let ext = |name: &str| -> Option<&Value> {
        v["tls"]["extensions"]
            .as_array()?
            .iter()
            .find(|e| e["name"].as_str().unwrap_or_default().starts_with(name))
    };
    let group_offered = ext("supported_groups")
        .and_then(|e| e["supported_groups"].as_array())
        .map(|g| {
            g.iter()
                .any(|x| x.as_str().unwrap_or_default().contains("4588"))
        })
        .unwrap_or(false);
    let share_sent = ext("key_share")
        .and_then(|e| e["shared_keys"].as_array())
        .map(|k| k.iter().any(|x| x.to_string().contains("4588")))
        .unwrap_or(false);
    if f.engine() == "wreq" && f.identity().chrome_major >= 131 {
        failures += check(
            "X25519MLKEM768 offered with a key share",
            group_offered && share_sent,
            format!("group={group_offered} share={share_sent} (Chrome 131+ sends both)"),
        );
    } else {
        eprintln!(
            "note X25519MLKEM768 offered: {group_offered} (asserted only for wreq, Chrome 131+)"
        );
    }
    failures
}
