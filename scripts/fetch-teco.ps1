# Fetch TECO, the only public corpus that can evaluate cross-page template detection.
# See fetch-teco.sh for what it is and why one category at a time.
#
#   pwsh scripts/fetch-teco.ps1 [-Target teco-corpus] [-Category forum]
param(
  [string]$Target = 'teco-corpus',
  [ValidateSet('companies', 'forum', 'organizations', 'media', 'personal')]
  [string]$Category = 'forum'
)
$ErrorActionPreference = 'Stop'

$base = 'https://mist.dsic.upv.es/teco/downloads/5.0'

New-Item -ItemType Directory -Force $Target | Out-Null
Push-Location $Target
try {
  $zip = "$Category.zip"
  # The archives are stored uncompressed and forum alone is 5 GB, so a partial download is the
  # normal failure. Checked against Content-Length rather than trusted, because a truncated zip
  # fails at extraction with a message about multi-part archives instead of about a short file.
  $expected = (Invoke-WebRequest -Uri "$base/$zip" -Method Head).Headers['Content-Length']
  if (Test-Path $zip) { $have = (Get-Item $zip).Length } else { $have = 0 }
  if ($have -ne [int64]$expected) {
    Write-Host "==> downloading $Category ($([math]::Round([int64]$expected / 1GB, 1)) GB)"
    Invoke-WebRequest -Uri "$base/$zip" -OutFile $zip
  }
  $have = (Get-Item $zip).Length
  if ($have -ne [int64]$expected) {
    throw "got $have of $expected bytes; run this again"
  }

  Write-Host '==> unpacking'
  Expand-Archive -Path $zip -DestinationPath '.' -Force

  # What has to be true for the corpus to be usable, checked rather than assumed.
  $sites = (Get-ChildItem $Category -Directory -ErrorAction SilentlyContinue | Measure-Object).Count
  $labelled = (Get-ChildItem $Category -Recurse -Include '*.htm', '*.html' -ErrorAction SilentlyContinue |
    Select-String -Pattern 'TECO_mainContent' -List | Measure-Object).Count

  Write-Host ''
  Write-Host "$sites site directories, $labelled labelled key pages"

  if ($labelled -lt 5) {
    Get-ChildItem -Directory -Depth 1 | Select-Object -First 20 | ForEach-Object { Write-Host $_.FullName }
    throw 'That is not the published corpus: every site carries one key page with per-node labels and its sibling pages beside it.'
  }

  Write-Host ''
  Write-Host "TECO/$Category ready under $(Join-Path (Get-Location) $Category)"
  Write-Host ''
  Write-Host 'Now run, from the repository root:'
  Write-Host "  cargo run -p svipall-bench --release -- extract --teco $(Join-Path (Get-Location) $Category)"
} finally {
  Pop-Location
}
