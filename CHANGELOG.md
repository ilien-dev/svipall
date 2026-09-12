# Changelog

## 1.0.2 — 2026-09-11

- **The headless tiers stop announcing that nothing is holding the mouse.** A headless Chrome
  answers `(pointer: fine)` and `(hover: hover)` with `false` — one media query, the oldest tell
  there is — and `bench tells` had failed `input_modality` on `browser`, `stealth` and the reused
  browser for as long as the probe has existed. The standing answer was the ladder: climb to a
  headful tier, which costs a real window and a compositor that may not cooperate with it. Blink
  takes the answer on the command line, so the headless launches now carry it. A touch identity
  gets the coarse pair instead, because the same probe reads `(any-pointer: coarse)` against
  `maxTouchPoints` in one breath and a fine pointer on a machine declaring a touch screen would
  trade one contradiction for another. Measured with `bench tells`: **155/160 probes clean to
  158/160**, with `input_modality` passing at every tier.

  The two that remain are `window_chrome_height` on `real` and `warm`, and they are this machine
  rather than this code: a tiling compositor ignores `--window-position` and sizes the window
  itself, which is exactly the case `--class=svipall-browser` exists for. `docs/configuration.md`
  has the rule.

- **`a_cancelled_job_stops_fetching` is no longer a race the test usually won.** Its seven routes
  are the whole crawl budget and loopback served all of them in under ten milliseconds — less than
  one turn of the loop waiting to see the job running — so the crawl was regularly finished before
  the cancel could land, and the test failed for having nothing left to stop. It failed twice in
  eight full-suite runs. `Reply::slow(ms)` gives a route a latency; at eighty milliseconds a page
  the cancel arrives with one page done and six to go. Ten consecutive runs clean.

- **The global memory block stops repeating the tool list the MCP server already sends.** It is the
  one piece of svipall that sits in every session of every project on the machine, web or not, and
  half of it was a per-tool routing table copied from the server's own `instructions` — read twice,
  and a second place to forget when a tool changes. It now points at those instructions and at the
  `svipall:svipall` skill, and keeps only what neither of them says: never set a tier by hand,
  never retry a blocked URL blindly, large results to `out_file`, credentials as `${NAME}`, stop
  rather than loop on verification a person has to pass, and `svipall doctor` before blaming a
  site. 2 082 characters to 997.

- **`query` says what it actually saves, which the query decides and the page does not.** It is the
  first thing the instructions offer for cutting tokens, and how much it cuts had never been
  stated. Measured on two long articles: `"robots.txt"` left 4 748 characters of 35 469 and
  `"Craigslist lawsuit"` left 2 017 — 13% and 6% of the page — while `"history of scraping"` left
  31 135, or 88%, because a broad query matches most of a page about scraping. A model writing
  topics instead of facts gets none of the saving and no hint why. The description now carries the
  range and the rule it implies: name the fact, not the topic. That, and the three other measured
  corrections, put the tool list at 38 009 characters against a 38 000 cap; the cap is 38 600 now,
  with what the 650 bought written next to it.

- **Two parameter descriptions promised savings that were not there.** `mobile` said "often half
  the tokens for the same article". Measured on a Wikipedia article and a BBC section, at both the
  http and the browser tier: **byte-identical output**, 35 501 against 35 469 and 3 018 against
  3 018. A responsive site — most of them — serves one document to every viewport, and the flag
  does not rewrite the host. It is also not free: it takes a browser page of its own, because no
  warm page is reused, and it rules out the native last resort. All of that is in the description
  now, and the promise is gone. `out_file` said "about twenty tokens for what could be forty
  thousand"; measured, the response is 418 characters against the 34 746 it wrote to disk, so it
  now says that. Every other documented default was checked against the code and holds: timeout
  60 000, `max_tokens` 25 000, `max_pages` 20, `max_depth` 2, snapshot 200 nodes, scroll 40 rounds.

- **`web_snapshot`'s cost is stated from a measurement instead of a guess.** The skill said "~150
  tokens for a whole page". Measured: 12 tokens on `example.com` and 1 609 on a Wikipedia article,
  where the node list hits its 200-node cap — an order of magnitude out, on a number a model uses
  to decide whether a snapshot is worth taking. It now gives the range, the cap, and what the same
  page's prose costs (8 700) so the comparison the tool is for is the one being made. The snapshot
  response also drops a `final_url` that repeats the `url`, as the fetch path already did: pruning
  a field on one tool and not the other would make its absence mean two things.

- **A link to the page's own site comes back the way the page wrote it, which is 14.7% of the
  delivered text.** The largest thing svipall puts in front of a model is the page itself, and on
  four real pages — Hacker News, MDN's header list, a Wikipedia article, a newspaper front page,
  112 KB of markdown between them — **40% of that was link URLs**. Most of it was one string
  repeated: the page's own HTML said `/wiki/Web_crawler` and the renderer resolved it to
  `https://en.wikipedia.org/wiki/Web_crawler` for display, adding the scheme and host to every
  same-site link on a document whose `url` already says which site it is. Same-site links are now
  written as the path, query and fragment the page used; a link to another host stays absolute,
  because nothing on the page says where another host is. Re-fetched with `--cache refresh`, the
  same four pages: 112 466 → 95 919 characters, −30.4% on Hacker News, −14.2% on MDN, −10.7% on
  Wikipedia. `include_links` still returns every link absolute, for a caller that wants a list it
  can fetch without thinking about where it came from, and `not_a_url` now names the join when a
  path is handed back to `web_fetch`.

  Measured, and one candidate measured away: stripping `utm_*` and the other tracking parameters
  from displayed links looked worth doing on a synthetic fixture and was worth **nothing** on the
  four real pages — zero tracking bytes between them. It was not built.

- **The strict-mode refusal names a tool that exists under either install.** The `WebFetch` hook
  denied the call and pointed at `mcp__svipall__web_fetch`. That prefix is not svipall's to choose:
  registered by hand with `claude mcp add -s user svipall` the tools do arrive under it, but
  installed as the plugin — the path this repository ships and documents — the same server's tools
  arrive as `mcp__plugin_svipall_svipall__web_fetch`. So on a plugin install the refusal named
  something that is not there, at the one moment the agent has nothing else to go on. The tools are
  now named bare, `web_fetch` and `web_search`, which resolves under both.

- **The note on a blocked page names the tool that returns the page, not the one that returns a
  token.** A captcha was reported as `call solve_turnstile(sitekey=…, pageUrl=…)` — with the
  arguments filled in, at the moment of the decision, which beats any instruction given earlier in
  a session. That tool answers with a bare token bound to the session and address that produced it,
  which is the wrong branch whenever the goal is the content. The note now offers
  `solve_and_continue` first and the token tool second, with the condition that makes it right
  attached. It also no longer repeats the page's own address, which on a `raw:` fetch meant
  printing the whole document inside the advice about it, twice. The wording moved into
  `steer.rs`, where the rest of the model-facing messages already live and are tested as prose, and
  a test now checks every tool and parameter any note names against the built tool list.
- **A response no longer spends tokens on fields that say nothing.** `"exit":null`,
  `"native_fallback":false`, `"stopped_reason":null` and a `final_url` repeating the `url`
  character for character were on every page svipall returned, and a response envelope is read once
  per *page* — fifty times in one `web_fetch_many`, hundreds in one crawl. Measured on a page whose
  content was 250 characters: 370 characters of envelope before, 295 after. Nothing is withheld —
  a `null` says exactly what a missing key says, and a flag that is false says what its absence
  says. Absent, each one becomes a signal: `native_fallback` now appears precisely when a native
  attempt was made, and `final_url` when something redirected. `identity_used` stays unconditional,
  because silence is not an acceptable way to tell somebody their real device characteristics were
  not exposed. Held by `crates/svipall/tests/response_surface.rs`, which is to the response what
  `tool_surface.rs` is to the tool list.
- **The MCP instructions no longer end with a sentence about language.** They closed with
  "Instructions in English." — a note about the project's own source, sitting in the system prompt
  for the whole session where it reads as a directive about the *answer*. What language a user is
  answered in was never svipall's call.

- **`/svipall:setup` asks its two consent questions in words a first-time user can answer.** The
  memory-file step showed three lines of markup — `<!-- BEGIN SVIPALL -->`, `@svipall/SVIPALL.md`,
  the end marker — and asked whether to add them. Somebody who has never seen `~/.claude/CLAUDE.md`
  has no way to judge that: not what the file is, not what an import line does, not what stays
  untouched. The step now requires saying all four things before the block is shown, and phrases
  the choices as outcomes rather than yes/no.
- **Strict mode is offered with its failure mode attached, and recommended against on a fresh
  install.** The hook `deny`s `WebFetch` and `WebSearch` with no fallback, so a machine where the
  MCP server is not answering — not on PATH, not installed, failed to start — has no web access at
  all rather than a degraded one. That is the whole risk and it was not being said out loud. It is
  now in the offer and in both option labels, since a dialog's labels are what gets read.

## 1.0.1 — 2026-09-11

- **Every build carries the captcha models now.** Three of the five targets shipped without them:
  the prebuilt ONNX Runtime `ort` downloads references glibc 2.38 and GCC 13's libstdc++, so a
  Linux artefact linking it starts on Ubuntu 24.04 and nothing older, and none is published for
  x86-64 macOS at all. The choice had been a binary that starts everywhere over one that answers an
  image captcha; it is now neither, because those three targets build the runtime from source
  (`tools/onnxruntime/build.sh`) and link that. A runtime compiled on the 22.04 runner needs that
  runner's glibc 2.35, exactly like the rest of the artefact, so Debian 12 and RHEL 9 keep working
  and the models come along. Windows and Apple-silicon macOS keep the prebuilt one, which works
  there. Measured end to end in containers: the artefact starts on Debian 12, `doctor` reports
  `detect` and `segment`, and `crates/svipall/tests/models.rs` runs real ONNX sessions there.
- **The export step reaches aarch64 Linux, which it never had.** It installed torch from
  `download.pytorch.org/whl/cpu`, whose newest aarch64 wheel is 2.0.1 and which has none for Python
  3.12 at all — so that leg had always been built without models. PyPI publishes a
  `manylinux_2_28_aarch64` wheel, and the path was run on an emulated arm64 machine: `torch
  2.14.0+cu130`, both models exported at the same 13.8 MB and 44.1 MB as on x86-64. It is the
  CUDA-tagged wheel, the only aarch64 one published, so the download is large; the export runs on
  the CPU either way. macOS arm64 keeps the CPU index, where its wheel has always been.
- **The runtime version is pinned to 1.27.1, and it is not free to move in either direction.**
  `ort 2.0.0-rc.13` asks for ONNX Runtime API 27. Build 1.22 and everything links, `svipall doctor`
  lists both embedded models, and the first session fails with `The requested API version [27] is
  not available` — a failure no smoke test that only starts the binary can see. Build 1.28 and the
  link itself breaks in a debug profile: it splits `model_package` into a static library of its own
  that `libonnxruntime_session.a` references and ort-sys does not know to link, which a release
  profile hides by garbage-collecting the section. `tools/onnxruntime/VERSION` carries the pin, the
  build script emits linker flags for any such split library so the next bump is not a mystery, the
  release's Linux gate runs `doctor` on Debian 12 and greps for the models rather than only checking
  that the binary starts, and `crates/svipall/tests/models.rs` is what to run against any build made
  this way.
- **The runtime is cached on `main`, not in the release.** Building ONNX Runtime takes the better
  part of an hour per target, and a cache written under a tag is unreadable from the next one — the
  same rule the dependency cache lives by. `cache-warm.yml` builds and caches it, keyed by the
  version file; the release restores that key and builds its own on a miss, so a cold cache costs
  time and never a release.
- **`svipall models install`, for a build that can read a model and has none.** `cargo install
  svipall` compiles the ONNX path in — `local-models` is a default feature — and the published
  crate carries no weights, because crates.io is not where 54 MB of them belong. So it reported
  `no_models` with nothing a user could do about it short of cloning the repository and installing
  torch. The release now publishes one `svipall-models-<version>.zip` for every platform (a weights
  file has no target; it is the runtime that does), and the command downloads it, checks it against
  the `sha256sums.txt` the release already publishes, refuses on a mismatch, and writes the files
  into `~/.svipall/models/`, where they already won over an embedded copy. `--from-file` installs an
  archive you already have and `SVIPALL_RELEASES_URL` points the download at a mirror, so an
  air-gapped machine is not locked out. Nothing downloads by itself: this is a command a person
  types, like `svipall browser install`. `svipall models status` reports `readable`, which is the
  distinction that matters — a binary built without any `onnx-*` feature cannot read a model file,
  and installing weights there would only trade `no_models` for `models_not_readable`. An archive's
  entries are checked before anything is written: only the five model names, only in the archive's
  root, and never an `.onnx` without its sidecar.
- **The headful tiers' window carries its own X11 class.** `real` and `warm` open a real window on
  purpose — a headless browser answers `pointer: fine` with `false` — and it was moved off the edge
  of the screen, which is a request a tiling compositor may ignore. Hyprland and sway do: the window
  lands in the layout, visible, and the page then reads a height the layout chose rather than the
  one the identity states. `svipall-browser` gives a compositor rule something to match that is not
  every Chrome window the user has open; `docs/configuration.md` carries the rule for Hyprland, sway
  and i3. Nothing on the page can read it: `WM_CLASS` never reaches the DOM.
- **`svipall doctor` honours the port variables it tells you about.** `dashboard_port_busy`'s fix
  names `SVIPALL_DASHBOARD_PORT`, and `svipall-mcp` binds what that variable says — but doctor read
  the config file alone, so on any machine with an mcp already listening, setting the variable
  changed nothing in the report and the problem read as permanent. Both ports now resolve through
  one `svipall_core::config::port_from_env`, which is also what the server uses, so the two cannot
  drift apart again.
- **Intel macOS loses its build.** It was the only artefact that could be built and never started:
  `macos-latest` is arm64, so it was cross-compiled, and GitHub has retired the Intel image far
  enough that a `macos-13` job sits queued with no runner rather than failing — measured, not
  assumed. Apple discontinued its last Intel Mac in 2023 and macOS 26 is the last release that
  supports one. **Anyone on that hardware moves to the container image**, which Docker Desktop runs
  natively there; `install.sh` and the npm package decline by name and point at it rather than
  404ing on a download, Homebrew no longer carries a formula for it, and the packaging manifests no
  longer list the archive. Reversing this is one entry in the release matrix.
- **The `slim` image carries the models too, and both images now say so at build time.** `slim` is
  repackaged from the published Linux archive rather than compiled a second time, and that archive
  has them now — the difference between the two flavours is the browser, which is what the tag
  always meant. Its build asserted the opposite, demanding `no_models` from `svipall doctor`, so the
  image build broke the moment the archive gained them; that is how this was found. Both flavours
  now assert `detect`, `segment` and the absence of `no_models`, because an image that quietly lost
  them looks identical from outside.

## 1.0.0 — 2026-09-10

**The first stable release, and the same program as `1.0.0-rc.3`.** Nothing that ships changed
between the two; what changed is the machinery that ships it, which rc.3 was the first to run end
to end — and which failed twice doing it.

- **The first version on the MCP Registry.** rc.3's `crates.io` job asked crates.io for an OIDC
  token before checking whether anything was missing, got `No Trusted Publishing config found`
  with all nine crates already published, and the registry job that waits on it never ran. The job
  now lists what is missing first and asks for a credential only then. It publishes with a
  `CARGO_REGISTRY_TOKEN` repository secret when there is one — the only route that can publish a
  crate crates.io has never seen — and over OIDC when there is not;
  `scripts/crates-trusted-publishing.sh` is the one-time setup for the latter.
- **The first version to move `latest`.** npm's `latest` and the image's `latest` and `slim` follow
  stable releases only, so every candidate before this one left them behind: npm stayed on
  `1.0.0-rc` and the image had neither tag, although the plugin's setup pulls `:latest`. Every
  later pre-release — rc, beta, alpha — goes under its version tag and npm's `next`, and
  `only_a_stable_release_moves_latest` holds that.
- **rc.3's first run built every binary and died in `packages`.** Its artifact download took
  everything in the run, including the `.dockerbuild` records the image jobs upload, which
  `download-artifact` cannot extract. Every download now names what it takes, by test.
- **CI keeps one run per pull request.** A newer push, the merge or closing the pull request cancels
  the run it superseded; pushes to `main` never cancel each other.

## 1.0.0-rc.3 — 2026-09-07

**`1.0.0-rc.2` was tagged and never published.** Its release workflow ran twice and failed both
times: three of the five targets — `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu` and
`aarch64-unknown-linux-gnu` — would not link, so no archive, no package and no image was ever
produced under that version. `v1.0.0-rc` remains the only release anyone can install. The cause was
still in the tree until this tag, which is why this is a third candidate rather than `1.0.0`: what
has never once run green is the machinery, and it now has more of it than rc.2 did.

### The build that could not link, and the test that will not let it happen again

`local-models` joined the top crate's default features in the rc.2 release commit itself. `bench`
took that crate with its defaults, and cargo unifies features across every package the build
selects — and the release job names binaries, not a package, so it selects the workspace. The
`--no-default-features --features impersonate` the three model-free targets are built with was
therefore true of the flag and false of the graph: `dep:ort` came back in through `bench`, and
those targets died on `undefined symbol: __isoc23_strtoll` out of `libort_sys`, plus an
`ort-sys: no prebuilt binaries available for target x86_64-apple-darwin` on the Intel Mac.

`bench` now takes `svipall` with `default-features = false`, and asks for inference by name in
its own `onnx` feature, which is what `bench micro`'s model budgets needed all along.
`crates/svipall/tests/release_build.rs` asserts that no workspace member takes `svipall`
with its defaults, offline and in `qc`, because this failure is invisible on a machine that builds
with the defaults on — which is every developer machine.

### A release is a merge, and the tag is the last thing that happens

`1.0.0-rc.2` is a tag with nothing under it. That shape is now impossible: the release workflow runs
on a push to `main`, reads `[workspace.package] version`, and does nothing at all unless that
version has no tag yet. The tag itself is created by the GitHub release, at the end — so a build
that fails leaves none behind, and the same commit pushed again is still a release waiting to
happen. Pushing a tag by hand still works and is the recovery path.

`[workspace.package] version` is now the only place the number is written by hand. Every member
inherits it, `scripts/sync-version` copies it into the plugin manifest, the npm wrapper and every
internal dependency line, and `crates/svipall/tests/release_version.rs` fails the build when any
of them drifts. `qc --fix` runs the sync beside the plugin-skill copy it already ran.

The Homebrew tap and the Scoop bucket now follow each release by themselves: the `tap-bucket` job
pushes the rendered formula and manifest the moment the release exists, through one deploy key
per repository that can touch nothing else. Until now a person copied them, and both were still on `1.0.0-rc`.

### The workspace is on crates.io

Every crate except the benchmark harness is published, over OIDC and with no stored secret, the
same mechanism the npm job uses. `cargo install svipall` is a supported way in, and — like any
source build — it carries no captcha models: the weights are exported at release time and are not
in the crate, so image challenges go to the human dashboard and `svipall doctor` reports
`no_models`. `svipall-extract` is the one worth depending on alone, under `MIT OR Apache-2.0`.

The two vendored forks, `svipall-cdp` and `svipall-quic`, go up under their own names because
crates.io resolves every dependency of a published crate, optional ones included: `svipall` and
`svipall-http` cannot exist there while either is missing. Publishing is permanent — there is no
unpublish, only `yank` — so the crates go up one at a time, in dependency order, after the release
exists, and a version already on the registry is skipped rather than reported. CI packages all nine
manifests on every run, because a metadata error found on release day is found after the crates
before it are already permanent.

### Routing that learns which way in worked here

The ladder used to try the same tiers in the same order on every site. `core::automatic` now scores
each route by what it delivered on this machine, halving that weight every twelve hours and
expiring it after a day, promotes an emulated winner after two supporting observations, and skips a
route that has refused twice for thirty minutes. It stores a SHA-256 digest of (origin, first path
segment, exit, environment) — never a URL, a query value or proxy credentials — and generates no
exploration traffic of its own.

`browser_identity` defaults to `auto`: emulated routes first, and at most one native attempt,
always last. That fallback is refused outright for named profiles, isolated visits, mobile
requests, non-`GET` methods and forced tiers, and pauses itself when it fails repeatedly. Privacy
stays a constraint rather than a score delivery can outweigh.

`core::traffic` adds a transactional SQLite ledger, so visit admission, pacing and holds survive
across processes and restarts. A reservation is taken before any network work; a refused call does
not extend a cooldown, and a concurrent success cannot shorten a server's `Retry-After`.

Native mode builds a real browser rather than undoing individual JS patches afterwards, so
`--disable-ipc-flooding-protection`, `IsolateOrigins`, `site-per-process` and client-side phishing
detection are no longer switched off where a visitor would have them. `fetch_in_document` refetches
same-origin HTML through the page's own fetch, keeping its SDK and in-memory state alive. The
vendored CDP grows one deviation — `worker_init_script` is carried per browser instead of per
process, which is what lets an emulated pool and a native pool disagree in the same process without
either contradicting its own workers, recorded in `crates/svipall-cdp/PATCHES.md`.

**No public-site delivery rate is claimed for `auto`**, because none was measured for it. The
`local-20260905` figures below describe different policies and must not be read as automatic-mode
results.

### What the local comparison measured, published whole

`bench/experiments/` now carries the raw responses rather than a summary of them.

- **`local-20260905`** — a paired before/after comparison: 27 runs, 918 samples, three arms across
  three rounds, frozen executable and browser hashes, target orders audited. Native mode raises
  `hard12` delivery from 9/12 and 8/12 to **11/12** on both visits, and the repeatable substantive
  recoveries are G2, Idealista and Crunchbase. The record also carries what went the other way: the
  default's Zillow regression, six challenge renewals that recovered nothing, zero live document
  reuses, and a content audit that disqualifies the Home Depot cells its own scoring rule had
  counted as delivered.
- **`auto-20260905`** — the automatic policy verified offline: full QC, 1,177 test executions, 160
  automation probes, eight browser fixtures.
- **`cpu-budgets-20260907`** — the CPU budget table had no log behind its `Measured` column. It now
  has one, and the column names the machine and the date it was taken on, because a CPU timing
  depends on the machine and the table never said so. The budget column is the part that does not
  move, and `bench micro --assert` is what holds it.

Absolute home paths were replaced by `<repo>` and `<home>` before publication, which both protocols
record; no measurement, verdict or hash changed, and the recorded SHA-256 values still verify.

### The extraction corpora, run rather than cited

`bench extract` was run against WCXB, DAnIEL and TECO and the raw output committed, so the figures
`README.md` and `docs/extraction.md` publish have a log somebody else can check. Every figure they
already stated reproduces: WCXB held-out **93.3% recall, 11.3% leak, 0.870 F1** over 505 pages;
WCXB development **0.806** over 1,476; the forum detector at precision **1.000**; DAnIEL over five
languages with **0.608** as its worst; TECO at **P 0.727, R 0.747, F1 0.676**, with cross-page
template removal firing on 2 of 11 armed sites, saving 3.4% and costing one labelled word. TECO's
licence requires published results, so this is also that. The three corpora are fetched on demand
and gitignored — WCXB is 193 MB, DAnIEL 176 MB, and the TECO forum archive unpacks to 13 GB.

**Four claims the runs did not back, corrected rather than left standing:**

- *"third of fourteen on that benchmark's published leaderboard."* The harness scores Svipall; it
  does not rank it against other people's submissions, and nothing here computes a placement.
- *"Turnstile cleared in all recorded runs … 1.5–2.1 s on hard12, 1.7–2.4 s on public31."*
  `bench/baseline/public31.txt` records `nowsecure-cf` clearing on the real tier in 2.7, 1.7 and
  1.7 seconds, and `canadianinsider` staying gated on http in all three runs. No tracked log
  carries `hard12` Turnstile timings at all.
- **The winget and AUR manifests were announced as package manager support** without saying neither
  is submitted, four lines before `docs/install.md` offered `winget uninstall` for a channel nobody
  could have installed from.
- *"Nine local strategies."* Nothing counts nine: the captcha table lists eleven, `WIDGETS` declares
  fifteen widget families and `Modality` has twelve variants, eleven of which reach the live page
  loop. The two counts the source supports are the ones now stated.

### Packaging

- **The Windows archives shipped without four Visual Studio runtime DLLs** an import audit found
  they depended on. Release packaging stages the redistributables beside the binaries and
  `install.ps1` copies their hash-checked manifest, so a loaded-module check proves the MCP process
  uses its own artefact directory rather than whatever the machine happens to have.
- **The installers no longer ask a separate browser-download question.** Provisioning happens when a
  request needs it; `browser_auto_install = false` turns it off.
- **A browser launched with no profile now gets a directory of its own.** Every profile-less browser
  was sent to one shared temporary directory, and Chrome refuses to start on a directory another
  instance holds (`ProcessSingleton`, exit 21). Each now gets a `scratch-*` directory under
  `sessions/` that goes when it does, and shutting a pool down waits for the process to leave, so
  "close, then open again" is a sequence rather than a race.
- **Headful Chrome on a Linux session with no display** exited within two seconds and the attempt
  line said only "launching browser". The pool now refuses before launching, in words, when neither
  `DISPLAY` nor `WAYLAND_DISPLAY` is set; attempt lines carry the whole error chain; and the Linux
  CI job holds an Xvfb display, as the Windows and macOS runners already hold a desktop session.
- `pkgconfiglite` is gone from the Windows dependency step: nothing built there uses pkg-config, and
  it downloads over plain HTTP with no checksum, which Chocolatey refused once already.
- The brand marks and the readme diagrams live under `assets/brand/` and `assets/readme/`. The
  diagrams previously sat outside the repository and the README pointed at nothing.
- **npm publishes over OIDC**, with no stored secret.
- A build-cache defect that shipped silently: `svipall-models/build.rs` read an
  `env!("CARGO_MANIFEST_DIR")` path captured under the workspace's previous name, so `doctor`
  reported inference enabled with no embedded weights. It reads Cargo's runtime environment now,
  and a regression asserts that assets present at build time are actually embedded.

### The README, cut to its first minute

It was 1,863 lines, and four sections were 59% of it: somebody arriving at this repository scrolled
past 487 lines of benchmark tables to find out how to install the thing. Nine sections move into
`docs/` as files of their own — proof, features, captcha, configuration, privacy, limits,
architecture, development, faq — with their links repointed for their new depth, and nothing
deleted. Nineteen same-page anchors pointed at subheadings that had moved and now point into the
file each landed in. What stays is what a reader needs first: what it is, how to install it, what it
can do, how the ladder works, the tool table, the REST routes, how it compares, and the closing
matter. `docs/local-configuration.md` documents the new settings and their presets.

### The gates, run against this tree

Offline, on Windows 11, at the commit this tag names. No network, so these say nothing about
delivery — the evasion sets are not re-taken here, and the figures `v1.0.0-rc.2` published for
them still carry the dates and policies they were measured under.

| gate | result |
|---|---|
| `cargo test --workspace` | **1218 passing**, 22 ignored |
| `bench tells --assert` | **160/160** probes clean, five browser passes |
| `bench fingerprint --engine chrome` | **8/8** identities coherent, 1500 drawn machines |
| `bench micro --assert` | 11 CPU budgets + 4 structural checks, with and without the model features |
| clippy | clean on the default set and on all nine feature configurations |

`bench extract --assert` is not in that list: the corpora are not on this machine. The run that
does back the extraction figures is the one committed under `bench/experiments/` above.

### The tool surface, rewritten for the model that reads it

Claude Code shows a model only the tool *names* and the server's `instructions` when a session
starts, loads a tool's description and schema the moment it is picked, and truncates descriptions
and instructions at 2 KB. Measured on the built `tools/list`, the 29 definitions cost 36 176
characters, about 9 000 tokens, and a third of that was schema furniture no client validates.
Nine descriptions were one line each; three tools had parameters with no description at all;
nothing said when to use `web_act` rather than `browser_open` + `browser_do`, or why a
`solve_turnstile` token is rarely worth having.

- **Every description now opens with what the tool does, says when to use it, names the sibling to
  prefer for the neighbouring case, and ends with what comes back.** The pattern is the one the
  best-regarded MCP servers converge on and Anthropic's own guidance asks for: a screenshot says
  it cannot be clicked and points at `web_snapshot`; each token-returning solver points at
  `solve_and_continue`; `web_status` and `web_log` say which is state and which is history.
- **`instructions` is a task-to-tool map**, about 1 300 characters, since it is the one piece of
  prose visible before a choice is made.
- **Schemas are slimmed on the way out**: `$schema`, `title`, `"default": null`, `nullable`, the
  integer `format` and `minimum: 0` are gone (`slim_schema`). Every parameter has a description; the
  measurement essays that lived in three of them moved to code comments. `web_fetch` went from
  8 116 to 5 864 characters with more said, not less.
- **Annotations**: every tool declares `readOnlyHint` and `openWorldHint`, which is what a client
  uses to decide whether to ask before running it.
- **`actions` on `web_act` and `browser_do` is typed**: `do` is an enum of the fourteen verbs and
  every field says which verb reads it, in place of `items: true` and a paragraph. A second blind
  run, with the definitions loaded, named this the weakest point of the surface. `schema` on
  `web_fetch` declares its two shapes, `"auto"` or an object, instead of no type at all.
- **`web_screenshot` takes `mobile`**, as `web_fetch` already did; both blind runs asked for it.
  **`web_fetch_many` and `web_crawl` take `schema` and `tables`**, so a listing spread over known
  pages, or over pages nobody has enumerated, comes back as rows instead of prose to parse. A
  crawl's `out_file` then holds one row per item, each carrying the `url` of its page.
- **`web_snapshot` leaves nothing on the page.** A reference used to be stamped onto its element as
  a `data-` attribute so a click could find it again: readable by any script on the page, and the
  one thing the project's rule forbids at every tier. A reference is now the walk's own index and
  nothing else; `web_act` and `browser_do` resolve it by running the same walk on the live page
  and asking for that element's `:nth-child` path. A reference past the end of the page says so
  and asks for a new snapshot. A page that changed underneath can still renumber, as it could
  before: a snapshot is of the page as it was.
- **`raw:` markup is no longer echoed back** as `url` and `final_url`, which sent the whole page
  twice on top of the content and wrote it into the request log as an address.
- **A cache hit honours what the call asked for.** The cache holds a page's markdown, and a hit
  answered with it whatever the parameters said: `css_selector` was ignored, `extraction: text`
  came back as markdown, and a stale copy revalidated with a 304 skipped `schema` and `tables`
  too. A call that wants anything but the stored markdown now parses the page again.
- **`web_diff` on a page never seen** answers `changed: null` with `first_seen: true`, not
  `changed: true`. **`web_status`** drops `cache_cleared: null` and shows `soft_line` as 0.7
  rather than an f32 printed through f64.
- `crates/svipall/tests/tool_surface.rs` holds the shape: per-tool and whole-list budgets, no
  boilerplate, every parameter described, every description naming its alternative, no vendor
  names, and every family reachable from `instructions`.

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
- **Package manager manifests**, plus `.deb`, `.rpm` and an npm wrapper — all rendered from the
  release's own `sha256sums.txt` by `scripts/render-packaging.sh`, so no checksum is ever typed
  twice. Homebrew and Scoop are published and installable. The winget and AUR manifests are
  rendered but not submitted, so neither is an install channel yet.
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
