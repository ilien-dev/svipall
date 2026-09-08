"""Write the workspace version into every file that repeats it, or check that it is written.

    python3 scripts/sync_version.py <root>              # propagate
    python3 scripts/sync_version.py <root> 1.0.0-rc.4   # set, then propagate
    python3 scripts/sync_version.py <root> --check      # print the version, fail on any drift

Called by scripts/sync-version.sh and scripts/sync-version.ps1, which is why the logic lives here
once instead of twice: a bash copy and a PowerShell copy of the same regexes drift, and the failure
they cause is a release that is half on one version.

The rewrites are deliberately textual. `cargo set-version` would need a plugin nobody has on a
fresh checkout, and a TOML round-trip would reflow manifests whose comments carry the reasoning
this project keeps in them.
"""

import json
import pathlib
import re
import sys

# Every file that repeats the number, and nothing that derives it at run time: install.js reads
# package.json, the binary reads CARGO_PKG_VERSION, and render-packaging.sh takes it as an argument.
#
# `server.json` is the MCP Registry entry and says it four times: once for the server and once for
# each of the three packages it points at — npm, the container tag, and the crate. A registry entry
# naming a version that was never published is an install that fails on somebody else's machine.
JSON_MANIFESTS = [
    ("plugins/svipall/.claude-plugin/plugin.json", 1),
    ("packaging/npm/package.json", 1),
    ("server.json", 3),
]

# The container tag inside `server.json`'s OCI package, which is a version written into the middle
# of an identifier rather than a field of its own.
OCI_IDENTIFIER = re.compile(r'("identifier": "ghcr\.io/[^:"]+:)[^"]+(")')
# server.json is the same job with two differences: it names the version once for the server and
# again for every package it points at, and its OCI package has no version field at all — the image
# tag is the version. The MCP Registry checks each of those against the artefact that is actually
# published, and refuses the whole submission if one of them names something that does not exist.
SERVER_MANIFEST = "server.json"

WORKSPACE_PACKAGE = re.compile(r'(\[workspace\.package\][^\[]*?version = ")([^"]+)(")', re.S)
# A dependency on a crate of this workspace, wherever it is declared. The `version` may already be
# there, in which case it is replaced, or absent, in which case it is added after the path.
INTERNAL_DEP = re.compile(
    r'(svipall(?:-[a-z]+)? = \{[^}]*?path = "[^"]+")(, version = "[^"]+")?'
)


def members(root: pathlib.Path) -> list[str]:
    manifest = (root / "Cargo.toml").read_text(encoding="utf-8")
    listed = manifest.split("members = [", 1)[1].split("]", 1)[0]
    return re.findall(r'"([^"]+)"', listed)


def main() -> int:
    root = pathlib.Path(sys.argv[1])
    argument = sys.argv[2] if len(sys.argv) > 2 else ""
    # The release workflow's only reading of the version, so it reads it the same way the sync
    # writes it. A second implementation over there is a second thing to get wrong on release day.
    check = argument == "--check"
    wanted = "" if check else argument

    manifest_path = root / "Cargo.toml"
    manifest = manifest_path.read_text(encoding="utf-8")
    found = WORKSPACE_PACKAGE.search(manifest)
    if not found:
        print("the root manifest declares no [workspace.package] version", file=sys.stderr)
        return 1
    version = wanted or found.group(2)

    # A pre-release is anything with a hyphen, and both the release workflow and the .deb/.rpm
    # naming depend on reading it the same way. Reject anything neither of them could handle
    # rather than discovering it three jobs into a release.
    if not re.fullmatch(r"\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?", version):
        print(f"{version} is not a version this release can name a tag after", file=sys.stderr)
        return 1

    changed = []

    if version != found.group(2):
        manifest = WORKSPACE_PACKAGE.sub(rf"\g<1>{version}\g<3>", manifest, count=1)
        changed.append(manifest_path)

    for path in [manifest_path] + [root / m / "Cargo.toml" for m in members(root)]:
        text = manifest if path == manifest_path else path.read_text(encoding="utf-8")
        after = INTERNAL_DEP.sub(rf'\g<1>, version = "{version}"', text)
        if after != text or (path == manifest_path and manifest_path in changed):
            if after != text:
                changed.append(path)
            if not check:
                path.write_text(after, encoding="utf-8")

    for relative, occurrences in JSON_MANIFESTS:
        path = root / relative
        text = path.read_text(encoding="utf-8")
        # Rewritten in place rather than re-serialised: json.dump would reorder nothing but would
        # reformat the whole file, and these are hand-kept manifests people read in diffs.
        after = re.sub(
            r'("version": ")[^"]+(")', rf"\g<1>{version}\g<2>", text, count=occurrences
        )
        after = OCI_IDENTIFIER.sub(rf"\g<1>{version}\g<2>", after)
        if json.loads(after)["version"] != version:
            print(f"{relative}: the version did not take", file=sys.stderr)
            return 1
        if after != text:
            changed.append(path)
            if not check:
                path.write_text(after, encoding="utf-8")

    path = root / SERVER_MANIFEST
    text = path.read_text(encoding="utf-8")
    after = re.sub(r'("version": ")[^"]+(")', rf"\g<1>{version}\g<2>", text)
    # The tag, rewritten from the identifier the file already carries rather than from a pattern:
    # a registry host is not something to match with a regex, and this leaves the repository alone.
    for package in json.loads(text).get("packages", []):
        identifier = package.get("identifier", "")
        if package.get("registryType") == "oci" and ":" in identifier:
            repository = identifier.rsplit(":", 1)[0]
            after = after.replace(f'"{identifier}"', f'"{repository}:{version}"')
    document = json.loads(after)
    named = [document["version"]] + [
        package["version"] for package in document["packages"] if "version" in package
    ]
    tagged = [
        package["identifier"]
        for package in document["packages"]
        if package.get("registryType") == "oci"
    ]
    if any(v != version for v in named) or any(
        not t.endswith(f":{version}") for t in tagged
    ):
        print(f"{SERVER_MANIFEST}: the version did not take", file=sys.stderr)
        return 1
    if after != text:
        changed.append(path)
        if not check:
            path.write_text(after, encoding="utf-8")

    if check:
        # The version, on stdout and alone, so a shell can take it; the drift, on stderr, so the
        # log says which file is behind rather than only that something is.
        for path in dict.fromkeys(changed):
            print(f"{path.relative_to(root).as_posix()} is not on {version}", file=sys.stderr)
        if changed:
            print("run scripts/sync-version", file=sys.stderr)
            return 1
        print(version)
        return 0

    for path in dict.fromkeys(changed):
        print(f"  {path.relative_to(root).as_posix()}")
    print(f"version {version}" + ("" if changed else ", everything already agreed"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
