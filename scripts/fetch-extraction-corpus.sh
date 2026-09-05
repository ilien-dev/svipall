#!/usr/bin/env bash
# Fetch the gold standard for `svipall-bench extract`.
#
# Bevendorff et al., "An Empirical Comparison of Web Content Extraction Algorithms" (SIGIR 2023):
# eight annotated datasets combined into 3,985 pages, plus the extractions every model in the study
# produced. Apache-2.0, and not carried in this repository — it is a few hundred megabytes of web
# pages, and vendoring someone else's corpus is not our business.
#
#   scripts/fetch-extraction-corpus.sh [target-dir]     (default: ./extraction-corpus)
#
# The tarballs are Git LFS pointers, so git-lfs has to be installed: a plain clone gets 133-byte
# text files where the data should be, and the failure is silent until the benchmark reads an empty
# corpus. That is checked for here rather than discovered later.
set -euo pipefail

REPO="https://github.com/chatnoir-eu/web-content-extraction-benchmark.git"
TARGET="${1:-extraction-corpus}"

if ! command -v git >/dev/null 2>&1; then
  echo "git is required" >&2
  exit 1
fi
if ! git lfs version >/dev/null 2>&1; then
  echo "git-lfs is required: the corpus tarballs are LFS objects, and without it you get" >&2
  echo "133-byte pointer files that look like a successful download." >&2
  echo "  Debian/Ubuntu: apt install git-lfs    macOS: brew install git-lfs" >&2
  exit 1
fi
if ! command -v tar >/dev/null 2>&1; then
  echo "tar is required to unpack the datasets" >&2
  exit 1
fi

if [ -d "$TARGET/.git" ]; then
  echo "==> updating $TARGET"
  git -C "$TARGET" pull --ff-only
  git -C "$TARGET" lfs pull
else
  echo "==> cloning the benchmark into $TARGET"
  # No submodules: those are the neural extractors, and we score the study's published outputs
  # rather than re-running anything.
  git clone --depth 1 "$REPO" "$TARGET"
  git -C "$TARGET" lfs pull
fi

cd "$TARGET"

# A pointer file is a few hundred bytes; the real tarball is orders of magnitude larger. Checking
# the size is the cheapest way to catch an LFS setup that silently did nothing.
for f in datasets/combined.tar.xz outputs/model-outputs.tar.xz; do
  if [ ! -f "$f" ]; then
    echo "$f is missing from the clone" >&2
    exit 1
  fi
  size=$(wc -c <"$f")
  if [ "$size" -lt 10000 ]; then
    echo "$f is $size bytes — that is an LFS pointer, not the data." >&2
    echo "Run 'git lfs install' once, then re-run this script." >&2
    exit 1
  fi
done

echo "==> unpacking the combined datasets"
tar xf datasets/combined.tar.xz -C datasets
echo "==> unpacking the study's own model outputs (the baselines to compare against)"
tar xf outputs/model-outputs.tar.xz -C outputs

truth="datasets/combined/ground-truth"
if [ ! -d "$truth" ]; then
  echo "expected $truth after unpacking; the corpus layout has changed" >&2
  exit 1
fi

echo
echo "corpus ready: $(ls "$truth"/*.jsonl 2>/dev/null | wc -l) datasets under $(pwd)"
echo
echo "Now run, from the repository root:"
echo "  cargo run -p svipall-bench --release -- extract --corpus $TARGET"
