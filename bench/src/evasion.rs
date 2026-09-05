//! How often svipall actually gets the page, and how fast the cache makes the second try.
//!
//! Run through `SvipallServer` itself, so what is measured is the product: the ladder, the identity,
//! the cache, the throttle. The old benchmark used a bare HTTP client and therefore measured none
//! of them.
//!
//! Two target sets (see `targets.rs`), several runs each, targets in a different order every run,
//! and the number reported is the median with its range — one run of twelve sites is inside the
//! noise this benchmark has already shown between two executions minutes apart.

use crate::targets::{public_verdict, Set, Target, Verdict};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Instant;
use svipall_mcp::server::SvipallServer;
use svipall_mcp::tools::WebFetchParams;

/// The server every run drives.
///
/// `http3` needs two things upstream does not need: a page cache, because `Alt-Svc` is where the
/// decision to speak h3 is remembered, and the config flag. An in-memory store keeps the run
/// self-contained — nothing here should read or write the operator's own cache.
fn server(http3: bool) -> SvipallServer {
    // `Config::default()` is used rather than the operator's `config.toml`, so the one knob a
    // measurement of held pages has to be able to move is read from the environment. Zero is the
    // control arm — hold nothing — and it is what makes the comparison a comparison rather than a
    // before-and-after taken hours apart.
    let warm_keep_max = std::env::var("SVIPALL_WARM_KEEP")
        .ok()
        .and_then(|v| v.trim().parse().ok());
    let cfg = svipall_core::Config {
        http3,
        warm_keep_max: warm_keep_max.unwrap_or(svipall_core::Config::default().warm_keep_max),
        ..Default::default()
    };
    // The same kind of store either way. `http3` needs one at all — `Alt-Svc` is where the
    // decision to speak QUIC is remembered — and giving one arm of a comparison the operator's
    // real cache and the other a fresh one would measure the caches, not the transports.
    let store = svipall_core::cache::Store::open_memory().expect("in-memory store");
    SvipallServer::with_store(None, cfg, None, Some(std::sync::Arc::new(store)))
}

fn text_of(v: &Value) -> String {
    v.get("content")
        .and_then(|c| c.as_str())
        .unwrap_or_default()
        .to_string()
}

/// A small deterministic shuffle so a run never hits targets in published order. Seeded from the
/// clock: the point is that two runs differ, not that the order is reproducible.
fn shuffled(targets: &[Target], seed: u64) -> Vec<&Target> {
    let mut v: Vec<&Target> = targets.iter().collect();
    let mut x = seed | 1;
    for i in (1..v.len()).rev() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        v.swap(i, (x % (i as u64 + 1)) as usize);
    }
    v
}

/// One attempt, as it goes into the baseline.
///
/// `wall_vendor` and `warm` are why a failed cell is readable at all: before them a Kasada block
/// and an app that simply never rendered were the same string, and a warm wait that ran out looked
/// exactly like one that was refused.
struct Cell {
    passed: bool,
    verdict: Verdict,
    tier: String,
    secs: f32,
    status: Option<u16>,
    blocked_reason: Value,
    wall_kind: Value,
    wall_vendor: Value,
    wall_evidence: Value,
    warm: Value,
}

async fn fetch_cell(s: &SvipallServer, t: &Target, html: bool, exit: Option<&str>) -> Cell {
    let t0 = Instant::now();
    let out = s
        .fetch_json(WebFetchParams {
            url: t.url.to_string(),
            timeout: Some(90_000),
            // Always a real fetch: a cached hit would make the numbers meaningless.
            cache: Some("bypass".into()),
            // The public rule scans markup for challenge scripts, so it needs the HTML.
            extraction: html.then(|| "html".to_string()),
            // The whole run through one operator-supplied exit. The published baseline is the
            // un-proxied single residential address, which the README names as its own ceiling;
            // this is how that ceiling gets measured rather than asserted.
            proxy: exit.map(str::to_string),
            ..Default::default()
        })
        .await;
    let secs = t0.elapsed().as_secs_f32();
    let v = &out.value;
    let body = text_of(v);
    let tier = v["tier_used"].as_str().unwrap_or("-").to_string();
    let title = v["title"].as_str().unwrap_or_default();
    let status = v["status"].as_u64().map(|s| s as u16);
    let verdict = public_verdict(status, title, &body);
    // A 200 is not success: the expected text has to be there and the classifier must not have
    // called it a wall. (It used to also require `body.len() > 200`, which scored example.com —
    // 167 characters of markdown, fetched perfectly — as a failure.)
    let passed = if t.expect.is_empty() {
        verdict == Verdict::Ok
    } else {
        let lower = body.to_lowercase();
        t.expect.iter().any(|e| lower.contains(&e.to_lowercase()))
            && v.get("blocked_reason").is_none()
    };
    Cell {
        passed,
        verdict,
        tier,
        secs,
        status,
        blocked_reason: v["blocked_reason"].clone(),
        wall_kind: v["wall_kind"].clone(),
        wall_vendor: v["wall_vendor"].clone(),
        wall_evidence: v["wall_evidence"].clone(),
        warm: v["warm"].clone(),
    }
}

fn median(mut xs: Vec<usize>) -> usize {
    xs.sort_unstable();
    xs[xs.len() / 2]
}

/// Targets whose address has already spent past the point where svipall starts slowing down, with
/// the seconds until each falls back under the line.
///
/// A pure function so the gate can be tested without a network: `bench/` has no test binary, and a
/// decision buried in `run`'s loop is a decision nothing can reach.
pub fn over_the_line(targets: &[Target], exit: Option<&str>) -> Vec<(&'static str, u64)> {
    let budget = svipall_core::reputation::budget();
    if budget <= 0.0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for t in targets {
        let domain = svipall_core::domain_from_url(t.url);
        let spent = svipall_core::reputation::spent(&domain, exit);
        let line = svipall_core::reputation::SOFT_LINE * budget;
        if spent > line {
            // Decay is exponential, so the wait is the half-life times log2 of the overshoot.
            let secs = (svipall_core::reputation::half_life_secs() as f32 * (spent / line).log2())
                .ceil()
                .max(0.0) as u64;
            out.push((t.name, secs));
        }
    }
    out
}

pub async fn run(
    set: Set,
    runs: usize,
    exit: Option<&str>,
    http3: bool,
    ignore_budget: bool,
    repeat: usize,
) -> usize {
    let targets = set.targets();
    // Checked once, before the first run, and never between runs.
    //
    // A list that starts already being slowed down produces numbers that are not comparable with
    // the baseline, which is the measurement defect this benchmark's own history records: the
    // lists were run against one address several times in a day and a cell went from passing to
    // failing. Cooldowns are cleared before every run because a cooldown is the site's word about
    // the previous run and clearing it makes the three comparable; the spend is this address's own
    // and clearing it would rebuild exactly that defect.
    //
    // Deliberately not checked *between* runs: the published procedure is a median of three, and a
    // gate that could fire halfway would change the denominator of the number it is protecting.
    //
    // Nor does this contradict "reported, not asserted" at the bottom of this function. The reason
    // written there is that the walls on the other end change without warning; this is our own
    // state, which we control and can explain to the second.
    let spent = over_the_line(targets, exit);
    if !spent.is_empty() {
        let worst = spent.iter().map(|(_, s)| *s).max().unwrap_or(0);
        eprintln!(
            "refusing to measure {}: this address has already spent its standing with {}",
            set.name(),
            spent
                .iter()
                .map(|(n, s)| format!("{n} ({}h{:02}m)", s / 3600, (s % 3600) / 60))
                .collect::<Vec<_>>()
                .join(", ")
        );
        eprintln!(
            "the numbers would not be comparable with the baseline. Enough standing in about \
             {}h{:02}m, or pass --exit URL to measure through another address.",
            worst / 3600,
            (worst % 3600) / 60
        );
        if !ignore_budget {
            eprintln!("--ignore-budget runs anyway and marks the run as forced.");
            return 1;
        }
        eprintln!("--ignore-budget given: running anyway, and the run is marked forced.");
    }
    let forced = ignore_budget && !spent.is_empty();
    let s = server(http3);
    let total = targets.len();
    let runs = runs.max(1);
    let html = set == Set::Public31;
    let mut per_run = Vec::new();
    let mut by_target: BTreeMap<&str, Vec<Value>> = BTreeMap::new();
    let mut by_tier: BTreeMap<String, usize> = Default::default();
    // Cost, not just success. A caller pulling a few pages cares about seconds; a caller pulling
    // thousands cares about how many of them opened a browser. Both come from the same run.
    let mut secs_per_run: Vec<f32> = Vec::new();
    let mut tier_all: BTreeMap<String, usize> = Default::default();

    // `Alt-Svc` is how a site says it speaks h3, and it arrives on a TCP response — so with no
    // priming, run 1 would be entirely TCP and only runs 2 and 3 would use QUIC. One cheap fetch
    // per target first makes all three runs measure the same thing, and it is exactly what a
    // machine that has been running for a day already has in its cache.
    if http3 {
        eprintln!("priming Alt-Svc for {} targets...", targets.len());
        for t in targets {
            let _ = s
                .fetch_json(WebFetchParams {
                    url: t.url.to_string(),
                    timeout: Some(20_000),
                    cache: Some("bypass".into()),
                    proxy: exit.map(str::to_string),
                    ..Default::default()
                })
                .await;
        }
    }

    for run in 0..runs {
        // A run that blocks a domain puts it on a 15-minute cooldown, and the next run would skip it
        // without ever making a request — the second run of the day scores better than the first for
        // a reason that has nothing to do with evasion. Start from a clean slate every time.
        //
        // The reputation spend is deliberately *not* cleared alongside it. A cooldown is the
        // site's word about the previous run, and clearing it is what makes three runs comparable.
        // The spend is what this address has done to that host, it is the thing these numbers are
        // supposed to respect, and clearing it would put the benchmark back in a position to burn
        // an address exactly as this file's own history records it doing.
        for t in targets {
            svipall_core::clear_cooldown(&svipall_core::domain_from_url(t.url));
        }
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(7)
            ^ (run as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        eprintln!(
            "\n== {} run {}/{} ==\n{:<20} {:<20} {:>8} {:>8}  {:<8} result",
            set.name(),
            run + 1,
            runs,
            "target",
            "wall",
            "tier",
            "secs",
            "verdict"
        );
        let mut passed = 0usize;
        let mut secs = 0f32;
        for t in shuffled(targets, seed) {
            let spent_before =
                svipall_core::reputation::spent(&svipall_core::domain_from_url(t.url), exit);
            // Back-to-back fetches of the same target. Only the last one is scored, and at
            // `--repeat 1` — the published default — this is exactly one fetch and the record is
            // unchanged. Above one, what is scored is the *second* fetch of a domain, which is the
            // only place a held page or a reused clearance can show up at all.
            let mut c = fetch_cell(&s, t, html, exit).await;
            for _ in 1..repeat.max(1) {
                c = fetch_cell(&s, t, html, exit).await;
            }
            secs += c.secs;
            *tier_all.entry(c.tier.clone()).or_default() += 1;
            if c.passed {
                passed += 1;
                *by_tier.entry(c.tier.clone()).or_default() += 1;
            }
            eprintln!(
                "{:<20} {:<20} {:>8} {:>8.1}  {:<8} {}",
                t.name,
                t.wall,
                c.tier,
                c.secs,
                c.verdict.name(),
                if c.passed {
                    "PASS".to_string()
                } else {
                    // The vendor, when the wire named one. Without it a block by the
                    // proof-of-work vendor and an app that never rendered read identically, and
                    // the text file was unreadable without opening the JSON beside it.
                    let vendor = c
                        .wall_vendor
                        .as_str()
                        .map(|v| format!(" [{v}]"))
                        .unwrap_or_default();
                    format!(
                        "FAIL {}{vendor}",
                        c.blocked_reason.as_str().unwrap_or("no expected text")
                    )
                }
            );
            by_target
                .entry(t.name)
                .or_default()
                .push(serde_json::json!({
                    "run": run + 1, "url": t.url, "wall": t.wall, "tier": c.tier, "secs": c.secs,
                    "passed": c.passed, "verdict": c.verdict.name(),
                    "status": c.status,
                    "blocked_reason": c.blocked_reason,
                    // Which wall, named by the vendor's own endpoint, and on what evidence. A
                    // failed cell used to say only that something went wrong.
                    "wall_kind": c.wall_kind,
                    "wall_vendor": c.wall_vendor,
                    "wall_evidence": c.wall_evidence,
                    // Why the warm wait stopped, and whether re-earning a lapsed clearance
                    // changed anything — the question the baseline could never answer.
                    "warm": c.warm,
                    // What this address had already spent with this host when the cell was
                    // measured. Every published number now carries the state it was taken in,
                    // which is the one thing the round that lost a target could not say.
                    "spent_before": (spent_before * 10.0).round() / 10.0,
                }));
        }
        eprintln!("run {}: {passed}/{total} in {secs:.1}s", run + 1);
        per_run.push(passed);
        secs_per_run.push(secs);
    }

    let med = median(per_run.clone());
    let min = *per_run.iter().min().unwrap_or(&0);
    let max = *per_run.iter().max().unwrap_or(&0);
    eprintln!(
        "\n{} — median {med}/{total} (range {min}..{max} over {runs} run{}) — resolved by tier: {:?}",
        set.name(),
        if runs == 1 { "" } else { "s" },
        by_tier
    );
    let mut secs_sorted = secs_per_run.clone();
    secs_sorted.sort_by(|a, b| a.total_cmp(b));
    let med_secs = secs_sorted[secs_sorted.len() / 2];
    // The other half of the answer. Two builds that get the same pages are not the same tool if
    // one of them opened a browser for every one of them.
    eprintln!(
        "cost — median {med_secs:.1}s per run of {total}, {:.1}s per page; every cell by tier: {tier_all:?}",
        med_secs / total as f32
    );
    if set == Set::Public31 {
        eprintln!(
            "note: public31 is scored by the public benchmark's verdict rule; hard12 by expected \
             text. The two numbers are not interchangeable."
        );
    }
    println!(
        "{}",
        serde_json::json!({
            "set": set.name(), "runs": runs, "total": total, "exit": exit, "http3": http3,
            "secs_per_run": secs_per_run, "median_secs": med_secs, "tier_all": tier_all,
            "per_run": per_run, "median": med, "min": min, "max": max,
            "by_tier": by_tier, "by_target": by_target,
            "budget": svipall_core::reputation::budget(),
            // A forced run must never be publishable as if it were an ordinary one.
            "forced_over_budget": forced,
        })
    );
    // Reported, not asserted: the walls on the other end change without warning, and a red build
    // because a third party shipped a new rule would train everyone to ignore it.
    0
}

pub async fn run_cache() -> usize {
    let s = server(false);
    let url = "https://en.wikipedia.org/wiki/Rust_(programming_language)";

    let cold_start = Instant::now();
    let cold = s
        .fetch_json(WebFetchParams {
            url: url.into(),
            cache: Some("write".into()),
            ..Default::default()
        })
        .await;
    let cold_secs = cold_start.elapsed().as_secs_f32();
    if text_of(&cold.value).len() < 200 {
        eprintln!("FAIL could not fetch the page at all; cache numbers would be meaningless");
        return 1;
    }

    let warm_start = Instant::now();
    let warm = s
        .fetch_json(WebFetchParams {
            url: url.into(),
            cache: Some("auto".into()),
            ..Default::default()
        })
        .await;
    let warm_secs = warm_start.elapsed().as_secs_f32();

    let from_cache = warm.value["from_cache"] == Value::Bool(true);
    let speedup = cold_secs / warm_secs.max(0.000_001);
    eprintln!("cold  {cold_secs:.3}s");
    eprintln!("warm  {warm_secs:.3}s  (from_cache={from_cache})");
    eprintln!("speedup {speedup:.0}x");

    let mut failures = 0;
    if !from_cache {
        eprintln!("FAIL the second fetch did not come from the cache");
        failures += 1;
    }
    if speedup < 20.0 {
        eprintln!("FAIL a cached read should be at least 20x faster, was {speedup:.1}x");
        failures += 1;
    }
    if failures == 0 {
        eprintln!("ok   cache is doing its job");
    }
    failures
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::targets::Set;

    /// The gate the benchmark now stands behind, exercised without a network.
    ///
    /// Stated as a *difference* rather than an absolute. `over_the_line` reads this machine's real
    /// ledger, and running the benchmark — which is what `baseline/README.md` tells you to do — puts
    /// real spend in it for hours. A test that demanded an empty ledger failed for anyone who had
    /// just followed those instructions, which is the test being wrong about the world rather than
    /// the world being wrong.
    #[test]
    fn a_list_that_starts_over_the_line_is_refused_rather_than_measured() {
        let targets = Set::Hard12.targets();
        let before: Vec<&str> = over_the_line(targets, None)
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        let t = targets
            .iter()
            .find(|t| !before.contains(&t.name))
            .expect("every target on this list is already over the line");
        let domain = svipall_core::domain_from_url(t.url);
        // Past the soft line, which is where the numbers stop being comparable with the baseline.
        svipall_core::reputation::add(
            &domain,
            None,
            svipall_core::reputation::budget() * (svipall_core::reputation::SOFT_LINE + 0.2),
        );
        let newly: Vec<(&str, u64)> = over_the_line(targets, None)
            .into_iter()
            .filter(|(n, _)| !before.contains(n))
            .collect();
        assert_eq!(newly.len(), 1, "{newly:?}");
        assert_eq!(newly[0].0, t.name);
        assert!(
            newly[0].1 > 0,
            "it must say how long until the list is measurable again"
        );
        assert!(
            over_the_line(targets, Some("http://elsewhere:1")).is_empty(),
            "another exit is another address, and has spent nothing here"
        );
        svipall_core::reputation::clear(&domain);
    }
}
