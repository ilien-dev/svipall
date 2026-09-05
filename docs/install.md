# Installing Svipall

This page is written to be executed rather than read. Hand it to any AI coding agent — Claude Code,
Cursor, Codex, opencode, Windsurf, Copilot — and it has everything it needs:

```
Install and configure Svipall by following the instructions here:
https://raw.githubusercontent.com/ilien-dev/svipall/main/docs/install.md
```

A person can follow it too. Every command is exact, and none of them needs an administrator.

---

## 0. Rules for whoever is running this

- **Show each command before running it, and get a yes.** Nothing here is urgent.
- **Never run any of it with `sudo`.** Everything installs into a directory the user owns. A step
  that seems to need root means something went wrong; stop and say so.
- **The browser download is ~190 MB.** Always ask separately.
- **If a step fails, stop and report the actual error.** Do not quietly try another channel: the
  user ends up with two installs and no idea which one is on PATH.

---

## 1. Is it already installed?

```bash
svipall --version
```

A JSON object means yes — go to [step 4](#4-check-the-installation). `command not found` means no.

---

## 2. Pick a channel

Prefer a package manager the user already has: it is also what upgrades Svipall later.

| Situation | Command |
|---|---|
| macOS or Linux, has Homebrew | `brew install ilien-dev/svipall/svipall` |
| macOS or Linux, anything else | `curl -fsSL https://raw.githubusercontent.com/ilien-dev/svipall/main/install.sh \| sh` |
| Windows, has Scoop | `scoop bucket add svipall https://github.com/ilien-dev/scoop-svipall` then `scoop install svipall` |
| Windows, has winget | `winget install ilien-dev.svipall` |
| Windows, anything else | `irm https://raw.githubusercontent.com/ilien-dev/svipall/main/install.ps1 \| iex` |
| Arch Linux | `yay -S svipall-bin` |
| Debian / Ubuntu | download `svipall_<version>_amd64.deb` from the release, then `sudo dpkg -i` it |
| Fedora / RHEL | download `svipall-<version>.x86_64.rpm` from the release, then `sudo rpm -i` it |
| Prefers containers | `docker pull ghcr.io/ilien-dev/svipall:latest` |
| Node is already there | `npx --yes svipall@latest --version` (downloads the same release build) |

The `install.sh` / `install.ps1` scripts put both binaries in `~/.local/bin` (POSIX) or
`%LOCALAPPDATA%\Programs\svipall` (Windows), add that directory to the **user's** PATH, verify the
download against the published `sha256sums.txt`, and print what they touched. `--help` lists the
flags; `--uninstall` reverses it.

### Which platforms have builds

| Platform | Build | Browser tiers |
|---|---|---|
| Linux x86-64 | yes | yes |
| Linux arm64 | yes | **no** — Chrome for Testing publishes no linux-arm64 build. Point `browser_path` at your own Chromium, or accept the http tier |
| macOS Intel | yes | yes |
| macOS Apple silicon | yes | yes |
| Windows x86-64 | yes | yes (Edge already counts) |
| Windows arm64 | no | run the x64 build under emulation, or use the container |
| anything else | no | build from source, or use the container |

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
| `no_browser` | Only the plain http tier works; any page behind a challenge stays blocked | Offer `svipall browser install` (~190 MB), or install Chrome/Edge |
| `no_models` | Image captchas go to the human dashboard instead of being answered | Use a release build, or see [models.md](models.md) |
| `models_not_readable` | The build carries model weights but no `onnx-*` feature to read them, so they answer nothing | Use a release build |
| `no_impersonation` | Built without BoringSSL; the http tier is recognisable in the first packet | Use a release build |
| `stale_browser` | The browser announces a Chrome old enough to be a signal | `svipall browser update` |
| `self_defending_browser` | Brave/Vivaldi/Opera contradict the identity every other layer states | `svipall browser install` |
| `dashboard_port_busy` | Usually a `svipall-mcp` already running, which is fine | Nothing, or change `dashboard_port` |
| `home_not_writable` | Nothing is remembered between runs | Fix the directory's permissions, or set `SVIPALL_HOME` |

---

## 5. Wire it into the agent

### Claude Code — the plugin does all of this

```
/plugin marketplace add ilien-dev/svipall
/plugin install svipall@svipall
/svipall:setup
```

The plugin registers the MCP server and ships the skills. `/svipall:setup` also offers to make
Svipall the way this user reaches the web everywhere, and `/svipall:uninstall` reverses it.

### Claude Code — by hand

```bash
claude mcp add svipall -- svipall-mcp
```

If `svipall-mcp` is not on the PATH of whatever launched Claude Code, use the absolute path:
`claude mcp add -s user svipall -- /absolute/path/to/svipall-mcp`.

### Claude Desktop, Cursor, Windsurf, and any other MCP client

Add to the client's MCP config (`claude_desktop_config.json`, `.cursor/mcp.json`, …):

```json
{
  "mcpServers": {
    "svipall": {
      "command": "svipall-mcp"
    }
  }
}
```

Use an absolute path for `command` if the client does not inherit your shell's PATH — GUI apps on
macOS usually do not.

### Codex, opencode, and agents that prefer a shell

Point them at the CLI instead. It is the same server for a fraction of the tokens: copy `SKILL.md`
from the release archive (or [`skill/SKILL.md`](../skill/SKILL.md)) into wherever that
agent keeps its skills — `~/.codex/skills/svipall/SKILL.md`, `.opencode/skills/`, and so on. The
whole surface is `svipall <command>`; `svipall --help` lists it.

### A container instead of a binary

```bash
claude mcp add svipall -- docker run -i --rm -v svipall-home:/data ghcr.io/ilien-dev/svipall:latest
```

`-i` keeps stdin open for MCP, and `-v svipall-home:/data` is what makes it remember anything.
Two tags: `latest` (browser and models, linux/amd64) and `slim` (http tier only, amd64 and arm64).

---

## 6. Optional: make Svipall the default way to reach the web

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
are declined with a pointer to the Svipall tool that does the same job. Turn it on by creating an
empty `~/.svipall/claude_strict`; delete the file to turn it off, no restart.

---

## 7. Known failures, and what they actually mean

| Symptom | Cause | Fix |
|---|---|---|
| macOS: *"svipall cannot be opened because the developer cannot be verified"* | The build is signed ad-hoc, not notarised — there is no Apple Developer ID for this project — and a file downloaded through a browser is quarantined | `xattr -d com.apple.quarantine /path/to/svipall`. `install.sh` and Homebrew do not hit this |
| Windows: SmartScreen warns about the installer | Unsigned, for the same reason | Check the sha256 against `sha256sums.txt` on the release page, then allow it |
| Windows source build: cmake fails with `MSB4184` | BoringSSL's paths exceed `MAX_PATH` | Set `CARGO_TARGET_DIR=C:\t` and rebuild |
| `svipall: command not found` right after installing | The PATH change only applies to new shells | Open a new terminal, or use the absolute path the installer printed |
| Every page comes back blocked | No browser, so only the http tier ran | `svipall doctor`, then `svipall browser install` |
| Captchas always go to the dashboard | The build carries no models | `svipall doctor`; use a release build |
| The MCP tools are missing while `svipall doctor` works | The binary is fine, the registration is not | Re-run step 5 with an absolute path, and restart the client |
| `docker run -p 8787:8787` and the dashboard does not load | Loopback inside a container is the container | The entrypoint writes a `/data/config.toml` binding `0.0.0.0` on first start; if you have your own config, set `dashboard_bind` yourself |

---

## 8. Removing it

```bash
# whichever way it went on
sh install.sh --uninstall          # or: install.ps1 -Uninstall
brew uninstall svipall
scoop uninstall svipall
winget uninstall ilien-dev.svipall
```

That leaves `~/.svipall` alone on purpose: profiles, cookies, cache, learned tiers and the
downloaded browser. Delete it by hand if you mean to, because none of it comes back.

In Claude Code, `/svipall:uninstall` removes the memory block, strict mode and the MCP entry, and
tells you what it did.
