---
name: update
description: Check the installed Svipall version against the latest stable release and, only after the user chooses, update the shared user-owned binaries through their existing installation channel. Use when the user runs /svipall:update or asks to check for, install, or manage Svipall updates.
---

# Update Svipall

Checking is read-only. Updating replaces the user-owned `svipall` and `svipall-mcp` binaries used
by all harnesses configured on this machine; it does not erase `~/.svipall`, profiles, cookies,
configuration, cache, models, or the managed browser.

## 1. Compare versions

Run:

```bash
svipall update --check
```

Read `current`, `latest`, `update_available`, `channel`, `install_command`, and `note`. If this is an
older binary that does not know `update`, run `svipall --version` and read the latest stable
`tag_name` from `https://api.github.com/repos/ilien-dev/svipall/releases/latest` instead. A failed
check is not permission to install anything.

If no binary exists, offer the ordinary setup flow rather than calling an update an installation.
If it is current, state both the current version and latest version and stop.

## 2. Let the user choose

When `update_available` is true, show the current version, latest version, detected channel, exact
command and these two outcomes:

- **Update the shared installation** — replace `svipall` and `svipall-mcp` for all harnesses on
  this user account; keep all Svipall data and configuration. Harnesses with a running MCP process
  must be restarted afterwards.
- **Keep the current version** — make no binary or data changes. An integration being installed may
  continue using this version.

Wait for the choice. Never interpret a request to install a plugin, MCP entry, or skill as consent
to update the shared binaries.

## 3. Update only after confirmation

For release-script installations, run:

```bash
svipall update --install
```

On Windows, the running executable cannot replace itself. The command therefore makes no changes
and returns an exact PowerShell `install_command`; ask the user to close every harness running
`svipall-mcp`, then run that command in PowerShell. Do not report the update complete until a new
`svipall --version` confirms it.

For another channel, `--install` deliberately returns without mixing installation methods. Run the
reported `install_command` only after the same confirmation. If the channel is `unknown`, stop and
identify who owns the executable; do not create a second copy on `PATH`.

An old binary without `update --install` must be updated through its existing channel. For a
release-script install, fetch the installer from the `latest` version tag, pass that exact version
and the existing prefix, and use its non-interactive flag because consent was already obtained.
Package-manager, Cargo, npm and container installations stay with their own manager.

Afterwards run `svipall --version` and `svipall doctor`. Report the version actually running and
name every harness that must restart. A Claude marketplace owns the plugin files separately; its
normal auto-update or a named reinstall refreshes those files, while this workflow owns the shared
binaries.
