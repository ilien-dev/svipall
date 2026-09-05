//! What a site says about HTTP/3, remembered even though nothing acts on it yet.
//!
//! Chrome never opens a first connection over QUIC; it learns from `Alt-Svc` that the origin
//! offers `h3` and switches on a later visit. svipall does not speak HTTP/3 — the engine that
//! gives it Chrome's TLS shape cannot produce Chrome's QUIC handshake, and a QUIC handshake that
//! is not Chrome's is worse than none (see the README) — but which sites offered it is worth
//! knowing: it is the list a future engine would have to serve, and the list a benchmark of that
//! engine would run against. One row per domain, expiring when the site said it would.

use crate::cache::Store;

pub const PREFIX: &str = "altsvc/";

/// The `h3` alternative in an `Alt-Svc` header, if any: `(port, max_age_secs)`. The default
/// max-age is the one the specification gives, twenty-four hours. `clear` withdraws every
/// alternative.
pub fn parse(header: &str) -> Option<(u16, u64)> {
    if header.trim().eq_ignore_ascii_case("clear") {
        return None;
    }
    for alt in header.split(',') {
        let mut params = alt.split(';').map(str::trim);
        let first = params.next()?;
        let (proto, authority) = first.split_once('=')?;
        // `h3`, `h3-29` and the like: the versioned drafts are still QUIC.
        if !(proto.eq_ignore_ascii_case("h3") || proto.to_ascii_lowercase().starts_with("h3-")) {
            continue;
        }
        let authority = authority.trim_matches('"');
        let port: u16 = authority
            .rsplit(':')
            .next()
            .and_then(|p| p.parse().ok())
            .unwrap_or(443);
        let ma = params
            .find_map(|p| p.strip_prefix("ma=").and_then(|v| v.trim().parse().ok()))
            .unwrap_or(86_400);
        return Some((port, ma));
    }
    None
}

/// Remember that `domain` offered h3 until `now + max_age`.
pub fn remember(store: &Store, domain: &str, port: u16, max_age: u64, now: i64) {
    let until = now.saturating_add(max_age.min(u64::from(u32::MAX)) as i64);
    let _ = store.kv_set(&format!("{PREFIX}{domain}"), &format!("{port} {until}"));
}

/// Domains whose offer has not expired, with the port they named.
pub fn offered(store: &Store, now: i64) -> Vec<(String, u16)> {
    store
        .kv_list(PREFIX)
        .into_iter()
        .filter_map(|(k, v)| {
            let domain = k.strip_prefix(PREFIX)?.to_string();
            let (port, until) = v.split_once(' ')?;
            let until: i64 = until.parse().ok()?;
            (until > now).then_some((domain, port.parse().ok()?))
        })
        .collect()
}

/// What this machine has learned about actually speaking h3 to a domain, as opposed to what the
/// domain advertised.
///
/// The two are different facts and both are needed. An advertisement is the *site* saying it
/// offers HTTP/3; this is *us* saying whether it worked from here. A site can advertise h3 while
/// the network in between drops UDP on the floor, and without this that costs a wasted attempt on
/// every fetch for ever.
const RESULT: &str = "h3ok/";

/// How long a failure is believed. A dropped UDP port is usually the network and not the site — a
/// laptop moves, a firewall changes, a captive portal ends — so "no" has to expire or one bad
/// café decides this machine never speaks h3 again.
pub const RETRY_FAILED_AFTER: i64 = 6 * 3600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Never attempted here, or the last failure is old enough to be worth re-testing.
    Untried,
    /// h3 carried a page from this domain. Ask for it first and give it a full budget.
    Works,
    /// It advertised h3 and did not deliver over it. Do not pay for that again yet.
    Fails,
}

/// Whether this one domain has a live offer. A direct lookup rather than `offered`, which builds
/// the whole list: this runs on every fetch, and its cost should be one row rather than all of
/// them.
pub fn offers(store: &Store, domain: &str, now: i64) -> bool {
    store
        .kv_get(&format!("{PREFIX}{domain}"))
        .and_then(|v| v.split_once(' ').and_then(|(_, u)| u.parse::<i64>().ok()))
        .is_some_and(|until| until > now)
}

pub fn verdict(store: &Store, domain: &str, now: i64) -> Verdict {
    match store.kv_get(&format!("{RESULT}{domain}")).as_deref() {
        Some("1") => Verdict::Works,
        Some(v) => match v.strip_prefix("0 ").and_then(|t| t.parse::<i64>().ok()) {
            Some(at) if now - at < RETRY_FAILED_AFTER => Verdict::Fails,
            _ => Verdict::Untried,
        },
        None => Verdict::Untried,
    }
}

/// Record what happened. A success is kept without a timestamp — it stops being true when the
/// advertisement expires, and that is already tracked one row over.
pub fn remember_result(store: &Store, domain: &str, worked: bool, now: i64) {
    let value = if worked {
        "1".to_string()
    } else {
        format!("0 {now}")
    };
    let _ = store.kv_set(&format!("{RESULT}{domain}"), &value);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_h3_alternative_is_read_with_its_port_and_lifetime() {
        assert_eq!(parse(r#"h3=":443"; ma=86400"#), Some((443, 86_400)));
        assert_eq!(
            parse(r#"h3-29=":8443"; ma=60, h2=":443""#),
            Some((8443, 60))
        );
        assert_eq!(parse(r#"h2=":443"; ma=60"#), None, "not QUIC");
        assert_eq!(
            parse(r#"h3=":443""#),
            Some((443, 86_400)),
            "default lifetime"
        );
        assert_eq!(parse("clear"), None);
    }

    #[test]
    fn an_offer_is_remembered_until_it_expires() {
        let store = Store::open_memory().unwrap();
        remember(&store, "a.example", 443, 100, 1_000);
        remember(&store, "b.example", 8443, 10, 1_000);
        let live = offered(&store, 1_050);
        assert_eq!(live, vec![("a.example".to_string(), 443)]);
        assert!(offered(&store, 2_000).is_empty());
    }

    #[test]
    fn one_domain_is_asked_about_directly_rather_than_by_listing_every_offer() {
        // `offers` exists because the alternative — listing every row and scanning it — runs on
        // every fetch of every domain, and grows with the size of the cache rather than staying
        // the cost of one lookup.
        let store = Store::open_memory().unwrap();
        remember(&store, "a.example", 443, 100, 1_000);
        assert!(offers(&store, "a.example", 1_050));
        assert!(!offers(&store, "b.example", 1_050), "never offered");
        assert!(!offers(&store, "a.example", 2_000), "expired");
    }

    #[test]
    fn a_domain_that_answered_over_quic_is_remembered_so_the_next_page_goes_straight_there() {
        let store = Store::open_memory().unwrap();
        assert_eq!(verdict(&store, "a.example", 1_000), Verdict::Untried);
        remember_result(&store, "a.example", true, 1_000);
        assert_eq!(verdict(&store, "a.example", 1_050), Verdict::Works);
    }

    #[test]
    fn a_domain_that_did_not_answer_over_quic_is_not_asked_again_every_time() {
        // The cost this defends against: a site that advertises h3 and does not answer over it
        // would otherwise be probed on every single fetch, for ever, paying a whole round trip
        // each time to learn what the last one already knew.
        let store = Store::open_memory().unwrap();
        remember_result(&store, "a.example", false, 1_000);
        assert_eq!(verdict(&store, "a.example", 1_050), Verdict::Fails);
    }

    #[test]
    fn a_failure_is_forgotten_after_a_while_because_a_network_is_not_forever() {
        // A dropped UDP port is usually the network, not the site: a laptop moves, a firewall
        // changes, a captive portal ends. Remembering "no" for ever would mean one bad café
        // decides this machine never speaks h3 again.
        let store = Store::open_memory().unwrap();
        remember_result(&store, "a.example", false, 1_000);
        assert_eq!(
            verdict(&store, "a.example", 1_000 + RETRY_FAILED_AFTER - 1),
            Verdict::Fails
        );
        assert_eq!(
            verdict(&store, "a.example", 1_000 + RETRY_FAILED_AFTER + 1),
            Verdict::Untried
        );
    }
}
