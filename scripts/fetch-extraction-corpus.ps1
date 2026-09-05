# Fetch the gold standard for `svipall-bench extract`. See fetch-extraction-corpus.sh for why.
#
#   pwsh scripts/fetch-extraction-corpus.ps1 [-Target extraction-corpus]
param([string]$Target = 'extraction-corpus')
$ErrorActionPreference = 'Stop'

$repo = 'https://github.com/chatnoir-eu/web-content-extraction-benchmark.git'

if (-not (Get-Command git -ErrorAction SilentlyContinue)) { throw 'git is required' }
git lfs version *> $null
if ($LASTEXITCODE -ne 0) {
  throw @'
git-lfs is required: the corpus tarballs are LFS objects, and without it you get 133-byte
pointer files that look like a successful download. Install it (winget install GitHub.GitLFS),
run "git lfs install" once, then re-run this script.
'@
}
if (-not (Get-Command tar -ErrorAction SilentlyContinue)) { throw 'tar is required to unpack the datasets' }

if (Test-Path (Join-Path $Target '.git')) {
  Write-Host "==> updating $Target"
  git -C $Target pull --ff-only
  git -C $Target lfs pull
} else {
  Write-Host "==> cloning the benchmark into $Target"
  git clone --depth 1 $repo $Target
  git -C $Target lfs pull
}

Push-Location $Target
try {
  # A pointer file is a few hundred bytes; the real tarball is orders of magnitude larger.
  foreach ($f in 'datasets/combined.tar.xz', 'outputs/model-outputs.tar.xz') {
    if (-not (Test-Path $f)) { throw "$f is missing from the clone" }
    $size = (Get-Item $f).Length
    if ($size -lt 10000) {
      throw "$f is $size bytes - that is an LFS pointer, not the data. Run 'git lfs install' and retry."
    }
  }

  Write-Host '==> unpacking the combined datasets'
  tar xf datasets/combined.tar.xz -C datasets
  Write-Host "==> unpacking the study's own model outputs (the baselines to compare against)"
  tar xf outputs/model-outputs.tar.xz -C outputs

  $truth = 'datasets/combined/ground-truth'
  if (-not (Test-Path $truth)) { throw "expected $truth after unpacking; the corpus layout has changed" }

  $n = (Get-ChildItem "$truth/*.jsonl" -ErrorAction SilentlyContinue | Measure-Object).Count
  Write-Host ''
  Write-Host "corpus ready: $n datasets under $(Get-Location)"
  Write-Host ''
  Write-Host 'Now run, from the repository root:'
  Write-Host "  cargo run -p svipall-bench --release -- extract --corpus $Target"
} finally {
  Pop-Location
}
