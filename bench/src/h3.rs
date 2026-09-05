//! What HTTP/3 is actually worth against the targets, measured rather than assumed.
//!
//! Two questions, and the first bounds the second. **How many of these sites offer h3 at all?** —
//! that is the ceiling on anything h3 can do here, and it is read from `Alt-Svc` on an ordinary TCP
//! fetch, exactly as `core::altsvc` does at run time. Then, for the ones that do: **does the QUIC
//! engine get the same page, and how does it compare?**
//!
//! This is deliberately not the evasion number. It says whether the transport works and how far it
//! reaches; whether it changes a verdict is `bench evasion --http3`, which is a different question
//! with a much noisier answer.
//!
//! Network, so it is not in `qc`.

use crate::targets::{Set, Target};
use std::sync::Arc;
use std::time::Instant;
use svipall_core::{IdentityProfile, Os, MAX_EMULATED_CHROME};
use svipall_http::{Engine, FetcherConfig, HttpFetcher, HttpRequest};

/// What one target had to say about HTTP/3.
struct Cell {
    name: &'static str,
    /// The `Alt-Svc` port the site advertised, if it advertised one.
    offers: Option<u16>,
    tcp: Option<(u16, usize, f64)>,
    /// Whether each body satisfied the set's expected-text rule, when it has one.
    tcp_ok: Option<bool>,
    h3_ok: Option<bool>,
    h3: Option<(u16, usize, f64)>,
    note: String,
}

fn identity() -> IdentityProfile {
    IdentityProfile::for_major(MAX_EMULATED_CHROME, Os::Windows)
}

fn fetcher_config() -> FetcherConfig {
    let mut cfg = FetcherConfig::new(identity());
    cfg.engine = Engine::Auto;
    cfg.timeout = std::time::Duration::from_secs(20);
    cfg
}

/// A fetcher that refuses, so a page that comes back can only have come over QUIC.
struct Never(IdentityProfile);

#[async_trait::async_trait]
impl HttpFetcher for Never {
    fn engine(&self) -> &'static str {
        "never"
    }
    fn identity(&self) -> &IdentityProfile {
        &self.0
    }
    async fn send(&self, _: HttpRequest) -> anyhow::Result<svipall_http::HttpResponse> {
        anyhow::bail!("fell back to tcp")
    }
}

/// Would the ladder's own verdict rule accept what came back over h3?
///
/// The point of the whole exercise: h3 returning a 200 is not the same as h3 returning *the page*.
/// This applies `hard12`'s rule — the expected text has to be in the body — to the h3 body, so the
/// column says what a caller would actually have got.
fn satisfies(t: &Target, body: &[u8]) -> Option<bool> {
    if t.expect.is_empty() {
        return None;
    }
    let text = String::from_utf8_lossy(body).to_lowercase();
    Some(t.expect.iter().any(|e| text.contains(&e.to_lowercase())))
}

/// One target, measured both ways.
///
/// `h3_first` exists because the order is a confound and not a small one: these vendors score an
/// address, so whichever request goes second is asking a server that has already seen us. A
/// difference that only appears in one order is an ordering artefact, not a transport result.
async fn measure(t: &Target, h3_first: bool) -> Cell {
    let mut cell = Cell {
        name: t.name,
        offers: None,
        tcp: None,
        tcp_ok: None,
        h3_ok: None,
        h3: None,
        note: String::new(),
    };

    let tcp = match svipall_http::build(fetcher_config()) {
        Ok(f) => f,
        Err(e) => {
            cell.note = format!("no tcp engine: {e}");
            return cell;
        }
    };
    let mut req = HttpRequest::get(t.url);
    req.headers = tcp.identity().nav_headers();

    // Whether a site offers h3 is read from `Alt-Svc` on a TCP response, exactly as `core::altsvc`
    // does at run time — so even with `--h3-first` that answer comes from the TCP fetch.
    let run_tcp = |req: HttpRequest| async {
        let started = Instant::now();
        match tcp.send(req).await {
            Ok(r) => Ok((
                r.status,
                r.body.len(),
                started.elapsed().as_secs_f64(),
                r.header("alt-svc")
                    .and_then(svipall_core::altsvc::parse)
                    .map(|(p, _)| p),
                satisfies(t, &r.body),
            )),
            Err(e) => Err(format!("tcp: {e}")),
        }
    };

    if h3_first {
        // Nothing has told us this host speaks h3 yet, so try it and let the attempt answer.
        if let Some(h3) = svipall_http::with_http3(&fetcher_config(), Arc::new(Never(identity()))) {
            let started = Instant::now();
            match h3.send(req.clone()).await {
                Ok(r) if r.http_version == "HTTP/3.0" => {
                    cell.h3_ok = satisfies(t, &r.body);
                    cell.h3 = Some((r.status, r.body.len(), started.elapsed().as_secs_f64()));
                }
                Ok(r) => cell.note = format!("came back over {}", r.http_version),
                Err(e) => cell.note = format!("h3: {e}"),
            }
        } else {
            cell.note = "built without --features http3".into();
        }
        match run_tcp(req).await {
            Ok((s, n, secs, offers, ok)) => {
                cell.tcp = Some((s, n, secs));
                cell.offers = offers;
                cell.tcp_ok = ok;
            }
            Err(e) => cell.note = e,
        }
        return cell;
    }

    match run_tcp(req.clone()).await {
        Ok((s, n, secs, offers, ok)) => {
            cell.tcp = Some((s, n, secs));
            cell.offers = offers;
            cell.tcp_ok = ok;
        }
        Err(e) => {
            cell.note = e;
            return cell;
        }
    }
    if cell.offers.is_none() {
        cell.note = "no Alt-Svc: h3 would never be tried".into();
        return cell;
    }
    let Some(h3) = svipall_http::with_http3(&fetcher_config(), Arc::new(Never(identity()))) else {
        cell.note = "built without --features http3".into();
        return cell;
    };
    let started = Instant::now();
    match h3.send(req).await {
        Ok(r) if r.http_version == "HTTP/3.0" => {
            cell.h3_ok = satisfies(t, &r.body);
            cell.h3 = Some((r.status, r.body.len(), started.elapsed().as_secs_f64()));
        }
        Ok(r) => cell.note = format!("came back over {}", r.http_version),
        Err(e) => cell.note = format!("h3: {e}"),
    }
    cell
}

pub async fn run(set: Set, h3_first: bool) -> usize {
    let targets = set.targets();
    eprintln!(
        "\n== {} over HTTP/3, {} first ==\n{:<20} {:>6}  {:>26}  {:>26}  note",
        set.name(),
        if h3_first { "h3" } else { "tcp" },
        "target",
        "offers",
        "tcp",
        "h3",
    );

    let mut cells = Vec::new();
    for t in targets {
        let c = measure(t, h3_first).await;
        let show = |x: &Option<(u16, usize, f64)>, ok: &Option<bool>| match x {
            Some((s, n, secs)) => format!(
                "{s} {n}b {secs:.1}s{}",
                match ok {
                    Some(true) => " OK",
                    Some(false) => " no-text",
                    None => "",
                }
            ),
            None => "—".into(),
        };
        eprintln!(
            "{:<20} {:>6}  {:>26}  {:>26}  {}",
            c.name,
            c.offers
                .map(|p| p.to_string())
                .unwrap_or_else(|| "no".into()),
            show(&c.tcp, &c.tcp_ok),
            show(&c.h3, &c.h3_ok),
            c.note
        );
        cells.push(c);
    }

    let offered = cells.iter().filter(|c| c.offers.is_some()).count();
    let carried = cells.iter().filter(|c| c.h3.is_some()).count();
    eprintln!(
        "\n{}/{} targets advertise h3; {carried} of those were fetched over it.",
        offered,
        targets.len()
    );
    println!(
        "{}",
        serde_json::json!({
            "set": set.name(),
            "order": if h3_first { "h3-first" } else { "tcp-first" },
            "targets": targets.len(),
            "offer_h3": offered,
            "fetched_over_h3": carried,
            "cells": cells.iter().map(|c| serde_json::json!({
                "target": c.name,
                "offers_h3": c.offers,
                "tcp": c.tcp.map(|(s, n, t)| serde_json::json!({"status": s, "bytes": n, "secs": t})),
                "h3": c.h3.map(|(s, n, t)| serde_json::json!({"status": s, "bytes": n, "secs": t})),
                "tcp_has_expected_text": c.tcp_ok,
                "h3_has_expected_text": c.h3_ok,
                "note": c.note,
            })).collect::<Vec<_>>(),
        })
    );
    // Measured, not asserted: this mode never fails a build.
    0
}
