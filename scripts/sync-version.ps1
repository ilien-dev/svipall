# Propagate the workspace version to everything else that repeats it.
#
#   scripts/sync-version.ps1              # propagate whatever the root manifest says
#   scripts/sync-version.ps1 1.0.0-rc.4   # set it first, then propagate
#
# See scripts/sync-version.sh for why each file repeats the number and what goes wrong when they
# disagree. Both wrappers call the same script, so neither platform can drift from the other.
[CmdletBinding()]
param([Parameter(Position = 0)][string]$Version = '')

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Split-Path -Parent $here
python (Join-Path $here 'sync_version.py') $root $Version
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
