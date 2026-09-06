# Run after comparison traffic has finished so these checks cannot affect its timing or history.
param([string]$Root = 'bench/experiments/local-20260905')
$ErrorActionPreference = 'Stop'
$rootPath = (Resolve-Path -LiteralPath $Root).Path
$manifest = Get-Content -LiteralPath (Join-Path $rootPath 'manifest.json') -Raw | ConvertFrom-Json
$env:SVIPALL_BROWSER = $manifest.browser
$env:SVIPALL_HOME = Join-Path $rootPath ('state/manual-network-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $env:SVIPALL_HOME | Out-Null
$checks = @(
    @{name='stealth';args=@('test','-p','svipall-mcp','--test','stealth','--','--ignored','--test-threads=1','--nocapture')},
    @{name='fingerprint';args=@('test','-p','svipall-http','--test','fingerprint','--','--ignored','--test-threads=1','--nocapture')},
    @{name='http3';args=@('test','-p','svipall-http','--features','http3','--test','h3','--','--ignored','--test-threads=1','--nocapture')}
)
$records = @(foreach ($check in $checks) {
    $stdout = Join-Path $rootPath "results/manual-$($check.name).txt"
    $stderr = Join-Path $rootPath "results/manual-$($check.name)-stderr.txt"
    $started = [DateTime]::UtcNow.ToString('o')
    $proc = Start-Process -FilePath cargo -ArgumentList $check.args -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $null = $proc.Handle
    $proc.WaitForExit()
    @{name=$check.name;exit_code=$proc.ExitCode;started=$started;ended=[DateTime]::UtcNow.ToString('o');stdout=$stdout;stderr=$stderr}
})
$evidence = @{browser=$env:SVIPALL_BROWSER;home=$env:SVIPALL_HOME;checks=$records}
[IO.File]::WriteAllText((Join-Path $rootPath 'results/network-tests.json'), ($evidence | ConvertTo-Json -Depth 5), (New-Object Text.UTF8Encoding($false)))
$evidence | ConvertTo-Json -Depth 5
if (@($records | Where-Object exit_code -ne 0).Count) { exit 1 }
