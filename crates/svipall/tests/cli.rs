//! The `svipall` binary, driven the way an agent drives it: spawned, with arguments, read back.
//!
//! Every run gets a home of its own with automatic browser installation off, so nothing here can
//! start a download or read the developer's real `~/.svipall`.

use std::process::{Command, Output};

fn home(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "svipall-cli-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("config.toml"),
        "browser_auto_install = false\nbrowser_auto_update = false\n",
    )
    .unwrap();
    dir
}

fn svipall(home: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_svipall"))
        .args(args)
        .env("SVIPALL_HOME", home)
        .env_remove("SVIPALL_BROWSER")
        .output()
        .expect("the binary runs")
}

fn text(o: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&o.stdout).to_string(),
        String::from_utf8_lossy(&o.stderr).to_string(),
    )
}

#[test]
fn help_on_a_subcommand_prints_that_subcommands_help_and_runs_nothing() {
    let h = home("help");
    for flag in ["--help", "-h"] {
        let o = svipall(&h, &["log", flag]);
        let (out, err) = text(&o);
        assert!(o.status.success(), "exit {:?}: {out}{err}", o.status);
        let both = format!("{out}{err}");
        assert!(both.contains("svipall log"), "names the command: {both}");
        assert!(both.contains("--domain"), "lists its flags: {both}");
        assert!(
            !out.contains("\"requests\""),
            "the command itself did not run: {out}"
        );
        assert!(
            !both.contains("svipall crawl"),
            "only this subcommand's help, not the whole usage: {both}"
        );
    }
    // The same for a command that would otherwise reach the network.
    let o = svipall(&h, &["fetch", "--help"]);
    let (out, err) = text(&o);
    assert!(o.status.success(), "{out}{err}");
    assert!(format!("{out}{err}").contains("--query"), "{out}{err}");
    // And `help <command>` says the same thing.
    let o = svipall(&h, &["help", "map"]);
    let (out, err) = text(&o);
    assert!(o.status.success(), "{out}{err}");
    assert!(format!("{out}{err}").contains("svipall map"), "{out}{err}");
}

#[test]
fn an_unknown_flag_is_an_error_that_names_it_and_the_closest_valid_one() {
    let h = home("flags");
    let o = svipall(&h, &["log", "--view", "summary", "--since-secs", "5000000"]);
    let (out, err) = text(&o);
    assert!(!o.status.success(), "an unknown flag must fail: {out}{err}");
    assert!(err.contains("--since-secs"), "names the flag: {err}");
    assert!(
        err.contains("--since"),
        "names the closest valid flag: {err}"
    );
    assert!(out.trim().is_empty(), "no answer was printed: {out}");

    let o = svipall(&h, &["fetch", "https://x.test/", "--qeury", "cache"]);
    let (_, err) = text(&o);
    assert!(!o.status.success());
    assert!(
        err.contains("--qeury") && err.contains("--query"),
        "a typo is named with its fix: {err}"
    );

    // A flag that takes a value and was given none is an error too, not a switch.
    let o = svipall(&h, &["log", "--domain"]);
    let (_, err) = text(&o);
    assert!(!o.status.success());
    assert!(err.contains("--domain"), "{err}");
}

#[test]
fn a_value_that_looks_like_a_flag_is_refused_naming_the_equals_form() {
    let h = home("dashvalue");
    let o = svipall(&h, &["fetch", "https://x.test/", "--query", "--effort"]);
    let (out, err) = text(&o);
    assert!(!o.status.success(), "refused, not guessed: {out}{err}");
    assert!(
        !err.contains("unknown flag"),
        "--effort was meant as the value, not a flag: {err}"
    );
    assert!(
        err.contains("--query") && err.contains("--query=--effort"),
        "says why and names the form that works: {err}"
    );
    assert!(out.trim().is_empty(), "nothing ran: {out}");

    // The per-command help says it, once.
    let o = svipall(&h, &["fetch", "--help"]);
    let (out, err) = text(&o);
    let both = format!("{out}{err}");
    assert_eq!(
        both.matches("--flag=value").count(),
        1,
        "the = form is in the help once: {both}"
    );
}

#[test]
fn the_view_flag_of_log_is_honoured() {
    // `--view summary` used to be ignored and the recent view came back.
    let h = home("view");
    let o = svipall(&h, &["log", "--view", "summary", "--since", "5000000"]);
    let (out, err) = text(&o);
    assert!(o.status.success(), "{out}{err}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("one JSON object");
    assert_eq!(v["view"], "summary", "{v}");
    assert_eq!(v["since_secs"], 5_000_000, "{v}");
}

#[test]
fn an_unknown_command_names_the_closest_one() {
    let h = home("cmd");
    let o = svipall(&h, &["fecth", "https://x.test/"]);
    let (_, err) = text(&o);
    assert!(!o.status.success());
    assert!(err.contains("fecth") && err.contains("fetch"), "{err}");
}

mod support;

/// A home whose settings make `host` a public-looking site reached through the loopback fixture,
/// as `web_route` would, so the limiter and the ladder memory run as they do for a real site.
fn routed_home(tag: &str, host: &str, site: &support::Site, settings: &str) -> std::path::PathBuf {
    let h = home(tag);
    let mut config = std::fs::read_to_string(h.join("config.toml")).unwrap();
    config.push_str(settings);
    config.push_str("request_min_interval_ms = 1\nhttp_engine = \"reqwest\"\n");
    std::fs::write(h.join("config.toml"), config).unwrap();
    std::fs::write(
        h.join("proxies.json"),
        serde_json::json!({ host: site.url("") }).to_string(),
    )
    .unwrap();
    h
}

async fn run(home: &std::path::Path, args: &[&str]) -> serde_json::Value {
    let (home, args): (std::path::PathBuf, Vec<String>) = (
        home.to_path_buf(),
        args.iter().map(|s| s.to_string()).collect(),
    );
    let o = tokio::task::spawn_blocking(move || {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        svipall(&home, &args)
    })
    .await
    .unwrap();
    let (out, err) = text(&o);
    serde_json::from_str(&out).unwrap_or_else(|_| panic!("not one JSON object: {out}{err}"))
}

/// `--wait` reaches the limiter, and the window it waits out is the one the previous process
/// spent: the budget is the machine's, not the process's.
#[tokio::test(flavor = "multi_thread")]
async fn fetch_wait_waits_out_a_window_another_process_filled() {
    let url = "http://cli-wait.test/article";
    let site = support::Site::start(vec![(
        url,
        support::Reply::page("Wait", &[]).header("cache-control", "no-store"),
    )])
    .await;
    let h = routed_home(
        "wait",
        "cli-wait.test",
        &site,
        "max_tier = \"http\"\nrequest_limit = 1\nrequest_window_seconds = 4\n",
    );
    let first = run(&h, &["fetch", url, "--timeout", "2000"]).await;
    assert_eq!(first["status"], 200, "{first}");
    let refused = run(&h, &["fetch", url, "--timeout", "2000", "--wait", "0"]).await;
    assert_eq!(refused["wall_kind"], "visit_limit", "{refused}");
    let waited = run(&h, &["fetch", url, "--timeout", "2000", "--wait", "8"]).await;
    assert_eq!(waited["status"], 200, "{waited}");
    assert!(
        waited["waited"]["seconds"].as_u64().unwrap_or(0) >= 1,
        "{waited}"
    );
    assert_eq!(site.hits(url), 2);
}

/// `--fresh` walks a ladder the previous process remembered as hopeless.
#[tokio::test(flavor = "multi_thread")]
async fn fetch_fresh_walks_again_a_ladder_another_process_remembered() {
    let url = "http://cli-fresh.test/article";
    let site = support::Site::start(vec![(url, support::Reply::cloudflare())]).await;
    // A browser that cannot start: every browser rung fails at once, so the walk is milliseconds.
    let fake = std::env::temp_dir().join(format!(
        "svipall-cli-not-a-browser-{}.txt",
        std::process::id()
    ));
    std::fs::write(&fake, "this is not a browser").unwrap();
    let h = routed_home(
        "fresh",
        "cli-fresh.test",
        &site,
        &format!(
            "max_tier = \"warm\"\nbrowser_path = {:?}\n",
            fake.to_string_lossy()
        ),
    );
    let first = run(&h, &["fetch", url]).await;
    assert!(first["blocked_reason"].is_string(), "{first}");
    let hits = site.hits(url);
    let remembered = run(&h, &["fetch", url]).await;
    assert!(remembered["remembered"].is_object(), "{remembered}");
    assert_eq!(site.hits(url), hits, "nothing was requested: {remembered}");
    let fresh = run(&h, &["fetch", url, "--fresh"]).await;
    assert!(fresh["remembered"].is_null(), "{fresh}");
    assert!(
        site.hits(url) > hits,
        "--fresh asked the site again: {fresh}"
    );
}
