//! `server.json` and the four places its claims have to be true.
//!
//! The MCP Registry hosts metadata, never artefacts, and it verifies every package it is pointed
//! at by looking for the server's own name inside the published artefact. Each registry hides that
//! token somewhere different, and each one fails at publish time rather than at edit time:
//!
//! | package     | where the name has to appear                                     |
//! |-------------|------------------------------------------------------------------|
//! | npm         | `mcpName` in `packaging/npm/package.json`                        |
//! | oci         | a `LABEL io.modelcontextprotocol.server.name` in the `Dockerfile` |
//! | cargo       | a **visible** `mcp-name:` line in `crates/svipall/README.md`  |
//!
//! The cargo one is the trap: crates.io strips HTML comments on the way to the rendered README the
//! validator reads, so the `<!-- mcp-name: … -->` form that works for PyPI and NuGet silently does
//! not work here. Hence the assertion that the token is not inside a comment.
//!
//! A published version is immutable and cannot be unpublished, so a name that drifts is not a bug
//! to fix later — it is a second server, permanently. This test is the only thing standing between
//! a rename and that.

use std::fs;
use std::path::{Path, PathBuf};

/// The name, written once here and asserted everywhere else.
const SERVER_NAME: &str = "dev.ilien.svipall/mcp";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/svipall sits two levels under the workspace root")
        .to_path_buf()
}

fn read(root: &Path, relative: &str) -> String {
    fs::read_to_string(root.join(relative)).unwrap_or_else(|e| panic!("{relative}: {e}"))
}

/// The one hand-written version: `[workspace.package] version`, same source `release_version.rs`
/// reads. Duplicated rather than shared because two test binaries cannot import each other.
fn workspace_version(root: &Path) -> String {
    read(root, "Cargo.toml")
        .split_once("[workspace.package]")
        .expect("the workspace declares [workspace.package]")
        .1
        .lines()
        .find_map(|line| line.trim().strip_prefix("version = "))
        .expect("[workspace.package] names a version")
        .trim()
        .trim_matches('"')
        .to_string()
}

fn server_json(root: &Path) -> serde_json::Value {
    serde_json::from_str(&read(root, "server.json")).expect("server.json is valid JSON")
}

fn packages(manifest: &serde_json::Value) -> Vec<&serde_json::Value> {
    manifest["packages"]
        .as_array()
        .expect("server.json declares packages")
        .iter()
        .collect()
}

fn package_of(manifest: &serde_json::Value, registry_type: &str) -> serde_json::Value {
    packages(manifest)
        .into_iter()
        .find(|p| p["registryType"] == registry_type)
        .unwrap_or_else(|| panic!("server.json declares no {registry_type} package"))
        .clone()
}

#[test]
fn the_server_name_is_the_one_the_dns_record_authorises() {
    let manifest = server_json(&workspace_root());
    assert_eq!(
        manifest["name"], SERVER_NAME,
        "the name is immutable once published and the registry cannot unpublish it. Changing it \
         here does not rename the server, it creates a second one"
    );
    // DNS auth over ilien.dev grants `dev.ilien.*`; HTTP auth (`.well-known`) grants only the bare
    // domain, so this name is unreachable that way. Keep the two in step.
    assert!(
        SERVER_NAME.starts_with("dev.ilien."),
        "this name needs DNS authentication over ilien.dev"
    );
    let (namespace, _) = SERVER_NAME
        .split_once('/')
        .expect("the schema requires exactly one forward slash");
    assert!(
        !namespace.contains('/'),
        "the schema pattern allows exactly one slash: ^[a-zA-Z0-9.-]+/[a-zA-Z0-9._-]+$"
    );
}

#[test]
fn the_description_fits_what_the_schema_allows() {
    let manifest = server_json(&workspace_root());
    let description = manifest["description"].as_str().expect("a description");
    assert!(
        (1..=100).contains(&description.chars().count()),
        "server.schema.json caps description at 100 characters; this one is {}",
        description.chars().count()
    );
}

#[test]
fn every_version_in_server_json_is_the_workspace_version() {
    let root = workspace_root();
    let version = workspace_version(&root);
    let manifest = server_json(&root);

    assert_eq!(
        manifest["version"], version,
        "server.json claims a version the workspace does not. Run scripts/sync-version"
    );
    for package in packages(&manifest) {
        let registry_type = package["registryType"].as_str().expect("a registryType");
        // An OCI package carries its version in the image tag instead of a `version` field.
        if registry_type == "oci" {
            let identifier = package["identifier"].as_str().expect("an identifier");
            assert!(
                identifier.ends_with(&format!(":{version}")),
                "the OCI tag is not on {version}: {identifier}. The release publishes \
                 ghcr.io/ilien-dev/svipall:<version>, and a tag that does not exist fails \
                 validation at publish time"
            );
            continue;
        }
        assert_eq!(
            package["version"], version,
            "the {registry_type} package in server.json is not on {version}. Run \
             scripts/sync-version"
        );
    }
}

#[test]
fn the_npm_package_carries_the_name_the_registry_looks_for() {
    let root = workspace_root();
    let package: serde_json::Value =
        serde_json::from_str(&read(&root, "packaging/npm/package.json"))
            .expect("packaging/npm/package.json is valid JSON");
    assert_eq!(
        package["mcpName"], SERVER_NAME,
        "the registry reads `mcpName` out of the published package.json to prove the package is \
         ours. Without it the publish fails with \"Registry validation failed for package\""
    );

    // The npm package is named `svipall` and its MCP binary is `svipall-mcp`, so `npx svipall-mcp`
    // resolves a package that does not exist. server.json has to spell the runtime out.
    let declared = package_of(&server_json(&root), "npm");
    assert_eq!(
        declared["identifier"], package["name"],
        "server.json points at an npm package by a name the wrapper does not publish under"
    );
    let runtime = declared["runtimeArguments"].to_string();
    assert!(
        runtime.contains("--package"),
        "`npx svipall-mcp` would look for a package called svipall-mcp, which is not published. \
         The runtime arguments have to name the package explicitly: npx --package=svipall \
         svipall-mcp"
    );
}

/// Every `npx` line anybody could copy out of this repository.
fn documented_npx_lines(root: &Path) -> Vec<(String, String)> {
    fn walk(directory: &Path, into: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, into);
            } else if matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("md" | "js" | "json" | "sh" | "ps1")
            ) {
                into.push(path);
            }
        }
    }

    let mut files = vec![root.join("README.md"), root.join("GET-STARTED.md")];
    for directory in ["docs", "packaging", "plugins", "skill"] {
        walk(&root.join(directory), &mut files);
    }

    files
        .iter()
        .filter(|path| path.exists())
        .filter_map(|path| Some((path, fs::read_to_string(path).ok()?)))
        .flat_map(|(path, text)| {
            let name = path
                .strip_prefix(root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            text.lines()
                .filter(|line| line.contains("npx "))
                .map(|line| (name.clone(), line.trim().to_string()))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// `npx svipall-mcp` reads as "run the MCP server" and does nothing of the kind: npx resolves a
/// *package* by that name, and the package is `svipall` — the binary is only a `bin` entry inside
/// it. It fails with a 404 against the npm registry, which names nothing anybody would connect to
/// this repository. It was in `packaging/npm/README.md` as a `claude mcp add` line people were
/// meant to paste.
#[test]
fn no_documented_npx_line_mistakes_the_binary_for_the_package() {
    for (file, line) in documented_npx_lines(&workspace_root()) {
        if !line.contains("svipall-mcp") {
            continue;
        }
        assert!(
            line.contains("--package"),
            "{file} tells somebody to run `npx` on svipall-mcp, which is a binary, not a \
             published package. npx would 404. Write `npx --yes --package=svipall \
             svipall-mcp`:\n    {line}"
        );
    }
}

#[test]
fn the_image_carries_the_name_the_registry_looks_for() {
    let root = workspace_root();
    let label = format!("LABEL io.modelcontextprotocol.server.name=\"{SERVER_NAME}\"");
    assert!(
        read(&root, "Dockerfile").contains(&label),
        "the registry reads the io.modelcontextprotocol.server.name annotation off the manifest \
         to prove the image is ours. Expected in the final stage:\n    {label}"
    );
}

#[test]
fn the_crate_readme_carries_the_name_where_crates_io_will_still_render_it() {
    let root = workspace_root();
    let readme = read(&root, "crates/svipall/README.md");
    let token = format!("mcp-name: {SERVER_NAME}");

    assert!(
        readme.contains(&token),
        "the registry looks for `{token}` in the README crates.io renders"
    );
    for line in readme.lines().filter(|line| line.contains(&token)) {
        assert!(
            !line.contains("<!--"),
            "crates.io strips HTML comments when it renders a README, so the validator never sees \
             this. PyPI and NuGet keep them; cargo does not. Write the token as visible \
             markdown:\n    {line}"
        );
    }

    // A README crates.io never publishes is a README the validator never reads.
    assert!(
        read(&root, "crates/svipall/Cargo.toml").contains("readme = \"README.md\""),
        "crates/svipall/Cargo.toml does not name its README, so crates.io renders none"
    );
}
