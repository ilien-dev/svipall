param([string]$Root = 'bench/experiments/local-20260905', [switch]$RequireLocalRuntime, [string]$BinaryDirectory = 'bin')
$ErrorActionPreference = 'Stop'
$rootPath = [IO.Path]::GetFullPath((Join-Path (Get-Location) $Root))
$state = Join-Path $rootPath 'state/mcp-smoke'
New-Item -ItemType Directory -Force -Path $state | Out-Null
$listener = New-Object Net.Sockets.TcpListener([Net.IPAddress]::Loopback, 0)
$listener.Start()
$port = $listener.LocalEndpoint.Port
$listener.Stop()
[IO.File]::WriteAllText((Join-Path $state 'config.toml'), "dashboard_port = $port`nbrowser_auto_install = false`n", (New-Object Text.UTF8Encoding($false)))
$env:SVIPALL_HOME = $state
$env:SVIPALL_BROWSER = "$env:USERPROFILE/.svipall/browser/cft/152.0.7977.75/chrome-win64/chrome.exe"
$start = New-Object Diagnostics.ProcessStartInfo
$binaryPath = Join-Path $rootPath $BinaryDirectory
$start.FileName = Join-Path $binaryPath 'svipall-mcp.exe'
$start.UseShellExecute = $false
$start.CreateNoWindow = $true
$start.RedirectStandardInput = $true
$start.RedirectStandardOutput = $true
$start.RedirectStandardError = $true
[Console]::InputEncoding = New-Object Text.UTF8Encoding($false)
$proc = [Diagnostics.Process]::Start($start)
$inputWriter = $proc.StandardInput
$inputWriter.AutoFlush = $true
$errors = $proc.StandardError.ReadToEndAsync()
function Exchange($message) {
  $wire = $message | ConvertTo-Json -Depth 12 -Compress
  [IO.File]::WriteAllText((Join-Path $rootPath 'results/mcp-smoke-request.json'), $wire, (New-Object Text.UTF8Encoding($false)))
  $inputWriter.WriteLine($wire)
  do {
    $line = $proc.StandardOutput.ReadLineAsync()
    if (!$line.Wait(30000)) { throw 'MCP response timed out' }
    if ($null -eq $line.Result) { throw 'MCP exited before responding' }
    $response = $line.Result | ConvertFrom-Json
  } while ($response.id -ne $message.id)
  if ($response.error) { throw ($response.error | ConvertTo-Json -Compress) }
  return $response
}
try {
  $init = Exchange @{jsonrpc='2.0';id=1;method='initialize';params=@{protocolVersion='2024-11-05';capabilities=@{};clientInfo=@{name='local-smoke';version='1'}}}
  $inputWriter.WriteLine('{"jsonrpc":"2.0","method":"notifications/initialized"}')
  $tools = Exchange @{jsonrpc='2.0';id=2;method='tools/list';params=@{}}
  $initial = Exchange @{jsonrpc='2.0';id=3;method='tools/call';params=@{name='web_status';arguments=@{}}}
  if ($initial.result.isError) { throw 'Initial status call returned an error' }
  $previous = ($initial.result.content[0].text | ConvertFrom-Json).config.warm_wait_ms
  $requested = if ($previous -eq 21000) { 22000 } else { 21000 }
  $saved = Exchange @{jsonrpc='2.0';id=4;method='tools/call';params=@{name='web_status';arguments=@{configure=@{warm_wait_ms=$requested}}}}
  if ($saved.result.isError) { throw 'Configuration call returned an error' }
  $status = Exchange @{jsonrpc='2.0';id=5;method='tools/call';params=@{name='web_status';arguments=@{}}}
  if ($status.result.isError) { throw 'Status call returned an error' }
  $data = $status.result.content[0].text | ConvertFrom-Json
  if ($data.config.warm_wait_ms -ne $requested -or $data.config.warm_wait_ms -eq $previous) { throw 'The live server did not apply the new configuration' }
  $evidence = @{initialized=$true;tools=$tools.result.tools.Count;saved=$true;previous_warm_wait_ms=$previous;effective_warm_wait_ms=$data.config.warm_wait_ms}
  if ($RequireLocalRuntime) {
    $proc.Refresh()
    $modules = @($proc.Modules | Where-Object ModuleName -in @('msvcp140.dll','msvcp140_1.dll','vcruntime140.dll','vcruntime140_1.dll'))
    if ($modules.Count -ne 4) { throw 'Expected four loaded compiler runtime libraries' }
    foreach ($module in $modules) {
      if ([IO.Path]::GetDirectoryName($module.FileName) -ne $binaryPath) { throw "Runtime not loaded app-locally: $($module.FileName)" }
    }
    $evidence.runtime_modules = @($modules | Select-Object ModuleName,FileName)
  }
  [IO.File]::WriteAllText((Join-Path $rootPath 'results/mcp-smoke.json'), ($evidence | ConvertTo-Json), (New-Object Text.UTF8Encoding($false)))
  $evidence | ConvertTo-Json
} finally {
  $inputWriter.Close()
  if (!$proc.WaitForExit(10000)) { $proc.Kill(); $proc.WaitForExit() }
  [IO.File]::WriteAllText((Join-Path $rootPath 'results/mcp-smoke-stderr.txt'), $errors.Result)
  $proc.Dispose()
}
