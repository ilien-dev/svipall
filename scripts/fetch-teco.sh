#!/usr/bin/env bash
# Fetch TECO, the only public corpus that can evaluate cross-page template detection.
#
# Alarte and Silva, "TeCo: A Template Extraction Corpus" (arXiv:1409.6182), BSD. 150 real websites,
# and — the reason it is the only one — **each key page is shipped with its sibling pages**, so a
# method that reads the rest of a site has a rest of the site to read. Every DOM node of the key
# page is labelled `TECO_mainContent`, `TECO_notTemplate` or `TECO_mainMenu`, in nine languages,
# reconciled between four engineers.
#
#   scripts/fetch-teco.sh [target-dir] [category]   (default: ./teco-corpus forum)
#
# ▲ **One category at a time, and that is not a convenience.** The archives are stored uncompressed
# and `forum` alone is 5.0 GB; all five together are north of 20 GB. `forum` is the default because
# it is the page type svipall does worst on and the one the cross-page work was written for.
# Categories: companies, forum, organizations, media, personal.
#
# The corpus's condition of use is that results obtained with it are published. Anything measured
# with it belongs in docs/extraction.md, including a result that says the method did not help.
set -euo pipefail

BASE="https://mist.dsic.upv.es/teco/downloads/5.0"
TARGET="${1:-teco-corpus}"
CATEGORY="${2:-forum}"

case "$CATEGORY" in
  companies|forum|organizations|media|personal) ;;
  *) echo "unknown category '$CATEGORY': companies, forum, organizations, media, personal" >&2
     exit 1 ;;
esac

for tool in curl unzip; do
  command -v "$tool" >/dev/null 2>&1 || { echo "$tool is required" >&2; exit 1; }
done

mkdir -p "$TARGET"
cd "$TARGET"

# `-C -` resumes: a five-gigabyte download over a university link does not always arrive in one
# piece, and a truncated zip fails at `unzip` with a message about a multi-part archive rather than
# about a short file.
echo "==> downloading $CATEGORY (about 5 GB for forum; resumable)"
curl -fL --retry 3 -C - -o "$CATEGORY.zip" "$BASE/$CATEGORY.zip"

expected=$(curl -sIL "$BASE/$CATEGORY.zip" | tr -d '\r' | awk 'tolower($1)=="content-length:"{n=$2} END{print n}')
actual=$(wc -c < "$CATEGORY.zip")
if [ -n "$expected" ] && [ "$expected" != "$actual" ]; then
  echo "got $actual of $expected bytes; run this again to resume" >&2
  exit 1
fi

echo "==> unpacking"
unzip -q -o "$CATEGORY.zip"

# What has to be true for the corpus to be usable, checked rather than assumed. A partial archive
# leaves directories in place and empty, and an empty corpus reads as zero sites rather than as a
# failure.
sites=$(find "$CATEGORY" -maxdepth 1 -mindepth 1 -type d 2>/dev/null | wc -l)
labelled=$(grep -rl "TECO_mainContent" --include='*.htm*' "$CATEGORY" 2>/dev/null | wc -l)

echo
echo "$sites site directories, $labelled labelled key pages"

if [ "$labelled" -lt 5 ]; then
  echo >&2
  echo "That is not the published corpus: every site carries one key page with per-node labels," >&2
  echo "and its sibling pages beside it. What is on disk now:" >&2
  find . -maxdepth 2 -type d | head -20 >&2
  exit 1
fi

echo
echo "TECO/$CATEGORY ready under $(pwd)/$CATEGORY"
echo
echo "Now run, from the repository root:"
echo "  cargo run -p svipall-bench --release -- extract --teco $(pwd)/$CATEGORY"
