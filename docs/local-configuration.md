# Local configuration and browser sessions

Configure the application itself; editing TOML is optional:

```text
svipall config show
svipall config preset local
svipall config preset auto
svipall config preset emulated
svipall config preset native
svipall config set warm_max_wait_ms=55000 warm_keep_max=2 parallelism=2
svipall config set browser_auto_install=false
```

Settings are validated before saving to `settings.toml` under the Svipall home directory. These
application-owned settings override `config.toml` without rewriting its comments, and every later
CLI command reads them automatically. Running MCP/REST servers refresh browser policy at request
boundaries and `web_status(configure={"browser_identity":"native"})` (or a POST to `/v1/status`)
saves browser policy directly from the connected tool. Calls already running and sessions opened
explicitly finish with their original policy. Listener addresses, ports and worker counts are
startup settings, but browser/session controls need no manual restart. `api_key` is redacted.

`browser_auto_install` defaults to true. On CLI/MCP startup, if a browser is required and none is
installed, the existing managed-browser provisioner downloads and verifies Chrome for Testing.
An existing local browser is reused, an explicit invalid browser path is an error, and
`max_tier=http` or `browser_auto_install=false` turns off automatic provisioning. This adds no
external automation framework or solving service. Platforms without a managed browser build still
need a compatible system browser, and the application reports that capability limit.
Explicit browser installation/update saves its executable selection for the next request, without
a manual server restart, and removing the managed browser disables automatic reinstallation.

Installers use the same automatic policy without an additional browser-download prompt: their
`--no-browser` / `-NoBrowser` switch persists the opt-out, and `--browser` / `-Browser` installs eagerly.
Windows release archives include app-local compiler runtime DLLs, copied by `install.ps1` and
kept by npm's archive extraction, and model-enabled builds need Windows 10 version 1903 or newer
for the system DirectML component; see [installation requirements](install.md).

## Identity modes

`auto` is the default and `local`/`auto` presets select it: emulated routes are learned first, with
at most one native fallback after those fail. Native is never promoted ahead of an emulated route, and
`auto_native_fallback=false` disables that fallback. Named profiles, isolated/mobile requests,
explicit tiers and non-GET requests never receive automatic native fallback. Explicit modes
saved earlier are kept until changed with `svipall config preset auto`.

`emulated` keeps the emulated identity policy with no native fallback. `native` is an explicit
desktop browser mode: the browser keeps its real version, hardware, WebGL implementation, screen,
locale and timezone, though explicit locale/timezone settings still apply. No synthetic navigator,
canvas, audio or worker identity script is installed in native mode. The HTTP tier keeps
its existing transport emulation: native refers to the browser tiers, not a new HTTP engine.

Choosing native is an experiment, not a promise that a server will accept the request, and public
detector tests and delivery rates must be measured independently. The native browser tests check
real APIs and that document and worker agree, not an assumed list of hardware values.

Native exposes real device characteristics and can link visits even with separate cookies.
Emulation is not an anonymity guarantee and neither mode hides the network exit. Automatic native
profiles are separate from emulated profiles, and no cookies are copied between them. The sandbox
stays enabled and the shared launch flags keep browser security defenses in place. See the
[README privacy and limits section](../README.md#automatic-routing-privacy-and-practical-limits).

## Session continuity and budgets

New profiles warm up on the origin once, including a nonstandard port. Persistent profile seeds
can reuse pages, but isolated and mobile fetches cannot. Only cleared pages with
runtime-bound authorization are held, capped by `warm_keep_max` and `warm_keep_secs`.

A returning request uses the live document's fetch implementation for a same-origin HTML fetch
with credentials and `cache=no-store` and does not return a cached copy or claim a navigation kept
the previous JavaScript context. Redirects, rejected requests, thin application shells and scrolling
requests fall back to ordinary navigation. The result reports `warm.document_reused=true` and
`warm.network_fetch=true` when this path actually delivers the response.

`warm_adaptive=true` gives a recognized proof-of-work path time to reach one renewal, bounded by
`warm_max_wait_ms`, while other challenges begin with `warm_wait_ms` and get one extension only when they
report progress. The caller's total fetch timeout still caps the whole ladder and an explicit refusal
stops early. `warm_adaptive=false` keeps the previous waiting policy for controlled comparisons.

The existing per-domain reputation budget and adaptive throttle still apply: no preset clears
reputation, rotates external addresses or invents trust at the remote server.

Admission also allows `request_limit=12` top-level visits in
`request_window_seconds=60`, with `request_min_interval_ms=1000` and at most
`auto_max_attempts=6` transport attempts per automatic fetch, and all identities and forced tiers share
the persistent `(domain, exit)` ledger. Exceeding a window starts
`request_cooldown_seconds=900` of backoff. HTTP 429/503 ends the fetch and retains the longer of
this backoff and `Retry-After`, which rejected calls do not extend. Browser subresources, redirects,
warmup and page-triggered traffic are not individual accounting entries, and local hosts are exempt.

All these settings are available through CLI and `web_status(configure={...})`, without editing
files or restarting the server. Results expose remaining cooldown seconds and preserve the last
page if later attempts are refused. `clear_cooldown` clears the older hard-block cooldown only,
not the persistent visit ledger. Fresh cache hits stay available without making a request.

Route evidence combines useful full-quality delivery and latency: a route needs two supporting
observations to be promoted and the evidence expires after 24 hours. Repeated failures pause that route for
30 minutes, including native, but the strongest allowed emulated probe is kept. Contexts
include domain, first path segment, exit and identity environment. Persisted keys omit query
strings and hash the context so proxy credentials are not stored in this learning file. This is
local heuristic learning and promises neither complete content nor universally optimal routing.

## Local models

The default desktop build enables the embedded detector, segmentation and grid inference paths
and release builds export the weights at build time and supported desktop artifacts carry them inside the
binary. No Python, model download or external solver is required at runtime. `svipall doctor`
reports the actual build's capabilities. Minimal/unsupported-runtime artifacts explicitly omit
models and cannot claim these capabilities and maintainers can use `--no-default-features` to build
such an artifact. A source build requires model assets at build time, as documented in models.md.

## Comparing variants

`svipall-bench compare --set public31 --repeat 2 --seed 20260906 --label candidate` records every
repeat, configuration, timestamps, browser path, local spend, returned response, delivery verdict
and original public verdict. `scripts/compare-local.ps1` alternates binary order across rounds with
fresh separate state directories and the same identity seed. It preserves complete outputs and
refuses to overwrite incomplete runs, so interrupted work must be inspected before it is resumed.

Neither local state isolation nor a pause resets server-side IP history. Report that limitation,
the full error distribution and unchanged/regressed cells next to any improvement.
