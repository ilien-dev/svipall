param([string]$Directory = 'bench/experiments/local-20260905/runtime')
$ErrorActionPreference = 'Stop'
$directoryPath = (Resolve-Path -LiteralPath $Directory).Path
$manifest = Get-Content -LiteralPath (Join-Path $directoryPath 'windows-runtime.json') -Raw | ConvertFrom-Json
foreach ($name in 'msvcp140.dll', 'msvcp140_1.dll', 'vcruntime140.dll', 'vcruntime140_1.dll') {
    $entry = @($manifest.files | Where-Object name -eq $name)
    if ($entry.Count -ne 1) { throw "Missing runtime record: $name" }
    $path = Join-Path $directoryPath $name
    if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $entry[0].sha256) {
        throw "Runtime checksum mismatch: $name"
    }
}
Write-Host 'Windows runtime manifest and required DLLs verified.'
