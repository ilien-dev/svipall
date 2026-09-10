//! What the release job builds, asserted where it is cheap to assert.
//!
//! Three of the five release targets are built without the model features, because the ONNX
//! Runtime binaries `ort` downloads will not link there: x86_64 macOS has no build published at
//! the pinned version, and the Linux artefacts are built on 22.04 so they start on Debian 12.
//! `.github/workflows/release.yml` expresses that as `--no-default-features --features
//! impersonate`, and that flag is only as good as the feature graph behind it.
//!
//! Cargo resolves features per package, then unifies them across every package it selects. The
//! release job names binaries, not a package, so the whole workspace is selected — and a member
//! that depends on `svipall` with its defaults on drags `local-models`, and therefore
//! `dep:ort`, back into the same build. This is not hypothetical: it is why the `v1.0.0-rc.2`
//! release failed on three targets with `undefined symbol: __isoc23_strtoll` out of
//! `libort_sys`, hours after `local-models` joined `default`.
//!
//! A member that wants inference asks for it by feature (`bench`'s own `onnx` names
//! `svipall/onnx-detect` and `onnx-segment` outright). Nobody gets it by default.

use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/svipall sits two levels under the workspace root")
        .to_path_buf()
}

/// The `members = [...]` list of the virtual workspace manifest, as written.
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

/// Every dependency line naming `svipall`, whichever table it sits in. The name is a prefix
/// of every other crate here, so it is matched with its separator rather than on its own.
fn mcp_dependency_lines(manifest: &str) -> Vec<&str> {
    manifest
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("svipall =") || line.starts_with("svipall="))
        .collect()
}

#[test]
fn no_workspace_member_takes_svipall_with_its_defaults() {
    let root = workspace_root();
    let members = members(&root);
    assert!(
        members.iter().any(|m| m == "bench"),
        "bench is a workspace member and not under crates/; if that changed, this test is stale"
    );

    for member in members {
        if member == "crates/svipall" {
            continue;
        }
        let manifest_path = root.join(&member).join("Cargo.toml");
        let manifest = fs::read_to_string(&manifest_path)
            .unwrap_or_else(|e| panic!("{}: {e}", manifest_path.display()));

        for line in mcp_dependency_lines(&manifest) {
            assert!(
                line.contains("default-features = false"),
                "{member} depends on svipall with its default features, which turns on \
                 local-models and so ort for the whole workspace build. The release job builds \
                 three targets without ort and they will not link. Ask for the features you \
                 need by name.\n    {line}"
            );
        }
    }
}

/// The reason the rule above exists, stated where a reader of the rule will look for it.
#[test]
fn local_models_is_a_default_and_pulls_ort() {
    let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .expect("svipall Cargo.toml");
    assert!(
        manifest.contains(r#"default = ["impersonate", "local-models"]"#),
        "if local-models leaves the defaults, the invariant next door is no longer needed"
    );
    for feature in ["onnx-grid", "onnx-detect", "onnx-segment"] {
        assert!(
            manifest
                .lines()
                .any(|l| l.trim_start().starts_with(feature) && l.contains("dep:ort")),
            "{feature} is expected to be one of the features that pulls ort"
        );
    }
}

/// One job of `release.yml`, from its `  name:` line to the next line at the same indent.
fn job<'a>(workflow: &'a str, name: &str) -> Option<&'a str> {
    let start = workflow.find(&format!("\n  {name}:\n"))? + 1;
    let body = &workflow[start..];
    let end = body
        .match_indices("\n  ")
        .map(|(i, _)| i)
        .find(|&i| !body[i + 3..].starts_with(' '))
        .unwrap_or(body.len());
    Some(&body[..end])
}

/// Every download names what it takes. With neither `name` nor `pattern` it takes every artefact
/// in the run, and the image jobs leave `.dockerbuild` records there that `download-artifact`
/// cannot extract: that is how the first `1.0.0-rc.3` run died in `packages`, every binary built.
#[test]
fn every_artifact_download_names_what_it_takes() {
    let workflow = release_yml();
    let lines: Vec<&str> = workflow.lines().collect();
    let mut seen = 0;
    for (i, line) in lines.iter().enumerate() {
        if !line.contains("uses: actions/download-artifact") {
            continue;
        }
        seen += 1;
        let indent = line.len() - line.trim_start().len();
        // The step's own keys sit deeper than its `- uses:` line, up to the next step.
        let step: Vec<&str> = lines[i + 1..]
            .iter()
            .take_while(|l| l.trim().is_empty() || l.len() - l.trim_start().len() > indent)
            .copied()
            .collect();
        assert!(
            step.iter().any(|l| {
                let l = l.trim_start();
                l.starts_with("name:") || l.starts_with("pattern:")
            }),
            "release.yml:{} downloads every artefact in the run; give it a `name` or a `pattern`",
            i + 1
        );
    }
    assert!(
        seen > 0,
        "release.yml downloads no artefacts; this test is stale"
    );
}

fn release_yml() -> String {
    fs::read_to_string(workspace_root().join(".github/workflows/release.yml"))
        .expect("release.yml")
        .replace("\r\n", "\n")
}

/// A pre-release left npm's `latest` on `1.0.0-rc` after `1.0.0-rc.3` shipped, and no image had a
/// `latest` at all, because every release so far is a pre-release. A moving tag stays put for a
/// pre-release only once a stable release exists to hold it; `version` decides that, once.
#[test]
fn a_pre_release_moves_latest_until_a_stable_release_exists() {
    let workflow = release_yml();
    let version = job(&workflow, "version").expect("a `version` job");
    assert!(
        version.contains("moving: ${{ steps.v.outputs.moving }}"),
        "`version` must export `moving`"
    );
    assert!(
        version.contains("git tag -l"),
        "`moving` is read from the tags"
    );
    for name in ["npm", "image-manifest"] {
        let job = job(&workflow, name).unwrap_or_else(|| panic!("a `{name}` job"));
        assert!(
            job.contains("needs.version.outputs.moving"),
            "`{name}` must move its tag on `moving`"
        );
        assert!(
            !job.contains("needs.version.outputs.prerelease"),
            "`{name}` still decides its tag on `prerelease`"
        );
    }
}

/// A re-run with every crate already on crates.io died asking for a token it did not need.
#[test]
fn crates_asks_for_a_token_only_when_a_crate_is_missing() {
    let workflow = release_yml();
    let crates = job(&workflow, "crates").expect("a `crates` job");
    let auth = crates
        .split("\n      - ")
        .find(|step| step.contains("crates-io-auth-action"))
        .expect("the crates job authenticates");
    assert!(
        auth.contains("if: steps.missing.outputs.crates != ''"),
        "authentication must wait on a missing crate:\n{auth}"
    );
}

/// Every crate the workspace publishes, as `cargo metadata` would list it. The trusted publishing
/// script reads the same list rather than keeping its own.
#[test]
fn trusted_publishing_setup_covers_every_published_crate() {
    let root = workspace_root();
    let script = fs::read_to_string(root.join("scripts/crates-trusted-publishing.sh"))
        .expect("scripts/crates-trusted-publishing.sh");
    for needle in [
        "cargo metadata",
        "/api/v1/trusted_publishing/github_configs",
        "\"workflow_filename\": \"release.yml\"",
        "read -rs",
    ] {
        assert!(script.contains(needle), "the script lacks {needle}");
    }
    // The `crates="..."` lines of the publish step, in the order they are written.
    let workflow = release_yml();
    let listed: Vec<&str> = job(&workflow, "crates")
        .expect("a `crates` job")
        .lines()
        .filter_map(|l| l.trim().strip_prefix("crates=\""))
        .flat_map(|l| l.trim_end_matches('"').split_whitespace())
        .filter(|c| *c != "$crates")
        .collect();
    for member in members(&root) {
        let manifest = fs::read_to_string(root.join(&member).join("Cargo.toml")).expect(&member);
        let name = manifest
            .lines()
            .find_map(|l| l.trim().strip_prefix("name = \""))
            .and_then(|l| l.strip_suffix('"'))
            .expect("a package name");
        assert_eq!(
            listed.contains(&name),
            !manifest.contains("publish = false"),
            "{name}: release.yml's crate list and `publish` in its manifest disagree"
        );
    }
}

/// The tap and the bucket follow every release by themselves. Left to a person, they stayed on
/// the first release while npm, crates.io and the image moved on.
#[test]
fn every_release_pushes_the_tap_and_the_bucket() {
    let root = workspace_root();
    // A Windows checkout is CRLF; the job is found by its lines.
    let read = |p: &str| {
        fs::read_to_string(root.join(p))
            .expect(p)
            .replace("\r\n", "\n")
    };
    let workflow = read(".github/workflows/release.yml");
    let render = read("scripts/render-packaging.sh");
    let job = job(&workflow, "tap-bucket").expect("release.yml has a `tap-bucket` job");

    // The manifests point at release assets, so they must not land before the release exists.
    let needs = job.lines().find(|l| l.trim_start().starts_with("needs:"));
    assert!(
        needs.is_some_and(|l| l.contains("publish")),
        "tap-bucket must wait on `publish`"
    );
    for (repo, from, to) in [
        (
            "ilien-dev/homebrew-svipall",
            "homebrew/svipall.rb",
            "Formula/svipall.rb",
        ),
        (
            "ilien-dev/scoop-svipall",
            "scoop/svipall.json",
            "bucket/svipall.json",
        ),
    ] {
        assert!(
            render.contains(&format!("\"$out/{from}\"")),
            "render-packaging.sh no longer writes {from}"
        );
        for needle in [repo, from, to] {
            assert!(job.contains(needle), "tap-bucket does not mention {needle}");
        }
    }
}
