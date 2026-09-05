# Fetch WCXB, the modern half of `svipall-bench extract`. See fetch-wcxb.sh for why.
#
#   pwsh scripts/fetch-wcxb.ps1 [-Target wcxb-corpus]
param([string]$Target = 'wcxb-corpus')
$ErrorActionPreference = 'Stop'

$repo = 'https://github.com/Murrough-Foley/web-content-extraction-benchmark.git'

if (-not (Get-Command git -ErrorAction SilentlyContinue)) { throw 'git is required' }

if (Test-Path (Join-Path $Target '.git')) {
  Write-Host "==> updating $Target"
  git -C $Target pull --ff-only
} else {
  Write-Host "==> cloning WCXB into $Target"
  git clone --depth 1 $repo $Target
}

Push-Location $Target
try {
  # Nothing to unpack - the pages are gzipped one file each - but plenty to check. A partial clone
  # leaves the directories in place and empty, and an empty corpus reads as zero pages rather than
  # as a failure.
  foreach ($d in 'dev/ground-truth', 'dev/html', 'test/ground-truth', 'test/html') {
    if (-not (Test-Path $d)) { throw "$d is missing from the clone; the corpus layout has changed" }
  }
  $dev = (Get-ChildItem 'dev/ground-truth/*.json' -ErrorAction SilentlyContinue | Measure-Object).Count
  $test = (Get-ChildItem 'test/ground-truth/*.json' -ErrorAction SilentlyContinue | Measure-Object).Count
  if ($dev -lt 1000 -or $test -lt 300) {
    throw "only $dev development and $test test annotations - the published counts are 1,497 and 511. The clone is incomplete."
  }
  if (-not (Test-Path 'metadata.json')) { throw 'metadata.json is missing; the page-type labels live there' }

  Write-Host ''
  Write-Host "WCXB ready: $dev development and $test held-out pages under $(Get-Location)"
  Write-Host ''
  Write-Host 'Now run, from the repository root:'
  Write-Host "  cargo run -p svipall-bench --release -- extract --wcxb $Target"
} finally {
  Pop-Location
}
