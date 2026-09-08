# Frequently asked questions

Lifted out of the README so that file stays readable. Everything here is the same text, with its links repointed.

<details>
<summary><b>Do I need an API key, an account, or a subscription?</b></summary>

No third-party account, API key or subscription is required by Svipall itself. The destination
site may require authorization. Browser provisioning can occur automatically when enabled;
blocklists, a configured DNS resolver, page resources and browser background traffic can also
contact remote servers. See [Privacy and safety](#privacy-and-safety).
</details>

<details>
<summary><b>Which platforms does it run on?</b></summary>

Windows, macOS and Linux. CI runs the shared checks on all three, with the platform-specific steps
listed in [Development](#the-gate), and tagged releases
attach binaries for **Windows x86-64, macOS Intel, macOS Apple silicon, Linux x86-64 and Linux
arm64**, with a `sha256sums.txt` and a build attestation. Install them with a one-line script, with
Homebrew or Scoop, from a `.deb` or `.rpm` where available, through npm, or as a container image on
`ghcr.io` — [docs/install.md](install.md) has the platform details. The winget and AUR manifests
are packaging preparation, not confirmed published installation channels.

The checked release has five binary targets. The current workflow's model matrix is below;
browser operation also depends on an installed compatible browser, OS libraries and a usable
display for headful tiers. A package's existence does not establish full functionality on every host.

| Platform | Published binary target | Browser provisioning | Model-enabled binary job |
|---|---|---|---|
| Windows x86-64 | yes | managed download or detected compatible browser | yes |
| macOS Apple silicon | yes | managed download or installed browser | yes |
| macOS Intel | yes | managed download or installed browser | no |
| Linux x86-64 | yes | managed download or installed browser | no |
| Linux arm64 | yes | operator-installed Chromium via `browser_path` | no |
| Windows arm64 | no native target in the release matrix | x64 emulation was not validated in this audit | no |

The workflow omits ONNX models from Linux and Intel-Mac binaries to accommodate the runtime
distributions it uses. The full Linux container supplies its own libraries and uses Debian's
Chromium on arm64. It is an option for those omitted components, with the headful-runtime
limitations described in [the container section](#or-run-it-in-a-container).

**A binary without models can still attempt non-model strategies.** Token widgets may clear in
the browser, or may demand further challenges. Proof-of-work, slider, rotation, drag and hold
strategies do not require ONNX weights. Model-dependent paths need compatible models or usable
human assistance; neither path guarantees acceptance.
`svipall doctor` reports whichever limitation applies to the machine it is on. On Windows, keep
`CARGO_TARGET_DIR` short when building from source — BoringSSL's paths run into `MAX_PATH`.
</details>

<details>
<summary><b>Can I use it without an AI agent?</b></summary>

Yes, two ways. Completed `svipall` data commands print one JSON object, so `| jq` works;
help is written to stderr and `serve` is a long-running server.
The `svipall serve` command puts nineteen of the twenty-nine MCP tools behind a local REST API that any language can
drive. MCP is one front end of three, not the product.
</details>

<details>
<summary><b>Will it get my IP blocked?</b></summary>

It can, and the tool is built around that being the scarce resource. Top-level attempts are paced
per domain and exit, a persistent visit window limits bursts, full `Retry-After` backoff is kept, a
hard block puts the domain on a 15-minute cooldown, and a reputation ledger tracks what each address
has spent with each host and decays it with a six-hour half-life. Two crawls of the same site cannot
run at once for exactly this reason. None of that makes you invisible — this project's own benchmark
has watched a home address get worse at three targets over a day of runs, and
[published it](#anti-bot-vendors8-four-vendors-named-two-targets-each).
</details>

<details>
<summary><b>Does my data leave my machine?</b></summary>

Svipall stores its cache, crawl state, cookies, profiles and captcha corpus locally under
`~/.svipall` (or `SVIPALL_HOME`). Web requests still reach remote sites, including credentials or
form input you submit, and results go to your connected agent or client. Native fallback can expose
real browser/device characteristics. There is no Svipall telemetry or cloud sync; see
[Privacy and safety](#privacy-and-safety) for downloads, browser traffic and configuration.
</details>

<details>
<summary><b>Will it get past Cloudflare / DataDome / Akamai / PerimeterX?</b></summary>

Sometimes, and the [Proof](#proof-every-number-with-the-command-that-reproduces-it) section says
which historical configurations passed and how often, with the raw logs committed. On `public31`,
Turnstile cleared on the `real` tier at nowsecure-cf in 2.7, 1.7 and 1.7 seconds, and did not clear
at canadianinsider, which stayed gated on `http` in all three runs.
Other outcomes varied across visits. The recorded DataDome browser visits returned a blocked-visitor
interstitial, while bare HTTP on the same address received a different challenge: those observations
do not isolate the cause to the IP address. `web_route` can try an exit you supply, without guaranteeing
acceptance. The [2026-09-06 automatic-policy snapshot](../bench/experiments/automatic-public-20260906/README.md)
reports delivery-check rates and content limitations on `dd8a304`; it predates the browser
directory/shutdown fix in `e60e10b`.
</details>

<details>
<summary><b>Is it a Firecrawl / Crawl4AI / Scrapling / Playwright MCP replacement?</b></summary>

There is overlapping functionality, but this repository has not established a current
head-to-head winner. The [comparison](#how-svipall-compares) describes documented project scope.
Svipall focuses on local operation, content labels, bounded routing and local challenge attempts;
compatibility, completeness and success still need validation on your workload.
</details>

<details>
<summary><b>Is scraping legal?</b></summary>

That depends on the site, the data and where you are, and it is your call rather than this project's.
Svipall grants you **no authorisation with respect to any system you point it at** — read
[`DISCLAIMER.md`](../DISCLAIMER.md) before you run it against something that is not yours. It evades bot
detection on public pages; it does not crack passwords, bypass paywalls or forge authentication.
</details>

<details>
<summary><b>Do I need a GPU?</b></summary>

No GPU is required for the supplied CPU model paths. Availability depends on the build and
installed weights. Browser software rendering can affect fingerprint consistency; `web_status`
reports detected limitations without proving how a site will classify them.
</details>

<details>
<summary><b>Why Rust?</b></summary>

Native executables, low parsing cost in the [measured fixture](#cpu-budgets--measured-not-recalled),
and BoringSSL linked directly for browser-like TLS handshakes. Windows packages carry the required
Visual C++ runtime DLLs beside the executables; no Node or Python runtime is needed to run them.
</details>

<details>
<summary><b>How do I say it?</b></summary>

*SVEE-pahl.* See below.
</details>

---
