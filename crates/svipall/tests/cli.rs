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
