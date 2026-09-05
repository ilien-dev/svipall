# Guard: CLAUDE.md must stay small (project rule: <= 1000 tokens).
# Token estimate is tokenizer-free and deliberately conservative: max(chars/4, words*1.3),
# rounded up. Exits non-zero when the estimate exceeds the cap so CI / the QC script can fail.
param(
  [string]$Path = (Join-Path $PSScriptRoot '..\CLAUDE.md'),
  [int]$Max = 1000
)
$ErrorActionPreference = 'Stop'
if (-not (Test-Path $Path)) { Write-Error "CLAUDE.md not found at $Path"; exit 2 }
$text  = Get-Content -Raw -Path $Path
$chars = $text.Length
$words = ($text -split '\s+' | Where-Object { $_ -ne '' }).Count
$est   = [math]::Ceiling([math]::Max($chars / 4.0, $words * 1.3))
$bar   = if ($est -le $Max) { 'OK' } else { 'OVER' }
Write-Host ("CLAUDE.md: ~{0} tokens (chars={1}, words={2}) cap={3} -> {4}" -f $est, $chars, $words, $Max, $bar)
if ($est -gt $Max) {
  Write-Error "CLAUDE.md is ~$est tokens, over the $Max cap. Trim it before committing."
  exit 1
}
exit 0
