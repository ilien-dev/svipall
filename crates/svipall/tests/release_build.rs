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

/// `latest` on npm and `latest`/`slim` on the image move for a stable release only. Every
/// pre-release — `-rc`, `-beta`, `-alpha`, anything with a hyphen — waits under its version tag
/// and npm's `next`, however old `latest` is: the operator's decision, not a rule to relax.
#[test]
fn only_a_stable_release_moves_latest() {
    let workflow = release_yml();
    assert!(
        !workflow.contains("outputs.moving"),
        "no second notion of `moving`: `prerelease` decides"
    );
    let npm = job(&workflow, "npm").expect("an `npm` job");
    assert!(
        npm.contains("tag=latest")
            && npm.contains("if [ \"${{ needs.version.outputs.prerelease }}\" = \"true\" ]")
            && npm.contains("tag=next"),
        "npm publishes a pre-release under `next`, and only a stable one under `latest`"
    );
    let image = job(&workflow, "image-manifest").expect("an `image-manifest` job");
    assert!(
        image.contains("if [ \"${{ needs.version.outputs.prerelease }}\" != \"true\" ]"),
        "the image's moving tags follow stable releases only"
    );
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
        auth.contains(
            "if: steps.missing.outputs.crates != '' && steps.missing.outputs.stored != 'true'"
        ),
        "OIDC must wait on a missing crate, and step aside for a stored token:\n{auth}"
    );
}

/// A stored `CARGO_REGISTRY_TOKEN` publishes whatever is missing, a crate crates.io has never seen
/// included, which OIDC cannot. It reaches only the two steps that need it.
#[test]
fn a_stored_registry_token_publishes_without_oidc() {
    let workflow = release_yml();
    let crates = job(&workflow, "crates").expect("a `crates` job");
    let steps: Vec<&str> = crates.split("\n      - ").collect();
    let with_secret: Vec<&&str> = steps
        .iter()
        .filter(|s| s.contains("secrets.CARGO_REGISTRY_TOKEN"))
        .collect();
    assert_eq!(
        with_secret.len(),
        2,
        "the secret belongs to `Missing crates` and `Publish` alone"
    );
    let publish = steps
        .iter()
        .find(|s| s.starts_with("name: Publish"))
        .expect("a Publish step");
    assert!(
        publish.contains("secrets.CARGO_REGISTRY_TOKEN || steps.auth.outputs.token"),
        "Publish must prefer the stored token and fall back to OIDC:\n{publish}"
    );
    assert!(
        !crates.lines().any(|l| l.starts_with("    env:")),
        "no job-level env: every step would see the secret"
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

/// crates.io answers a request with no User-Agent with a 403 that reads like "not published":
/// 1.0.0's registry check took nine published crates for missing that way and never submitted.
#[test]
fn every_crates_io_request_carries_a_user_agent() {
    let dir = workspace_root().join(".github/workflows");
    let mut seen = 0;
    for entry in fs::read_dir(&dir).expect(".github/workflows") {
        let path = entry.expect("a workflow").path();
        let text = fs::read_to_string(&path)
            .expect("workflow")
            .replace("\r\n", "\n");
        for (i, line) in text.lines().enumerate() {
            if line.contains("curl ") && line.contains("crates.io/api") {
                seen += 1;
                assert!(
                    line.contains(" -A "),
                    "{}:{} asks crates.io without a User-Agent:\n{line}",
                    path.display(),
                    i + 1
                );
            }
        }
    }
    assert!(
        seen > 0,
        "no workflow asks crates.io anything; this test is stale"
    );
}

/// A pasted secret keeps its trailing newline, and `mcp-publisher` rejects the key on that byte
/// alone (`invalid byte: U+000A`): the first dispatch of 1.0.0's entry. The key is stripped of
/// whitespace, checked for shape, and only then handed over.
#[test]
fn the_registry_key_is_read_without_its_whitespace() {
    let own = fs::read_to_string(workspace_root().join(".github/workflows/mcp-registry.yml"))
        .expect("mcp-registry.yml")
        .replace("\r\n", "\n");
    for needle in [
        "tr -d '[:space:]'",
        "[0-9a-fA-F]{64}",
        "--private-key \"$key\"",
    ] {
        assert!(own.contains(needle), "mcp-registry.yml lacks {needle}");
    }
    assert!(
        !own.contains("--private-key \"$MCP_PRIVATE_KEY\""),
        "the raw secret must not reach mcp-publisher"
    );
}

/// The registry's `?search=` finds nothing for the full name `dev.ilien.svipall/mcp`, even with
/// 1.0.0 published, so a second send would have been refused as a duplicate instead of skipped.
/// The server's own versions endpoint answers exactly.
#[test]
fn the_registry_is_asked_for_this_server_by_name() {
    let own = fs::read_to_string(workspace_root().join(".github/workflows/mcp-registry.yml"))
        .expect("mcp-registry.yml")
        .replace("\r\n", "\n");
    assert!(
        own.contains("/v0.1/servers/dev.ilien.svipall%2Fmcp/versions"),
        "mcp-registry.yml must read the server's own versions"
    );
    assert!(
        !own.contains("servers?search="),
        "`?search=` does not find the full name"
    );
}

/// A release that publishes everything but the registry entry has no way back through
/// `release.yml`: its re-run reuses the broken file, and `main` will not release a tagged
/// version twice. The entry lives in its own workflow, which the release calls and a person can
/// dispatch.
#[test]
fn the_mcp_registry_entry_can_be_sent_on_its_own() {
    let root = workspace_root();
    let own = fs::read_to_string(root.join(".github/workflows/mcp-registry.yml"))
        .expect("mcp-registry.yml")
        .replace("\r\n", "\n");
    for needle in ["workflow_call:", "workflow_dispatch:", "MCP_PRIVATE_KEY"] {
        assert!(own.contains(needle), "mcp-registry.yml lacks {needle}");
    }
    let workflow = release_yml();
    let call = job(&workflow, "mcp-registry").expect("release.yml keeps an `mcp-registry` job");
    assert!(
        call.contains("uses: ./.github/workflows/mcp-registry.yml"),
        "release.yml must call mcp-registry.yml rather than repeat it"
    );
    assert!(
        !call.contains("secrets: inherit"),
        "pass MCP_PRIVATE_KEY by name, not every secret"
    );
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

/// One leg of the build matrix, as `release.yml` writes it.
struct Leg {
    os: String,
    target: String,
    ort_source: bool,
}

/// The `build` job's matrix, read the way the rest of this file reads the workflow: textually, so
/// that a test of what CI does needs no yaml dependency to say it.
fn build_legs(workflow: &str) -> Vec<Leg> {
    let job = job(workflow, "build").expect("a `build` job");
    let include = job
        .split_once("include:\n")
        .expect("the matrix is written as an include list")
        .1;
    let end = include.find("\n    steps:").unwrap_or(include.len());
    let mut legs = Vec::new();
    for block in include[..end].split("- os: ").skip(1) {
        let field = |name: &str| {
            block
                .lines()
                .find_map(|l| l.trim().strip_prefix(&format!("{name}: ")))
                .map(|v| v.trim().trim_matches('"').to_string())
        };
        legs.push(Leg {
            os: block.lines().next().expect("the os").trim().to_string(),
            target: field("target").expect("every leg names a target"),
            ort_source: field("ort_source").expect("every leg says where its runtime comes from")
                == "true",
        });
    }
    assert!(legs.len() >= 5, "the matrix lost legs; this test is stale");
    legs
}

/// One artefact per target, and every one of them carries the captcha models. The release used to
/// ship three targets that could not answer an image captcha anywhere, which is a different program
/// wearing the same version number.
#[test]
fn every_target_is_built_once_and_carries_the_models() {
    let workflow = release_yml();
    let legs = build_legs(&workflow);
    let mut targets: Vec<&str> = legs.iter().map(|l| l.target.as_str()).collect();
    let before = targets.len();
    targets.sort_unstable();
    targets.dedup();
    assert_eq!(
        before,
        targets.len(),
        "two legs build the same target, and they would upload under one artifact name"
    );
    let build = job(&workflow, "build").expect("a `build` job");
    assert!(
        build.contains("--features impersonate,onnx-ocr,onnx-grid,onnx-audio,onnx-detect,onnx-segment,onnx-zeroshot"),
        "the build step no longer asks for every model feature"
    );
    assert!(
        !build.contains("matrix.models"),
        "`models` is no longer a property a leg can lack; nothing should branch on it"
    );
}

/// The three targets pyke publishes no usable runtime for build their own, with the script this
/// repository keeps, and the other two take the prebuilt one. A leg that quietly stopped building
/// its own would link the downloaded runtime and reintroduce the glibc floor this removed.
#[test]
fn the_targets_without_a_usable_prebuilt_runtime_build_their_own() {
    let workflow = release_yml();
    let legs = build_legs(&workflow);
    for leg in &legs {
        let expected = matches!(
            leg.target.as_str(),
            "x86_64-unknown-linux-gnu" | "aarch64-unknown-linux-gnu" | "x86_64-apple-darwin"
        );
        assert_eq!(
            leg.ort_source, expected,
            "{} on {}: ort_source should be {expected}",
            leg.target, leg.os
        );
    }
    let build = job(&workflow, "build").expect("a `build` job");
    assert!(
        build.contains("tools/onnxruntime/build.sh"),
        "the build job no longer runs the runtime build script"
    );
    assert!(
        build.contains("ORT_LIB_LOCATION=$PWD/.ort/build/Release"),
        "ort-sys reads ORT_LIB_LOCATION, and it must name the directory holding the static libs"
    );
    // Linux still builds on the oldest runner available: the whole reason for building the runtime
    // is that the artefact keeps that runner's glibc rather than the prebuilt runtime's.
    for leg in legs.iter().filter(|l| l.target.contains("linux")) {
        assert!(
            leg.os.starts_with("ubuntu-22.04"),
            "{} builds on {}, which raises the glibc floor the source build exists to keep low",
            leg.target,
            leg.os
        );
    }
}

/// A runtime built from source is slow enough that a release cannot pay for it every time, and a
/// cache written under a tag is unreadable from the next one — so `cache-warm.yml` writes it on
/// `main` and the release restores it. Two different keys means the release always misses, silently
/// and expensively.
#[test]
fn the_runtime_cache_is_warmed_under_the_key_the_release_restores() {
    let root = workspace_root();
    let release = release_yml();
    let warm = fs::read_to_string(root.join(".github/workflows/cache-warm.yml"))
        .expect("cache-warm.yml")
        .replace("\r\n", "\n");
    let key = "ort-${{ hashFiles('tools/onnxruntime/VERSION') }}-${{ matrix.target }}";
    assert!(
        release.contains(key),
        "release.yml restores a different key"
    );
    assert!(warm.contains(key), "cache-warm.yml writes a different key");
    assert!(
        warm.contains("tools/onnxruntime/build.sh"),
        "cache-warm.yml must build the runtime it caches"
    );
    assert!(
        root.join("tools/onnxruntime/VERSION").is_file(),
        "the key hashes a file that has to exist"
    );
}

/// Every Linux artefact is started on a distribution older than the one that built it, and asked
/// whether its models answer there. Starting is not enough: a runtime built against a newer glibc
/// links and then fails at the first session, and `svipall doctor` lists the embedded models either
/// way — that failure is invisible until a captcha arrives.
#[test]
fn each_linux_artefact_answers_on_an_older_distribution() {
    let workflow = release_yml();
    let step = workflow
        .split("\n      - ")
        .find(|s| s.starts_with("name: Runs on an older distribution"))
        .expect("release.yml has the older-distribution step");
    assert!(
        step.contains("debian:bookworm-slim"),
        "the gate must be a distribution older than the runner:\n{step}"
    );
    for model in ["detect", "segment"] {
        assert!(
            step.contains(&format!("'\"{model}\"'")),
            "the gate must assert {model} answers there, not merely that the binary starts"
        );
    }
}

/// `svipall models install` asks the release for an asset by name, and the release job builds one
/// by name. They are the same string or the command 404s against a release that has the file —
/// which is why the format is asserted here against the literal the workflow writes, rather than
/// described in two places and hoped about.
#[test]
fn the_models_archive_is_published_under_the_name_the_installer_asks_for() {
    let workflow = release_yml();
    let models = job(&workflow, "models").expect("release.yml has a `models` job");
    assert!(
        models.contains("svipall-models-$VERSION.zip"),
        "the models job no longer builds the asset the installer asks for:\n{models}"
    );
    assert_eq!(
        svipall::model_install::asset_name("$VERSION"),
        "svipall-models-$VERSION.zip"
    );
    assert!(
        workflow.contains("artifacts/svipall-models-*.zip"),
        "the release does not publish the models archive"
    );
    // Both checksum steps glob `svipall-*`, so a job that runs before this one writes a
    // `sha256sums.txt` without the archive in it — and then the installer downloads something it
    // cannot verify, which it reports as a warning and nobody reads.
    for consumer in ["packages", "publish"] {
        let body = job(&workflow, consumer).expect("a job");
        let needs = body
            .lines()
            .find(|l| l.trim_start().starts_with("needs:"))
            .unwrap_or_default();
        assert!(
            needs.contains("models"),
            "{consumer} must wait for the models archive: {needs}"
        );
    }
}

/// The runtime's architecture comes from the target, not from the runner. `macos-latest` is arm64
/// and the x86-64 macOS artefact is cross-compiled on it: rustc knows that from the target triple,
/// a cmake build does not, and a runtime built for the wrong architecture fails at the link step
/// with a page of unresolved symbols. Both workflows map it, the same way, and the script refuses a
/// Linux cross-build rather than producing one quietly.
#[test]
fn the_runtime_is_built_for_the_target_rather_than_the_runner() {
    let root = workspace_root();
    let script = fs::read_to_string(root.join("tools/onnxruntime/build.sh")).expect("build.sh");
    assert!(
        script.contains("CMAKE_OSX_ARCHITECTURES=$arch"),
        "the script must tell cmake which architecture to produce on macOS"
    );
    assert!(
        script.contains("cannot build the runtime for"),
        "a Linux cross-build must be refused rather than attempted"
    );
    let release = release_yml();
    let warm = fs::read_to_string(root.join(".github/workflows/cache-warm.yml"))
        .expect("cache-warm.yml")
        .replace("\r\n", "\n");
    for (name, text) in [("release.yml", &release), ("cache-warm.yml", &warm)] {
        for needle in [
            "x86_64-*) arch=x86_64 ;;",
            "aarch64-apple-*) arch=arm64 ;;",
            "aarch64-*) arch=aarch64 ;;",
            "tools/onnxruntime/build.sh \"$PWD/.ort\" \"$arch\"",
        ] {
            assert!(
                text.contains(needle),
                "{name} does not map the target to a runtime architecture: {needle}"
            );
        }
    }
}

/// Both container images assert what they carry, at build time, in the image itself. `slim` used to
/// assert the opposite — it demanded `no_models`, because it is repackaged from a Linux archive that
/// had none — and that assertion failed the moment the archive gained them, which is how this was
/// found. An image that quietly loses the models looks identical from outside, so the assertion is
/// the only thing standing between that and a user.
#[test]
fn both_container_images_assert_the_models_they_carry() {
    let root = workspace_root();
    for file in ["Dockerfile", "Dockerfile.slim"] {
        let text = fs::read_to_string(root.join(file)).expect(file);
        for needle in ["grep -q '\"detect\"'", "grep -q '\"segment\"'"] {
            assert!(
                text.contains(needle),
                "{file} does not assert the models it ships: {needle}"
            );
        }
        assert!(
            text.contains("! grep -q '\"no_models\"'"),
            "{file} must fail the build when doctor reports no_models"
        );
    }
}
