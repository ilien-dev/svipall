# Fill the package-manager manifests in packaging/templates/ from a published release.
#
#   pwsh scripts/render-packaging.ps1 1.0.0-rc
#   pwsh scripts/render-packaging.ps1 1.0.0-rc sha256sums.txt packaging/dist
#
# The PowerShell twin of render-packaging.sh, because every other script in this directory has one
# and the person most likely to be publishing a Scoop bucket is on Windows.
#
# Every manifest names the same five artefacts and repeats their sha256. Writing those by hand,
# five times, on every release, is a job that is wrong the first time somebody is in a hurry, so
# the checksums come from the file the release already publishes rather than from anybody's memory.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$Sums = '',
    [string]$Out = 'packaging/dist'
)

$ErrorActionPreference = 'Stop'
$repo = 'ilien-dev/svipall'
$Version = $Version -replace '^v', ''
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root

$tmp = $null
try {
    if (-not $Sums) {
        $tmp = Join-Path ([IO.Path]::GetTempPath()) ("svipall-sums-" + [Guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Force -Path $tmp | Out-Null
        $Sums = Join-Path $tmp 'sha256sums.txt'
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        Invoke-WebRequest -UseBasicParsing `
            -Uri "https://github.com/$repo/releases/download/v$Version/sha256sums.txt" `
            -OutFile $Sums
    }
    $lines = Get-Content -LiteralPath $Sums

    $targets = @(
        'x86_64-unknown-linux-gnu'
        'aarch64-unknown-linux-gnu'
        'x86_64-apple-darwin'
        'aarch64-apple-darwin'
        'x86_64-pc-windows-msvc'
    )

    # The checksum of a target's archive, or a loud failure. A manifest with an empty hash installs
    # nothing and says nothing useful about why.
    function Get-Sha([string]$target) {
        $ext = if ($target -like '*windows*') { 'zip' } else { 'tar.gz' }
        $name = "svipall-$Version-$target.$ext"
        $line = $lines | Where-Object { $_.TrimEnd().EndsWith($name) } | Select-Object -First 1
        if (-not $line) { throw "no checksum for $name in $Sums" }
        return ($line -split '\s+')[0]
    }

    function Render([string]$src, [string]$dst) {
        # `-Out` may be absolute or relative, and Join-Path on an already-rooted path produces
        # something .NET refuses to write to.
        $full = if ([IO.Path]::IsPathRooted($dst)) { $dst } else { Join-Path $root $dst }
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $full) | Out-Null
        $text = Get-Content -LiteralPath $src -Raw
        foreach ($t in $targets) {
            $key = 'SHA_' + ($t -replace '-', '_').ToUpperInvariant()
            $value = Get-Sha $t
            $text = $text.Replace("@${key}_UPPER@", $value.ToUpperInvariant())
            $text = $text.Replace("@$key@", $value)
        }
        # Arch forbids a hyphen in pkgver; every other manifest wants the real semver string.
        $text = $text.Replace('@PKGVER@', ($Version -replace '-', '_'))
        $text = $text.Replace('@VERSION@', $Version)
        if ($text -match '@[A-Z_]+@') {
            $left = [regex]::Matches($text, '@[A-Z_]+@') | ForEach-Object { $_.Value } | Sort-Object -Unique
            throw "unfilled placeholder left in ${dst}: $($left -join ', ')"
        }
        # UTF-8 without a BOM, and LF: a formula or a PKGBUILD with CRLF is a formula that fails
        # on the machine it is for.
        $text = $text -replace "`r`n", "`n"
        [IO.File]::WriteAllText($full, $text, (New-Object Text.UTF8Encoding $false))
        Write-Host "rendered $dst"
    }

    if (Test-Path -LiteralPath $Out) { Remove-Item -LiteralPath $Out -Recurse -Force }
    Render 'packaging/templates/homebrew.rb' "$Out/homebrew/svipall.rb"
    Render 'packaging/templates/scoop.json' "$Out/scoop/svipall.json"
    Render 'packaging/templates/PKGBUILD'   "$Out/aur/PKGBUILD"
    Get-ChildItem 'packaging/templates/winget' -Filter *.yaml | ForEach-Object {
        Render "packaging/templates/winget/$($_.Name)" "$Out/winget/$($_.Name)"
    }

    Write-Host ''
    Write-Host 'Done. What to do with each is in packaging/README.md.'
} finally {
    if ($tmp) { Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue }
    Pop-Location
}
