# Install svipall on Windows.
#
#   irm https://raw.githubusercontent.com/ilien-dev/svipall/main/install.ps1 | iex
#
# It downloads a release build, checks it against the published sha256, puts both binaries in a
# directory this user owns, and tells you what it touched. It never needs an administrator, never
# writes outside $Prefix and your own user PATH, and never downloads a browser without asking.
#
# With arguments, run it as a file rather than through `iex`:
#   .\install.ps1 -Version v1.0.0 -Prefix C:\tools\svipall -Yes
#   .\install.ps1 -NoBrowser        # never offer the ~190 MB Chrome for Testing download
#   .\install.ps1 -Uninstall
#
# Windows PowerShell 5.1 and PowerShell 7 both work.

[CmdletBinding()]
param(
    [string]$Version = '',
    [string]$Prefix = "$env:LOCALAPPDATA\Programs\svipall",
    [string]$FromFile = '',
    [switch]$NoPath,
    [switch]$Yes,
    [switch]$Browser,
    [switch]$NoBrowser,
    [switch]$Uninstall
)

$ErrorActionPreference = 'Stop'
$repo = 'ilien-dev/svipall'
# GitHub serves TLS 1.2 only; Windows PowerShell 5.1 does not always default to it, and the
# failure is an unhelpful "could not create SSL/TLS secure channel".
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

function Say($msg) { Write-Host $msg }
function Die($msg) { Write-Host "svipall: $msg" -ForegroundColor Red; exit 1 }

function Confirm-Step($question) {
    if ($Yes) { return $true }
    if (-not [Environment]::UserInteractive) { return $false }
    $answer = Read-Host "$question [y/N]"
    return $answer -match '^(y|yes)$'
}

if ($Uninstall) {
    $removed = $false
    foreach ($b in 'svipall.exe', 'svipall-mcp.exe') {
        $p = Join-Path $Prefix $b
        if (Test-Path -LiteralPath $p) { Remove-Item -LiteralPath $p -Force; Say "removed $p"; $removed = $true }
    }
    if (-not $removed) { Say "nothing to remove in $Prefix" }
    Say ''
    Say 'Left alone on purpose: ~\.svipall (profiles, cache, learned tiers, any browser it'
    Say 'downloaded) and the PATH entry. Remove them by hand if you want to.'
    exit 0
}

# ---- platform --------------------------------------------------------------------------------
$arch = $env:PROCESSOR_ARCHITECTURE
if ($env:PROCESSOR_ARCHITEW6432) { $arch = $env:PROCESSOR_ARCHITEW6432 }
if ($arch -ne 'AMD64') {
    Say "svipall: no release build for Windows $arch."
    Say 'Only Windows x86-64 is published. On arm64, x64 emulation runs it but nothing here'
    Say 'installs that for you; the container image is the supported route:'
    Say "  docker pull ghcr.io/$repo`:latest"
    exit 1
}
$target = 'x86_64-pc-windows-msvc'

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("svipall-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
try {
    # ---- get the archive ---------------------------------------------------------------------
    if ($FromFile) {
        if (-not (Test-Path -LiteralPath $FromFile)) { Die "$FromFile does not exist" }
        $archive = (Resolve-Path -LiteralPath $FromFile).Path
        Say "installing from $archive"
    } else {
        if (-not $Version) {
            Say "looking up the latest release of $repo"
            try {
                $latest = Invoke-RestMethod -UseBasicParsing -Uri "https://api.github.com/repos/$repo/releases/latest"
                $Version = $latest.tag_name
            } catch {
                # /releases/latest only ever names a stable release. Before the first one exists
                # there is nothing there at all, which would leave this script unable to install
                # the project for exactly as long as it is in pre-release. Fall back to the newest
                # release of any kind - and say which one it is, rather than installing a
                # pre-release quietly.
                try {
                    $any = Invoke-RestMethod -UseBasicParsing -Uri "https://api.github.com/repos/$repo/releases?per_page=1"
                    $Version = @($any)[0].tag_name
                } catch { $Version = '' }
                if (-not $Version) { Die "could not read any release; pass -Version vX.Y.Z" }
                Say "no stable release yet; using the pre-release $Version"
            }
        }
        $num = $Version -replace '^v', ''
        $name = "svipall-$num-$target.zip"
        $base = "https://github.com/$repo/releases/download/$Version"
        $archive = Join-Path $tmp $name
        Say "downloading $name"
        try { Invoke-WebRequest -UseBasicParsing -Uri "$base/$name" -OutFile $archive }
        catch { Die "could not download $base/$name" }

        # Verified, or the reason it could not be. A silent skip here is how a corrupted or
        # swapped download becomes a binary somebody runs.
        $sums = Join-Path $tmp 'sha256sums.txt'
        try {
            Invoke-WebRequest -UseBasicParsing -Uri "$base/sha256sums.txt" -OutFile $sums
            $want = (Select-String -LiteralPath $sums -Pattern ([Regex]::Escape($name) + '$') |
                     Select-Object -First 1).Line -split '\s+' | Select-Object -First 1
            $got = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLower()
            if (-not $want) {
                Write-Host "svipall: warning: $name is not listed in sha256sums.txt; not verified" -ForegroundColor Yellow
            } elseif ($want.ToLower() -ne $got) {
                Die "checksum mismatch for $name (expected $want, got $got) - not installing"
            } else {
                Say 'checksum ok'
            }
        } catch {
            Write-Host 'svipall: warning: sha256sums.txt could not be downloaded, so the archive was not verified' -ForegroundColor Yellow
        }
    }

    # ---- install -----------------------------------------------------------------------------
    $unpack = Join-Path $tmp 'unpack'
    New-Item -ItemType Directory -Force -Path $unpack | Out-Null
    # Unblock first: a .zip that came through a browser carries the mark of the web, and every
    # file expanded out of it inherits it.
    try { Unblock-File -LiteralPath $archive -ErrorAction SilentlyContinue } catch {}
    Expand-Archive -LiteralPath $archive -DestinationPath $unpack -Force
    New-Item -ItemType Directory -Force -Path $Prefix | Out-Null
    foreach ($b in 'svipall.exe', 'svipall-mcp.exe') {
        $src = Join-Path $unpack $b
        if (-not (Test-Path -LiteralPath $src)) { Die "$b is missing from the archive" }
        Copy-Item -LiteralPath $src -Destination (Join-Path $Prefix $b) -Force
    }
    Say "installed svipall.exe and svipall-mcp.exe in $Prefix"

    # ---- PATH --------------------------------------------------------------------------------
    $onPath = ($env:PATH -split ';') -contains $Prefix
    if (-not $onPath -and -not $NoPath) {
        # The user's own PATH, never the machine's: this script does not need and does not want
        # an administrator.
        $userPath = [Environment]::GetEnvironmentVariable('PATH', 'User')
        if (-not $userPath) { $userPath = '' }
        if (($userPath -split ';') -contains $Prefix) {
            Say "your user PATH already contains $Prefix"
        } else {
            $new = if ($userPath.TrimEnd(';')) { $userPath.TrimEnd(';') + ';' + $Prefix } else { $Prefix }
            [Environment]::SetEnvironmentVariable('PATH', $new, 'User')
            Say "added $Prefix to your user PATH"
        }
        # So the rest of this script, and this shell, can already see it.
        $env:PATH = "$env:PATH;$Prefix"
    }

    # ---- check -------------------------------------------------------------------------------
    $exe = Join-Path $Prefix 'svipall.exe'
    Say ''
    & $exe --version
    if ($LASTEXITCODE -ne 0) { Die 'the installed binary does not run' }
    Say ''
    $report = & $exe doctor 2>$null
    $report | Write-Host

    if (-not $NoBrowser -and $report -match '"code":\s*"no_browser"') {
        Say ''
        Say 'No browser was found. Without one, only the plain http tier works and any page behind'
        Say 'a challenge stays blocked. Chrome for Testing is about 190 MB.'
        Say '(Microsoft Edge ships with Windows and svipall will use it, so this may already be'
        Say 'answered - check the doctor output above.)'
        # Asked, never assumed. 190 MB is not something to spend on somebody's connection
        # because they passed -Yes to get past a PATH question.
        if ($Browser -or (Confirm-Step 'Download Chrome for Testing now?')) {
            & $exe browser install
        } else {
            Say 'Skipped. Run `svipall browser install` whenever you want it.'
        }
    }

    Say ''
    Say 'Done. Next:'
    if (-not $onPath -and -not $NoPath) {
        Say '  open a new terminal so svipall is on your PATH'
    }
    Say "  claude mcp add svipall -- $Prefix\svipall-mcp.exe   # wire it into Claude Code"
    Say '  svipall fetch https://example.com                    # or just use it from a shell'
    Say ''
    Say 'In Claude Code, the plugin does the wiring for you:'
    Say "  /plugin marketplace add $repo"
    Say '  /plugin install svipall@svipall'
} finally {
    Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
