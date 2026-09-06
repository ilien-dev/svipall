//! Loader for `~/.svipall/config.toml`. Every field has a default, so a missing or
//! partial file is fine.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Where svipall keeps everything: the config, the cache, browser profiles and learned state.
///
/// `SVIPALL_HOME` overrides it. That is what makes a portable install possible, and it is what lets
/// the test suite run against a directory of its own instead of the developer's real one.
pub fn home_dir() -> PathBuf {
    match std::env::var_os("SVIPALL_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".svipall"),
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    /// Port for the embedded captcha API and human dashboard.
    pub dashboard_port: u16,
    /// Address the dashboard binds to. Loopback by default: the dashboard shows the URLs svipall is
    /// visiting and accepts captcha answers, so it is not something to expose by accident.
    pub dashboard_bind: String,
    /// Background workers draining the captcha queue.
    pub solver_workers: usize,
    pub log_level: String,
    /// Chromium-based browser executable. Empty = auto-detect (Chrome, Edge, Brave, Chromium).
    pub browser_path: String,
    /// `auto` learns emulated routes first and allows one native fallback. `emulated` never
    /// falls back; `native` exposes real browser characteristics from the first browser visit.
    pub browser_identity: String,
    /// Allow the last-resort native route in auto. Never used for isolated/mobile/named sessions.
    pub auto_native_fallback: bool,
    /// Maximum transport attempts in one automatic fetch, including native fallback.
    pub auto_max_attempts: usize,
    /// Maximum top-level visits per domain and exit in a rolling window; subresources excluded.
    pub request_limit: u32,
    pub request_window_seconds: u64,
    pub request_cooldown_seconds: u64,
    pub request_min_interval_ms: u64,
    /// Provision the managed browser on startup when no local browser is available.
    pub browser_auto_install: bool,
    /// Highest tier `mode=auto` may climb to: http | browser | stealth | real | warm.
    pub max_tier: String,
    /// If non-empty, nothing outside these origins is fetched.
    pub allow_origins: Vec<String>,
    /// Never fetched, whatever else says.
    pub block_origins: Vec<String>,
    /// Refuse loopback, link-local and private-range addresses.
    ///
    /// Off by default, and not because the risk is small: svipall is a local-first tool and fetching
    /// `http://localhost` is an ordinary thing for an operator to ask it to do. Turn it on for an
    /// installation where an agent chooses its own URLs.
    pub refuse_private_addresses: bool,
    /// How a domain with a pool of exits (`web_route` with several proxies) picks one: `sticky`
    /// (default) keeps the same exit until the domain retires it; `round_robin` rotates.
    pub exit_strategy: String,
    /// How much one address may have outstanding with one host before svipall slows down and then
    /// declines. Zero turns it off.
    ///
    /// Points, not visits: a plain HTTP fetch costs 1 and a headful browser waiting out a
    /// challenge costs 12, doubled when the page comes back walled. And because the spend decays
    /// rather than resetting, this is a *rate* — a steady spend settles at
    /// `budget * ln 2 / half_life`, so 250 at six hours is about 29 points an hour.
    pub reputation_budget: f32,
    /// How long it takes for half of what was spent on a host to stop counting. Six hours means a
    /// day of quiet leaves a sixteenth of yesterday's spend.
    pub reputation_half_life_hours: u32,
    /// Days a finished captcha keeps its images so `svipall solver export-corpus` can turn them
    /// into training data. Zero keeps none. Everything stays on this machine either way.
    pub corpus_keep_days: u32,
    /// Directories a `file://` URL may read from. Empty = only `~/.svipall/in`. A file anywhere
    /// else is refused, so an agent cannot be talked into reading a key file through a
    /// page-to-markdown tool.
    pub local_roots: Vec<String>,
    /// Resolve names over HTTPS instead of in the clear, using this template
    /// (e.g. `https://dns.example/dns-query`). Empty disables it.
    ///
    /// This covers the browser tiers making direct connections. It does nothing when a proxy is
    /// configured, and does not need to: an HTTP or socks5h proxy resolves the name at its own
    /// end, so nothing on this machine's network ever sees it.
    pub dns_over_https: String,
    /// Refuse known advertising and tracking hosts, and hide consent overlays.
    ///
    /// Off by default, and it is a real trade rather than a free win: a page whose third parties
    /// all fail loads differently from one where they succeed, and some anti-bot scripts notice.
    /// On a crawl of ordinary pages it is most of the bytes.
    pub block_ads: bool,
    /// Where the lists come from. Fetched once on first use and cached under `blocklists/`; with
    /// no network and no cache, nothing is blocked and nothing fails.
    pub blocklist_sources: Vec<String>,
    /// Navigation timeout for browser tiers, in ms.
    pub browser_timeout_ms: u64,
    /// How long the `warm` tier keeps waiting for a challenge to clear on its own, in ms.
    pub warm_wait_ms: u64,
    /// Extend only progressing challenges, bounded by this total warm budget and the caller's
    /// overall request deadline. Proof-of-work gets enough time for at most one renewal.
    pub warm_adaptive: bool,
    pub warm_max_wait_ms: u64,
    /// Idle pooled browsers are closed after this many seconds.
    pub browser_idle_secs: u64,
    /// How many cleared pages may be held open between fetches. An offscreen tab with a live
    /// runtime costs real memory, so this is small on purpose: enough for the domain being worked
    /// plus one. **Zero disables holding pages entirely**, which is also the control arm when
    /// measuring whether holding them was worth anything.
    pub warm_keep_max: usize,
    /// How long a held page may go unused before it is closed. Must stay above
    /// `classify::POW_TOKEN_LIFETIME_SECS` for a reuse to be worth having, and below
    /// `browser_idle_secs` so a held page expires before the browser holding it.
    pub warm_keep_secs: u64,
    /// Parallel fetches for web_fetch_many / web_crawl.
    pub parallelism: usize,
    /// `Accept-Language` and browser locale. Empty = the built-in default.
    pub locale: String,
    /// IANA timezone the browser reports. Empty = the built-in default. Set this when routing
    /// through a proxy in another country, or the timezone gives the proxy away.
    pub timezone: String,
    /// HTTP engine for the `http` tier: `auto` (impersonation when compiled in), `reqwest`,
    /// `impersonate`. `SVIPALL_HTTP_ENGINE` overrides it.
    pub http_engine: String,
    /// Speak HTTP/3 to sites that advertised it. Off by default, and never on a first visit:
    /// `Alt-Svc` is what a site uses to say it offers h3, so the first fetch of a domain is TCP
    /// whatever this says, and a domain that stops answering over UDP falls back to TCP with the
    /// page intact. Needs a binary built with `--features http3`.
    pub http3: bool,
    /// Present the http tier as Firefox rather than Chrome: Gecko TLS, Gecko headers, Gecko UA,
    /// all coherent. The browser tiers stay Chrome (their CDP is Chrome's), so a domain that only
    /// needs the http tier gets a Firefox that is Firefox all the way down, which some
    /// engine-aware WAFs weight differently. Off by default. See docs/firefox.md.
    #[serde(default)]
    pub http_firefox: bool,
    /// Default cap on content returned by a single fetch, in estimated tokens.
    pub max_tokens_per_fetch: usize,
    /// Default cap across a whole crawl.
    pub max_tokens_total: usize,
    /// How many blocks of the previous page a truncated fetch repeats when it is resumed with a
    /// cursor, so the continuation is not read cold. 0 = none.
    pub overlap_blocks: usize,
    /// Port for the local REST API (`POST /v1/fetch`, …). 0 = off, which is the default: the API
    /// can fetch, and it can read this machine's logged-in profiles, so it is opened on purpose
    /// rather than by installing. `svipall serve` starts it regardless; this field is what makes
    /// `svipall-mcp` mount it alongside the dashboard. `SVIPALL_REST_PORT` overrides it.
    pub rest_port: u16,
    /// Address the REST API binds to. Loopback by default, for the same reason as the dashboard.
    pub rest_bind: String,
    /// Bearer token the REST API requires. Empty = read `~/.svipall/api_key`, generating one on
    /// first use. Set it here to pin one (a container, a CI job). `SVIPALL_API_KEY` overrides it.
    pub api_key: String,
    /// Long jobs running at once. Deliberately not `parallelism`, which bounds requests *inside*
    /// one job: two crawls at parallelism 4 is eight in-flight fetches and, on a browser tier,
    /// eight Chrome pages — more than `capacity::concurrency` would ever grant one crawl. Over the
    /// cap a job stays queued, which is what a queue is for.
    pub max_jobs: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            dashboard_port: 8787,
            dashboard_bind: "127.0.0.1".into(),
            solver_workers: 4,
            log_level: "info".into(),
            browser_path: String::new(),
            browser_identity: "auto".into(),
            auto_native_fallback: true,
            auto_max_attempts: 6,
            request_limit: 12,
            request_window_seconds: 60,
            request_cooldown_seconds: 900,
            request_min_interval_ms: 1000,
            browser_auto_install: true,
            max_tier: "warm".into(),
            allow_origins: Vec::new(),
            block_origins: Vec::new(),
            refuse_private_addresses: false,
            exit_strategy: "sticky".into(),
            reputation_budget: crate::reputation::DEFAULT_BUDGET,
            reputation_half_life_hours: crate::reputation::DEFAULT_HALF_LIFE_HOURS,
            corpus_keep_days: 30,
            local_roots: Vec::new(),
            dns_over_https: String::new(),
            block_ads: false,
            blocklist_sources: vec![
                "https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts".into(),
                "https://easylist.to/easylist/easyprivacy.txt".into(),
            ],
            browser_timeout_ms: 45_000,
            warm_wait_ms: 20_000,
            warm_adaptive: true,
            warm_max_wait_ms: 55_000,
            browser_idle_secs: 180,
            warm_keep_max: 2,
            warm_keep_secs: 120,
            parallelism: 4,
            locale: String::new(),
            timezone: String::new(),
            http_engine: "auto".into(),
            http3: false,
            http_firefox: false,
            max_tokens_per_fetch: 25_000,
            max_tokens_total: 60_000,
            overlap_blocks: 1,
            rest_port: 0,
            rest_bind: "127.0.0.1".into(),
            api_key: String::new(),
            max_jobs: 2,
        }
    }
}

/// Where the REST API's bearer key came from, so the operator can be told once and never again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeySource {
    /// `SVIPALL_API_KEY`.
    Env,
    /// `api_key` in `config.toml`.
    Config,
    /// A key file written by an earlier run.
    File(PathBuf),
    /// Written just now. This is the only run that prints the key itself.
    Generated(PathBuf),
}

/// The REST API's bearer key for the installation rooted at `home`.
///
/// Deliberately a sibling file rather than a field written back into `config.toml`: `Config` does
/// not derive `Serialize`, so persisting a generated key there would mean adding it and then
/// rewriting a file the operator edits by hand, comments and all. `secrets.env`, `pools.json` and
/// `domain_tiers.json` are already the house pattern for "something svipall keeps beside the
/// settings rather than in them".
///
/// Takes the home directory rather than reading it, so a test can exercise the whole resolution
/// order without `set_var` racing another thread in the same process.
pub fn api_key_in(home: &std::path::Path) -> (String, KeySource) {
    api_key_from(std::env::var("SVIPALL_API_KEY").ok(), home)
}

/// The resolution order, with the environment passed in rather than read.
///
/// Split out only so the precedence can be tested: `std::env::set_var` in a `#[test]` is visible to
/// every other test in the same binary, and a test that can change another test's answer is worse
/// than no test.
fn api_key_from(env: Option<String>, home: &std::path::Path) -> (String, KeySource) {
    // Env first, and it must not need a writable home: that is the container and the CI job.
    if let Some(k) = env {
        if !k.trim().is_empty() {
            return (k.trim().to_string(), KeySource::Env);
        }
    }
    let path = home.join("api_key");
    if let Ok(s) = std::fs::read_to_string(home.join("config.toml")) {
        if let Ok(cfg) = toml::from_str::<Config>(&s) {
            if !cfg.api_key.trim().is_empty() {
                return (cfg.api_key.trim().to_string(), KeySource::Config);
            }
        }
    }
    if let Ok(k) = std::fs::read_to_string(&path) {
        if !k.trim().is_empty() {
            return (k.trim().to_string(), KeySource::File(path));
        }
    }
    let key = uuid::Uuid::new_v4().simple().to_string();
    let _ = std::fs::create_dir_all(home);
    if std::fs::write(&path, &key).is_err() {
        // A home that cannot be written to is a real configuration — a read-only container, a
        // locked-down profile. The key still works for this run; it simply will not survive it,
        // and `Generated` is what makes the caller say so.
        tracing::warn!("could not write {}; this key lasts one run", path.display());
        return (key, KeySource::Generated(path));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    (key, KeySource::Generated(path))
}

/// `api_key_in` for the installation this process is running against.
pub fn api_key() -> (String, KeySource) {
    api_key_in(&home_dir())
}

pub fn load() -> Config {
    load_in(&home_dir()).unwrap_or_else(|e| {
        tracing::warn!("invalid configuration ({e}); using defaults");
        Config::default()
    })
}

/// The CLI-owned overlay leaves the user's config.toml and its comments intact.
pub fn load_in(home: &std::path::Path) -> anyhow::Result<Config> {
    let mut value = serde_json::to_value(Config::default())?;
    for name in ["config.toml", "settings.toml"] {
        let path = home.join(name);
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let table: toml::Table = toml::from_str(&text)?;
                for (key, v) in table {
                    anyhow::ensure!(value.get(&key).is_some(), "unknown setting: {key}");
                    value[&key] = serde_json::to_value(v)?;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    let cfg: Config = serde_json::from_value(value)?;
    cfg.validate()?;
    Ok(cfg)
}

impl Config {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            matches!(
                self.browser_identity.as_str(),
                "auto" | "emulated" | "native"
            ),
            "browser_identity must be auto, emulated or native"
        );
        anyhow::ensure!(
            self.auto_max_attempts > 0 && self.auto_max_attempts <= 6,
            "auto_max_attempts must be between 1 and 6"
        );
        anyhow::ensure!(self.request_limit > 0 && self.request_window_seconds > 0
            && self.request_window_seconds <= 86400 && self.request_cooldown_seconds > 0
            && self.request_cooldown_seconds <= 604800 && self.request_min_interval_ms > 0
            && self.request_min_interval_ms <= 60000,
            "request limits must be positive; window <= 1 day, cooldown <= 7 days, interval <= 60000ms");
        anyhow::ensure!(
            crate::types::TIERS.contains(&self.max_tier.as_str()),
            "invalid max_tier"
        );
        anyhow::ensure!(
            matches!(
                self.http_engine.as_str(),
                "auto" | "reqwest" | "impersonate"
            ),
            "invalid http_engine"
        );
        anyhow::ensure!(
            self.parallelism > 0 && self.max_jobs > 0,
            "concurrency must be positive"
        );
        anyhow::ensure!(
            self.warm_wait_ms > 0 && self.warm_max_wait_ms >= self.warm_wait_ms,
            "warm_max_wait_ms must be at least warm_wait_ms, both positive"
        );
        anyhow::ensure!(
            self.browser_timeout_ms > 0,
            "browser_timeout_ms must be positive"
        );
        anyhow::ensure!(
            self.warm_keep_max == 0
                || (self.warm_keep_secs > 0 && self.warm_keep_secs < self.browser_idle_secs),
            "held-page lifetime must be positive and shorter than browser_idle_secs"
        );
        Ok(())
    }
}

pub fn update_in(home: &std::path::Path, patch: serde_json::Value) -> anyhow::Result<Config> {
    let mut value = serde_json::to_value(load_in(home)?)?;
    let patch = patch
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("settings must be a JSON object"))?;
    for (key, v) in patch {
        anyhow::ensure!(value.get(key).is_some(), "unknown setting: {key}");
        value[key] = v.clone();
    }
    let cfg: Config = serde_json::from_value(value)?;
    cfg.validate()?;
    let path = home.join("settings.toml");
    let mut overlay: toml::Table = match std::fs::read_to_string(&path) {
        Ok(s) => toml::from_str(&s)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
        Err(e) => return Err(e.into()),
    };
    for (key, v) in patch {
        overlay.insert(key.clone(), toml::Value::try_from(v)?);
    }
    std::fs::create_dir_all(home)?;
    let tmp = home.join(format!("settings-{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&tmp, toml::to_string_pretty(&overlay)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(cfg)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_are_validated_and_do_not_rewrite_user_configuration() {
        let dir = std::env::temp_dir().join(format!("svipall-settings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let original = "# keep this comment\nparallelism = 2\n";
        std::fs::write(dir.join("config.toml"), original).unwrap();
        let cfg = update_in(
            &dir,
            serde_json::json!({"browser_identity":"native","warm_keep_max":4}),
        )
        .unwrap();
        assert_eq!(cfg.parallelism, 2);
        assert_eq!(load_in(&dir).unwrap().browser_identity, "native");
        assert_eq!(
            std::fs::read_to_string(dir.join("config.toml")).unwrap(),
            original
        );
        assert!(update_in(&dir, serde_json::json!({"browser_identity":"wrong"})).is_err());
        assert!(update_in(&dir, serde_json::json!({"paralellism":3})).is_err());
        assert!(update_in(&dir, serde_json::json!({"warm_max_wait_ms":1})).is_err());
        assert_eq!(load_in(&dir).unwrap().browser_identity, "native");
        update_in(&dir, serde_json::json!({"parallelism":3})).unwrap();
        assert_eq!(load_in(&dir).unwrap().parallelism, 3);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_kept_page_budget_expires_before_the_browser_that_holds_it() {
        // Both ends of this matter and neither is obvious from the numbers alone. Too short and a
        // held page is gone before the clearance it holds would have lapsed anyway; too long and it
        // outlives the browser, which would mean holding a tab in a process kept alive only for it.
        let c = Config::default();
        assert!(
            c.warm_keep_secs > crate::classify::POW_TOKEN_LIFETIME_SECS,
            "a page dropped before the clearance lapses buys nothing"
        );
        assert!(
            c.warm_keep_secs < c.browser_idle_secs,
            "a held page must never be the reason a browser stays open"
        );
        assert_eq!(c.warm_keep_max, 2);
    }

    /// A home of this test's own. `api_key_in` takes a path precisely so these do not have to
    /// touch `SVIPALL_HOME`, and so they cannot interfere with each other.
    fn home(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "svipall-cfg-{}-{}-{}",
            std::process::id(),
            name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("temp home");
        dir
    }

    #[test]
    fn the_key_survives_a_restart() {
        // A key regenerated on every start is a key every script has to be told again. The file is
        // the whole point, so the second call must read it rather than mint a second one.
        let dir = home("restart");
        let (first, source) = api_key_from(None, &dir);
        assert!(matches!(source, KeySource::Generated(_)));
        assert_eq!(first.len(), 32, "a v4 uuid, simple: {first}");
        let (second, source) = api_key_from(None, &dir);
        assert_eq!(first, second);
        assert!(
            matches!(source, KeySource::File(_)),
            "the second run must read the file, not write one"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_pinned_key_in_the_config_beats_the_file() {
        // An operator who wrote a key into config.toml has said which key they want. Handing them
        // a generated one instead would be a silently different answer to a question they answered.
        let dir = home("pinned");
        std::fs::write(dir.join("api_key"), "from-the-file").expect("write");
        std::fs::write(dir.join("config.toml"), "api_key = \"from-the-config\"\n").expect("write");
        let (key, source) = api_key_from(None, &dir);
        assert_eq!(key, "from-the-config");
        assert_eq!(source, KeySource::Config);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_environment_beats_the_file_when_both_have_a_key() {
        // The container case: a pinned key handed in at run time, and a home that may not even be
        // writable. Neither the file nor the config may override what the operator passed.
        let dir = home("env");
        std::fs::write(dir.join("api_key"), "from-the-file").expect("write");
        std::fs::write(dir.join("config.toml"), "api_key = \"from-the-config\"\n").expect("write");
        let (key, source) = api_key_from(Some("from-the-env".into()), &dir);
        assert_eq!(key, "from-the-env");
        assert_eq!(source, KeySource::Env);
        // And it did not quietly rewrite the file it ignored.
        assert_eq!(
            std::fs::read_to_string(dir.join("api_key")).expect("read"),
            "from-the-file"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_environment_variable_is_not_a_key() {
        // `-e SVIPALL_API_KEY=` in a compose file is a variable that exists and says nothing.
        // Treating it as a secret would set the key to the empty string, which opens the door.
        let dir = home("env-empty");
        std::fs::write(dir.join("api_key"), "from-the-file").expect("write");
        let (key, source) = api_key_from(Some("  ".into()), &dir);
        assert_eq!(key, "from-the-file");
        assert!(matches!(source, KeySource::File(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_key_in_the_config_is_not_a_key() {
        // `api_key = ""` is the default written into the documented toml block. Reading it as a
        // secret would mean every operator who copied that block shipped an empty bearer token.
        let dir = home("empty");
        std::fs::write(dir.join("config.toml"), "api_key = \"\"\n").expect("write");
        let (key, source) = api_key_from(None, &dir);
        assert!(!key.is_empty());
        assert!(matches!(source, KeySource::Generated(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_rest_api_is_off_until_it_is_asked_for() {
        // The API can fetch and can read this machine's logged-in profiles. Installing svipall
        // must not be the same act as opening it.
        let cfg = Config::default();
        assert_eq!(cfg.rest_port, 0);
        assert_eq!(cfg.rest_bind, "127.0.0.1");
        assert!(cfg.api_key.is_empty());
        assert_eq!(cfg.max_jobs, 2);
    }

    #[test]
    fn a_config_file_that_says_nothing_about_the_rest_api_still_parses() {
        // Every field is `#[serde(default)]`, and this is the test that says so for the new ones:
        // an existing installation's config.toml must not start failing to load.
        let cfg: Config = toml::from_str("parallelism = 8\n").expect("partial config");
        assert_eq!(cfg.parallelism, 8);
        assert_eq!(cfg.rest_port, 0);
        assert_eq!(cfg.max_jobs, 2);
    }
}
