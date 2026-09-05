use url::Url;

/// Extract registrable domain (lowercased, strips www.)
pub fn domain_from_url(url: &str) -> String {
    if let Ok(parsed) = Url::parse(url) {
        if let Some(host) = parsed.host_str() {
            let lower = host.to_lowercase();
            return lower
                .strip_prefix("www.")
                .map(str::to_string)
                .unwrap_or(lower);
        }
    }
    String::new()
}

/// True when two hosts belong to the same site, treating a subdomain as part of its parent.
pub fn same_site(a: &str, b: &str) -> bool {
    a == b || a.ends_with(&format!(".{}", b)) || b.ends_with(&format!(".{}", a))
}

/// Query parameters that identify a campaign or a click, never a document. Dropping them is what
/// keeps `?utm_source=twitter` from being fetched as a second page.
const TRACKING_PARAMS: &[&str] = &[
    "utm_source",
    "utm_medium",
    "utm_campaign",
    "utm_term",
    "utm_content",
    "utm_id",
    "utm_reader",
    "utm_name",
    "fbclid",
    "gclid",
    "gclsrc",
    "dclid",
    "msclkid",
    "mc_cid",
    "mc_eid",
    "igshid",
    "twclid",
    "ttclid",
    "yclid",
    "_ga",
    "ref_src",
    "ref_url",
];

/// Canonical form of a URL for cache keys and de-duplication: lowercased scheme and host, no
/// fragment, no default port, no tracking parameters, query sorted so parameter order stops
/// producing distinct keys for the same page.
pub fn normalize_url(url: &str) -> Option<String> {
    let mut u = Url::parse(url).ok()?;
    if u.scheme() != "http" && u.scheme() != "https" {
        return None;
    }
    u.set_fragment(None);
    if let Some(host) = u.host_str() {
        let lower = host.to_lowercase();
        let _ = u.set_host(Some(&lower));
    }
    // `set_port` with the scheme default makes the port implicit again.
    let default_port = if u.scheme() == "https" { 443 } else { 80 };
    if u.port() == Some(default_port) {
        let _ = u.set_port(None);
    }
    let mut pairs: Vec<(String, String)> = u
        .query_pairs()
        .filter(|(k, _)| !TRACKING_PARAMS.contains(&k.as_ref()))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    pairs.sort();
    if pairs.is_empty() {
        u.set_query(None);
    } else {
        let mut q = u.query_pairs_mut();
        q.clear();
        for (k, v) in &pairs {
            q.append_pair(k, v);
        }
        drop(q);
    }
    Some(u.to_string())
}

/// FNV-1a 64.
///
/// The algorithm is fixed on purpose and pinned by a test: these hashes are written to disk as
/// cache keys and content fingerprints, so a different hash would silently invalidate everything
/// stored by an earlier build. `DefaultHasher` cannot be used here — it makes no stability promise
/// across Rust releases.
pub fn stable_hash(s: &str) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(PRIME);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_site_covers_subdomains_both_ways() {
        assert!(same_site("example.com", "example.com"));
        assert!(same_site("blog.example.com", "example.com"));
        assert!(same_site("example.com", "blog.example.com"));
        assert!(!same_site("example.com", "example.org"));
        assert!(!same_site("notexample.com", "example.com"));
    }

    #[test]
    fn normalize_strips_tracking_and_sorts_the_query() {
        assert_eq!(
            normalize_url("https://Example.com/a?b=2&utm_source=x&a=1#frag").unwrap(),
            "https://example.com/a?a=1&b=2"
        );
        assert_eq!(
            normalize_url("https://example.com:443/a").unwrap(),
            "https://example.com/a"
        );
        assert_eq!(
            normalize_url("https://example.com/a?fbclid=zzz").unwrap(),
            "https://example.com/a"
        );
        assert_eq!(normalize_url("ftp://example.com/a"), None);
        assert_eq!(normalize_url("not a url"), None);
    }

    #[test]
    fn normalize_is_idempotent() {
        let once = normalize_url("https://Example.com/a?b=2&utm_source=x&a=1#frag").unwrap();
        assert_eq!(normalize_url(&once).unwrap(), once);
    }

    /// If this test ever needs updating, every cached page and content fingerprint written by an
    /// older build has just been invalidated. Change the algorithm only with a migration.
    #[test]
    fn stable_hash_is_pinned() {
        // The first two are the published FNV-1a 64 vectors, so they also check the algorithm.
        assert_eq!(stable_hash(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(stable_hash("a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(stable_hash("https://example.com/"), 903_888_511_234_722_068);
    }

    #[test]
    fn test_domain() {
        assert_eq!(
            domain_from_url("https://www.example.com/path"),
            "example.com"
        );
        assert_eq!(
            domain_from_url("https://sub.example.com"),
            "sub.example.com"
        );
        assert_eq!(domain_from_url("https://EXAMPLE.COM"), "example.com");
    }
}
