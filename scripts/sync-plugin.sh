#!/usr/bin/env bash
# Copy the canonical skill into the Claude Code plugin.
#
# `skill/SKILL.md` is the one that exists: it is what the release tarball ships, and what the
# binary's own conformance test reads. The plugin needs its own copy because a plugin carries its
# skills inside itself, and a test asserts the two are byte-identical. This is what makes them so.
#
# Not a symlink: it would not survive a Windows checkout, and the tarball would ship a link.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(dirname "$here")"
src="$root/skill/SKILL.md"
dst="$root/plugins/svipall/skills/svipall/SKILL.md"
mkdir -p "$(dirname "$dst")"
cp "$src" "$dst"
echo "synced $src -> $dst"

# The updater is a standalone `$svipall-update` skill in Agent Skills clients, but Claude plugins
# namespace it as `/svipall:update`.  Its body stays canonical; only the frontmatter name and the
# Claude-specific invocation phrase differ.
update_src="$root/skills/svipall-update/SKILL.md"
update_dst="$root/plugins/svipall/skills/update/SKILL.md"
mkdir -p "$(dirname "$update_dst")"
sed \
  -e 's/^name: svipall-update$/name: update/' \
  -e 's/Use when the user asks/Use when the user runs \/svipall:update or asks/' \
  "$update_src" > "$update_dst"
echo "synced $update_src -> $update_dst"
