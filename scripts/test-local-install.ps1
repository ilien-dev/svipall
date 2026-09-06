# Offline install/uninstall fixture. The archive is for this machine, not redistribution.
param([string]$Root = 'bench/experiments/local-20260905')
$ErrorActionPreference = 'Stop'
$rootPath = (Resolve-Path -LiteralPath $Root).Path
$runtime = Join-Path $rootPath 'runtime'
$state = Join-Path $rootPath ('state/install-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $state | Out-Null
$prefix = Join-Path $state 'installed'
$archive = Join-Path $state 'local-test.zip'
$env:SVIPALL_HOME = Join-Path $state 'home'
$files = @('svipall.exe','svipall-mcp.exe','windows-runtime.json') | ForEach-Object { Join-Path $runtime $_ }
$files += @(Get-ChildItem -LiteralPath $runtime -Filter '*.dll' -File | Select-Object -ExpandProperty FullName)
Compress-Archive -LiteralPath $files -DestinationPath $archive
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot '../install.ps1') -FromFile $archive -Prefix $prefix -NoPath -NoBrowser
if ($LASTEXITCODE -ne 0) { throw 'Offline installation failed' }
& (Join-Path $PSScriptRoot 'test-windows-runtime.ps1') -Directory $prefix
$config = & (Join-Path $prefix 'svipall.exe') config show | ConvertFrom-Json
if ($config.browser_auto_install -ne $false -and $config.config.browser_auto_install -ne $false) { throw 'Browser opt-out was not persisted' }
[IO.File]::WriteAllText((Join-Path $prefix 'unrelated.dll'), 'keep unrelated file')
[IO.File]::WriteAllText((Join-Path $prefix 'msvcp140.dll'), 'keep modified runtime')
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot '../install.ps1') -Prefix $prefix -Uninstall
if ($LASTEXITCODE -ne 0) { throw 'Offline uninstall failed' }
foreach ($retained in 'unrelated.dll','msvcp140.dll') {
    if (!(Test-Path -LiteralPath (Join-Path $prefix $retained))) { throw "Uninstall removed $retained" }
}
foreach ($removed in 'svipall.exe','svipall-mcp.exe','vcruntime140.dll','windows-runtime.json') {
    if (Test-Path -LiteralPath (Join-Path $prefix $removed)) { throw "Uninstall left $removed" }
}
$evidence = @{installed=$true;runtime_verified=$true;browser_opt_out=$true;uninstalled=$true;unrelated_and_modified_preserved=$true;fixture=$state}
[IO.File]::WriteAllText((Join-Path $rootPath 'results/install-smoke.json'), ($evidence | ConvertTo-Json), (New-Object Text.UTF8Encoding($false)))
$evidence | ConvertTo-Json
