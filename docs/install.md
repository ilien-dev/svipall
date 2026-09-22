# Installing Svipall

This page is written to be executed rather than read. Hand it to any AI coding agent — Claude Code,
Cursor, Codex, opencode, Copilot — and it has everything it needs:

```
Install and configure Svipall by following the instructions here:
https://raw.githubusercontent.com/ilien-dev/svipall/main/docs/install.md
```

A person can follow it too. Every command is exact, and none of them needs an administrator.

---

## 0. Rules for whoever is running this

- **Explain the selected installation channel.** An installation request authorizes its normal setup.
- **Before writing anything, ask which integration the user wants.** Offer these outcomes in these
  words, with the trade-off in the choice itself:

  - **CLI + Skill (recommended)** — lower context use; the agent runs `svipall` through its shell.
    This does not register an MCP server, so Svipall will not appear in the client's MCP list and
    MCP-only interactive sessions are unavailable.
  - **MCP + Skill** — register `svipall-mcp` and install the same skill. The client gets the full
    MCP tool surface and persistent browser interactions, at the cost of a larger tool catalogue.

  Do not silently substitute one for the other.
- **Then ask for scope:** *all projects for this user* or *this project only*. Before making changes,
  show the detected platform, harness, install channel, exact config and skill paths, download sizes,
  and whether any existing Svipall entry will be replaced. One confirmation authorizes that stated
  set of writes; a later conflict or optional download gets its own question.
  Scope applies to the harness integration; the two binaries remain one user-owned installation.
- **Detect the harness from the running agent and its environment**, not merely from commands found
  on PATH. A machine can have several clients installed. Inspect any existing `svipall` MCP entry
  and skill before proposing a change.
- **Never run any of it with `sudo`.** Everything installs into a directory the user owns. A step
  that seems to need root means something went wrong; stop and say so.
- **The managed browser download is about 190 MB.** The tool provisions it automatically when
  needed; `--no-browser` / `-NoBrowser` disables automatic provisioning.
- **If a step fails, stop and report the actual error.** Do not quietly try another channel: the
  user ends up with two installs and no idea which one is on PATH.
- **Never overwrite an unrelated config file.** Prefer the harness's own registration command. If
  JSON or TOML must be edited, preserve every other key and make a timestamped backup first.

---

## 1. Is it already installed?

```bash
svipall --version
```

A JSON object means an installation already exists. Note its current version and executable path,
then check the latest version **before writing the selected plugin, MCP entry or skill**:

```bash
svipall update --check
```

That command exists in 1.0.5 and newer. For an older binary, read the latest stable `tag_name` from
`https://api.github.com/repos/ilien-dev/svipall/releases/latest` and identify the existing channel
from the executable path; do not install a second copy through a guessed channel.

Always show the current version and latest version. If the current version is older, show the exact
channel-specific update command and ask the user to choose:

- **Update the shared installation** — replace the user-owned `svipall` and `svipall-mcp` binaries
  used by all harnesses configured on this machine. Keep `~/.svipall`, including configuration,
  profiles, cookies, cache, models and its managed browser. Running MCP clients must restart.
- **Keep the current version** — make no binary or data changes and continue installing the chosen
  integration with the version already present.

An integration install never implies consent to update the shared binaries. On update, stay with
the detected installation channel; release-script installations on 1.0.5 and newer can run
`svipall update --install` after confirmation. On keep, use the current version when selecting the
matching skill. If the versions are equal, say so and go to [step 4](#4-check-the-installation).
`command not found` means there is no existing binary, so continue to step 2.

---

## 2. Pick a channel

Two ways in, plus a container. The package managers are not published yet; the note under the
table says which and why.

| Situation | Command |
|---|---|
| macOS or Linux | `curl -fsSL https://raw.githubusercontent.com/ilien-dev/svipall/main/install.sh \| sh` |
| Windows | `irm https://raw.githubusercontent.com/ilien-dev/svipall/main/install.ps1 \| iex` |
| Prefers containers | `docker pull ghcr.io/ilien-dev/svipall:latest` |
| macOS or Linux, has Homebrew | `brew install ilien-dev/svipall/svipall` |
| Windows, has Scoop | `scoop bucket add svipall https://github.com/ilien-dev/scoop-svipall` then `scoop install svipall` |
| Node is already there | `npx --yes svipall doctor` — downloads the same release build on first use |
| Rust toolchain is already there | `cargo install svipall` — builds from source, so it can read a model and carries none; `svipall models install` fetches them |

**winget and the AUR are not published yet.** Their manifests exist and are rendered from each
release by `scripts/render-packaging.sh`, but each needs a one-time step outside the repository
that has not been taken: a pull request to `microsoft/winget-pkgs`, an AUR package. Suggesting
either to a user gets them `No package found matching input criteria`, so do not offer them.

The `install.sh` / `install.ps1` scripts put both binaries in `~/.local/bin` (POSIX) or
`%LOCALAPPDATA%\Programs\svipall` (Windows), add that directory to the **user's** PATH, verify the
download against the published `sha256sums.txt`, and print what they touched. `--help` lists the
flags; `--uninstall` reverses it.

Windows archives include the release Visual C++ runtime beside the executables. Keep the DLLs
and `windows-runtime.json` when extracting manually; the installer and npm preserve them. This
uses [app-local deployment](https://learn.microsoft.com/en-us/cpp/windows/choosing-a-deployment-method?view=msvc-170)
and needs no separate runtime installer or administrator. Model-enabled Windows builds require
Windows 10 version 1903 or newer for the operating system's
[DirectML component](https://learn.microsoft.com/en-us/windows/ai/directml/dml-debug-layer).

### Which platforms have builds

| Platform | Binary | Browser tiers | Models | Everything works via |
|---|---|---|---|---|
| Linux x86-64 | yes | yes | yes | the binary |
| Linux arm64 | yes | **no** — point `browser_path` at your own Chromium, or accept the http tier | yes | the container |
| macOS Intel | **no** | — | — | the container (`linux/amd64` runs natively on it) |
| macOS Apple silicon | yes | yes | yes | the binary |
| Windows x86-64 | yes | yes (Edge already counts) | yes | the binary |
| Windows arm64 | no | — | — | the x64 build under emulation, or the container |
| anything else | no | — | — | the container, or build from source |

**Why the Linux builds compile their own ONNX Runtime.** The prebuilt runtimes `ort` downloads
reference glibc 2.38 and GCC 13's libstdc++, so a Linux binary linking them starts on Ubuntu 24.04
and newer and nowhere older — not Debian 12, Ubuntu 22.04, RHEL 9 or Amazon Linux 2023. For a while
that meant those targets shipped without models. They now build the runtime from source instead
(`tools/onnxruntime/build.sh`), which links whatever the build machine has: the artefacts are built
on Ubuntu 22.04, so the floor is its glibc 2.35 and the models come along. Windows and
Apple-silicon macOS keep the prebuilt runtime, which works there.

**Why there is no Intel macOS build.** There was, and it was the one artefact that could be built
and never started: `macos-latest` is arm64, so it was cross-compiled, and GitHub has retired its
Intel image far enough that a job asking for one waits without ever being scheduled. Apple
discontinued its last Intel Mac in 2023 and macOS 26 is the final release supporting one. Publishing
a binary nobody can test, from a build nobody can run, is worse than saying so: `install.sh` and the
npm package decline by name, Homebrew has no formula for it, and the container image runs
`linux/amd64` natively on that hardware.

Each Linux artefact is then started on Debian 12 — older than the machine that built it — and asked
whether its models answer there, because a runtime built against a newer glibc links cleanly and
fails at the first session, and `svipall doctor` lists the embedded models either way.

So every published archive, every package built from one, and both container images carry the two
vision models. A build from source does not unless `tools/models/export.py` ran first, which is what
`svipall models install` is for.

**The container image has everything, on both architectures**, because a container carries its own
glibc and none of the above constrains the host. On arm64 its browser is Debian's own Chromium
rather than Chrome for Testing, which publishes no linux-arm64 build; `svipall doctor` reports it
as `chromium` instead of `managed`, one step down on fingerprint quality and a real browser.

---

## 3. Building from source instead

Only if the user asked for it, or no build exists for their platform. It needs a Rust toolchain
plus `cmake`, `nasm`, `perl` and `llvm` (BoringSSL), and it takes a while.

```bash
git clone https://github.com/ilien-dev/svipall
cd svipall
cargo build --release
```

Three ways a source build differs from a release one:

- On **Windows**, set a short `CARGO_TARGET_DIR` first (e.g. `C:\t`) — BoringSSL's build paths run
  into `MAX_PATH` and the failure is an unhelpful cmake error.
- A plain `cargo build --release` in this repo picks up `target-cpu=native` from
  `.cargo/config.toml`. That binary is for this machine only, and copied elsewhere it can die with
  an illegal instruction. Release artefacts are built with `--profile dist` and an explicit
  baseline.
- A source build carries no captcha models unless `tools/models/export.py` was run first, which
  needs Python, torch and onnx. Without them, image challenges go to the human dashboard instead of
  being answered. `svipall doctor` says which build you have.

`cargo install svipall` is the same source build with the clone done for you, and it inherits
that last point without a way around it: the weights are exported at release time and are not
inside the published crate, so an installation from crates.io answers image challenges through the
human dashboard and reports `no_models`. Every crate of the workspace is published there except
the benchmark harness; `svipall-extract` is the one worth depending on by itself, under
`MIT OR Apache-2.0` rather than the workspace's AGPL.

No BoringSSL toolchain at all? `cargo build --release --no-default-features` drops to reqwest and
loses the browser-grade TLS fingerprint. `svipall doctor` reports it as `no_impersonation`.

---

## 4. Check the installation

```bash
svipall doctor
```

One JSON object. `ok: true` means it is ready. Otherwise every entry in `problems[]` carries a
`message` and a `fix`, both written to be relayed as they are:

| `code` | What it means | What to do |
|---|---|---|
| `no_browser` | Browser tiers need a compatible browser | Normal startup provisions one automatically on supported platforms; `svipall browser install` provisions it immediately |
| `no_models` | Image captchas go to the human dashboard instead of being answered | Expected only from a source build: `svipall models install` fetches them. A published archive reporting this is a bug, not a configuration. [models.md](models.md) |
| `models_not_readable` | The build carries model weights but no `onnx-*` feature to read them, so they answer nothing | Use a release build that carries the models; installing more weights cannot help this one |
| `no_impersonation` | Built without BoringSSL; the http tier is recognisable in the first packet | Use a release build |
| `stale_browser` | The browser announces a Chrome old enough to be a signal | `svipall browser update` |
| `self_defending_browser` | Brave/Vivaldi/Opera contradict the identity every other layer states | `svipall browser install` |
| `dashboard_port_busy` | Usually a `svipall-mcp` already running, which is fine | Nothing, or change `dashboard_port` |
| `home_not_writable` | Nothing is remembered between runs | Fix the directory's permissions, or set `SVIPALL_HOME` |

---

## 5. Install the skill for the selected scope

Both integration choices include the canonical Agent Skill and the small explicit updater skill.
Use the `version` printed by `svipall --version` and install both from the **matching release tag**:
`skill/SKILL.md` and `skills/svipall-update/SKILL.md`. For example, join `v<version>` into
`https://raw.githubusercontent.com/ilien-dev/svipall/v<version>/skill/SKILL.md`. A release archive
already contains `SKILL.md` and `svipall-update/SKILL.md`. For a source build, copy them from the
same source checkout. Never pair a stable binary with the current `main` skills: their commands may
have changed.

Copy the canonical skill to the harness's path for the scope the user chose, creating only its
`svipall` directory:

| Harness | All projects for this user | This project only |
|---|---|---|
| Claude Code | `~/.claude/skills/svipall/SKILL.md` | `.claude/skills/svipall/SKILL.md` |
| Codex | `$HOME/.agents/skills/svipall/SKILL.md` | `.agents/skills/svipall/SKILL.md` at the repository root |
| Cursor | `~/.cursor/skills/svipall/SKILL.md` | `.cursor/skills/svipall/SKILL.md` |
| OpenCode | `~/.config/opencode/skills/svipall/SKILL.md` | `.opencode/skills/svipall/SKILL.md` |

Copy the updater beside it as `svipall-update/SKILL.md` in the same user- or project-level skills
directory. Codex invokes it as `$svipall-update` and also shows enabled skills in its slash-command
list. Cursor exposes the skill directly as `/svipall-update`. For OpenCode, also copy
`integrations/opencode/commands/svipall-update.md` from the matching tag to
`~/.config/opencode/commands/` or `.opencode/commands/` for the selected scope; that supplies
`/svipall-update` and delegates to the skill. The Claude plugin supplies the same workflow as
`/svipall:update`, so do not copy a duplicate updater when using that plugin.

Exception: for user-wide **MCP + Skill** in Claude Code, ask whether to use the recommended plugin
before copying a standalone skill. If the user accepts, continue to the Claude Code plugin steps in
section 6; the plugin supplies its own namespaced skill, so do not install a duplicate here.

If the destination already exists and differs, show that fact and ask before replacing it. After
copying, compare the source and destination hashes. A client that was already open may need a new
session; state which client must refresh rather than saying only "restart".

If the user chose **CLI + Skill**, stop after a successful `svipall fetch https://example.com`.
Do not add an MCP entry. If one already exists, ask whether to keep it or remove it; the chosen CLI
mode does not itself authorize deleting an earlier MCP setup.

If the user chose **MCP + Skill**, continue with step 6.

---

## 6. Register the MCP server

Resolve `svipall-mcp` to an absolute path and use it below. For a container, the command and args are
`docker run -i --rm -v svipall-home:/data ghcr.io/ilien-dev/svipall:latest`; `-i` keeps MCP stdin
open and the volume preserves profiles, cache and learned routes. For npm, use
`npx --yes --package=svipall svipall-mcp`: `--package` is required because `svipall-mcp` is a binary
inside the `svipall` package, not a package of its own.

### Claude Code

For a user-wide MCP setup, recommend the plugin because it already bundles the MCP entry and skill:

```
/plugin marketplace add ilien-dev/svipall
/plugin install svipall@svipall
/svipall:setup
```

The plugin's setup keeps its existing optional memory and strict-mode questions. Do not install the
plugin for **CLI + Skill**, because the plugin registers MCP. For manual registration use:

```bash
claude mcp add --scope user svipall -- /absolute/path/to/svipall-mcp
claude mcp add --scope project svipall -- /absolute/path/to/svipall-mcp
```

Use only the line matching the selected scope. Verify with `claude mcp list`; if the tools are not
available in the current session, restart Claude Code and inspect `/mcp`.

### Codex

User-wide registration uses the CLI, which writes to the active Codex home. Respect `CODEX_HOME`
when it is set instead of assuming that the config is under the ordinary home directory:

```bash
codex mcp add svipall -- /absolute/path/to/svipall-mcp
codex mcp list
```

For project scope, merge this into `.codex/config.toml` in a trusted project:

```toml
[mcp_servers.svipall]
command = "/absolute/path/to/svipall-mcp"
```

Verify the entry with `codex mcp list`, then start a new Codex session and inspect `/mcp`. A config
entry is not proof that an already-running session dynamically gained the tools.

### Cursor

Merge the entry into `~/.cursor/mcp.json` for user scope or `.cursor/mcp.json` for project scope:

```json
{
  "mcpServers": {
    "svipall": {
      "command": "/absolute/path/to/svipall-mcp"
    }
  }
}
```

Preserve every other server and key. Restart Cursor, run `agent mcp list`, then
`agent mcp list-tools svipall`; both the server and its tools must be present.

### OpenCode

Use its configuration-aware command; omit `--global` only for project scope:

```bash
opencode mcp add svipall --global -- /absolute/path/to/svipall-mcp
opencode mcp add svipall -- /absolute/path/to/svipall-mcp
opencode mcp list
```

Use exactly one add command. The list must report Svipall connected. If the installed OpenCode
version does not accept that syntax, inspect `opencode mcp add --help` and show the user the config
it proposes before writing it; do not guess between incompatible config schemas.

### Unknown or unsupported harness

Find its documented user- or project-level MCP and Agent Skills locations. Show the target paths and
this generic STDIO entry, then get confirmation before writing anything:

```json
{
  "mcpServers": {
    "svipall": {
      "command": "/absolute/path/to/svipall-mcp"
    }
  }
}
```

Prefer `.agents/skills/svipall/SKILL.md` only if that harness implements the Agent Skills standard.
If no supported config location or verification command can be established, give the snippet and
manual verification steps and report the integration as unfinished rather than claiming success.

---

## 7. Optional: make Svipall the default way to reach the web

In Claude Code, `/svipall:setup` offers this and does it for you. By hand, add to
`~/.claude/CLAUDE.md` (or your agent's equivalent memory file):

```
<!-- BEGIN SVIPALL -->
@svipall/SVIPALL.md
<!-- END SVIPALL -->
```

and put [the routing rules](https://raw.githubusercontent.com/ilien-dev/svipall/main/plugins/svipall/memory/SVIPALL.md)
at `~/.claude/svipall/SVIPALL.md`. Keep the markers, because they are what makes it removable.

There is also a strict mode, off by default, in which Claude Code's own `WebFetch` and `WebSearch`
are declined with a pointer to the Svipall tool that replaces them. Turn it on by creating an
empty `~/.svipall/claude_strict`; delete the file to turn it off, no restart.

---

## 8. Known failures, and what they actually mean

| Symptom | Cause | Fix |
|---|---|---|
| macOS: *"svipall cannot be opened because the developer cannot be verified"* | The build is signed ad-hoc, not notarised — there is no Apple Developer ID for this project — and a file downloaded through a browser is quarantined | `xattr -d com.apple.quarantine /path/to/svipall`. `install.sh` and Homebrew do not hit this |
| Windows: SmartScreen warns about the installer | Unsigned, for the same reason | Check the sha256 against `sha256sums.txt` on the release page, then allow it |
| Windows source build: cmake fails with `MSB4184` | BoringSSL's paths exceed `MAX_PATH` | Set `CARGO_TARGET_DIR=C:\t` and rebuild |
| `svipall: command not found` right after installing | The PATH change only applies to new shells | Open a new terminal, or use the absolute path the installer printed |
| Every page comes back blocked | No browser, so only the http tier ran | `svipall doctor`, then `svipall browser install` |
| Captchas always go to the dashboard | The build carries no models, or none for that modality | `svipall doctor`. A source build wants `svipall models install`; a published build already has `detect` and `segment`, and text, audio and unknown grid subjects have no published model at all |
| The MCP tools are missing while `svipall doctor` works | The binary is fine, the registration is not | Re-run step 5 with an absolute path, and restart the client |
| `docker run -p 8787:8787` and the dashboard does not load | Loopback inside a container is the container | The entrypoint writes a `/data/config.toml` binding `0.0.0.0` on first start; if you have your own config, set `dashboard_bind` yourself |

---

## 9. Removing it

```bash
# whichever way it went on
sh install.sh --uninstall          # or: install.ps1 -Uninstall
brew uninstall svipall
scoop uninstall svipall
```

Remove only the integration and scope the user selected:

| Harness | Remove MCP | Remove skill |
|---|---|---|
| Claude Code plugin | `/svipall:uninstall` | removed with the plugin |
| Claude Code manual | `claude mcp remove --scope user svipall` or the project equivalent | `~/.claude/skills/{svipall,svipall-update}` or the project equivalents |
| Codex | `codex mcp remove svipall`, or remove only `[mcp_servers.svipall]` from the project config | `$HOME/.agents/skills/{svipall,svipall-update}` or the project equivalents |
| Cursor | remove only `mcpServers.svipall` from the selected `mcp.json` | `~/.cursor/skills/{svipall,svipall-update}` or the project equivalents |
| OpenCode | remove only the selected config's Svipall MCP entry | `~/.config/opencode/skills/{svipall,svipall-update}` and `~/.config/opencode/commands/svipall-update.md`, or the project equivalents |

Restore a timestamped backup if a manual merge damaged a config, but do not replace newer unrelated
changes with an old whole-file backup. CLI + Skill has no MCP entry to remove unless the user chose
to keep a pre-existing one.

That leaves `~/.svipall` alone on purpose: profiles, cookies, cache, learned tiers and the
downloaded browser. Delete it by hand if you mean to, because none of it comes back.

In Claude Code, `/svipall:uninstall` removes the memory block, strict mode and the MCP entry, and
tells you what it did.
