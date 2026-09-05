# svipall quality control — the "oxlint" pass. Run occasionally to keep the tree clean.
# Fails on the first problem so nothing rots: dead code, unused deps, messy args, format drift.
#
#   pwsh scripts/qc.ps1          # check only
#   pwsh scripts/qc.ps1 -Fix     # apply fmt + clippy autofixes, then re-check
param([switch]$Fix)
$ErrorActionPreference = 'Stop'
$root = Join-Path $PSScriptRoot '..'
Push-Location $root
$fail = 0
function Step($name, [scriptblock]$body) {
  Write-Host "`n=== $name ===" -ForegroundColor Cyan
  & $body
  if ($LASTEXITCODE -ne 0) { Write-Host "FAIL: $name" -ForegroundColor Red; $script:fail = 1 }
}

if ($Fix) {
  Step 'rustfmt (apply)'      { cargo fmt --all }
  Step 'clippy --fix'         { cargo clippy --workspace --all-targets --fix --allow-dirty --allow-staged -- -D warnings }
  # The plugin's copy of the skill. Mechanical, so it belongs with the other mechanical fixes; the
  # test that compares them is what makes forgetting this a failure rather than a surprise.
  Step 'sync plugin skill'    { & (Join-Path $PSScriptRoot 'sync-plugin.ps1') }
}

# Formatting must be clean.
Step 'rustfmt --check'        { cargo fmt --all --check }
# Lints as errors: dead_code, unused vars/imports, and the full clippy set (bad args, redundant clones, etc.).
# svipall-cdp is vendored upstream code and caps its own clippy lints in its Cargo.toml; only the
# patches we wrote there opt back in. See crates/svipall-cdp/PATCHES.md.
Step 'clippy (default)'       { cargo clippy --workspace --exclude svipall-cdp --all-targets -- -D warnings }
Step 'clippy (onnx-ocr)'      { cargo clippy -p svipall-mcp --all-targets --features onnx-ocr -- -D warnings }
Step 'clippy (onnx-grid)'     { cargo clippy -p svipall-mcp --all-targets --features onnx-grid -- -D warnings }
Step 'clippy (onnx-audio)'    { cargo clippy -p svipall-mcp --all-targets --features onnx-audio -- -D warnings }
Step 'clippy (onnx-detect)'   { cargo clippy -p svipall-mcp --all-targets --features onnx-detect -- -D warnings }
Step 'clippy (onnx-segment)'  { cargo clippy -p svipall-mcp --all-targets --features onnx-segment -- -D warnings }
Step 'clippy (onnx-zeroshot)' { cargo clippy -p svipall-mcp --all-targets --features onnx-zeroshot -- -D warnings }
# The QUIC stack is off by default, so nothing else in this list ever compiles it.
Step 'clippy (http3)' { cargo clippy -p svipall-mcp --all-targets --features http3 -- -D warnings }
# Tests are the contract (TDD): they must pass.
Step 'tests'                  { cargo test --workspace }
# The h3 engine and the shape of the QUIC handshake it produces, offline.
Step 'tests (http3)'          { cargo test -p svipall-http --features http3 --test h3 }
# The inference paths, executed: a real ONNX Runtime session over hand-built fixture graphs, and
# over the embedded models when the build carries them. Lint-only coverage let a model path rot.
Step 'tests (onnx models)'    { cargo test -p svipall-mcp --features onnx-grid,onnx-segment,onnx-detect --test models }
# Unused dependencies (install once: cargo install cargo-machete).
if (Get-Command cargo-machete -ErrorAction SilentlyContinue) {
  Step 'cargo-machete'        { cargo machete }
} else {
  Write-Host "`n=== cargo-machete (skipped) ===" -ForegroundColor Yellow
  Write-Host "install with: cargo install cargo-machete"
}
# CLAUDE.md size guard.
Step 'CLAUDE.md size'         { & (Join-Path $PSScriptRoot 'check-claude-md.ps1') }
# The marketplace and the plugin manifests, if the CLI that reads them is here. Skipped rather than
# failed when it is not: this gate has to pass on a machine that has never installed Claude Code.
if (Get-Command claude -ErrorAction SilentlyContinue) {
  Step 'plugin manifests'     { claude plugin validate . }
  Step 'plugin manifest (svipall)' { claude plugin validate ./plugins/svipall }
} else {
  Write-Host "`n=== plugin manifests (skipped) ===" -ForegroundColor Yellow
  Write-Host "no claude CLI on PATH"
}
# CPU budgets and the structural counts (one DOM parse, no disk reads on the hot path).
# No network, so it belongs in the standard gate.
Step 'perf budgets'           { cargo run -p svipall-bench --release -- micro --assert }

# The extraction floors, but only where the corpora are. They are several hundred megabytes of
# other people's web pages and are not in this repository, so a machine without them has not failed
# anything - it simply has not measured. Set SVIPALL_CORPUS (and optionally SVIPALL_WCXB,
# SVIPALL_DANIEL) to what scripts/fetch-*.ps1 put on disk and this becomes a gate. SVIPALL_DANIEL is
# the subdirectory fetch-daniel.ps1 prints, not the clone root: that repository holds several
# corpora and only one of them is this one. With SVIPALL_WCXB set, this also runs the cross-page
# template gate, which is absolute.
if ($env:SVIPALL_CORPUS -and (Test-Path $env:SVIPALL_CORPUS)) {
  Step 'extraction floors' {
    $a = @('extract', '--corpus', $env:SVIPALL_CORPUS, '--assert')
    if ($env:SVIPALL_WCXB -and (Test-Path $env:SVIPALL_WCXB)) { $a += @('--wcxb', $env:SVIPALL_WCXB) }
    if ($env:SVIPALL_DANIEL -and (Test-Path $env:SVIPALL_DANIEL)) { $a += @('--daniel', $env:SVIPALL_DANIEL) }
    if ($env:SVIPALL_TECO -and (Test-Path $env:SVIPALL_TECO)) { $a += @('--teco', $env:SVIPALL_TECO) }
    cargo run -p svipall-bench --release -- @a
  }
} else {
  Write-Host ''
  Write-Host '=== extraction floors ==='
  Write-Host 'skipped: set SVIPALL_CORPUS to where scripts/fetch-extraction-corpus.ps1 put the gold'
  Write-Host 'standard to hold the extractor to its floors.'
}
# Its own target directory on purpose. The feature flag changes the binary, so sharing a path with
# the steps around it means relinking `svipall-bench.exe` three times in a row — and on Windows the
# image of a process that has just exited stays locked for a moment, so the next link fails with
# "Access is denied" and takes an unrelated step down with it.
Step 'perf budgets (models)'  {
  $prev = $env:CARGO_TARGET_DIR
  $base = if ($prev) { $prev } else { 'target' }
  $env:CARGO_TARGET_DIR = Join-Path $base 'onnx'
  try { cargo run -p svipall-bench --release --features onnx -- micro --assert }
  finally { $env:CARGO_TARGET_DIR = $prev }
}
# What a detector reads off the session, against a page served on loopback. No network, so it
# belongs in the gate; skips itself when there is no browser to open.
Step 'automation tells'       { cargo run -p svipall-bench --release -- tells --assert }
# Identity coherence, offline: every identity svipall would wear, checked against itself. Fails
# the build on a contradiction (a Chrome UA on a Firefox engine, a taskbarless desktop, ...).
Step 'identity coherence'     { cargo run -p svipall-bench --release -- fingerprint --engine chrome }

Pop-Location
if ($fail -ne 0) { Write-Host "`nQC FAILED" -ForegroundColor Red; exit 1 }
Write-Host "`nQC PASSED" -ForegroundColor Green
