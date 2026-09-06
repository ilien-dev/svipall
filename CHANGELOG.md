# Changelog

## 1.0.0-rc.2 — 2026-09-06

The first release, and a release candidate on purpose. The code below has been in the tree and
measured for a while. What has never run even once is the machinery that publishes it: the tag job,
the package manifests, the `.deb` and `.rpm`, the build attestation, the multi-architecture image
push. The `-rc` is about that pipeline. If it comes out clean, `1.0.0` is the same tree with a
different tag.

Two things behave differently while this is a pre-release. The container tags `:latest` and `:slim`
are not moved, and GitHub's `/releases/latest` does not name a pre-release, so `install.sh` and
`install.ps1` fall back to the newest release of any kind and say that is what they are installing.

### What it is

A local-first MCP server and CLI, in Rust, that gives an LLM agent a real window onto the web:
**29 MCP tools**, **19 REST routes**, **nine crates** (seven of our own plus two vendored — a
patched Chrome DevTools Protocol client and a patched QUIC/HTTP-3 stack). No cloud, no API keys, no
paid captcha service, no telemetry. Nothing leaves the machine it runs on.

### Installing it

Until this tag the only way in was to build from source: a Rust toolchain, BoringSSL's four build
dependencies, a `MAX_PATH` workaround on Windows, and a `python tools/models/export.py` that no
page listed as a prerequisite. A release workflow existed and had never run, because nothing had
ever been tagged. This is what it produces.

- **`install.sh` and `install.ps1`**, one line each, verifying the published sha256 before they
  unpack anything, writing only into a directory the user owns and that user's own PATH, and
  saying which file they touched. `--uninstall` reverses it. Both are exercised end to end in CI,
  on every platform, against the binaries that job just built.
- **`svipall --version`** and **`svipall doctor`**: what this build is (version, target triple,
  compiled-in features) and whether it will work here — browser, captcha models, http engine,
  ports, home directory — with the exact command that fixes anything that is wrong. The judgement
  is a pure function of collected facts, so it is tested against machines this one is not.
- **A Claude Code plugin** in `plugins/svipall/`, with a marketplace in this repository:
  `/plugin marketplace add ilien-dev/svipall`. It registers the MCP server, ships the skill, and
  adds `/svipall:setup`, `/svipall:doctor` and `/svipall:uninstall`. `setup` installs the binary if
  it is missing, and offers — asking each time, never assuming — to add a routing block to the
  user's global `CLAUDE.md` between removable markers. A test keeps the plugin's copy of
  `SKILL.md` byte-identical to the canonical one.
- **`svipall hook claude-web`**, a `PreToolUse` answer that declines Claude Code's own `WebFetch`
  and `WebSearch` in favour of the svipall tool that does the same job. Registered by the plugin
  from the start and **inert** until `~/.svipall/claude_strict` exists, so installing the plugin
  changes nothing about how anybody's fetches behave.
- **Package manager manifests** for Homebrew, Scoop, winget and the AUR, plus `.deb`, `.rpm` and an
  npm wrapper — all rendered from the release's own `sha256sums.txt` by
  `scripts/render-packaging.sh`, so no checksum is ever typed twice.
- **`docs/install.md`**, written to be executed by an agent rather than read, and
  **`GET-STARTED.md`** for somebody who has never installed anything from a terminal.

Every channel below was installed from the published `v1.0.0-rc` the way a user would, and each one
ends with a real `svipall fetch https://example.com` returning `200 Example Domain`:

| Channel | Where it was run |
|---|---|
| `install.sh` | `debian:bookworm-slim` — real download, checksum, PATH |
| `install.ps1` | Windows 11 — real download, all seven features present |
| `.deb` | `dpkg -i` on Debian 12, installs as `1.0.0~rc` |
| `.rpm` | `rpm -i` on Fedora 41, installs as `1.0.0~rc-1` |
| Homebrew | `brew install ilien-dev/svipall/svipall` in `homebrew/brew` |
| Scoop | `scoop bucket add` + `scoop install` in a sandboxed Scoop root |
| Container | `:1.0.0-rc` reports `ok: true` with the models and Chrome 152; `:1.0.0-rc-slim` is a real amd64 + arm64 manifest |
| Claude Code plugin | `/plugin marketplace add ilien-dev/svipall`, then install: five skills, one hook, one MCP server |

**macOS is not on that list, and is not claimed.** There is no Mac here. The release smoke test runs
the arm64 binary on its own runner; the Intel one is cross-built and cannot be run where it is made.
**npm is unpublished.**

### Getting in

- **A tiered fetch ladder** — `http → browser → stealth → real → warm` — learned per domain and
  remembered between runs, that climbs only as far as a page requires.
- **Chrome- and Firefox-accurate TLS/HTTP2** on BoringSSL: JA4, SETTINGS order, header order,
  GREASE, and the post-quantum key share (`X25519MLKEM768`).
- **An opt-in HTTP/3 engine** (`--features http3`) on a vendored quiche, whose QUIC ClientHello
  carries twelve of Chrome's thirteen extensions, permutes them as Chrome does, and GREASEs a
  transport parameter as Chrome does. Triggered only by `Alt-Svc`, never on a first visit — which
  is Chrome's own rule — with a two-second handshake deadline of its own so a network that silently
  drops UDP costs seconds rather than the page budget.
- **One coherent identity per session** across TLS, headers, CDP, the stealth script and every
  worker realm, checked against itself offline in a gate that fails the build on a contradiction.
- **Human-like input**: Bézier pointer paths that land off-centre, typing cadence by digraph,
  wheel-notch scrolling. Never a bare `click()`.
- **Sessions retired rather than reused** when a site turns on one, and exits keyed by
  `(domain, exit)` with health that heals.
- **A local captcha strategy engine** that orders strategies by what has worked on this route, and
  a human-in-the-loop dashboard that finishes a live challenge from a phone.

### Reading

- Any URL as LLM-ready Markdown; tables as typed rows; docx, xlsx, pptx, odt, epub, rtf, csv and
  pdf as prose.
- `schema: "auto"` induces a schema from a listing's own repeated structure, and a schema's
  selectors are fingerprinted per domain so a redesign relocates them by similarity rather than
  breaking.
- `web_capture` returns the JSON the page fetched while loading — usually the site's real API.
- Hidden text never reaches the model.

### What is measured, and what it says

Every evasion figure is the **median of three runs with its range**, fresh order each run,
cooldowns cleared, from **one residential address with no proxy**. Raw logs are committed under
`bench/baseline/`, including the rounds that improved nothing.

| gate | result | network |
|---|---|---|
| `cargo test --workspace` | **1143 passing**, 16 ignored | no |
| `bench tells --assert` | **160/160** probes clean, five browser passes | no |
| `bench fingerprint --engine chrome` | **8/8** identities coherent | no |
| `bench micro --assert` | 11 CPU budgets + 4 structural checks | no |
| `bench extract --assert` | median F1 **0.920** (floor 0.900), content loss 11.8% (ceiling 15.0%), 3,975 pages | no, corpus on disk |
| `bench evasion --set public31` | **26/31** (25..26), zero hard blocks | yes |
| `bench evasion --set hard12` | **7/12** (range 7..8) | yes |
| `bench evasion --set vendors8` | **3/8** (range 2..3) | yes |

`public31` was re-taken against this tree on 2026-09-05, once the reputation gate allowed it — it
refused for the better part of two hours first, and `--ignore-budget` was not used. It moved from
25/31 to 26/31, which by this project's rule counts as an improvement because the median left the
previous range. The annotation matters more than the number: **the one cell that moved is
`indeed-jobs`**, a Cloudflare managed challenge this benchmark has watched flip in both directions
across four rounds, and what was different was that the address had rested two hours. A number that
moves when the address rests is a number about the address.

`hard12` (2026-09-04) and `vendors8` (2026-09-05) still carry their committed figures and **predate parts of this tree — the HTTP/3 SETTINGS work,
the CDP change and the window-geometry corrections in this release.** They were not re-taken because
running `public31` spends the same addresses they score, and taking all three back to back is the
exact thing that produced a round this project already published as a warning.

### What it does not do, stated plainly

The six `public31` cells and the walls in `vendors8` that do not open are decided by **IP
reputation**, not by fingerprint: the fingerprinting vendor returns `blocked visitor` for this
address with a clean browser, a fresh profile and a rotated machine identity.
Svipall's answer is `web_route` — send the domain through an exit you supply — and that is
the one thing a local-only tool cannot provide for itself. It will never bundle proxies, never call
a captcha farm, and never report a block as a success.

`evasion --exit URL` runs the whole set through an operator-supplied exit, so *"Svipall cannot"*
can be separated from *"this address cannot"*. **No committed baseline has ever used it**; every
one reads `"exit": null`. Until somebody does, that qualifier applies to every number above.

### Closed in this release

- **The container image carried no captcha models.** The `Dockerfile` never ran
  `tools/models/export.py` and never passed the `onnx-*` features, so any image built from it
  answered image challenges by sending them to the human dashboard — while the README said
  "models ship in the release binary". True of the tarballs, false of the image. The `full` image
  now exports and compiles them in, and the release smoke test checks that they arrived.
- **`-p 8787:8787` did not reach the dashboard.** `dashboard_bind` defaults to loopback, which
  inside a container is the container. The entrypoint now writes a `/data/config.toml` binding
  `0.0.0.0` on first start, and never touches one you wrote.
- **The image was `linux/amd64` only** while arm64 tarballs were built. `slim` is now built for
  both. `full` stays amd64, because Chrome for Testing publishes no linux-arm64 build and an arm64
  image with no browser in it is worse than an honest slim one — now stated rather than discovered.
- **The Windows artefact was a `.tar.gz`**, which winget will not accept, Scoop will not accept,
  and Windows Explorer will not open without help. It is a `.zip`.
- **Nothing about a release would have been verifiable.** macOS builds are now signed ad-hoc and every artefact
  carries a GitHub build attestation. Notarisation still needs an Apple Developer ID this project
  does not have, and `docs/install.md` says so rather than implying otherwise.
- **The image build was not reproducible, and `slim` carried 223 MB it could not use.**
  `svipall-models` embeds whatever weights are in its directory, and that directory is gitignored
  but present on any machine that has run the export — so the same `docker build` produced a
  different image depending on whose tree it ran in, and a `slim` image built on a developer's
  machine shipped 58 MB of ONNX with no `onnx-*` feature able to read it. The build context now
  excludes them; `slim` went from 449 MB to 226 MB.
- **`doctor` separates having weights from being able to read them** (`models_not_readable`).
  Listing `models.embedded` on a build with no inference feature reads as a capability and is not.
- Release artefacts are named `svipall-<version>-<target>`, so a manifest can construct the URL.

- **The HTTP/3 SETTINGS frame.** `bench h3-ref` runs a QUIC server on loopback that a real Chrome
  completes a handshake with, so the frame can be read at all. Chrome for Testing 152.0.7977.75,
  four runs, sends `QPACK_MAX_TABLE_CAPACITY=65536`, `MAX_FIELD_SECTION_SIZE=262144`,
  `QPACK_BLOCKED_STREAMS=100`, `H3_DATAGRAM=1` and one fresh GREASE, in that order. We were sending
  upstream quiche's two — one of them a draft codepoint Chrome does not use. Now matched, asserted
  offline. `crates/svipall-quic/PATCHES.md` entry 10.
- **No private key in the repository.** The loopback server that reference needs a certificate for
  generates one in-process and deletes it when the run ends, on the BoringSSL already linked here.
  rustls, hyper and quiche all commit a test key; a key in a public tree is still a finding every
  scanner raises and some push protections block, and a committed certificate expires on somebody
  else's watch. `crates/svipall-quic/PATCHES.md` entry 11.
- **A log that misreported its own severity.** The CDP client raised `error!` for every event from a
  protocol domain newer than its pinned definitions — 55 lines per `tells` run, about a thousand per
  evasion baseline. An event has no `id` and nothing waits on it; a response does, and still errors
  loudly. `crates/svipall-cdp/PATCHES.md` entry 9.
- **The extraction gate, run rather than skipped.** `qc` has carried it for rounds and it printed
  *"skipped: set `SVIPALL_CORPUS`"* on a machine without the corpora. Run against the SIGIR-23 gold
  standard it reproduces the figures `docs/extraction.md` already publishes, exactly — including
  the three published extractors this project is **below** on median and says so.

- **The Linux binary only ran on the distribution that built it.** `ubuntu-latest` is 24.04, so the
  first published artefact wanted `GLIBC_2.39` and would not start on Debian 12, Ubuntu 22.04,
  RHEL 9 or Amazon Linux 2023 — nor would the `.deb` and `.rpm`, which carry the same binaries.
  Linux builds on 22.04 now, and a release step starts the binary inside a `debian:bookworm-slim`
  container before packaging it, because this failure is invisible on the machine that produces it.
- **Linux and macOS Intel gave up the model features to get there.** The ONNX Runtime builds `ort`
  downloads reference glibc 2.38 and GCC 13's libstdc++, so a Linux binary either uses them or
  starts on Debian 12. It starts. The container image keeps the models, because a container carries
  its own glibc, and `svipall doctor` reports `no_models` wherever they are absent.
- **The `.deb` and `.rpm` versions.** RPM rejects a hyphen outright, and dpkg takes `1.0.0-rc` and
  then sorts it *above* `1.0.0`, so somebody on the candidate would never be offered the release.
  Both are `1.0.0~rc`.
- **A browser in a shared `bin` directory read its version off a neighbour.** The arm64 image
  reported Chromium at major 11, because `/usr/bin/X11` is a directory and the sibling-directory
  lookup read the first number out of any name it found. It matters past a wrong field in `doctor`:
  the browser major feeds the identity, and a user agent naming a Chrome the running binary is not
  is the cross-layer contradiction this project spends the most effort avoiding.
- **The container images were built through QEMU.** `slim` was still emulating arm64 after
  thirty-two minutes while the native amd64 half finished in eight. Each architecture now builds on
  a runner of that architecture, and the tags are assembled from the digests.

### Still open, and named so it is not lost

- `trust_anchors` is sent with an empty payload where Chrome sends a populated list.
- Extension `0x12e0` is absent from the linked BoringSSL, which gives the h3 engine a Chrome
  version ceiling of its own, set by the age of that library rather than by a user agent.
- The QUIC Initial's own shape — connection id lengths, padding, version negotiation — is
  unmeasured.
- `MAX_EMULATED_CHROME` is 149, bounded by the newest emulation profile available to the TCP
  engine, while the provisioned browser is 152.

### Licence

AGPL-3.0-only. See `DISCLAIMER.md` for what this tool is and is not for.
