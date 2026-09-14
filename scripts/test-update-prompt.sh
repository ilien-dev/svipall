#!/usr/bin/env bash
# The installer must ask before replacing an older shared installation, and declining must happen
# before any network request or write.  A fake binary makes the whole check offline.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
scratch="$(mktemp -d)"
cleanup() {
  case "$scratch" in /tmp/*|"${TMPDIR:-/tmp}"/*) rm -rf "$scratch" ;; esac
}
trap cleanup EXIT INT TERM

mkdir -p "$scratch/bin"
printf '%s\n' '#!/bin/sh' 'printf '\''{"version":"1.0.3"}\n'\''' > "$scratch/bin/svipall"
chmod +x "$scratch/bin/svipall"

set +e
out="$(sh "$root/install.sh" --version v1.0.5 --prefix "$scratch/bin" </dev/null 2>&1)"
status=$?
set -e

[ "$status" -eq 0 ]
printf '%s' "$out" | grep -q 'current version 1.0.3; latest version 1.0.5'
printf '%s' "$out" | grep -q 'used by all harnesses'
printf '%s' "$out" | grep -q 'nothing was downloaded or changed'
[ "$("$scratch/bin/svipall" --version)" = '{"version":"1.0.3"}' ]
