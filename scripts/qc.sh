#!/usr/bin/env bash
# svipall quality control — the "oxlint" pass. Run occasionally to keep the tree clean.
# Fails on the first problem so nothing rots: dead code, unused deps, messy args, format drift.
#
#   scripts/qc.sh           # check only
#   scripts/qc.sh --fix     # apply fmt + clippy autofixes, then re-check
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$here/.."
fail=0
step() { echo; echo "=== $1 ==="; shift; "$@" || { echo "FAIL"; fail=1; }; }

if [ "${1:-}" = "--fix" ]; then
  step 'rustfmt (apply)' cargo fmt --all
  step 'clippy --fix'    cargo clippy --workspace --all-targets --fix --allow-dirty --allow-staged -- -D warnings
  # The plugin's copy of the skill. Mechanical, so it belongs with the other mechanical fixes; the
  # test that compares them is what makes forgetting this a failure rather than a surprise.
  step 'sync plugin skill' bash "$here/sync-plugin.sh"
fi

step 'rustfmt --check' cargo fmt --all --check
step 'clippy (default)' cargo clippy --workspace --all-targets -- -D warnings
step 'clippy (onnx-ocr)' cargo clippy -p svipall-mcp --all-targets --features onnx-ocr -- -D warnings
step 'clippy (onnx-grid)' cargo clippy -p svipall-mcp --all-targets --features onnx-grid -- -D warnings
step 'clippy (onnx-audio)' cargo clippy -p svipall-mcp --all-targets --features onnx-audio -- -D warnings
step 'clippy (onnx-detect)' cargo clippy -p svipall-mcp --all-targets --features onnx-detect -- -D warnings
step 'clippy (onnx-segment)' cargo clippy -p svipall-mcp --all-targets --features onnx-segment -- -D warnings
step 'clippy (onnx-zeroshot)' cargo clippy -p svipall-mcp --all-targets --features onnx-zeroshot -- -D warnings
# The QUIC stack is off by default, so nothing else in this list ever compiles it.
step 'clippy (http3)' cargo clippy -p svipall-mcp --all-targets --features http3 -- -D warnings
step 'tests' cargo test --workspace
# The h3 engine and the shape of the QUIC handshake it produces, offline.
step 'tests (http3)' cargo test -p svipall-http --features http3 --test h3
# The inference paths, executed: a real ONNX Runtime session over hand-built fixture graphs, and
# over the embedded models when the build carries them.
step 'tests (onnx models)' cargo test -p svipall-mcp --features onnx-grid,onnx-segment,onnx-detect --test models
if command -v cargo-machete >/dev/null 2>&1; then
  step 'cargo-machete' cargo machete
else
  echo; echo "=== cargo-machete (skipped) ==="; echo "install with: cargo install cargo-machete"
fi
step 'CLAUDE.md size' bash "$here/check-claude-md.sh"
step 'AGENTS.md size' bash "$here/check-claude-md.sh" "$here/../AGENTS.md"
# The marketplace and the plugin manifests, if the CLI that reads them is here. Skipped rather than
# failed when it is not: this gate has to pass on a machine that has never installed Claude Code.
if command -v claude >/dev/null 2>&1; then
  step 'plugin manifests' claude plugin validate .
  step 'plugin manifest (svipall)' claude plugin validate ./plugins/svipall
else
  echo; echo "=== plugin manifests (skipped) ==="; echo "no claude CLI on PATH"
fi
# CPU budgets and the structural counts (one DOM parse, no disk reads on the hot path).
# No network, so it belongs in the standard gate.
step 'perf budgets' cargo run -p svipall-bench --release -- micro --assert

# The extraction floors, but only where the corpora are. They are several hundred megabytes of
# other people's web pages and are not in this repository, so a machine without them is not
# failing anything — it simply has not measured. Point SVIPALL_CORPUS, SVIPALL_WCXB and
# SVIPALL_DANIEL at what scripts/fetch-*.sh put on disk and this becomes a gate. SVIPALL_DANIEL is
# the subdirectory fetch-daniel.sh prints, not the clone root: that repository holds several corpora.
# SVIPALL_TECO is one downloaded category (5 GB for `forum`) and is the only corpus that can score
# the cross-page template at all.
#
# With SVIPALL_TECO set, this also gates the cross-page template. That gate is "no worse than
# measured" rather than absolute, because the feature it measures is off by default for exactly the
# reason the gate would otherwise fail: see bench::teco::MAX_TEMPLATE_LOSS.
if [ -d "${SVIPALL_CORPUS:-}" ]; then
  extract_args=(extract --corpus "$SVIPALL_CORPUS" --assert)
  [ -d "${SVIPALL_WCXB:-}" ] && extract_args+=(--wcxb "$SVIPALL_WCXB")
  [ -d "${SVIPALL_DANIEL:-}" ] && extract_args+=(--daniel "$SVIPALL_DANIEL")
  [ -d "${SVIPALL_TECO:-}" ] && extract_args+=(--teco "$SVIPALL_TECO")
  step 'extraction floors' cargo run -p svipall-bench --release -- "${extract_args[@]}"
else
  echo
  echo "=== extraction floors ==="
  echo "skipped: set SVIPALL_CORPUS to where scripts/fetch-extraction-corpus.sh put the gold"
  echo "standard (and optionally SVIPALL_WCXB, SVIPALL_DANIEL) to hold the extractor to its floors."
fi
# Its own target directory on purpose. The feature flag changes the binary, so sharing a path with
# the steps around it means relinking `svipall-bench.exe` three times in a row — and on Windows the
# image of a process that has just exited stays locked for a moment, so the next link fails with
# "Access is denied" and takes an unrelated step down with it.
step 'perf budgets (models)' env CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target}/onnx" \
  cargo run -p svipall-bench --release --features onnx -- micro --assert
# What a detector reads off the session, against a page served on loopback. No network, so it
# belongs in the gate; skips itself when there is no browser to open.
step 'automation tells' cargo run -p svipall-bench --release -- tells --assert
# Identity coherence, offline: every identity checked against itself; fails on a contradiction.
step 'identity coherence' cargo run -p svipall-bench --release -- fingerprint --engine chrome

echo
if [ "$fail" -ne 0 ]; then echo "QC FAILED"; exit 1; fi
echo "QC PASSED"
