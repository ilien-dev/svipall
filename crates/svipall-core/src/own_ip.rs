//! This machine's public address, kept out of what a caller reads.
//!
//! A wall that prints "Your IP: 203.0.113.7" puts the machine's public address into the content,
//! and from there into a transcript that may be pasted anywhere. The address is replaced by a
//! label; the page is otherwise returned as it came, and the response says how many it replaced.
//!
//! The address is never looked up. Nothing here asks a third-party service "what is my IP": it is
//! learned only from what a page already said, where the page itself labels it as the visitor's,
//! and only when the request left without a proxy, so the address it names is this machine's and
//! not an exit's.

use regex::Regex;
use std::net::IpAddr;
use std::sync::{LazyLock, Mutex};

/// What the address is replaced by.
pub const LABEL: &str = "[own IP]";

/// How many addresses are remembered. A home connection gets a new one now and then, and a
/// laptop moves between networks; the oldest are forgotten first.
const KEEP: usize = 4;

static IPV4: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[^0-9.])((?:\d{1,3}\.){3}\d{1,3})(?:$|[^0-9.])").unwrap());
static IPV6: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|[^0-9a-f:])([0-9a-f]{0,4}(?::[0-9a-f]{0,4}){2,7})").unwrap()
});

/// Words a page puts in front of the visitor's own address, in the languages the wall phrases
/// cover. Matched within a short distance before the address.
const LABELS: &[&str] = &[
    "your ip",
    "your address",
    "your public ip",
    "client ip",
    "visitor ip",
    "remote ip",
    "tu ip",
    "tu dirección ip",
    "tu direccion ip",
    "su dirección ip",
    "seu ip",
    "seu endereço ip",
    "votre ip",
    "votre adresse ip",
    "ihre ip",
    "deine ip",
    "\"origin\"",
];

/// How far before an address its label may be.
const LABEL_REACH: usize = 48;

static KNOWN: LazyLock<Mutex<Vec<IpAddr>>> = LazyLock::new(|| Mutex::new(load()));

fn path() -> std::path::PathBuf {
    crate::config::home_dir().join("own_ip.json")
}

fn load() -> Vec<IpAddr> {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default()
        .iter()
        .filter_map(|s| s.parse().ok())
        .collect()
}

/// An address that could be this machine's on the internet. A private, loopback or link-local
/// one is the machine's on some network, and saying it gives nothing away.
pub fn is_public(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            !(v.is_private()
                || v.is_loopback()
                || v.is_link_local()
                || v.is_unspecified()
                || v.is_multicast()
                || v.is_broadcast()
                || (o[0] == 100 && (64..128).contains(&o[1]))
                || o[0] == 0)
        }
        IpAddr::V6(v) => {
            let s = v.segments();
            !(v.is_loopback()
                || v.is_unspecified()
                || v.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00
                || (s[0] & 0xffc0) == 0xfe80)
        }
    }
}

/// Every address in `text`, with where it starts.
fn addresses(text: &str) -> Vec<(usize, &str, IpAddr)> {
    let mut out = Vec::new();
    for re in [&*IPV4, &*IPV6] {
        for c in re.captures_iter(text) {
            let Some(m) = c.get(1) else { continue };
            if let Ok(ip) = m.as_str().parse::<IpAddr>() {
                out.push((m.start(), m.as_str(), ip));
            }
        }
    }
    out
}

/// The public addresses a page labels as the visitor's.
pub fn labelled(text: &str) -> Vec<IpAddr> {
    let mut out: Vec<IpAddr> = Vec::new();
    for (start, _, ip) in addresses(text) {
        if !is_public(&ip) || out.contains(&ip) {
            continue;
        }
        let mut from = start.saturating_sub(LABEL_REACH);
        while !text.is_char_boundary(from) {
            from -= 1;
        }
        let before = text[from..start].to_lowercase();
        if LABELS.iter().any(|l| before.contains(l)) {
            out.push(ip);
        }
    }
    out
}

/// Remember addresses as this machine's.
pub fn learn(ips: &[IpAddr]) {
    if ips.is_empty() {
        return;
    }
    let mut known = KNOWN.lock().unwrap_or_else(|e| e.into_inner());
    let before = known.clone();
    for ip in ips.iter().filter(|ip| is_public(ip)) {
        known.retain(|k| k != ip);
        known.insert(0, *ip);
    }
    known.truncate(KEEP);
    if *known != before {
        let list: Vec<String> = known.iter().map(|i| i.to_string()).collect();
        if let Ok(s) = serde_json::to_string(&list) {
            let _ = std::fs::write(path(), s);
        }
    }
}

/// The addresses learned so far.
pub fn known() -> Vec<IpAddr> {
    KNOWN.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Replace each of `ips` in `text` by [`LABEL`], and say how many were replaced.
pub fn mask(text: &str, ips: &[IpAddr]) -> (String, usize) {
    if ips.is_empty() {
        return (text.to_string(), 0);
    }
    let mut hits: Vec<(usize, usize)> = addresses(text)
        .into_iter()
        .filter(|(_, _, ip)| ips.contains(ip))
        .map(|(start, s, _)| (start, start + s.len()))
        .collect();
    if hits.is_empty() {
        return (text.to_string(), 0);
    }
    hits.sort_unstable();
    hits.dedup();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    let mut n = 0;
    for (a, b) in hits {
        if a < at {
            continue;
        }
        out.push_str(&text[at..a]);
        out.push_str(LABEL);
        at = b;
        n += 1;
    }
    out.push_str(&text[at..]);
    (out, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_address_the_page_calls_the_visitors_is_taken_for_ours() {
        let page = "Access denied. Your IP address: 198.51.100.23. Server 192.0.2.9 refused.";
        assert_eq!(
            labelled(page),
            vec!["198.51.100.23".parse::<IpAddr>().unwrap()]
        );
        assert!(labelled("Your IP: 192.168.1.10").is_empty(), "private");
        assert!(
            labelled("The resolver 8.8.8.8 is popular.").is_empty(),
            "no label"
        );
        assert_eq!(
            labelled(r#"{"origin": "2001:db8::7"}"#),
            vec!["2001:db8::7".parse::<IpAddr>().unwrap()]
        );
        assert!(labelled("The meeting is at 12:30:45.").is_empty());
    }

    #[test]
    fn masking_replaces_the_address_and_nothing_else() {
        let ip: IpAddr = "198.51.100.23".parse().unwrap();
        let (out, n) = mask(
            "ip 198.51.100.23, not 198.51.100.230 or 1198.51.100.23",
            &[ip],
        );
        assert_eq!(n, 1);
        assert_eq!(out, "ip [own IP], not 198.51.100.230 or 1198.51.100.23");
        assert_eq!(mask("nothing here", &[ip]), ("nothing here".to_string(), 0));
    }
}
