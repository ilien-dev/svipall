#!/usr/bin/env bash
# Fetch WCXB, the modern half of `svipall-bench extract`.
#
# Foley, "WCXB: A Multi-Type Web Content Extraction Benchmark" (2026): 2,008 human-reviewed pages
# from 1,613 domains, split 1,497 development / 511 held-out test, and — the reason it matters —
# labelled by *page type*: article, service, product, collection, forum, listing, documentation.
# CC-BY-4.0.
#
#   scripts/fetch-wcxb.sh [target-dir]      (default: ./wcxb-corpus)
#
# Why a second corpus at all. The SIGIR-23 gold standard is article-heavy and its pages are old;
# the study says so itself. Rankings invert between the two: Readability is first on SIGIR-23 and
# twelfth of thirteen here. Fitting an extractor against either one alone fits it to that corpus.
set -euo pipefail

REPO="https://github.com/Murrough-Foley/web-content-extraction-benchmark.git"
TARGET="${1:-wcxb-corpus}"

if ! command -v git >/dev/null 2>&1; then
  echo "git is required" >&2
  exit 1
fi

if [ -d "$TARGET/.git" ]; then
  echo "==> updating $TARGET"
  git -C "$TARGET" pull --ff-only
else
  echo "==> cloning WCXB into $TARGET"
  git clone --depth 1 "$REPO" "$TARGET"
fi

cd "$TARGET"

# The pages are gzipped in the tree, one file each, so there is nothing to unpack — but there is
# something to check. A partial clone leaves the directories in place and empty, and an empty
# corpus reads as a corpus of zero pages rather than as a failure.
for split in dev test; do
  for dir in "$split/ground-truth" "$split/html"; do
    if [ ! -d "$dir" ]; then
      echo "$dir is missing from the clone; the corpus layout has changed" >&2
      exit 1
    fi
  done
done

dev=$(find dev/ground-truth -name '*.json' | wc -l)
test=$(find test/ground-truth -name '*.json' | wc -l)
if [ "$dev" -lt 1000 ] || [ "$test" -lt 300 ]; then
  echo "only $dev development and $test test annotations — the published counts are 1,497 and 511." >&2
  echo "The clone is incomplete." >&2
  exit 1
fi

if [ ! -f metadata.json ]; then
  echo "metadata.json is missing; the page-type labels live there" >&2
  exit 1
fi

echo
echo "WCXB ready: $dev development and $test held-out pages under $(pwd)"
echo
echo "Now run, from the repository root:"
echo "  cargo run -p svipall-bench --release -- extract --wcxb $TARGET"
