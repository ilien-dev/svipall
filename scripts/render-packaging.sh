#!/usr/bin/env bash
# Fill the package-manager manifests in packaging/templates/ from a published release.
#
#   scripts/render-packaging.sh 1.0.0 [sha256sums.txt] [out-dir]
#
# Every manifest names the same five artefacts and repeats their sha256. Writing those by hand,
# five times, on every release, is a job that is wrong the first time somebody is in a hurry — so
# the release workflow runs this instead, and the checksums come from the file the release
# already publishes rather than from anybody's memory.
#
# Without a checksums file it downloads the one attached to the tag.
set -euo pipefail

version="${1:-}"
sums="${2:-}"
out="${3:-packaging/dist}"
repo="ilien-dev/svipall"

if [ -z "$version" ]; then
    echo "usage: $0 <version> [sha256sums.txt] [out-dir]" >&2
    exit 1
fi
version="${version#v}"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(dirname "$here")"
cd "$root"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

if [ -z "$sums" ]; then
    sums="$tmp/sha256sums.txt"
    curl -fsSL "https://github.com/$repo/releases/download/v$version/sha256sums.txt" -o "$sums"
fi

# sha_for x86_64-unknown-linux-gnu -> the checksum of that target's archive, or fail loudly. A
# manifest with an empty hash installs nothing and says nothing useful about why.
sha_for() {
    target="$1"
    ext="tar.gz"
    case "$target" in *windows*) ext="zip" ;; esac
    name="svipall-$version-$target.$ext"
    value="$(grep -E "  ?$name\$" "$sums" | awk '{print $1}' | head -n1)"
    if [ -z "$value" ]; then
        echo "no checksum for $name in $sums" >&2
        exit 1
    fi
    printf '%s' "$value"
}

targets="x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin x86_64-pc-windows-msvc"

render() {
    src="$1"
    dst="$2"
    mkdir -p "$(dirname "$dst")"
    cp "$src" "$dst"
    # sed -i is not portable between GNU and BSD; a temporary file is.
    for t in $targets; do
        key="SHA_$(printf '%s' "$t" | tr 'a-z-' 'A-Z_')"
        value="$(sha_for "$t")"
        upper="$(printf '%s' "$value" | tr 'a-f' 'A-F')"
        sed -e "s/@${key}_UPPER@/$upper/g" -e "s/@${key}@/$value/g" "$dst" > "$dst.tmp"
        mv "$dst.tmp" "$dst"
    done
    # Arch forbids a hyphen in pkgver; every other manifest wants the real semver string.
    pkgver="$(printf '%s' "$version" | tr '-' '_')"
    sed -e "s/@PKGVER@/$pkgver/g" -e "s/@VERSION@/$version/g" "$dst" > "$dst.tmp"
    mv "$dst.tmp" "$dst"
    if grep -q '@[A-Z_]*@' "$dst"; then
        echo "unfilled placeholder left in $dst:" >&2
        grep -o '@[A-Z_]*@' "$dst" | sort -u >&2
        exit 1
    fi
    echo "rendered $dst"
}

rm -rf "$out"
render packaging/templates/homebrew.rb "$out/homebrew/svipall.rb"
render packaging/templates/scoop.json "$out/scoop/svipall.json"
render packaging/templates/PKGBUILD "$out/aur/PKGBUILD"
for f in packaging/templates/winget/*.yaml; do
    render "$f" "$out/winget/$(basename "$f")"
done

echo
echo "Done. What to do with each is in packaging/README.md."
