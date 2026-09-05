# The Firefox engine: what ships today, and what the fork adds

svipall has two browser identities. One of them runs today; the other needs a build this repository
describes but does not carry.

| | Engine | Status | What it gets you |
|---|---|---|---|
| **http tier** | Gecko | **ships now** (`http_firefox = true`) | TLS, HTTP/2, headers and User-Agent that are Firefox *together*. No browser, no patches, no build. |
| **browser tiers** | Chrome | ships now | The CDP stack, the stealth script, the whole existing ladder. |
| **browser tiers** | patched Gecko | **not built** — see below | Fingerprints spoofed in C++ instead of JavaScript. |

## What ships now: a coherent Firefox on the http tier

Set `http_firefox = true` in `~/.svipall/config.toml`. The http tier then presents
`IdentityProfile::firefox(...)`:

- **TLS/HTTP2** — `wreq-util`'s Firefox emulation profile, selected by `profile_of` from the
  identity's engine. Never a Chrome profile under a Firefox UA.
- **Headers** — Firefox's real navigation set and order: `User-Agent` first, Firefox's own `accept`
  (no `application/signed-exchange`), and **no `Sec-CH-UA*` at all**. Firefox sends no client hints;
  emitting one is the single loudest way to be caught pretending.
- **User-Agent** — `Mozilla/5.0 (…; rv:151.0) Gecko/20100101 Firefox/151.0`, with the macOS token
  spelled `10.15` the way Gecko spells it, not Chrome's frozen `10_15_7`.

The browser tiers stay Chrome, because their protocol is Chrome's. A domain the http tier can serve
therefore gets a Firefox that is Firefox all the way down; a domain that escalates to a browser gets
Chrome. That split is deliberate and is the honest limit of what runs without a patched build.

`svipall status` reports which engine the http tier is wearing.

## What the fork would add, and what it costs

Everything below is designed and unimplemented. It is written down so the decision is made on real
numbers rather than on enthusiasm.

### Why patch the engine at all

`stealth_js` is good, and it is still JavaScript. A page can ask
`Object.getOwnPropertyDescriptor` what a property really is, stringify a function and look for
`[native code]`, or compare the window realm against a worker realm. Every one of those has a
workaround and every workaround has a next question. Patching the C++ getter ends the conversation:
there is no wrapper to find because the value is native.

Camoufox proved the approach. Its own documentation also names where it fails, which is the part
worth learning from: **not the technique, the coherence.** "Anti-bot providers test Camoufox over
and over to find even 1 unique inconsistency, then immediately update their scripts to test for it."
svipall already answers that — see *the linter*, below.

And the argument runs the other way too, which the enthusiastic version of this section omits: **a
public patched build has a static signature of its own.** The patch set is public, so the font
list it bundles, the pref set it ships, the version cadence it rebases on and the exact values its
native getters return are all enumerable by anyone who downloads it — which is what that quoted
sentence describes happening to Camoufox, continuously. A stock Chrome for Testing binary with
nothing named on `window` is a lower-profile target than a bespoke Gecko, not a higher one. The
case for the fork is that it removes a *category* of question; it is not that it removes attention.

### Control protocol: WebDriver BiDi, not Juggler

Firefox removed CDP entirely in 141. Camoufox drives Juggler, which is Playwright's patch, so it
carries Playwright's whole patch stack and rebases it on every Firefox release. **BiDi is upstream
and Mozilla-maintained**, so a fork that speaks BiDi rebases only its own fingerprint patches. That
is strictly less maintenance for the same control, and it is the main design difference from
Camoufox.

- `script.addPreloadScript` replaces `Page.addScriptToEvaluateOnNewDocument`.
- There is no equivalent of `Emulation.setUserAgentOverride` or `setHardwareConcurrencyOverride`:
  those become prefs or patches, which is precisely the argument for building at all.
- BiDi turns on Marionette, which sets `navigator.webdriver`. The first answer is a pref —
  `dom.webdriver.enabled` in `user.js`, below — so `Navigator::Webdriver` is the fallback for the
  case where Marionette overrides it at runtime, not a prerequisite. Whichever wins, the property
  must end up **present and `false`**: removing it is a state no real browser produces, which is a
  lesson this tree learned on the Chrome side and paid for (`bench tells`, `navigator_webdriver`).
- BiDi is new enough that it needs its own entries in the fingerprint bench; assume nothing.

### The patch set

`patches/`, applied to mozilla-central. Transport copies Camoufox's design because the design is
correct and public: the fingerprint JSON is chunked across `SVIPALL_FP_1..N` environment variables,
reassembled by a C++ singleton at startup, and read by the native getters. The configuration never
enters the JavaScript realm, so there is nothing in the page for a script to find.

Injection points: `nsGlobalWindowInner` (navigator and window surfaces), `nsScreen`,
`ClientWebGLContext`, `gfxPlatformFontList` (the font whitelist), `Navigator::Webdriver` → `false`,
headless/headful parity, WebRTC.

### Three things this fork does that Camoufox does not

**A. The GPU tells the truth.** Spoofing the WebGL renderer string is a losing game: it is checked
against `MAX_TEXTURE_SIZE`, the shader-precision triple, the extension list, and above all the
**hash of a rendered image**, which no spoof reproduces without owning that GPU. Camoufox spoofs the
string because its users run in containers. svipall runs on the operator's own desktop, so the
correct move is the opposite one — **report the real GPU and choose the rest of the identity around
it**. `fleet.rs` gains a host-GPU mode that reads the true renderer once and draws a coherent
screen, core count and memory to sit beside it. The patch needed is the *inverse* of everyone
else's: it disables Firefox's default `Mozilla`/`Mozilla` masking.

The exception is a host with no usable GPU, where the renderer is `SwiftShader` or `llvmpipe` — the
signature of a server. `coherence::is_software_renderer` detects that, the host-GPU mode is not
entered, and `web_status` says so. That check ships today.

**B. Hardware entropy stays real.** Kasada's fourth layer measures the timing variance of JavaScript
execution; uniform server hardware is the tell. Running on the operator's machine is structurally
right here, so the rule is: **never uniform the timers**, and keep a bench check that fails if
variance leaves the real-hardware band.

**C. The coherence linter — and this one ships today.** `svipall-core::coherence` checks an identity
against itself: engine ↔ user agent, client hints ↔ engine, screen ↔ availHeight ↔ viewport, form
factor ↔ platform, timezone ↔ language, renderer ↔ engine, and the macOS OS-token spelling that
differs between the two engines. `cargo run -p svipall-bench --release -- fingerprint --engine chrome`
runs the whole set offline and **fails the build on a contradiction**; it is part of `qc` and CI.
This is the thing Camoufox says it keeps getting wrong, expressed as a test suite, which is the
culture this repository already has.

### The build, stated plainly

`mach` over Windows, macOS and Linux. Mozilla's artifact builds do **not** apply, because the whole
point is changed C++. The recurring cost is real: about 3 GB of source, one to three hours per
platform, and a rebase every four weeks when Firefox ships. Output is consumed by
`browser_setup --engine firefox`, reusing the resumable, hash-verified download path
`provision.rs` already implements for Chrome for Testing.

### The rest of the harness

Design unless marked otherwise; `Brand::of` is the one item below that describes code in the tree.

- A Firefox GPU table in `fleet.rs` beside the ANGLE strings.
- Per-OS fonts bundled and selected by the declared OS (Camoufox does this; it is necessary).
- Launcher: `-profile` instead of `--user-data-dir`, `-headless`, and prefs in `user.js`
  (`media.peerconnection.ice.*`, `network.trr.mode`, `network.proxy.*`, `dom.webdriver.enabled`)
  instead of Chromium switches.
- `PROFILE_CACHE_DIRS` gains `cache2/` and `startupCache/`.
- **Exists today:** `Brand::of` classifies Firefox as `Chromium` through its `else` arm — the `Brand`
  enum has no Firefox variant. Latent rather than live, since `BrowserPool` only ever discovers
  Chromium binaries, but it is the operator-facing gap `README.md` describes when a hardened Firefox
  is the host default browser.
- The JavaScript stealth surface is mostly **removal**: no `window.chrome`, no `deviceMemory`, no
  `navigator.connection`, no `performance.memory`, no Chrome plugin list. Reporting any of them on a
  Gecko engine *is* the tell. The canvas, audio and text-geometry noise and the screen geometry
  transfer unchanged.

### The honest limit

A Firefox pretending to be Chrome does not work and will not: SpiderMonkey's observable behaviour —
error formats, timing, engine quirks — cannot be made to look like V8 by editing native getters,
because that is not where the difference lives. Camoufox states this too. A patched Firefox is a
very good Firefox, and against a detector that fingerprints the engine itself, that is exactly what
it is.

## The spike, and why it was not built (2026-09-04)

The case for a Gecko browser tier rested on one measured claim: that `google.com/search` is a cell
Firefox passes and Chromium does not. An independent benchmark published exactly that in May 2026 —
Camoufox passed `google-search` where three Chromium-based tools were hard-blocked.

**That premise no longer holds here.** `google-search` passes 3/3 in svipall's own `public31` runs.
It is won at the `stealth` tier, not for free: the tier histogram for that round is
`browser 21, http 34, real 2, stealth 3, warm 14`, and all three `stealth` cells are this one
target — it is the single most-escalated passing cell on the list. So the premise is dead but not
comfortably: the claim is "Chrome reaches it", not "Chrome reaches it cheaply". The spike began
where a bounded investigation should, with whether the thing it was meant to win is still lost.
It is not.

Working through the rest of the list for a cell a second engine could take:

| Cell | Chrome, here | What a Gecko tier would add |
|---|---|---|
| `google-search` | passes 3/3, at `stealth` | nothing — already won, though only by escalating |
| `bot-incolumitas`, `browserscan-bot`, `sedarplus` | gated | nothing — gated for **all seven** tools in the public benchmark, Camoufox included |
| `medium`, `canadianinsider` | gated by the rule, page delivered | nothing — these are not walls (see `docs/bench.md`) |
| `indeed-jobs` | gated at `warm` | unknown, and the only candidate |

`dev.to` used to carry a row here, priced as a cell the fork would lose: Camoufox is the one tool
the public matrix records as *blocked* there, on a Firefox TLS shape the CDN flags. **That row was a
category error and it is gone.** `devto` resolves at the **http** tier in about 100 ms
(`bench/src/targets.rs`, wall class `content`) and never escalates, so no browser tier — Chrome or
Gecko — is ever constructed for it. A browser-tier fork cannot lose a cell it never touches.

The risk was real; it just belonged to something else. The Firefox TLS shape that could flip `devto`
is `http_firefox = true`, which **ships today** and moves the tier that serves 34 of the 93 cells —
so it was argued rather than measured, which was the actual gap.

**It is measured now, and it costs nothing here.** One run per arm, same address, same afternoon:
`public31` came back **25/31 both ways**, with the same six cells failing, and `devto` passed on the
first rung in both. That is a smoke test rather than a median — one run, one address, on a list
where twenty-five of thirty-one pass for every tool ever measured on it — so it says "no difference
visible", not "no difference". The runs are `bench/baseline/public31-firefox.{json,txt}` and the
reading is in `bench/baseline/README.md`.

One uncertain cell to win, none known to lose, in exchange for a second protocol client
(Marionette or WebDriver BiDi — CDP does not apply), a second stealth surface, a second browser to
provision, and every future change made twice. The `BrowserEngine` trait the spike would have
extracted is a small change (21 references to `svipall_cdp` outside that crate, six of them `use`
lines a trait would remove wholesale), but an abstraction with one implementation and no second one
coming is scaffolding around an empty lot.

So it is not built, and this is the record of why rather than a silence.

### What would reopen it

Any one of these, measured rather than argued:

* A `public31` or `hard12` cell that Chrome fails three runs running **and** a manual Firefox
  passes by hand from the same address.
* `google-search` regressing on Chrome and a manual Firefox still passing it.
* A vendor shipping a V8-specific check — engine internals rather than fingerprint values — that
  shows up in `bench tells` or in a `blocked_reason`.

Until one of those is on the table, the Firefox work that pays for itself is the one already
shipping: `http_firefox = true`, a coherent Gecko on the http tier, checked by the same offline
coherence pass as every other identity.
