#!/usr/bin/env bash
# Fetch DAnIEL, the language half of `svipall-bench extract`.
#
# Lejeune et al., "DAnIEL: Language Independent Character-Based News Surveillance" (2012), as used
# by "Multilingual Benchmarking of Main Content Extractors" (SIGIR 2025): 1,689 news pages in
# Greek, English, Polish, Russian and Chinese, each with a human reference extraction.
#
#   scripts/fetch-daniel.sh [target-dir]      (default: ./daniel-corpus)
#
# Why. Every widely used extraction benchmark is English. The SIGIR-2025 study measured what that
# costs: on the same code, Readability scores 0.862 on English and 0.672 on Chinese, and the share
# of pages scoring under 0.3 goes from 16% (English) to 95% (Chinese). The named cause is the
# features themselves — character counts and comma counts are not language-neutral. svipall answers
# in whatever language the page is written in, so this is not an academic point for it.
set -euo pipefail

REPO="https://github.com/rundimeco/waddle.git"
TARGET="${1:-daniel-corpus}"
SUB="corpora/Corpus_daniel_v2.1"

if ! command -v git >/dev/null 2>&1; then
  echo "git is required" >&2
  exit 1
fi

if [ -d "$TARGET/.git" ]; then
  echo "==> updating $TARGET"
  git -C "$TARGET" pull --ff-only
else
  # Sparse: the repository carries several corpora and we want one of them.
  echo "==> cloning DAnIEL into $TARGET"
  git clone --depth 1 --filter=blob:none --sparse "$REPO" "$TARGET"
  git -C "$TARGET" sparse-checkout set "$SUB"
fi

cd "$TARGET/$SUB"

for d in html reference; do
  if [ ! -d "$d" ]; then
    echo "$d is missing; the corpus layout has changed" >&2
    exit 1
  fi
done
if [ ! -f doc_lg.json ]; then
  echo "doc_lg.json is missing; the language labels live there" >&2
  exit 1
fi

pages=$(find html -type f | wc -l)
refs=$(find reference -type f | wc -l)
if [ "$pages" -lt 1500 ] || [ "$refs" -lt 1500 ]; then
  echo "only $pages pages and $refs references — the published count is 1,689. The clone is incomplete." >&2
  exit 1
fi

echo
echo "DAnIEL ready: $pages pages under $(pwd)"
echo
echo "Now run, from the repository root:"
echo "  cargo run -p svipall-bench --release -- extract --daniel $TARGET/$SUB"
