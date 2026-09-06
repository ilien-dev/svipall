# App-local release runtime from Visual Studio's redistributable directory.
# https://learn.microsoft.com/en-us/cpp/windows/choosing-a-deployment-method
param(
    [Parameter(Mandatory = $true)][string]$Directory,
    [string]$RedistDirectory = ''
)
$ErrorActionPreference = 'Stop'
if (!$RedistDirectory) {
    $roots = @()
    if ($env:VCToolsRedistDir) { $roots += $env:VCToolsRedistDir }
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    if (Test-Path -LiteralPath $vswhere) {
        foreach ($installation in (& $vswhere -all -products '*' -property installationPath)) {
            $versions = Join-Path $installation 'VC/Redist/MSVC'
            if (Test-Path -LiteralPath $versions) {
                $roots += @(Get-ChildItem -LiteralPath $versions -Directory | Sort-Object Name -Descending | Select-Object -ExpandProperty FullName)
            }
        }
    }
    foreach ($root in $roots) {
        $candidate = Get-ChildItem -Path (Join-Path $root 'x64/Microsoft.VC*.CRT') -Directory -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($candidate) { $RedistDirectory = $candidate.FullName; break }
    }
}
if (!$RedistDirectory) { throw 'Visual C++ x64 release redistributables were not found; supply -RedistDirectory.' }
foreach ($required in 'msvcp140.dll', 'msvcp140_1.dll', 'vcruntime140.dll', 'vcruntime140_1.dll') {
    if (!(Test-Path -LiteralPath (Join-Path $RedistDirectory $required))) { throw "Missing release runtime: $required" }
}
New-Item -ItemType Directory -Force -Path $Directory | Out-Null
$records = @(foreach ($dll in (Get-ChildItem -LiteralPath $RedistDirectory -Filter '*.dll' -File)) {
    Copy-Item -LiteralPath $dll.FullName -Destination $Directory -Force
    @{name=$dll.Name;sha256=(Get-FileHash -LiteralPath $dll.FullName -Algorithm SHA256).Hash;version=$dll.VersionInfo.FileVersion}
})
$manifest = @{schema=1;deployment='app-local';source='Visual Studio x64 release redistributables';documentation='https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files';files=$records}
[IO.File]::WriteAllText((Join-Path ([IO.Path]::GetFullPath($Directory)) 'windows-runtime.json'), ($manifest | ConvertTo-Json -Depth 5), (New-Object Text.UTF8Encoding($false)))
Write-Host "Staged $($records.Count) runtime DLLs in $Directory"
