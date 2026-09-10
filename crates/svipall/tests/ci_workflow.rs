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
