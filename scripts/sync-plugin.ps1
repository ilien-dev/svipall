# Copy the canonical skill into the Claude Code plugin.
#
# `skill/SKILL.md` is the one that exists: it is what the release tarball ships, and what the
# binary's own conformance test reads. The plugin needs its own copy because a plugin carries its
# skills inside itself, and a test asserts the two are byte-identical. This is what makes them so.
#
# Not a symlink: it would not survive a Windows checkout, and the tarball would ship a link.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$src = Join-Path $root 'skill/SKILL.md'
$dst = Join-Path $root 'plugins/svipall/skills/svipall/SKILL.md'
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $dst) | Out-Null
Copy-Item -LiteralPath $src -Destination $dst -Force
Write-Host "synced $src -> $dst"
