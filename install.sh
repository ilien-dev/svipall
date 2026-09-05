#!/bin/sh
# Install svipall on Linux or macOS.
#
#   curl -fsSL https://raw.githubusercontent.com/ilien-dev/svipall/main/install.sh | sh
#
# It downloads a release build, checks it against the published sha256, puts both binaries in a
# directory this user owns, and tells you what it touched. It never asks for root, never writes
# outside $PREFIX and your own shell profile, and never downloads a browser without asking.
#
#   --version vX.Y.Z   a specific release instead of the latest
#   --prefix DIR       where the binaries go (default ~/.local/bin)
#   --from-file FILE   install a tarball you already have, skipping the download (used by CI)
#   --no-path          do not touch any shell profile
#   --yes              answer yes to every prompt
#   --browser          download Chrome for Testing without asking (~190 MB)
#   --no-browser       never download it, and do not ask
#   --uninstall        remove what a previous run installed
#
# POSIX sh on purpose: this runs before anything is installed, including bash on some images.
set -eu

REPO="ilien-dev/svipall"
PREFIX="${SVIPALL_PREFIX:-$HOME/.local/bin}"
VERSION=""
FROM_FILE=""
NO_PATH=0
ASSUME_YES=0
BROWSER=ask
UNINSTALL=0

usage() {
    cat <<'USAGE'
Install svipall on Linux or macOS.

    --version vX.Y.Z   a specific release instead of the latest
    --prefix DIR       where the binaries go (default ~/.local/bin)
    --from-file FILE   install an archive you already have, skipping the download
    --no-path          do not touch any shell profile
    --yes              answer yes to every prompt
    --browser          download Chrome for Testing without asking (~190 MB)
    --no-browser       never download it, and do not ask
    --uninstall        remove what a previous run installed
USAGE
}

say() { printf '%s\n' "$*"; }
err() { printf 'svipall: %s\n' "$*" >&2; }
die() { err "$*"; exit 1; }

while [ $# -gt 0 ]; do
    case "$1" in
        --version) VERSION="${2:-}"; shift 2 ;;
        --version=*) VERSION="${1#*=}"; shift ;;
        --prefix) PREFIX="${2:-}"; shift 2 ;;
        --prefix=*) PREFIX="${1#*=}"; shift ;;
        --from-file) FROM_FILE="${2:-}"; shift 2 ;;
        --from-file=*) FROM_FILE="${1#*=}"; shift ;;
        --no-path) NO_PATH=1; shift ;;
        --yes|-y) ASSUME_YES=1; shift ;;
        --browser) BROWSER=yes; shift ;;
        --no-browser) BROWSER=no; shift ;;
        --uninstall) UNINSTALL=1; shift ;;
        # `$0` is "sh" or "-" under `curl | sh`, so reading the script back is not an option.
        -h|--help) usage; exit 0 ;;
        *) die "unknown option $1" ;;
    esac
done

# A `curl | sh` install has the script on stdin, so a prompt would read the rest of the script.
# Ask the terminal directly, and when there is no terminal there is no question to ask.
ask() {
    [ "$ASSUME_YES" -eq 1 ] && return 0
    [ -e /dev/tty ] || return 1
    printf '%s [y/N] ' "$1" > /dev/tty
    read -r answer < /dev/tty || return 1
    case "$answer" in [yY]|[yY][eE][sS]) return 0 ;; *) return 1 ;; esac
}

if [ "$UNINSTALL" -eq 1 ]; then
    removed=0
    for b in svipall svipall-mcp; do
        if [ -e "$PREFIX/$b" ]; then rm -f "$PREFIX/$b"; say "removed $PREFIX/$b"; removed=1; fi
    done
    [ "$removed" -eq 0 ] && say "nothing to remove in $PREFIX"
    say ""
    say "Left alone on purpose: ~/.svipall (profiles, cache, learned tiers, any browser it"
    say "downloaded) and the PATH line in your shell profile. Remove them by hand if you want to."
    exit 0
fi

# ---- platform ------------------------------------------------------------------------------
# Only ever used to name the file to download. `--from-file` skips the question entirely, which is
# what makes an air-gapped install, and the CI smoke test, possible at all.
os="$(uname -s)"
arch="$(uname -m)"
TARGET=""
EXT="tar.gz"
case "$os-$arch" in
    Linux-x86_64|Linux-amd64)      TARGET="x86_64-unknown-linux-gnu"; EXT="tar.gz" ;;
    Linux-aarch64|Linux-arm64)     TARGET="aarch64-unknown-linux-gnu"; EXT="tar.gz" ;;
    Darwin-x86_64)                 TARGET="x86_64-apple-darwin"; EXT="tar.gz" ;;
    Darwin-arm64)                  TARGET="aarch64-apple-darwin"; EXT="tar.gz" ;;
    MINGW*|MSYS*|CYGWIN*) # matched against "$os-$arch", so the trailing glob covers the arch
        # Git Bash, MSYS2 and Cygwin are Windows. Sending someone here to Docker would be absurd
        # when the Windows installer is one line away, and the Windows build is the one that runs.
        err "this is Windows (under $os), and the Windows build is what you want."
        err "In PowerShell:"
        err "  irm https://raw.githubusercontent.com/$REPO/main/install.ps1 | iex"
        exit 1
        ;;
    *)
        # Not fatal yet: only a download needs a target, and --from-file does not download.
        TARGET=""
        ;;
esac

unsupported() {
    err "no release build for $os $arch."
    err "Builds exist for Linux x86-64 and arm64, and macOS Intel and Apple silicon."
    err "On anything else, the container image works: docker pull ghcr.io/$REPO:latest"
    err "Or build from source: https://github.com/$REPO#build-from-source"
    exit 1
}

# ---- tools ---------------------------------------------------------------------------------
if command -v curl >/dev/null 2>&1; then
    fetch() { curl -fsSL "$1" -o "$2"; }
    fetch_stdout() { curl -fsSL "$1"; }
elif command -v wget >/dev/null 2>&1; then
    fetch() { wget -qO "$2" "$1"; }
    fetch_stdout() { wget -qO- "$1"; }
else
    die "neither curl nor wget is installed"
fi

if command -v sha256sum >/dev/null 2>&1; then
    sha256() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
    sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
    sha256() { echo ""; }
fi

# The first `"tag_name": "vX.Y.Z"` in a GitHub API response. No jq: this script runs before
# anything at all is installed on the machine, and that includes jq.
tag_name() {
    fetch_stdout "$1" 2>/dev/null | tr ',' '\n' | grep '"tag_name"' | head -n1 | cut -d'"' -f4
}

tmp="$(mktemp -d)"
cleanup() { rm -rf "$tmp"; }
trap cleanup EXIT INT TERM

# ---- get the archive -----------------------------------------------------------------------
if [ -n "$FROM_FILE" ]; then
    [ -f "$FROM_FILE" ] || die "$FROM_FILE does not exist"
    archive="$FROM_FILE"
    say "installing from $archive"
else
    [ -n "$TARGET" ] || unsupported
    if [ -z "$VERSION" ]; then
        say "looking up the latest release of $REPO"
        VERSION="$(tag_name "https://api.github.com/repos/$REPO/releases/latest")"
        if [ -z "$VERSION" ]; then
            # `/releases/latest` only ever names a stable release. Before the first one exists
            # there is nothing there at all, which would leave this script unable to install the
            # project for exactly as long as it is in pre-release. Fall back to the newest release
            # of any kind — and say which one it is, rather than installing a pre-release quietly.
            VERSION="$(tag_name "https://api.github.com/repos/$REPO/releases?per_page=1")"
            [ -n "$VERSION" ] || die "could not read any release; pass --version vX.Y.Z"
            say "no stable release yet; using the pre-release $VERSION"
        fi
    fi
    num="${VERSION#v}"
    name="svipall-$num-$TARGET.$EXT"
    base="https://github.com/$REPO/releases/download/$VERSION"
    say "downloading $name"
    fetch "$base/$name" "$tmp/$name" || die "could not download $base/$name"

    # Verified, or the reason it could not be. A silent skip here is how a corrupted or swapped
    # download becomes a binary somebody runs.
    if fetch "$base/sha256sums.txt" "$tmp/sha256sums.txt" 2>/dev/null; then
        want="$(grep " $name\$" "$tmp/sha256sums.txt" | cut -d' ' -f1 | head -n1)"
        got="$(sha256 "$tmp/$name")"
        if [ -z "$got" ]; then
            err "warning: no sha256sum or shasum on this machine, so the download was not verified"
        elif [ -z "$want" ]; then
            err "warning: $name is not listed in sha256sums.txt; not verified"
        elif [ "$want" != "$got" ]; then
            die "checksum mismatch for $name (expected $want, got $got) — not installing"
        else
            say "checksum ok"
        fi
    else
        err "warning: sha256sums.txt could not be downloaded, so the archive was not verified"
    fi
    archive="$tmp/$name"
fi

# ---- install -------------------------------------------------------------------------------
mkdir -p "$tmp/unpack" "$PREFIX" || die "cannot create $PREFIX"
tar -xzf "$archive" -C "$tmp/unpack" || die "could not unpack $archive"
for b in svipall svipall-mcp; do
    [ -f "$tmp/unpack/$b" ] || die "$b is missing from the archive"
    # Replace rather than write in place: overwriting a running binary fails on some systems, and
    # a rename is atomic, so a half-copied svipall never exists.
    cp "$tmp/unpack/$b" "$PREFIX/.$b.new"
    chmod +x "$PREFIX/.$b.new"
    mv -f "$PREFIX/.$b.new" "$PREFIX/$b"
done
say "installed svipall and svipall-mcp in $PREFIX"

# macOS marks anything a browser downloaded. This script is not a browser, but a user who fetched
# the tarball by hand and passed --from-file has a quarantined file, and Gatekeeper's message for
# it ("cannot be opened because the developer cannot be verified") does not say what to do.
if [ "$os" = "Darwin" ] && command -v xattr >/dev/null 2>&1; then
    xattr -d com.apple.quarantine "$PREFIX/svipall" 2>/dev/null || true
    xattr -d com.apple.quarantine "$PREFIX/svipall-mcp" 2>/dev/null || true
fi

# ---- PATH ----------------------------------------------------------------------------------
on_path=0
case ":$PATH:" in *":$PREFIX:"*) on_path=1 ;; esac
profile=""
if [ "$on_path" -eq 0 ] && [ "$NO_PATH" -eq 0 ]; then
    case "${SHELL:-}" in
        */zsh)  profile="$HOME/.zshrc" ;;
        */bash) [ -f "$HOME/.bash_profile" ] && profile="$HOME/.bash_profile" || profile="$HOME/.bashrc" ;;
        */fish) profile="$HOME/.config/fish/config.fish" ;;
        *)      profile="$HOME/.profile" ;;
    esac
    line="export PATH=\"$PREFIX:\$PATH\""
    case "$profile" in *config.fish) line="fish_add_path $PREFIX" ;; esac
    if [ -f "$profile" ] && grep -Fq "$PREFIX" "$profile"; then
        say "$profile already mentions $PREFIX"
    else
        mkdir -p "$(dirname "$profile")"
        printf '\n# added by the svipall installer\n%s\n' "$line" >> "$profile"
        say "added $PREFIX to PATH in $profile"
    fi
fi

# ---- check ---------------------------------------------------------------------------------
say ""
"$PREFIX/svipall" --version || die "the installed binary does not run"
say ""
report="$("$PREFIX/svipall" doctor 2>/dev/null || true)"
printf '%s\n' "$report"

if [ "$BROWSER" != "no" ] && printf '%s' "$report" | grep -q '"code": *"no_browser"'; then
    say ""
    say "No browser was found. Without one, only the plain http tier works and any page behind a"
    say "challenge stays blocked. Chrome for Testing is about 190 MB."
    # Asked, never assumed. 190 MB is not something to spend on somebody's connection because
    # they typed --yes to get past a PATH question.
    if [ "$BROWSER" = "yes" ] || ask "Download it now?"; then
        "$PREFIX/svipall" browser install
    else
        say "Skipped. Run \`svipall browser install\` whenever you want it."
    fi
fi

say ""
say "Done. Next:"
if [ "$on_path" -eq 0 ] && [ "$NO_PATH" -eq 0 ]; then
    say "  open a new terminal (or: . $profile) so svipall is on your PATH"
fi
say "  claude mcp add svipall -- $PREFIX/svipall-mcp        # wire it into Claude Code"
say "  svipall fetch https://example.com                     # or just use it from a shell"
say ""
say "In Claude Code, the plugin does the wiring for you:"
say "  /plugin marketplace add $REPO"
say "  /plugin install svipall@svipall"
