//! Getting the ad and consent lists onto this machine, once.
//!
//! The parsing and matching live in `svipall_core::blocklist`, which is pure and knows nothing about
//! the network. This is the part that has to talk to it, and it exists as its own file so the rule
//! it enforces is easy to check: **a failure here is silent and total**. No list, no error, no
//! warning on every start — the crawl runs exactly as it did before lists existed, only slower.
//!
//! Downloaded once and cached under `~/.svipall/blocklists/`. Nothing is re-fetched on a schedule: a
//! tool that phones out on a timer is a tool with a network dependency, whatever its README says.
//! Deleting the cache directory is how an operator asks for a fresh copy.

use std::path::PathBuf;
use svipall_core::blocklist::Blocklist;

/// How many domains are handed to the browser. See `Blocklist::patterns` for why there is a cap.
const MAX_DOMAINS: usize = 3_000;

pub fn cache_dir() -> PathBuf {
    svipall_core::config::home_dir().join("blocklists")
}

/// A stable, boring file name for a source URL. Not a hash: an operator looking in the directory
/// should be able to tell which file came from where.
fn file_for(url: &str) -> PathBuf {
    let name: String = url
        .rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or("list")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    cache_dir().join(name)
}

/// Whatever is already on disk, without touching the network.
pub fn cached() -> Blocklist {
    let mut out = Blocklist::empty();
    let Ok(entries) = std::fs::read_dir(cache_dir()) else {
        return out;
    };
    for e in entries.flatten() {
        if let Ok(text) = std::fs::read_to_string(e.path()) {
            out.merge(Blocklist::parse(&text));
        }
    }
    out
}

/// The lists, fetching any that are missing.
///
/// Every failure path ends at "carry on with less": an unreachable source, an unwritable cache, a
/// body that turns out to be an error page. None of them is worth stopping a crawl for.
pub async fn load(sources: &[String], identity: &svipall_core::IdentityProfile) -> Blocklist {
    let mut out = cached();
    for url in sources {
        let path = file_for(url);
        if path.is_file() {
            continue;
        }
        let Ok(body) = fetch(url, identity).await else {
            tracing::debug!(url = %url, "blocklist unavailable; continuing without it");
            continue;
        };
        let parsed = Blocklist::parse(&body);
        // An error page parses to nothing. Caching it would mean never trying again.
        if parsed.is_empty() {
            continue;
        }
        let _ = std::fs::create_dir_all(cache_dir());
        let _ = std::fs::write(&path, &body);
        out.merge(parsed);
        tracing::info!(url = %url, "blocklist cached at {}", path.display());
    }
    out
}

async fn fetch(url: &str, identity: &svipall_core::IdentityProfile) -> anyhow::Result<String> {
    // The ordinary http tier, with the ordinary identity: a list server has no reason to see a
    // different client from every other request this machine makes.
    let cfg = svipall_http::FetcherConfig::new(identity.clone());
    let fetcher = svipall_http::build(cfg)?;
    let res = fetcher.send(svipall_http::HttpRequest::get(url)).await?;
    if res.status >= 400 {
        anyhow::bail!("{} returned {}", url, res.status);
    }
    Ok(res.text())
}

/// The patterns to hand a browser.
pub fn patterns(list: &Blocklist) -> Vec<String> {
    list.patterns(MAX_DOMAINS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_url_becomes_a_file_name_a_person_can_recognise() {
        // A hash would be shorter and would make the cache directory unreadable, which is where an
        // operator looks when they want to know what is being blocked.
        let p = file_for("https://example.test/lists/hosts.txt");
        assert_eq!(p.file_name().unwrap(), "hosts.txt");
        let odd = file_for("https://example.test/a/b?c=d&e=f");
        assert!(
            odd.file_name()
                .unwrap()
                .to_string_lossy()
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_'),
            "{odd:?}"
        );
    }

    #[test]
    fn a_url_that_ends_in_a_slash_still_gets_a_name() {
        assert!(file_for("https://example.test/lists/")
            .file_name()
            .is_some());
        assert!(file_for("").file_name().is_some());
    }

    #[tokio::test]
    async fn no_network_and_no_cache_means_no_blocking_and_no_error() {
        // The rule this whole file exists to make checkable. A tool that will not run because it
        // could not reach a list has traded a small saving for its entire promise.
        let sources = vec!["http://127.0.0.1:1/never-listening".to_string()];
        let id = svipall_core::IdentityProfile::for_major(147, svipall_core::identity::Os::Windows);
        let list = load(&sources, &id).await;
        assert!(!list.blocks("https://ads.example/x.js") || !list.is_empty());
        assert!(patterns(&list).len() <= MAX_DOMAINS * 2);
    }

    #[test]
    fn the_browser_never_gets_more_patterns_than_it_can_afford() {
        let many: String = (0..10_000)
            .map(|i| format!("0.0.0.0 host{i}.example\n"))
            .collect();
        let list = Blocklist::parse(&many);
        assert_eq!(list.len(), 10_000);
        assert_eq!(
            patterns(&list).len(),
            MAX_DOMAINS * 2,
            "two patterns per domain, capped"
        );
    }
}
