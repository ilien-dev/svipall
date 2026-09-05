//! Which origins this installation is allowed to touch.
//!
//! A crawl is an agent following links it has not read yet. Most of the time that is the point, and
//! occasionally it is a problem: a crawl of an internal wiki that follows an external link, a
//! research task that must stay on one domain, an agent handed a URL by a page rather than by a
//! person. `robots.txt` does not help — it is the site's opinion about crawlers, not the operator's
//! about this machine.
//!
//! Two lists, checked before a request is made rather than after it comes back. Blocking wins over
//! allowing, because the failure that matters is a request that should not have happened, and a
//! rule that can be talked out of by a second rule is not a rule.
//!
//! Loopback and private addresses get a rule of their own. It is **off by default**, and that is a
//! judgement rather than an oversight: svipall is a local-first tool, and fetching `http://localhost`
//! is an ordinary thing for its operator to ask it to do. Turn it on for an installation where an
//! agent chooses its own URLs — a page linking to `http://169.254.169.254/` and an agent following
//! it is the oldest trick there is, and it costs the operator their cloud credentials.

/// What the operator will and will not have this machine fetch.
#[derive(Debug, Clone, Default)]
pub struct OriginPolicy {
    /// If non-empty, nothing outside it is fetched.
    pub allow: Vec<String>,
    /// Never fetched, whatever else says.
    pub block: Vec<String>,
    /// Refuse loopback, link-local and private-range addresses.
    pub refuse_private: bool,
    /// Directories a `file://` URL may point into. Empty means no local file is readable: an
    /// agent that was talked into reading `~/.ssh/id_ed25519` through a page-to-markdown tool is
    /// the failure this list exists to make impossible.
    pub local_roots: Vec<std::path::PathBuf>,
}

/// Why a URL was refused, in words the caller can pass on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// It matched a block rule.
    Blocked(String),
    /// There is an allow list and it is not on it.
    NotAllowed,
    /// It points inside this machine or this network.
    Private,
    /// It is not a URL anything should be asked to fetch.
    NotFetchable,
    /// A `file://` path outside the directories this installation declared readable.
    OutsideLocalRoots,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Blocked(rule) => write!(f, "blocked by the rule '{rule}'"),
            Refusal::NotAllowed => {
                write!(f, "outside the allowed origins for this installation")
            }
            Refusal::Private => write!(
                f,
                "points at this machine or this private network, which this installation refuses"
            ),
            Refusal::NotFetchable => write!(f, "not an http, https, file or raw: URL"),
            Refusal::OutsideLocalRoots => write!(
                f,
                "a local file outside the directories this installation may read (local_roots)"
            ),
        }
    }
}

/// The file a `file://` URL names, or `None` when it is not a well-formed file URL.
pub fn file_url_path(url: &str) -> Option<std::path::PathBuf> {
    url::Url::parse(url).ok()?.to_file_path().ok()
}

impl OriginPolicy {
    /// Nothing restricted, except that private addresses are refused.
    ///
    /// For an installation where an agent picks its own URLs. Not what `Config` builds by default —
    /// see the module note.
    pub fn guarded() -> Self {
        Self {
            refuse_private: true,
            ..Default::default()
        }
    }

    /// May this URL be fetched?
    pub fn check(&self, url: &str) -> Result<(), Refusal> {
        // Inline markup makes no request at all, so no origin rule can have an opinion on it.
        if url.starts_with("raw:") {
            return Ok(());
        }
        if url.starts_with("file://") {
            return self.check_local_file(url);
        }
        let scheme_ok = url.starts_with("http://") || url.starts_with("https://");
        if !scheme_ok {
            return Err(Refusal::NotFetchable);
        }
        let Some(host) = host_of(url) else {
            return Err(Refusal::NotFetchable);
        };
        if let Some(rule) = self.block.iter().find(|r| matches(&host, r)) {
            return Err(Refusal::Blocked(rule.clone()));
        }
        if self.refuse_private && is_private(&host) {
            return Err(Refusal::Private);
        }
        if !self.allow.is_empty() && !self.allow.iter().any(|r| matches(&host, r)) {
            return Err(Refusal::NotAllowed);
        }
        Ok(())
    }

    pub fn allows(&self, url: &str) -> bool {
        self.check(url).is_ok()
    }

    /// A `file://` URL is readable only when its real path — symlinks resolved — sits under one
    /// of the declared roots. Comparing the canonical form is what stops `..` and links from
    /// walking out.
    fn check_local_file(&self, url: &str) -> Result<(), Refusal> {
        let path = file_url_path(url).ok_or(Refusal::NotFetchable)?;
        let real = std::fs::canonicalize(&path).map_err(|_| Refusal::OutsideLocalRoots)?;
        let inside = self
            .local_roots
            .iter()
            .filter_map(|r| std::fs::canonicalize(r).ok())
            .any(|root| real.starts_with(&root));
        if inside {
            Ok(())
        } else {
            Err(Refusal::OutsideLocalRoots)
        }
    }
}

/// Does `host` match a rule?
///
/// A rule is a host, and a host rule covers everything under it: `example.com` covers
/// `www.example.com`. `*.example.com` is written that way too and means the same thing, because
/// somebody will write it and being surprised by a security rule is how holes happen.
fn matches(host: &str, rule: &str) -> bool {
    let rule = rule.trim().trim_start_matches("*.").to_ascii_lowercase();
    if rule.is_empty() {
        return false;
    }
    host == rule || host.ends_with(&format!(".{rule}"))
}

fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://").map(|(_, r)| r)?;
    let host = rest
        .split(['/', '?', '#'])
        .next()?
        .rsplit('@')
        .next()?
        .trim_start_matches('[');
    // IPv6 literals are bracketed, so the colon split has to come after that.
    let host = match host.split_once(']') {
        Some((v6, _)) => v6.to_string(),
        None => host.split(':').next()?.to_string(),
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// Is this name or address inside this machine or this network?
///
/// Names are checked too, not just addresses: `localhost` and anything under `.internal` or
/// `.local` resolve inside, and refusing only the numeric forms catches none of them.
fn is_private(host: &str) -> bool {
    if host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || host.ends_with(".home.arpa")
    {
        return true;
    }
    if let Ok(v6) = host.parse::<std::net::Ipv6Addr>() {
        return v6.is_loopback() || v6.is_unspecified() || (v6.segments()[0] & 0xfe00) == 0xfc00;
    }
    let Ok(ip) = host.parse::<std::net::Ipv4Addr>() else {
        return false;
    };
    let o = ip.octets();
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_link_local()
        || ip.is_broadcast()
        || o[0] == 10
        || (o[0] == 172 && (16..32).contains(&o[1]))
        || (o[0] == 192 && o[1] == 168)
        // Carrier-grade NAT: not "private" by the usual definition and not somewhere an agent
        // following links has any business being.
        || (o[0] == 100 && (64..128).contains(&o[1]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guarded_policy_lets_the_web_through_and_keeps_the_machine_out() {
        let p = OriginPolicy::guarded();
        assert!(p.allows("https://example.com/page"));
        assert_eq!(p.check("http://127.0.0.1:8080/"), Err(Refusal::Private));
        assert_eq!(p.check("http://localhost/"), Err(Refusal::Private));
    }

    #[test]
    fn the_address_that_hands_out_cloud_credentials_is_refused() {
        // The oldest trick there is: a page contains a link to the metadata service, an agent
        // follows it, and the reply is a set of keys.
        let p = OriginPolicy::guarded();
        assert_eq!(
            p.check("http://169.254.169.254/latest/"),
            Err(Refusal::Private)
        );
        assert_eq!(p.check("http://192.168.1.1/"), Err(Refusal::Private));
        assert_eq!(p.check("http://10.0.0.5/"), Err(Refusal::Private));
        assert_eq!(p.check("http://172.16.4.4/"), Err(Refusal::Private));
        assert_eq!(p.check("http://[::1]/"), Err(Refusal::Private));
    }

    #[test]
    fn a_public_address_that_only_looks_private_is_still_fetched() {
        // 172.32 is public; a rule written as "starts with 172." would refuse it.
        let p = OriginPolicy::guarded();
        assert!(p.allows("http://172.32.0.1/"));
        assert!(p.allows("http://192.169.0.1/"));
        assert!(p.allows("http://11.0.0.1/"));
    }

    #[test]
    fn an_allow_list_is_a_fence_not_a_preference() {
        let p = OriginPolicy {
            allow: vec!["docs.example".into()],
            ..OriginPolicy::guarded()
        };
        assert!(p.allows("https://docs.example/a"));
        assert!(
            p.allows("https://api.docs.example/a"),
            "subdomains are inside"
        );
        assert_eq!(p.check("https://elsewhere.test/"), Err(Refusal::NotAllowed));
    }

    #[test]
    fn blocking_wins_over_allowing() {
        // A rule that can be talked out of by a second rule is not a rule.
        let p = OriginPolicy {
            allow: vec!["example.com".into()],
            block: vec!["ads.example.com".into()],
            ..OriginPolicy::guarded()
        };
        assert!(p.allows("https://www.example.com/"));
        assert_eq!(
            p.check("https://ads.example.com/x"),
            Err(Refusal::Blocked("ads.example.com".into()))
        );
        assert!(
            !p.allows("https://deep.ads.example.com/x"),
            "a blocked host blocks what is under it"
        );
    }

    #[test]
    fn a_rule_written_with_a_star_means_what_the_person_writing_it_meant() {
        // Being surprised by a security rule is how holes happen.
        let p = OriginPolicy {
            block: vec!["*.tracker.example".into()],
            ..OriginPolicy::guarded()
        };
        assert!(!p.allows("https://a.tracker.example/"));
        assert!(!p.allows("https://tracker.example/"));
    }

    #[test]
    fn a_host_is_matched_whole_and_never_by_its_ending_as_text() {
        // `evil-example.com` must not match a rule for `example.com`.
        let p = OriginPolicy {
            block: vec!["example.com".into()],
            ..OriginPolicy::guarded()
        };
        assert!(p.allows("https://evil-example.com/"));
        assert!(p.allows("https://notexample.com/"));
        assert!(!p.allows("https://example.com/"));
    }

    #[test]
    fn only_http_urls_are_fetchable_at_all() {
        let p = OriginPolicy::guarded();
        for u in [
            "ftp://example.com/",
            "javascript:alert(1)",
            "data:text/html,<b>x",
            "",
        ] {
            assert_eq!(p.check(u), Err(Refusal::NotFetchable), "{u}");
        }
    }

    #[test]
    fn raw_html_needs_no_network_and_no_permission() {
        let p = OriginPolicy {
            allow: vec!["example.com".into()],
            ..OriginPolicy::guarded()
        };
        assert_eq!(p.check("raw:<html><body>hi</body></html>"), Ok(()));
    }

    #[test]
    fn a_local_file_is_refused_unless_a_root_covers_it() {
        let dir = std::env::temp_dir().join(format!("svipall-roots-{}", std::process::id()));
        let inside = dir.join("in");
        std::fs::create_dir_all(&inside).unwrap();
        let ok = inside.join("page.html");
        std::fs::write(&ok, "<p>x</p>").unwrap();
        let outside = dir.join("secret.txt");
        std::fs::write(&outside, "no").unwrap();
        let url_of = |p: &std::path::Path| url::Url::from_file_path(p).unwrap().to_string();

        let none = OriginPolicy::guarded();
        assert_eq!(
            none.check(&url_of(&ok)),
            Err(Refusal::OutsideLocalRoots),
            "no roots at all"
        );

        let p = OriginPolicy {
            local_roots: vec![inside.clone()],
            ..OriginPolicy::guarded()
        };
        assert_eq!(p.check(&url_of(&ok)), Ok(()));
        assert_eq!(p.check(&url_of(&outside)), Err(Refusal::OutsideLocalRoots));
        // `..` inside the URL is resolved before the comparison, not matched as text.
        let climb = url_of(&inside.join("..").join("secret.txt"));
        assert_eq!(p.check(&climb), Err(Refusal::OutsideLocalRoots), "{climb}");
        let missing = url_of(&inside.join("nope.html"));
        assert_eq!(
            p.check(&missing),
            Err(Refusal::OutsideLocalRoots),
            "a missing file is not readable either"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_refusal_says_enough_for_the_caller_to_act_on_it() {
        let p = OriginPolicy {
            block: vec!["bad.example".into()],
            ..OriginPolicy::guarded()
        };
        let why = p.check("https://bad.example/").expect_err("refused");
        assert!(why.to_string().contains("bad.example"), "{why}");
        assert!(OriginPolicy::guarded()
            .check("http://10.0.0.1/")
            .expect_err("refused")
            .to_string()
            .contains("private"),);
    }

    #[test]
    fn turning_the_private_guard_off_is_possible_for_someone_who_means_it() {
        // Crawling an internal wiki is a real thing to want; it just has to be asked for.
        let p = OriginPolicy {
            refuse_private: false,
            ..Default::default()
        };
        assert!(p.allows("http://192.168.1.10/wiki"));
    }

    #[test]
    fn a_url_with_credentials_is_matched_on_its_real_host() {
        // `https://example.com@evil.test/` is a link to evil.test, and reading the host as the part
        // before the `@` is exactly how that trick works.
        let p = OriginPolicy {
            allow: vec!["example.com".into()],
            ..OriginPolicy::guarded()
        };
        assert_eq!(
            p.check("https://example.com@evil.test/"),
            Err(Refusal::NotAllowed)
        );
    }
}
