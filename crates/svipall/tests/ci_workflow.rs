//! What `ci.yml` runs, and what it stops running.
//!
//! A pull request pushed and merged a minute apart left its own run building for forty minutes
//! next to the push to `main`, on the same tree. The push to `main` is the run that counts; a
//! pull request run is worth nothing once a newer push or the merge has superseded it.

use std::fs;
use std::path::Path;

fn ci() -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/svipall sits two levels under the workspace root");
    fs::read_to_string(root.join(".github/workflows/ci.yml"))
        .expect("ci.yml")
        .replace("\r\n", "\n")
}

/// The value of the first `key:` line, trimmed, anywhere in the file.
fn value<'a>(workflow: &'a str, key: &str) -> Option<&'a str> {
    workflow
        .lines()
        .find_map(|l| l.trim_start().strip_prefix(&format!("{key}:")))
        .map(str::trim)
}

#[test]
fn a_superseded_pull_request_run_is_cancelled() {
    let ci = ci();
    assert!(
        ci.contains("\nconcurrency:\n"),
        "ci.yml has no concurrency group"
    );
    // One group per pull request, and one per run otherwise: two pushes to `main` must never
    // cancel or queue behind each other.
    let group = value(&ci, "group").expect("a concurrency group");
    assert!(
        group.contains("github.event.pull_request.number") && group.contains("github.run_id"),
        "the group must be the pull request, or the run itself: {group}"
    );
    let cancel = value(&ci, "cancel-in-progress").expect("cancel-in-progress");
    assert!(
        cancel.contains("github.event_name == 'pull_request'"),
        "only pull request runs are cancelled: {cancel}"
    );
}

/// A merge is not a push to the pull request, so without `closed` its last run keeps going.
/// The run `closed` starts only takes the group; it must build nothing.
#[test]
fn merging_a_pull_request_cancels_its_run() {
    let ci = ci();
    let types = value(&ci, "types").expect("pull_request lists its activity types");
    for t in ["opened", "synchronize", "reopened", "closed"] {
        assert!(types.contains(t), "pull_request types lack {t}: {types}");
    }
    assert!(
        ci.contains("if: github.event.action != 'closed'"),
        "the run a closed pull request starts must skip the build"
    );
}

/// `@stable` moved to 1.99 under an unchanged tree and its new lint failed every job on `main`:
/// a red run the code did not cause. The toolchain is a pinned version, written once in
/// `rust-toolchain.toml`, and every workflow installs that same one; a new release arrives as a
/// change of its own, with its lints fixed in it.
#[test]
fn every_workflow_installs_the_pinned_toolchain() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/svipall sits two levels under the workspace root");
    let toml = fs::read_to_string(root.join("rust-toolchain.toml"))
        .expect("rust-toolchain.toml")
        .replace("\r\n", "\n");
    let channel = value(&toml, "channel")
        .expect("rust-toolchain.toml names a channel")
        .trim_matches('"');
    assert!(
        channel.split('.').count() == 3 && channel.split('.').all(|n| n.parse::<u32>().is_ok()),
        "the channel is an exact version, not {channel}"
    );
    for c in ["clippy", "rustfmt"] {
        assert!(toml.contains(&format!("\"{c}\"")), "{c} is not a component");
    }
    let pinned = format!("dtolnay/rust-toolchain@{channel}");
    let mut steps = 0;
    for entry in fs::read_dir(root.join(".github/workflows")).expect("workflows") {
        let path = entry.expect("entry").path();
        let text = fs::read_to_string(&path).expect("workflow");
        for line in text.lines().filter(|l| l.contains("dtolnay/rust-toolchain@")) {
            steps += 1;
            assert!(line.contains(&pinned), "{}: {}", path.display(), line.trim());
        }
    }
    assert!(steps > 0, "no workflow installs a toolchain");
}

/// v1.3.0 shipped from a `main` whose `ci` was red: `release.yml` ran on the same push, beside
/// `ci` rather than after it. Every job now waits on `ci-green`, which waits for `ci` on the same
/// commit. `release.yml` keeps its own trigger: npm trusts it by name, and a workflow `ci.yml`
/// called would publish under `ci.yml`'s.
#[test]
fn a_release_waits_for_a_green_ci() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/svipall sits two levels under the workspace root");
    let release = fs::read_to_string(root.join(".github/workflows/release.yml"))
        .expect("release.yml")
        .replace("\r\n", "\n");
    assert!(
        !release.contains("workflow_call"),
        "release.yml must publish under its own name"
    );
    let gate = release
        .split_once("\n  ci-green:\n")
        .map(|(_, rest)| rest.split_once("\n\n").map_or(rest, |(job, _)| job))
        .expect("release.yml has a `ci-green` job");
    for needle in [
        "--workflow ci.yml",
        "--commit \"$GITHUB_SHA\"",
        "\"completed success \"*) ",
        "refs/heads/main",
    ] {
        assert!(gate.contains(needle), "ci-green lacks {needle}: {gate}");
    }
    let version = release
        .split_once("\n  version:\n")
        .map(|(_, rest)| rest.split_once("\n    steps:").map_or(rest, |(head, _)| head))
        .expect("release.yml has a `version` job");
    assert!(
        version.contains("needs: ci-green"),
        "`version`, which every other job needs, does not wait for ci: {version}"
    );
}
