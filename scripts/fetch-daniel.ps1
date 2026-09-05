# Fetch DAnIEL, the language half of `svipall-bench extract`. See fetch-daniel.sh for why.
#
#   pwsh scripts/fetch-daniel.ps1 [-Target daniel-corpus]
param([string]$Target = 'daniel-corpus')
$ErrorActionPreference = 'Stop'

$repo = 'https://github.com/rundimeco/waddle.git'
$sub = 'corpora/Corpus_daniel_v2.1'

if (-not (Get-Command git -ErrorAction SilentlyContinue)) { throw 'git is required' }

if (Test-Path (Join-Path $Target '.git')) {
  Write-Host "==> updating $Target"
  git -C $Target pull --ff-only
} else {
  # Sparse: the repository carries several corpora and we want one of them.
  Write-Host "==> cloning DAnIEL into $Target"
  git clone --depth 1 --filter=blob:none --sparse $repo $Target
  git -C $Target sparse-checkout set $sub
}

Push-Location (Join-Path $Target $sub)
try {
  foreach ($d in 'html', 'reference') {
    if (-not (Test-Path $d)) { throw "$d is missing; the corpus layout has changed" }
  }
  if (-not (Test-Path 'doc_lg.json')) { throw 'doc_lg.json is missing; the language labels live there' }
  $pages = (Get-ChildItem html -File | Measure-Object).Count
  $refs = (Get-ChildItem reference -File | Measure-Object).Count
  if ($pages -lt 1500 -or $refs -lt 1500) {
    throw "only $pages pages and $refs references - the published count is 1,689. The clone is incomplete."
  }
  Write-Host ''
  Write-Host "DAnIEL ready: $pages pages under $(Get-Location)"
  Write-Host ''
  Write-Host 'Now run, from the repository root:'
  Write-Host "  cargo run -p svipall-bench --release -- extract --daniel $Target/$sub"
} finally {
  Pop-Location
}
