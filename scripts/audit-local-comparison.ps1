param([string]$Root = 'bench/experiments/local-20260905')
$ErrorActionPreference = 'Stop'
$rootPath = (Resolve-Path -LiteralPath $Root).Path
$manifest = Get-Content -LiteralPath (Join-Path $rootPath 'manifest.json') -Raw | ConvertFrom-Json
$frozenHashes = @{
    before='1BB26A54B3ADF851891C2529BF1B9464846150468EB9B4FED1D8C45FC5CAFA32'
    after='404677BB83A9A59E27A2AEEB1F659034A5A0A84D2782458B94EC5B5744BCD874'
}
foreach ($arm in 'before','after') {
    $actual = (Get-FileHash -LiteralPath (Join-Path $rootPath "bin/$arm.exe") -Algorithm SHA256).Hash
    if ($actual -ne $manifest."${arm}_sha256" -or $actual -ne $frozenHashes[$arm]) { throw "Changed $arm executable" }
}
if ((Get-FileHash -LiteralPath $manifest.browser -Algorithm SHA256).Hash -ne $manifest.browser_sha256) { throw 'Changed browser executable' }
if ($manifest.browser_sha256 -ne '62ADCA492CFAEB58B6A677FCE795C1865D7481FB80A9751C951F491B41CC808C') { throw 'Browser differs from frozen experiment' }
$samples = @()
$measurements = @()
$orders = @{}
$labels = @()
foreach ($round in 1..3) {
    foreach ($set in 'public31','hard12','vendors8') {
        foreach ($arm in 'before','after','native') {
            $label = "$arm-$set-$round"
            $run = Get-Content -LiteralPath (Join-Path $rootPath "results/$label.json") -Raw | ConvertFrom-Json
            $expected = @{public31=31;hard12=12;vendors8=8}[$set] * 2
            if ($run.label -ne $label -or $run.set -ne $set -or $run.cells.Count -ne $expected -or !$run.ended_unix) { throw "Incomplete $label" }
            if ($run.seed -ne (20260905 + $round) -or $run.timeout_ms -ne 90000 -or $run.repeat -ne 2 -or $run.browser -ne $manifest.browser) { throw "Protocol mismatch: $label" }
            $identity = if ($arm -eq 'native') { 'native' } else { 'emulated' }
            $adaptive = if ($arm -eq 'before') { 'false' } else { 'true' }
            if ($run.config_toml -notmatch "browser_identity = '$identity'" -or $run.config_toml -notmatch "warm_adaptive = $adaptive" -or $run.config_toml -notmatch 'warm_max_wait_ms = 55000') { throw "Configuration mismatch: $label" }
            $order = ($run.cells | ForEach-Object { "$($_.target):$($_.position):$($_.url)" }) -join '|'
            if ($orders.ContainsKey("$set-$round") -and $orders["$set-$round"] -ne $order) { throw "Target order mismatch: $label" }
            $orders["$set-$round"] = $order
            if (@($run.cells | Group-Object target,position | Where-Object Count -ne 1).Count) { throw "Duplicate sample: $label" }
            $labels += $label
            $measurements += $run
            $samples += @(foreach ($cell in $run.cells) {
                $verdict = $cell.valid_status -and [bool]$cell.response.content -and !$cell.response.blocked_reason -and $cell.expected
                if ([bool]$verdict -ne $cell.delivered) { throw "Delivery mismatch: $label/$($cell.target)/$($cell.position)" }
                [pscustomobject]@{arm=$arm;set=$set;round=$round;target=$cell.target;position=$cell.position;delivered=$cell.delivered;seconds=$cell.secs;status=$cell.response.status;chars=$cell.response.chars;outcome=$cell.outcome;blocked_reason=$cell.response.blocked_reason;document_reused=($cell.response.warm.document_reused -eq $true);renewed=($cell.response.warm.reissued -eq $true)}
            })
        }
    }
}
if ($samples.Count -ne 918 -or $labels.Count -ne 27) { throw 'Unexpected experiment size' }
$samples | Export-Csv -LiteralPath (Join-Path $rootPath 'samples.csv') -NoTypeInformation -Encoding UTF8
$previous = $null
$timeline = @(foreach ($run in ($measurements | Sort-Object started_unix)) {
    $gap = if ($previous) { $run.started_unix - $previous.ended_unix } else { $null }
    if ($null -ne $gap -and $gap -lt 0) { throw 'Completed benchmark runs overlapped' }
    [pscustomobject]@{label=$run.label;started_unix=$run.started_unix;ended_unix=$run.ended_unix;seconds_since_previous_complete_run=$gap}
    $previous = $run
})
$timeline | Export-Csv -LiteralPath (Join-Path $rootPath 'timeline.csv') -NoTypeInformation -Encoding UTF8
$partial = 0
Get-ChildItem -LiteralPath (Join-Path $rootPath 'interrupted') -Filter progress.txt -Recurse -File | ForEach-Object {
    $partial += @(Get-Content -LiteralPath $_.FullName | Select-String 'repeat [12]:').Count
}
$evidence = @{complete_runs=$labels.Count;complete_samples=$samples.Count;additional_interrupted_records=$partial;executable_hashes_verified=$true;browser_hash_verified=$true;paired_target_orders_verified=$true;delivery_verdicts_verified=$true;document_reused=@($samples | Where-Object document_reused).Count;renewed=@($samples | Where-Object renewed).Count}
[IO.File]::WriteAllText((Join-Path $rootPath 'audit.json'), ($evidence | ConvertTo-Json), (New-Object Text.UTF8Encoding($false)))
$evidence | ConvertTo-Json
