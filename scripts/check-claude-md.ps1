# Guard: agent instructions must stay small (project rule: <= 1000 tokens per file).
# Token estimate is tokenizer-free and deliberately conservative: max(chars/4, words*1.3),
# rounded up. Exits non-zero when the estimate exceeds the cap so CI / the QC script can fail.
param(
  [string]$Path = (Join-Path $PSScriptRoot '..\CLAUDE.md'),
  [int]$Max = 1000
)
$ErrorActionPreference = 'Stop'
$name = Split-Path -Leaf $Path
if (-not (Test-Path -LiteralPath $Path)) { Write-Error "$name not found at $Path"; exit 2 }
$text  = Get-Content -Raw -LiteralPath $Path -Encoding utf8
$chars = $text.Length
$words = ($text -split '\s+' | Where-Object { $_ -ne '' }).Count
$est   = [math]::Ceiling([math]::Max($chars / 4.0, $words * 1.3))
$bar   = if ($est -le $Max) { 'OK' } else { 'OVER' }
Write-Host ("{0}: ~{1} tokens (chars={2}, words={3}) cap={4} -> {5}" -f $name, $est, $chars, $words, $Max, $bar)
if ($est -gt $Max) {
  Write-Error "$name is ~$est tokens, over the $Max cap. Trim it before committing."
  exit 1
}
exit 0
