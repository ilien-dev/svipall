# Run by the user's interactive Task Scheduler session so a conversation restart cannot terminate
# a partially measured round. The task is temporary and is removed after the experiment completes.
$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath (Split-Path $PSScriptRoot -Parent)
$experimentPath = Join-Path (Get-Location) 'bench/experiments/local-20260905'
$stamp = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ')
$logPath = Join-Path $experimentPath "controller-task-$stamp.txt"
$resultPath = Join-Path $experimentPath 'controller-result.json'
$started = [DateTime]::UtcNow.ToString('o')
$startRecord = @{pid=$PID;started=$started;log=$logPath}
[IO.File]::WriteAllText((Join-Path $experimentPath 'controller-start.json'), ($startRecord | ConvertTo-Json), (New-Object Text.UTF8Encoding($false)))
try {
    & (Join-Path $PSScriptRoot 'compare-local.ps1') *> $logPath
    $result = @{ok=$true;started=$started;ended=[DateTime]::UtcNow.ToString('o');log=$logPath}
    [IO.File]::WriteAllText($resultPath, ($result | ConvertTo-Json), (New-Object Text.UTF8Encoding($false)))
    exit 0
} catch {
    $result = @{ok=$false;started=$started;ended=[DateTime]::UtcNow.ToString('o');error=$_.Exception.Message;log=$logPath}
    [IO.File]::WriteAllText($resultPath, ($result | ConvertTo-Json), (New-Object Text.UTF8Encoding($false)))
    exit 1
}
