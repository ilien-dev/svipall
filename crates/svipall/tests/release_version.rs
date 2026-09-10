//! One version, written once, and a workspace crates.io can actually accept.
//!
//! Five files used to claim the version independently — the crate, the plugin manifest, the npm
//! wrapper, the git tag and now every internal dependency line — and each one is a different way
//! to be wrong. Claude Code silently keeps a cached plugin when `plugin.json` repeats a version it
//! has already seen; the npm postinstall builds its download URL from its own version, so a
//! package one behind fetches an archive that does not exist; and a `path` dependency without a
//! `version` is rejected by crates.io at publish time, which is the worst place to find out.
//!
//! So the root manifest's `[workspace.package] version` is the only place the number is written by
//! hand. `scripts/sync-version.sh` propagates it; this asserts it propagated.

use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/svipall sits two levels under the workspace root")
        .to_path_buf()
}

fn members(root: &Path) -> Vec<String> {
    let manifest = fs::read_to_string(root.join("Cargo.toml")).expect("workspace Cargo.toml");
    let list = manifest
        .split_once("members = [")
        .expect("the workspace declares members")
        .1
        .split_once(']')
        .expect("the members list closes")
        .0;
    list.lines()
        .filter_map(|line| line.trim().trim_end_matches(',').strip_prefix('"'))
        .filter_map(|line| line.strip_suffix('"'))
        .map(str::to_string)
        .collect()
}

/// The one hand-written version: `[workspace.package] version`.
fn workspace_version(root: &Path) -> String {
    let manifest = fs::read_to_string(root.join("Cargo.toml")).expect("workspace Cargo.toml");
    let table = manifest
        .split_once("[workspace.package]")
        .expect("the workspace declares [workspace.package], the single source of the version")
        .1;
    table
        .lines()
        .find_map(|line| line.trim().strip_prefix("version = "))
        .expect("[workspace.package] names a version")
        .trim()
        .trim_matches('"')
        .to_string()
}

/// The value of a top-level string field of a small, flat JSON file. Enough for two manifests we
/// write ourselves, and one less dependency than a parser for them.
fn json_field(path: &Path, field: &str) -> String {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let needle = format!("\"{field}\":");
    let rest = text
        .split_once(needle.as_str())
        .unwrap_or_else(|| panic!("{} has no {field}", path.display()))
        .1;
    rest.split('"')
        .nth(1)
        .unwrap_or_else(|| panic!("{} has an unquoted {field}", path.display()))
        .to_string()
}

/// Every dependency line naming another crate of this workspace, whichever table it sits in.
fn internal_dependency_lines(manifest: &str) -> Vec<&str> {
    manifest
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .filter(|line| line.starts_with("svipall") && line.contains('='))
        .filter(|line| line.contains("path =") || line.contains("workspace = true"))
        .collect()
}

/// The `[workspace.dependencies]` line for a crate of this workspace.
fn workspace_dependency(root: &Path, name: &str) -> String {
    let manifest = fs::read_to_string(root.join("Cargo.toml")).expect("workspace Cargo.toml");
    manifest
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with(name) && line.contains('='))
        .unwrap_or_else(|| panic!("the workspace declares {name}"))
        .to_string()
}

fn publishes(manifest: &str) -> bool {
    !manifest
        .lines()
        .any(|line| line.trim().starts_with("publish = false"))
}

#[test]
fn every_member_inherits_the_workspace_version() {
    let root = workspace_root();
    for member in members(&root) {
        let path = root.join(&member).join("Cargo.toml");
        let manifest = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{member}: {e}"));
        assert!(
            manifest.contains("version.workspace = true"),
            "{member} writes its own version. The workspace root is the only place the number is \
             written by hand; say `version.workspace = true` here and run scripts/sync-version"
        );
    }
}

#[test]
fn every_manifest_that_repeats_the_version_is_on_it() {
    let root = workspace_root();
    let version = workspace_version(&root);
    for (path, what) in [
        (
            root.join("plugins/svipall/.claude-plugin/plugin.json"),
            "Claude Code keeps a cached plugin when it sees a version it already has",
        ),
        (
            root.join("packaging/npm/package.json"),
            "the npm postinstall builds its download URL from its own version",
        ),
        (
            root.join("server.json"),
            "the MCP Registry hands out this entry, and every artefact it names has to exist",
        ),
    ] {
        assert_eq!(
            json_field(&path, "version"),
            version,
            "{} is not on {version} — {what}. Run scripts/sync-version",
            path.display()
        );
    }
}

/// `server.json` says the version four times, not once: the server, then each of the three packages
/// it points at. Two of those are not a `version` field at all — the container tag is written into
/// the middle of an identifier — and an entry that names an artefact nobody published is an install
/// that fails on somebody else's machine.
#[test]
fn the_registry_entry_names_this_version_in_every_package_it_points_at() {
    let root = workspace_root();
    let version = workspace_version(&root);
    let entry = fs::read_to_string(root.join("server.json")).expect("server.json");

    let versions = entry
        .matches(&format!("\"version\": \"{version}\""))
        .count();
    assert_eq!(
        versions, 3,
        "server.json names {version} in {versions} version fields; the server and its npm and \
         cargo packages are three. Run scripts/sync-version"
    );
    assert!(
        entry.contains(&format!("ghcr.io/ilien-dev/svipall:{version}\"")),
        "the OCI package in server.json points at a container tag that is not {version}. \
         Run scripts/sync-version"
    );
}

#[test]
fn every_internal_dependency_carries_the_version_crates_io_needs() {
    let root = workspace_root();
    let version = workspace_version(&root);
    let pinned = format!("version = \"{version}\"");

    for member in members(&root) {
        let path = root.join(&member).join("Cargo.toml");
        let manifest = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{member}: {e}"));
        if !publishes(&manifest) {
            continue;
        }
        for line in internal_dependency_lines(&manifest) {
            // Inherited from `[workspace.dependencies]`, which is checked here too, or written out.
            if line.contains("workspace = true") {
                let name = line
                    .split_whitespace()
                    .next()
                    .expect("a dependency has a name");
                assert!(
                    workspace_dependency(&root, name).contains(&pinned),
                    "the workspace entry for {name} carries no version, so every member \
                     inheriting it publishes a path dependency crates.io will reject"
                );
                continue;
            }
            assert!(
                line.contains(&pinned),
                "{member} takes a workspace crate by path alone. crates.io rejects a path \
                 dependency without a version, and it does it after the crates published before \
                 it in the same run are already permanent.\n    {line}\n    expected: {pinned}"
            );
        }
    }
}

#[test]
fn nothing_publishable_depends_on_something_unpublishable() {
    let root = workspace_root();
    let unpublishable: Vec<String> = members(&root)
        .into_iter()
        .filter(|member| {
            let manifest = fs::read_to_string(root.join(member).join("Cargo.toml"))
                .unwrap_or_else(|e| panic!("{member}: {e}"));
            !publishes(&manifest)
        })
        .filter_map(|member| member.rsplit('/').next().map(str::to_string))
        .collect();

    for member in members(&root) {
        let manifest = fs::read_to_string(root.join(&member).join("Cargo.toml"))
            .unwrap_or_else(|e| panic!("{member}: {e}"));
        if !publishes(&manifest) {
            continue;
        }
        for line in internal_dependency_lines(&manifest) {
            for name in &unpublishable {
                assert!(
                    !line.starts_with(name.as_str()),
                    "{member} is published and depends on {name}, which is not. crates.io resolves \
                     optional dependencies too, so the feature it hides behind does not save it."
                );
            }
        }
    }
}
