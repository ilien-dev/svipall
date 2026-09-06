param(
  [string]$Root = 'bench/experiments/local-20260905',
  [string]$Browser = "$env:USERPROFILE/.svipall/browser/cft/152.0.7977.75/chrome-win64/chrome.exe",
  [int]$Runs = 3,
  [int]$Repeat = 2,
  [string[]]$Sets = @('public31', 'hard12', 'vendors8'),
  [string[]]$Arms = @('before', 'after', 'native'),
  [int]$PauseSeconds = 120
)
$ErrorActionPreference = 'Stop'
$experimentRoot = [IO.Path]::GetFullPath((Join-Path (Get-Location) $Root))
New-Item -ItemType Directory -Force -Path (Join-Path $experimentRoot 'results'),(Join-Path $experimentRoot 'state') | Out-Null
$utf8 = New-Object System.Text.UTF8Encoding($false)
function Wait-AfterMeasurement($measurement) {
  if ($PauseSeconds -le 0) { return }
  # A resumed controller must honor the last completed run's pause too. The extra second
  # accounts for the integer timestamps in the measurement file.
  $remaining = [long]$measurement.ended_unix + $PauseSeconds + 1 - [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
  if ($remaining -gt 0) { Start-Sleep -Seconds $remaining }
}
foreach ($arm in $Arms) {
  $binary = if ($arm -eq 'before') { 'before.exe' } else { 'after.exe' }
  if (!(Test-Path -LiteralPath (Join-Path $experimentRoot "bin/$binary"))) { throw "Missing $binary" }
}
$manifest = @{
  browser = $Browser
  browser_sha256 = (Get-FileHash -LiteralPath $Browser -Algorithm SHA256).Hash
  runs = $Runs
  repeat = $Repeat
  sets = $Sets
  arms = $Arms
  pause_seconds = $PauseSeconds
  baseline_source = 'b291e45c68e41604546542d753f0808e738b4a48 plus the shared comparison harness'
  before_sha256 = (Get-FileHash -LiteralPath (Join-Path $experimentRoot 'bin/before.exe') -Algorithm SHA256).Hash
  after_sha256 = (Get-FileHash -LiteralPath (Join-Path $experimentRoot 'bin/after.exe') -Algorithm SHA256).Hash
}
[IO.File]::WriteAllText((Join-Path $experimentRoot 'manifest.json'), ($manifest | ConvertTo-Json -Depth 6), $utf8)
for ($run = 1; $run -le $Runs; $run++) {
  foreach ($set in $Sets) {
    # Rotate order: with three arms and three rounds, each occupies each position once.
    $order = @(for ($i = 0; $i -lt $Arms.Count; $i++) { $Arms[($i + $run - 1) % $Arms.Count] })
    foreach ($arm in $order) {
      $label = "$arm-$set-$run"
      $expectedCount = @{public31=31;hard12=12;vendors8=8}[$set] * $Repeat
      $output = Join-Path $experimentRoot "results/$label.json"
      if (Test-Path -LiteralPath $output) {
        $existing = Get-Content -Raw -LiteralPath $output | ConvertFrom-Json
        if ($existing.cells.Count -eq $expectedCount -and $existing.ended_unix -and $existing.label -eq $label) {
          Write-Output "Already measured: $label"
          Wait-AfterMeasurement $existing
          continue
        }
        throw "Incomplete output for $label; inspect the original process before resuming"
      }
      $state = Join-Path $experimentRoot "state/$label"
      if (Test-Path -LiteralPath $state) { throw "State already exists for unrecorded $label; inspect before resuming" }
      New-Item -ItemType Directory -Path $state | Out-Null
      [IO.File]::WriteAllText((Join-Path $state 'machine.seed'), '75bb21a068def901', $utf8)
      $mode = if ($arm -eq 'native') { 'native' } else { 'emulated' }
      $adaptive = if ($arm -eq 'before') { 'false' } else { 'true' }
      $browserToml = $Browser.Replace('\','/')
      $config = "browser_path = '$browserToml'`nbrowser_identity = '$mode'`nbrowser_auto_install = false`nwarm_adaptive = $adaptive`nwarm_max_wait_ms = 55000`n"
      [IO.File]::WriteAllText((Join-Path $state 'config.toml'), $config, $utf8)
      $env:SVIPALL_HOME = $state
      $env:SVIPALL_BROWSER = $Browser
      $binary = if ($arm -eq 'before') { 'before.exe' } else { 'after.exe' }
      $exe = Join-Path $experimentRoot "bin/$binary"
      $seed = 20260905 + $run
      Write-Output "START $label at $([DateTime]::UtcNow.ToString('o'))"
      $proc = Start-Process -FilePath $exe -ArgumentList @('compare','--set',$set,'--repeat',$Repeat,'--seed',$seed,'--label',$label) -WindowStyle Hidden -PassThru -RedirectStandardOutput $output -RedirectStandardError (Join-Path $experimentRoot "results/$label.txt")
      # Keep the native handle open: Windows PowerShell can otherwise lose ExitCode for a
      # process obtained through Start-Process after it exits. Refresh would clear cached state.
      $null = $proc.Handle
      Write-Output "PID $($proc.Id): $label"
      $proc.WaitForExit()
      if ($proc.ExitCode -ne 0) { throw "$label failed with exit $($proc.ExitCode); keep logs for diagnosis" }
      $data = Get-Content -Raw -LiteralPath $output | ConvertFrom-Json
      if (!$data.ended_unix -or $data.cells.Count -ne $expectedCount -or $data.label -ne $label) { throw "$label has no complete measurement" }
      Write-Output "DONE ${label}: $($data.cells.Count) samples"
      # Separate requests within an arm measure returning sessions; separate arms get the same
      # pause. This does not reset the site's IP reputation, and the report must say so.
      Wait-AfterMeasurement $data
    }
  }
}
