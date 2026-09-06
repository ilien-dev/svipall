//! Measurement, not vibes.
//!
//! The previous benchmark did not exercise the product: it built a bare `reqwest` client and made
//! sequential GETs, so it measured neither the ladder, nor the cache, nor the fingerprint. Its
//! `all` mode ran nothing at all.
//!
//! Five modes:
//!   * `micro`  — CPU budgets with no network. `--assert` fails the command if one is exceeded.
//!   * `tells`  — what a detector reads off the session, against a page served on loopback. No
//!     network, so `--assert` belongs in the gate.
//!   * `cache`  — cold versus warm fetch through the real ladder.
//!   * `fingerprint` — what public detectors see. This is the regression guard for stealth work.
//!   * `evasion` — success rate against sites with known walls, by tier.
//!
//! Only `micro` and `tells` are safe to run in CI; the rest reach the network and are run by hand.

use std::time::Instant;

mod comparison;
mod daniel;
mod evasion;
mod extraction;
mod fingerprint;
mod h3;
#[cfg(feature = "http3")]
mod h3ref;
mod micro;
mod summary;
mod targets;
mod teco;
mod tells;
mod template;
mod wcxb;

fn usage() -> ! {
    eprintln!(
        "usage: svipall-bench <micro [--assert] | tells [--assert] | cache | extract [--corpus DIR] | fingerprint [--engine E] | h3-ref | h3 [--set S] | evasion [--set S] [--runs N] [--exit URL] [--repeat N]>\n\
         \n\
         micro        CPU budgets, no network (use --assert in CI)\n\
         tells        automation tells a page can read, probed at every browser tier against a\n\
                      document served on loopback. No network (use --assert in CI)\n\
         compare      record every first/returning visit through the configured product\n\
                      --set S --repeat N --seed N --label NAME --timeout MS\n\
         summarize    aggregate saved comparisons offline: --dir EXPERIMENT_DIRECTORY\n\
         cache        cold vs warm fetch through the ladder\n\
         extract      main-content extraction quality against the SIGIR-23 gold standard, beside\n\
                      the study's own published extractions (ROUGE-LSum), and optionally against\n\
                      WCXB, which is labelled by page type (word-level F1)\n\
                      --corpus DIR    where scripts/fetch-extraction-corpus.sh put it\n\
                      --wcxb DIR      where scripts/fetch-wcxb.sh put it\n\
                      --daniel DIR    where scripts/fetch-daniel.sh put it (five languages)\n\
                      --teco DIR      where scripts/fetch-teco.sh put a category. The only\n\
                                      corpus that ships each page's siblings, so the only\n\
                                      one that can score the cross-page template.\n\
         fingerprint  what tls.peet.ws, sannysoft and incolumitas report, plus identity coherence\n\
                      --engine chrome  coherence only, offline: every identity checked against itself\n\
                      --engine firefox coherence of the Firefox identities alone\n\
         h3-ref       what Chrome sends as HTTP/3 SETTINGS, read off a loopback QUIC server this\n\
                      process runs. Needs --features http3 and the provisioned browser, and no\n\
                      network\n\
         h3           how many targets advertise HTTP/3 and whether the QUIC engine fetches them
\n                      --set S         hard12 | public31 | vendors8 (default hard12)
\n         evasion      success rate against a target set, median of N runs (default hard12, 3)\n\
                      --set hard12    svipall's own walls, scored by expected text\n\
                      --set public31  the public May 2026 list, scored by its own verdict rule\n\
                      --set vendors8  two targets each behind Kasada, Akamai, DataDome, Cloudflare\n\
                      --http3         speak HTTP/3 to targets that advertised it (needs the
\n                                      http3 feature); Alt-Svc is primed first so every run
\n                                      measures the same thing
\n                      --exit URL      run the whole set through one proxy, to separate\n\
                                      \"svipall cannot\" from \"this address cannot\"\n\
                      --repeat N      fetch each target N times back to back and score the last.\n\
                                      1 (the default) is the published shape; above 1 is how a\n\
                                      change that only pays off on a second fetch gets a number\n\
                      --ignore-budget measure even though this address has already spent its\n\
                                      standing with a target; the run is marked forced\n\
\n                    SVIPALL_WARM_KEEP=N overrides how many cleared pages may be held between\n\
                                      fetches; 0 holds none, which is the control arm"
    );
    std::process::exit(2)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // A measurement tool with no way to see what the product decided is a measurement tool that
    // can only report the number, never explain it. Off unless `RUST_LOG` asks, so the tables
    // stay clean.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("error")),
        )
        .with_writer(std::io::stderr)
        .try_init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str).unwrap_or("micro");
    let assert = args.iter().any(|a| a == "--assert");

    let started = Instant::now();
    let failures = match mode {
        "compare" => comparison::run(&args).await?,
        "summarize" => summary::run(std::path::Path::new(
            &flag(&args, "--dir").unwrap_or_else(|| "bench/experiments/local-20260905".into()),
        ))?,
        "micro" => micro::run(assert),
        "tells" => tells::run(assert).await,
        "cache" => evasion::run_cache().await,
        // The coherence linter runs with no network: it checks the identities svipall would wear
        // against themselves, and fails the build on a contradiction. `--engine firefox` checks
        // the Firefox identities too. The live browser/TLS checks run without an engine flag.
        "fingerprint" => {
            let coherence = fingerprint::coherence(flag(&args, "--engine").as_deref());
            match flag(&args, "--engine").as_deref() {
                // A pure coherence pass, offline, for CI and for the Firefox identities the live
                // checks cannot exercise without a patched build.
                Some(_) => coherence,
                None => coherence + fingerprint::run().await,
            }
        }
        // Does HTTP/3 reach these targets at all, and how far? Not the evasion number: this asks
        // whether the transport works and how many sites even advertise it, which is the ceiling
        // on anything the evasion number could show.
        "h3" => {
            let set = match flag(&args, "--set") {
                Some(s) => targets::Set::parse(&s).unwrap_or_else(|| {
                    eprintln!("unknown set {s:?}; use hard12, public31 or vendors8");
                    usage()
                }),
                None => targets::Set::Hard12,
            };
            h3::run(set, args.iter().any(|a| a == "--h3-first")).await
        }
        // What Chrome actually sends as HTTP/3 SETTINGS. Needs a browser and a loopback
        // socket, no network, and it is the reference `svipall-quic`'s offline test asserts
        // against. Not in the gate: it starts a browser.
        #[cfg(feature = "http3")]
        "h3-ref" => match h3ref::run() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("h3-ref: {e:#}");
                1
            }
        },
        #[cfg(not(feature = "http3"))]
        "h3-ref" => {
            eprintln!("h3-ref needs the http3 feature: cargo run -p svipall-bench --release --features http3 -- h3-ref");
            1
        }
        "evasion" => {
            let set = match flag(&args, "--set") {
                Some(s) => targets::Set::parse(&s).unwrap_or_else(|| {
                    eprintln!("unknown set {s:?}; use hard12, public31 or vendors8");
                    usage()
                }),
                None => targets::Set::Hard12,
            };
            let runs = flag(&args, "--runs")
                .and_then(|r| r.parse().ok())
                .unwrap_or(3);
            // The same targets through an operator-supplied exit. Publishing both columns is the
            // only honest way to separate "svipall cannot do this" from "this address cannot".
            let exit = flag(&args, "--exit");
            // How many times each target is fetched, back to back, inside a run. One is the
            // published shape and must stay the default: at one, the JSON is byte-identical to
            // what the baseline holds, so it stays comparable. More than one is how a change that
            // only pays off on the *second* fetch — a held page, a reused clearance — gets a
            // number at all.
            let repeat = flag(&args, "--repeat")
                .and_then(|r| r.parse().ok())
                .unwrap_or(1);
            evasion::run(
                set,
                runs,
                exit.as_deref(),
                args.iter().any(|a| a == "--http3"),
                args.iter().any(|a| a == "--ignore-budget"),
                repeat,
            )
            .await
        }
        // Extraction quality, against whichever corpora are on disk. Neither is in the repository
        // and neither is in the gate: they are hundreds of megabytes of other people's web pages.
        "extract" => {
            let corpus = flag(&args, "--corpus").unwrap_or_else(|| "extraction-corpus".into());
            let mut failures = extraction::run(std::path::Path::new(&corpus), assert);
            // WCXB is optional and asked for by name. Without it the run still reports the
            // SIGIR-23 numbers; with it, it also reports what those numbers hide, which is every
            // page that is not an article.
            if let Some(w) = flag(&args, "--wcxb") {
                failures += wcxb::run(std::path::Path::new(&w), assert);
            }
            if let Some(d) = flag(&args, "--daniel") {
                failures += daniel::run(std::path::Path::new(&d), assert);
            }
            if let Some(t) = flag(&args, "--teco") {
                failures += teco::run(std::path::Path::new(&t), assert);
            }
            failures
        }
        "-h" | "--help" | "help" => usage(),
        other => {
            eprintln!("unknown mode {other:?}");
            usage()
        }
    };

    eprintln!(
        "\n{mode} finished in {:.1}s",
        started.elapsed().as_secs_f32()
    );
    if failures > 0 {
        eprintln!("{failures} check(s) failed");
        std::process::exit(1);
    }
    // Explicit, and for the same reason the failure path is: a mode that opened browsers leaves the
    // runtime with children to reap, and a benchmark that has printed its number should not sit
    // there holding its own binary open while the next `cargo build` tries to replace it.
    std::io::Write::flush(&mut std::io::stdout()).ok();
    std::process::exit(0);
}

/// `--name value` or `--name=value`.
fn flag(args: &[String], name: &str) -> Option<String> {
    let eq = format!("{name}=");
    args.iter().enumerate().find_map(|(i, a)| {
        if a == name {
            args.get(i + 1).cloned()
        } else {
            a.strip_prefix(&eq).map(str::to_string)
        }
    })
}
