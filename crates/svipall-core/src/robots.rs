//! robots.txt, per RFC 9309.
//!
//! The crawler had no notion of it at all: it followed whatever links it found. That is the wrong
//! default for an automated, high-volume path, which is precisely what the standard exists for.
//!
//! One deliberate deviation from the RFC, marked at the point it happens: a 5xx or a timeout is
//! treated as "allow", not "disallow everything". For a crawler run by an operator on their own
//! machine, a transient 503 on robots.txt should not silently make a whole domain unreachable for
//! the next fifteen minutes.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RobotsPolicy {
    /// Refuse disallowed URLs. The default for crawling.
    Obey,
    /// Fetch anyway, but say so in the result. The default for a URL a person named explicitly.
    Warn,
    /// Never inferred: it has to be asked for.
    Ignore,
}

impl RobotsPolicy {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "obey" | "respect" | "true" => Some(Self::Obey),
            "warn" => Some(Self::Warn),
            "ignore" | "false" => Some(Self::Ignore),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
struct Rule {
    /// Path pattern, `*` and `$` included.
    pattern: String,
    allow: bool,
}

#[derive(Debug, Default, Clone)]
struct Group {
    agents: Vec<String>,
    rules: Vec<Rule>,
    crawl_delay: Option<f64>,
}

#[derive(Debug, Default, Clone)]
pub struct Robots {
    groups: Vec<Group>,
    pub sitemaps: Vec<String>,
}

/// Glob match for robots patterns: `*` is any run of characters, a trailing `$` anchors the end.
fn matches(pattern: &str, path: &str) -> bool {
    let anchored = pattern.ends_with('$');
    let pattern = if anchored {
        &pattern[..pattern.len() - 1]
    } else {
        pattern
    };
    let parts: Vec<&str> = pattern.split('*').collect();

    let mut pos = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            // The first segment must sit at the very start.
            if !path[pos..].starts_with(part) {
                return false;
            }
            pos += part.len();
        } else {
            match path[pos..].find(part) {
                Some(at) => pos += at + part.len(),
                None => return false,
            }
        }
    }
    if anchored {
        // Everything after the last wildcard has to land exactly on the end.
        return match parts.last() {
            Some(last) if !last.is_empty() => path.ends_with(last) && pos == path.len(),
            _ => true,
        };
    }
    true
}

impl Robots {
    pub fn parse(text: &str) -> Self {
        let mut robots = Robots::default();
        let mut current: Option<Group> = None;
        // Consecutive `User-agent` lines share one group; a rule line ends the agent run.
        let mut expecting_agents = false;

        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim();

            match key.as_str() {
                "user-agent" => {
                    if !expecting_agents {
                        if let Some(g) = current.take() {
                            robots.groups.push(g);
                        }
                        current = Some(Group::default());
                        expecting_agents = true;
                    }
                    if let Some(g) = current.as_mut() {
                        g.agents.push(value.to_ascii_lowercase());
                    }
                }
                "allow" | "disallow" => {
                    expecting_agents = false;
                    if let Some(g) = current.as_mut() {
                        // `Disallow:` with nothing after it means "allow everything".
                        if key == "disallow" && value.is_empty() {
                            continue;
                        }
                        g.rules.push(Rule {
                            pattern: value.to_string(),
                            allow: key == "allow",
                        });
                    }
                }
                "crawl-delay" => {
                    expecting_agents = false;
                    if let Some(g) = current.as_mut() {
                        g.crawl_delay = value.parse().ok();
                    }
                }
                // Sitemap is global: it belongs to the file, not to a group.
                "sitemap" => robots.sitemaps.push(value.to_string()),
                _ => {}
            }
        }
        if let Some(g) = current {
            robots.groups.push(g);
        }
        robots
    }

    /// Group for this agent: an exact-ish match wins, otherwise `*`, otherwise nothing.
    fn group_for(&self, ua: &str) -> Option<&Group> {
        let ua = ua.to_ascii_lowercase();
        let mut wildcard = None;
        let mut best: Option<(&Group, usize)> = None;
        for g in &self.groups {
            for a in &g.agents {
                if a == "*" {
                    wildcard = Some(g);
                } else if ua.contains(a.as_str()) && best.map(|(_, l)| a.len() > l).unwrap_or(true)
                {
                    best = Some((g, a.len()));
                }
            }
        }
        best.map(|(g, _)| g).or(wildcard)
    }

    /// RFC 9309 precedence: the longest matching pattern wins, and `Allow` wins a tie.
    pub fn allows(&self, ua: &str, path_and_query: &str) -> bool {
        let Some(group) = self.group_for(ua) else {
            return true;
        };
        let mut best: Option<(usize, bool)> = None;
        for rule in &group.rules {
            if !matches(&rule.pattern, path_and_query) {
                continue;
            }
            let len = rule.pattern.len();
            match best {
                Some((blen, _)) if blen > len => {}
                Some((blen, _)) if blen == len => best = Some((len, ballow_or(rule.allow, best))),
                _ => best = Some((len, rule.allow)),
            }
        }
        best.map(|(_, allow)| allow).unwrap_or(true)
    }

    pub fn crawl_delay(&self, ua: &str) -> Option<Duration> {
        let d = self.group_for(ua)?.crawl_delay?;
        if !d.is_finite() || d <= 0.0 {
            return None;
        }
        // A site asking for five minutes between requests would otherwise hang the tool. Honour
        // the intent, cap the damage, and let the caller see the cap in web_status.
        Some(Duration::from_secs_f64(d.min(10.0)))
    }
}

/// Allow wins a same-length tie.
fn ballow_or(rule_allow: bool, best: Option<(usize, bool)>) -> bool {
    rule_allow || best.map(|(_, a)| a).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
        # comment
        User-agent: *
        Disallow: /private/
        Allow: /private/public/
        Crawl-delay: 2
        Sitemap: https://example.com/sitemap.xml

        User-agent: BadBot
        Disallow: /

        Sitemap: https://example.com/sitemap-2.xml
    "#;

    #[test]
    fn the_wildcard_group_applies_to_an_unknown_agent() {
        let r = Robots::parse(SAMPLE);
        assert!(r.allows("svipall", "/index.html"));
        assert!(!r.allows("svipall", "/private/secret"));
    }

    /// The whole point of the length rule: a more specific Allow beats a broader Disallow.
    #[test]
    fn a_longer_allow_overrides_a_shorter_disallow() {
        let r = Robots::parse(SAMPLE);
        assert!(r.allows("svipall", "/private/public/page"));
    }

    #[test]
    fn a_named_agent_gets_its_own_group() {
        let r = Robots::parse(SAMPLE);
        assert!(!r.allows("BadBot/1.0", "/anything"));
        assert!(r.allows("svipall", "/anything"));
    }

    #[test]
    fn sitemaps_are_global_regardless_of_where_they_appear() {
        let r = Robots::parse(SAMPLE);
        assert_eq!(r.sitemaps.len(), 2);
        assert!(r.sitemaps[1].ends_with("sitemap-2.xml"));
    }

    #[test]
    fn crawl_delay_is_read_and_capped() {
        assert_eq!(
            Robots::parse(SAMPLE).crawl_delay("svipall"),
            Some(Duration::from_secs(2))
        );
        let slow = Robots::parse("User-agent: *\nCrawl-delay: 300");
        assert_eq!(
            slow.crawl_delay("svipall"),
            Some(Duration::from_secs(10)),
            "an extreme crawl-delay must be capped, not obeyed literally"
        );
        assert_eq!(Robots::parse("User-agent: *").crawl_delay("svipall"), None);
    }

    /// The pattern table from RFC 9309.
    #[test]
    fn rfc_pattern_matching() {
        assert!(matches("/fish", "/fish"));
        assert!(matches("/fish", "/fish.html"));
        assert!(matches("/fish", "/fish/salmon.html"));
        assert!(!matches("/fish", "/Fish.asp"));
        assert!(!matches("/fish", "/catfish"));

        assert!(matches("/fish*", "/fish.html"));
        assert!(matches("/*.php", "/index.php"));
        assert!(matches("/*.php", "/folder/any.php"));
        // Unanchored, so a suffix after `.php` still matches — the specification lists
        // `/folder/any.php.file.html` as a match for exactly this reason.
        assert!(matches("/*.php", "/folder/any.php.file.html"));
        assert!(matches("/*.php", "/filename.php?parameters"));
        assert!(
            !matches("/*.php", "/windows.PHP"),
            "matching is case sensitive"
        );

        assert!(matches("/*.php$", "/filename.php"));
        assert!(!matches("/*.php$", "/filename.php?param=1"));
        assert!(matches("/$", "/"));
        assert!(!matches("/$", "/page"));
    }

    #[test]
    fn an_empty_disallow_means_allow_everything() {
        let r = Robots::parse("User-agent: *\nDisallow:");
        assert!(r.allows("svipall", "/anything/at/all"));
    }

    #[test]
    fn consecutive_user_agent_lines_share_one_group() {
        let r = Robots::parse("User-agent: a\nUser-agent: b\nDisallow: /x");
        assert!(!r.allows("a", "/x"));
        assert!(!r.allows("b", "/x"));
        assert!(r.allows("c", "/x"), "an unlisted agent has no group here");
    }

    #[test]
    fn directives_are_case_insensitive_and_comments_are_stripped() {
        let r = Robots::parse("USER-AGENT: *  # who\nDISALLOW: /nope  # why");
        assert!(!r.allows("svipall", "/nope"));
    }

    #[test]
    fn a_malformed_file_does_not_panic_and_allows_everything() {
        for junk in ["", "garbage", "::::", "User-agent\nDisallow"] {
            assert!(
                Robots::parse(junk).allows("svipall", "/x"),
                "failed on {junk:?}"
            );
        }
    }

    #[test]
    fn policies_parse_from_their_words() {
        assert_eq!(RobotsPolicy::parse("obey"), Some(RobotsPolicy::Obey));
        assert_eq!(RobotsPolicy::parse("WARN"), Some(RobotsPolicy::Warn));
        assert_eq!(RobotsPolicy::parse("ignore"), Some(RobotsPolicy::Ignore));
        assert_eq!(RobotsPolicy::parse("maybe"), None);
    }
}
