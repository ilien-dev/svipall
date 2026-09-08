#!/usr/bin/env bash
# Propagate the workspace version to everything else that repeats it.
#
#   scripts/sync-version.sh            # propagate whatever the root manifest says
#   scripts/sync-version.sh 1.0.0-rc.4 # set it first, then propagate
#
# `[workspace.package] version` in the root Cargo.toml is the only place the number is written by
# hand. Five other kinds of file repeat it, and each is a different way to be wrong: Claude Code
# silently keeps a cached plugin when `plugin.json` names a version it has already seen; the npm
# postinstall builds its download URL from its own version, so a package one behind fetches an
# archive that does not exist; and crates.io rejects a `path` dependency that carries no `version`
# — after the crates published before it in the same run are already permanent.
#
# `crates/svipall/tests/release_version.rs` fails the build when any of them drifts. This is
# what makes them agree; that is what proves it.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(dirname "$here")"
python3 "$here/sync_version.py" "$root" "${1:-}"
