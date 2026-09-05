<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="logo/svipall-lockup-dark.svg">
    <img src="logo/svipall-lockup.svg" alt="svipall — local-first web scraping and browsing MCP server for AI agents" width="440">
  </picture>
</p>

<h3 align="center">A different face at every gate.</h3>

<p align="center">
  <b>The whole web, readable by your AI agent — on your own machine.</b><br>
  An MCP server and CLI, in Rust, that turns any page into LLM-ready Markdown,<br>
  crawls whole sites, searches without an API key, and answers the challenges it can —<br>
  then tells you plainly about the ones it cannot.
</p>

<p align="center">
  <a href="https://www.rust-lang.org"><img alt="Rust" src="https://img.shields.io/badge/Rust-stable-A7472C?style=flat-square&labelColor=0B1A2B"></a>
  <a href="#license"><img alt="License: AGPL-3.0" src="https://img.shields.io/badge/License-AGPL--3.0-EAD9C4?style=flat-square&labelColor=0B1A2B"></a>
  <a href="#mcp-tools"><img alt="MCP" src="https://img.shields.io/badge/MCP-29%20tools-DF8D27?style=flat-square&labelColor=0B1A2B"></a>
  <a href="#development"><img alt="Tests" src="https://img.shields.io/badge/tests-1143%20passing-EAD9C4?style=flat-square&labelColor=0B1A2B"></a>
  <a href="#proof-every-number-with-the-command-that-reproduces-it"><img alt="Benchmarks" src="https://img.shields.io/badge/benchmarks-published%2C%20failures%20included-A7472C?style=flat-square&labelColor=0B1A2B"></a>
  <a href="#privacy-and-safety"><img alt="No telemetry" src="https://img.shields.io/badge/telemetry-none-3F7D63?style=flat-square&labelColor=0B1A2B"></a>
</p>

<p align="center">
  <a href="#install"><b>Install</b></a> &middot;
  <a href="#what-you-can-actually-do-with-it"><b>Use cases</b></a> &middot;
  <a href="#proof-every-number-with-the-command-that-reproduces-it"><b>Proof</b></a> &middot;
  <a href="#mcp-tools"><b>Tools</b></a> &middot;
  <a href="#captcha-solving-fully-local"><b>Captcha</b></a> &middot;
  <a href="#the-rest-api"><b>REST API</b></a> &middot;
  <a href="#how-svipall-compares"><b>Compare</b></a> &middot;
  <a href="#faq"><b>FAQ</b></a>
</p>

---

**No cloud. No API keys. No paid captcha services. No telemetry. Nothing leaves your computer.**

An LLM on its own is a very well-read person locked in a room with no window. Svipall is the window
— and a good one, because a large part of the web slams the shutters the moment it notices a robot
looking in. Svipall looks, behaves and waits like a real visitor, so the shutters mostly stay open;
and when they do not, it says so plainly instead of handing your agent an error page dressed as an
article.

## Why Svipall

| What goes wrong | Svipall |
|---|---|
| Your agent reads a "checking your browser" screen and summarises it as the article. It was a `200`, so nothing flagged it | Twelve wall kinds, each naming the move it implies. **A block is never reported as a success** [→](#judging-what-came-back) |
| You crawl 5,000 pages and can't tell which are worth keeping | Every page labelled on arrival — integrity, substance, provenance, near-duplicates — and **never removed** [→](#judging-what-came-back) |
| One page = 300,000 tokens of raw HTML. The fixes are four manual jobs you now own | All four are defaults: clean Markdown, tables as rows, `out_file` to disk, the site's own JSON API [→](#reading) |
| Beating a captcha means paying a service — quotas, per-solve fees, your pages sent away | Nine strategies, vision models inside the binary, and a phone dashboard for the rest [→](#captcha-solving-fully-local) |

It shows its work: three benchmark lists with the raw logs committed, including four sites it
*cannot* beat, a round where the score dropped, and two finished features shipped **off** because
the measurement said so. [→](#proof-every-number-with-the-command-that-reproduces-it)

Rust · MCP + CLI + REST · no Node, no Python, no API key · nothing leaves your machine
→ **[Install it ↓](#install)**

Where another tool is the better choice, [the comparison table](#how-svipall-compares) says so.

<details>
<summary><b>Table of contents</b></summary>

- [**Why Svipall**](#why-svipall)
- [Install](#install) — [ask your agent](#ask-the-agent-you-already-have) · [**Claude Code plugin**](#claude-code-install-the-plugin) · [install it yourself](#install-it-yourself) · [from a shell](#or-drive-it-from-a-shell) · [what comes back](#what-comes-back) · [in a container](#or-run-it-in-a-container) · [full install guide](docs/install.md) · [never done this before](GET-STARTED.md)
- [What you can actually do with it](#what-you-can-actually-do-with-it) · [who it is for](#who-it-is-for)
- [**Proof**: every number, with the command that reproduces it](#proof-every-number-with-the-command-that-reproduces-it)
  - [Extraction quality vs. readability, trafilatura and resiliparse](#extraction-quality--measured-against-public-corpora-including-where-it-loses)
  - [`public31` — the independent list](#anti-bot-public31-the-independent-list-scored-by-its-own-rule) · [`hard12`](#anti-bot-hard12-our-own-list-chosen-because-it-has-walls) · [`vendors8`](#anti-bot-vendors8-four-vendors-named-two-targets-each)
  - [**What Svipall does not get past, and why**](#what-svipall-does-not-get-past-and-why)
  - [Automation tells](#automation-tells--160-of-160-offline-and-it-fails-the-build) · [identity coherence](#identity-coherence--asserted-offline-in-ci) · [CPU budgets](#cpu-budgets--measured-not-recalled)
- [Features](#features) — [reading](#reading) · [judging what came back](#judging-what-came-back) · [getting in](#getting-in) · [acting](#acting) · [crawling](#crawling) · [remembering](#remembering-and-staying-safe)
- [How it works, in plain words](#how-it-works-in-plain-words)
- [MCP tools](#mcp-tools) · [the CLI](#the-cli)
- [Captcha solving, fully local](#captcha-solving-fully-local) · [the human dashboard](#the-human-dashboard) · [training your own models](#training-your-own-models)
- [The REST API](#the-rest-api)
- [Privacy and safety](#privacy-and-safety) · [limits, stated on purpose](#limits-stated-on-purpose)
- [Configuration](#configuration)
- [How Svipall compares](#how-svipall-compares) — Firecrawl, Crawl4AI, Scrapling, Playwright MCP
- [Architecture](#architecture) · [documentation](#documentation) · [development](#development) — [build from source](#build-from-source)
- [FAQ](#faq) · [about the name](#about-the-name) · [licence](#license) · [trademark](#trademark) · [disclaimer](#disclaimer)

</details>

---

## Install

Three ways in. The first two install Svipall **and** wire it into your assistant in one step; the
third is for everyone else.

### Ask the agent you already have

Paste this into Claude Code, Cursor, Codex, opencode, or anything else that can run a command:

```
Install and configure Svipall by following the instructions here:
https://raw.githubusercontent.com/ilien-dev/svipall/main/docs/install.md
```

That page is written to be executed rather than read: it works out the platform, picks a channel,
verifies the download and registers the MCP server, asking you before each step.

### Claude Code: install the plugin

```
/plugin marketplace add ilien-dev/svipall
/plugin install svipall@svipall
/svipall:setup
```

`/svipall:setup` installs the binary if it is missing, checks the server answers, and offers to make
Svipall the way Claude reaches the web in every project. It asks before each of those.
`/svipall:doctor` reports what this installation can actually do, and `/svipall:uninstall` reverses
everything setup touched.

### Install it yourself

One line, no toolchain, nothing to compile:

```bash
curl -fsSL https://raw.githubusercontent.com/ilien-dev/svipall/main/install.sh | sh   # macOS, Linux
irm https://raw.githubusercontent.com/ilien-dev/svipall/main/install.ps1 | iex        # Windows
```

Or pull the container image:

```bash
docker pull ghcr.io/ilien-dev/svipall:latest
```

<details>
<summary>Homebrew, Scoop, winget, the AUR and npm: not yet</summary>

Each of those needs a one-time step outside this repository — a tap, a bucket, a pull request to
`microsoft/winget-pkgs`, an AUR package, an `npm publish` — and none of them has been taken. The
manifests are written and are rendered from each release's own `sha256sums.txt` by
`scripts/render-packaging.sh`, so the work left is publishing them rather than writing them;
[`packaging/README.md`](packaging/README.md) says what each one needs.

Until then, `install.sh`, `install.ps1` and the container image are the ways in, and a command like
`winget install ilien-dev.svipall` will tell you there is no such package. `.deb` and `.rpm` files
are attached to each release.
</details>

Never installed anything from a terminal before? [**GET-STARTED.md**](GET-STARTED.md) is the same
thing with nothing assumed. Everything else, including
[building from source](docs/install.md#3-building-from-source-instead), is in
[docs/install.md](docs/install.md).

Then ask it what it can do on this machine, and what to run for anything it cannot:

```bash
svipall doctor
```

Wiring it into a client, when nothing did it for you:

```bash
claude mcp add svipall -- svipall-mcp
```

<details>
<summary>Claude Desktop, Cursor, or any other MCP client</summary>

```json
{
  "mcpServers": {
    "svipall": {
      "command": "svipall-mcp"
    }
  }
}
```

Use an absolute path if the client does not inherit your shell's PATH — GUI apps on macOS usually
do not.
</details>

### Then ask for something

No key to paste, no account to create, no service to sign up for.

> *"Read this page and summarise the pricing."*
> *"Crawl these docs and write me an `llms.txt`."*
> *"Watch this listing and tell me when the price moves."*
> *"Get me every row of that table as CSV."*

The assistant picks the right tool on its own. A human dashboard for anything that needs a pair of
eyes lives at `http://localhost:8787/human`.

### Or drive it from a shell

```bash
svipall fetch https://example.com/article
svipall fetch https://shop.example/item --query "shipping costs"
svipall fetch https://docs.example/api --schema auto        # rows from a listing you've never seen
svipall crawl https://docs.example/ --pages 50 --out pages.csv
svipall search "rust async runtime" --engine all
svipall snapshot https://news.ycombinator.com                # the page as roles and refs, not markup
svipall serve --port 8788                                    # the same server as a local REST API
```

Every command prints **one JSON object** to stdout; diagnostics go to stderr, so `| jq` always works.

### What comes back

A real run of `svipall fetch https://example.com` — every field verbatim, with only the `content`
string cut short for this page:

```json
{
  "attempts": ["http: 200 (170ms) OK"],
  "chars": 167,
  "content": "# Example Domain\n\nThis domain is for use in documentation examples…",
  "exit": null,
  "final_url": "https://example.com/",
  "optimization": "ordinary",
  "quality": "thin",
  "quality_reasons": ["thin_text"],
  "status": 200,
  "tier_used": "http",
  "title": "Example Domain",
  "tokens_estimated": 42,
  "url": "https://example.com"
}
```

`tier_used` says how hard it had to try. `quality` says what actually arrived — and when a page does
*not* arrive, the same object carries `blocked_reason`, `wall_kind`, `wall_vendor`, `wall_evidence`
and a `note` telling your agent what to do next. Straight from a committed benchmark record:

```json
{ "wall_kind": "vendor", "wall_vendor": "kpsdk.io", "wall_evidence": "header x-kpsdk-ct" }
```

**A block is never reported as a success.** That single rule is what the rest of this README is about.

### Or run it in a container

```bash
claude mcp add svipall -- docker run -i --rm -v svipall-home:/data ghcr.io/ilien-dev/svipall:latest
```

Two tags, and the difference between them is real. `latest` carries Chrome for Testing **and** the
captcha models, both put there at build time and never fetched at run time, so every tier works.
`slim` carries neither: it is the http tier, and a page behind a challenge stays blocked. `latest`
is `linux/amd64` only, because Chrome for Testing publishes no linux-arm64 build and an arm64
"full" image would be a full image with no browser in it; `slim` is built for both architectures.

Everything it learns lives in the `svipall-home` volume, and `-i` is what keeps stdin open for MCP.
Publish `-p 8787:8787` to reach the dashboard: loopback inside a container means the container, so
the entrypoint writes a `/data/config.toml` binding `0.0.0.0` the first time it starts, and never
touches one you wrote yourself.

Tagged releases attach builds for **Windows x86-64, macOS Intel, macOS Apple silicon, Linux x86-64
and Linux arm64** with a `sha256sums.txt` and a GitHub build attestation, and push both images to
`ghcr.io`.

---

## What you can actually do with it

| You want to… | It looks like this |
|---|---|
| **Read one page cleanly** | `web_fetch` → main content as Markdown, boilerplate stripped, hidden text removed, `query=` to keep only the relevant parts |
| **Turn a listing into rows** | `schema: "auto"` reads the page's own repeated structure, names the columns and hands back typed rows — no model, no API, one parse |
| **Pull a data table** | `tables=true` → typed rows; `out_file: rows.csv` writes them to disk so thousands of rows never touch your context |
| **Skip the scraping entirely** | `web_capture` returns the JSON the page fetched while loading — usually the site's real API, with `?page=2` waiting for you |
| **Turn a docs site into a corpus** | `web_crawl` with `llms.txt` output, near-duplicate removal, resumable frontier, and a stop when it stops learning |
| **Search without a key** | `web_search` scrapes DuckDuckGo, Bing and Brave; `engine="all"` merges them by agreement |
| **Let the agent click things** | `web_snapshot` (roles + refs, a fraction of the tokens) then `web_act` — click, type, scroll, wait, all through human-like input |
| **Get through "Just a moment…"** | Automatic: the ladder climbs to a patient browser tier and answers the challenge, or tells you exactly why it could not |
| **Answer a captcha locally** | Nine strategies plus embedded vision models — and a phone-friendly dashboard for the rest. No vendor, no quota, no per-solve fee |
| **Log in once and stay in** | `web_login` opens a real window; you sign in; the cookies are kept in a profile you can export |
| **Watch a page** | `web_watch` on the whole page or one CSS region — and the region survives a redesign |
| **Read PDFs and Office files** | docx, xlsx, pptx, odt, epub, rtf, csv and pdf come back as Markdown, from the web or from `file://` |
| **Drive it from any language** | `svipall serve` → 19 local REST routes, one per tool, behind a bearer key it generates for you |

### Who it is for

| You are… | Svipall gives you… |
|---|---|
| **A Claude Code / Claude Desktop / Cursor user** | One line of setup and tools your assistant picks by itself. Research, documentation, price comparison, monitoring |
| **A developer building AI agents** | A local, deterministic, token-cheap web layer with structured output, file export and resumable crawls |
| **A RAG / dataset builder** | Whole-site crawls to clean Markdown, near-duplicate removal, `llms.txt`, and a page-quality label on every document |
| **A data or research person** | Pages that sit behind "checking your browser" walls — and an honest answer when your address cannot open one |
| **A privacy-conscious operator** | No scraping API, no captcha farm, no geolocation lookup, no update check, no telemetry. The only things it ever fetches other than your pages are the browser you asked it to install and the blocklists you enabled |
| **A security or QA engineer** testing your own site | A reproducible benchmark whose raw run logs are committed in this repository, and a request log that names which tier answered and which wall appeared |

Svipall is **not** a hosted scraping API and does not try to be one. If you want a URL you can `curl`
from a serverless function, use a cloud service. If you want the web inside your own agent, on your
own hardware, with nothing phoning home, this is that.

---

## Proof: every number, with the command that reproduces it

This project publishes its own benchmarks and reports the number it gets, not the number it would
like. Raw run logs and JSON are committed in [`bench/baseline/`](bench/baseline/) — including the
rounds of work that improved nothing, and the rounds where a number went *down*.

| Gate | Result | Needs network? | Command |
|---|---|---|---|
| Test suite | **1,143 passing**, 0 failing, 16 ignored | no | `cargo test --workspace` |
| Automation tells | **160 / 160** probes clean — 32 probes × 5 browser passes | no, but needs a browser | `bench tells --assert` |
| Identity coherence | **8 / 8** — 7 identities plus a 1,500-machine sweep | no | `bench fingerprint --engine chrome` |
| Network fingerprint | **8 / 8** wire checks against `tls.peet.ws` | yes | `bench fingerprint` |
| CPU budgets | **11 timed budgets + 4 structural checks**, all inside budget | no | `bench micro --assert` |
| Extraction quality | median ROUGE-LSum F1 **0.920** over 3,975 pages | no, once the corpus is fetched | `bench extract --corpus DIR` |
| Anti-bot, independent list | **26 / 31**, range 25..26, **zero hard blocks** | yes | `bench evasion --set public31 --runs 3` |
| Anti-bot, our own hard list | **7 / 12**, range 7..8 | yes | `bench evasion --set hard12 --runs 3` |
| Anti-bot, four named vendors | **3 / 8**, range 2..3 | yes | `bench evasion --set vendors8 --runs 3` |

**The rule every evasion figure is read under:** median of three runs with its range, targets in a
fresh random order each run, cooldowns cleared first, from **a single residential address with no
proxy**. A change counts as an improvement only when the median leaves the previous range. The
reputation spend is deliberately *not* cleared, and `bench evasion` refuses to start a list whose
address has already spent past the line.

> `public31` was re-taken against the current tree on 2026-09-05. **`hard12` and `vendors8` carry
> their 2026-09-04 and 2026-09-05 figures and predate parts of this tree.** They were not re-taken
> because running `public31` spends the same addresses they score, and taking all three back to back
> is the exact thing that produced a round this project already published as a warning.

### Extraction quality — measured against public corpora, including where it loses

ROUGE-LSum F1, median over the 3,975 gradable pages of the **SIGIR-23 gold standard**, scored by
`svipall-bench extract` against the study's own published extractions:

| | median | mean | IQR |
|---|---|---|---|
| readability | **0.963** | 0.861 | 0.881 – 0.987 |
| trafilatura | **0.958** | 0.877 | 0.870 – 0.986 |
| resiliparse | **0.936** | 0.826 | 0.810 – 0.980 |
| **Svipall** | **0.920** | 0.831 | 0.773 – 0.976 |
| Svipall, boilerplate removal off | 0.732 | 0.696 | 0.551 – 0.887 |

Three published extractors are above Svipall on median and the table says so. Boilerplate removal
is worth **+0.19 F1** over the same Markdown with it switched off, which is the number that
actually matters to a token bill.

<details>
<summary><b>The ensemble vote, and the router that was tried and retired</b></summary>

SIGIR-23 benchmarked fourteen extractors and then built three ensembles on top of them; all three
beat every individual system, and the paper's closing advice is that combining simple models may
beat a larger single one. `svipall-extract` implements that as a vote of several heuristics reading
one page, but at unanimity rather than the paper's two-thirds. A block is removed only when *every*
voter condemns it.

That makes the failure mode one-sided by construction: a misfiring voter, a badly-tuned threshold or
a page type nobody anticipated can only ever cause boilerplate to be **kept**, which costs tokens.
None of them can drop content, which costs the answer. The two-thirds rule is still available as
`Rule::Majority` for a caller who wants precision over recall; it is not the default and the module
says it never will be.

The vote is 0.919 against 0.920 on median, a wash, but it lifts the mean from 0.831 to 0.846 and the lower quartile from 0.773 to 0.804. It helps the pages that were
going badly and does nothing for the ones that were already fine.

A model to classify page type was tried here and retired, because the cheap structural signal beat it: the posting types the forum detector reads have
precision 1.000 on both halves of WCXB, against a model that named forums right about a third of
the time. When the cheaper signal is the more reliable one, it is the only one left — and it costs
one pass over a tree that is already parsed.

</details>

F1 cannot see whether the sentences a person marked as *required* survived extraction, and a page
scoring 0.92 that dropped the one sentence carrying the answer is still a failure. WCXB ships those
phrases, written by the corpus author:

| | required kept | boilerplate leaked | F1 |
|---|---|---|---|
| WCXB held-out, 505 pages | **93.3%** | 11.3% | 0.870 |
| WCXB dev, 1,476 pages | **86.3%** | 13.1% | 0.806 |

0.870 on the held-out set places Svipall **third of fourteen** on that benchmark's published
leaderboard. Five languages on DAnIEL, the remaining losses traced phrase by phrase, and the three
experiments that were tried against them and *rejected* are all in
[`docs/extraction.md`](docs/extraction.md) — with the reason each one stayed out.

These are floors rather than figures: `bench extract --assert` fails the run if any of them slips,
so the table above cannot quietly rot:

| what is held | floor | measured |
|---|---|---|
| SIGIR-23 median F1 | ≥ 0.90 | **0.920** |
| WCXB development F1 | ≥ 0.78 | **0.806** |
| Worst DAnIEL language | ≥ 0.55 | **0.608** (Chinese) |
| Required snippets kept | ≥ 0.84 | **0.863** (dev) |
| Boilerplate leaked | ≤ 0.15 | **0.131** (dev) |
| Reachable gold words dropped | ≤ 0.15 | **0.118** |

The floors sit a little below the measurements on purpose — at the measured number, ordinary
variation turns into a red build; far below it, the gate stops being one. Raising one after an
improvement is the intended use; lowering one has to be argued for in the commit that does it.

None of these corpora are vendored — they are other people's data and
hundreds of megabytes of it — so four scripts fetch them, each naming its paper and its licence:

```bash
scripts/fetch-extraction-corpus.sh   # SIGIR-23 gold standard (Bevendorff et al.), Apache-2.0
scripts/fetch-wcxb.sh                # WCXB (Foley, 2026) — the required/forbidden phrases
scripts/fetch-daniel.sh              # DAnIEL (Lejeune et al.) — the five-language arm
scripts/fetch-teco.sh                # TeCo (Alarte & Silva), BSD — sibling pages, for template detection
cargo run -p svipall-bench --release -- extract --corpus ./extraction-corpus
```

`.ps1` equivalents sit beside each. The SIGIR tarballs are Git LFS pointers, so `git-lfs` has to be
installed first — without it a clone silently yields 133-byte text files where the pages should be,
and the script says so rather than letting the benchmark score an empty corpus.

### Anti-bot: `public31`, the independent list, scored by its own rule

`public31` is the list an independent benchmark published in May 2026 — seven stealth tools, 31
targets, 651 verdicts — scored with **that benchmark's own four-way rule** (`ok | gated | blocked |
error`) ported verbatim into `bench/src/targets.rs`, so a cell here means what a cell there means.

| | runs | median | range | `blocked` verdicts |
|---|---|---|---|---|
| **Svipall** | 25, 26, 26 | **26 / 31** | 25..26 | **0** |

Zero `blocked` is the column that matters: 77 `ok` and 16 `gated` across 93 cells, and not one
cell where a site made a decision about *this address* rather than about a request. The benchmark
this list comes from published a `blocked` column for all seven tools it measured, and only one of
them reached zero:

| | OK | gated | **blocked** |
|---|---|---|---|
| nodriver | 28 | 3 | **0** |
| CloakBrowser | 26 | 3 | 2 |
| curl_cffi | 26 | 3 | 2 |
| Patchright | 25 | 3 | 3 |
| Camoufox | 25 | 3 | 3 |
| Playwright (vanilla) | 24 | 2 | 5 |
| rebrowser-playwright | 24 | 2 | 5 |
| **Svipall** | **26** | 5 | **0** |

That table is a citation, not a measurement. The seven rows above Svipall are the figures
that benchmark published; this project did not run those tools and cannot vouch for them. Different
machine, different address, months apart — **the OK counts are not comparable cell for cell and are
not offered as if they were.** What *is* checkable here is the porting: the target list and the
four-way rule live in [`bench/src/targets.rs`](bench/src/targets.rs), so you can read exactly what
Svipall's own row was scored under and re-run it yourself.

The `blocked` column is the one that survives the comparison, because it is the failure that costs
something: a gate is a retry, a hard block is an address the site has decided about.

Twenty-five of these 31 targets pass for every tool measured there, *including unpatched
automation*; the signal lives in six cells. Resolved by tier across three runs: `http` 44, `real`
29, `warm` 4 — **nearly half of that list never needs a browser at all**. Median cost: **115.4 s per
run of 31, or 3.7 s per page.**

<details>
<summary><b>The five cells that do not pass, and what each one actually is</b></summary>

| Consistently gated | What it is |
|---|---|
| `bot.incolumitas.com`, `browserscan.net` bot page | Detection panels that score a visitor and print a verdict. There is no article to come back with. Gated for every tool the public benchmark measured, too |
| `sedarplus.ca` | A WAF. Also gated for all seven |
| `medium.com`, `canadianinsider.com` | **Not walls.** Both answer `200` with their own titles and 45–50 KB of their own content. The ported rule counts `cdn-cgi/challenge-platform` in the body as a gate, and every Cloudflare customer page carries that script whether or not a challenge was served |

That last row is the ported rule being over-broad, measured directly rather than argued about.
Svipall's own classifier is right and the imported one is wrong — and **the cells are still reported
as failures**, because moving a target or bending a scoring function to win two cells is how a
benchmark stops meaning anything. What is *not* done is escalating those pages to a browser to
satisfy the rule: opening a browser on a page already in hand would make the tool worse in exchange
for a number.

`indeed-jobs` used to be a sixth, and it is the single cell that took this list from 25 to 26. It is
a real Cloudflare managed challenge and it is also the flakiest target here — this benchmark has
watched it, `crunchbase` and `zillow` swap places across four separate rounds. What was different on
the run that moved it is **not the code but the address**, which had been left alone for two hours.
A number that moves when the address rests is a number about the address. It is reported as an
improvement only because the median left the previous range, which is this project's rule.

A Firefox arm was also measured once — `http_firefox = true`, one run, `25/31`, committed as
`bench/baseline/public31-firefox.*`. One run is not a median, so the default Chrome configuration
stays the headline.
</details>

### Anti-bot: `hard12`, our own list, chosen *because* it has walls

Twelve sites, scored by whether the expected text came back with no wall reported. Three runs,
2026-09-04. A 7/12 here and a 26/31 there are not the same kind of number, and quoting one
against the other — in either direction — is reading noise as signal. Both are published, each with
its list, so nobody has to.

| Site | Protection | Passed | Tier that answered | Time |
|---|---|---|---|---|
| example.com | none | 3/3 | `http` | 0.2 s |
| en.wikipedia.org | none | 3/3 | `http` | 0.3 s |
| news.ycombinator.com | none, JS-light | 3/3 | `browser` | 1.5–1.7 s |
| nowsecure.nl | Cloudflare Turnstile | 3/3 | `browser` | 1.5–2.1 s |
| amazon.com search | Amazon's own detection + JS-rendered listings | 3/3 | `browser` | 2.9–3.3 s |
| newegg.com listing | Akamai Bot Manager | 3/3 | `browser` | 6.9–11.6 s |
| stackoverflow.com | Cloudflare (403 on plain HTTP) | 3/3 | `warm` | 2.0–3.0 s |
| zillow.com | PerimeterX / HUMAN "Press & Hold" | **1/3** | `warm`, in the run that passed | 4.8 s pass; 57–61 s on the two failures |
| crunchbase.com | Cloudflare managed challenge | **0/3** | — | ~22 s to give up |
| indeed.com | Cloudflare managed challenge | **0/3** | — | 27–33 s |
| g2.com | DataDome (`captcha-delivery.com`) | **0/3** | — | 25–32 s |
| idealista.com | DataDome (`captcha-delivery.com`) | **0/3** | — | 25–32 s |

Resolved by tier across the three runs: `browser` 12, `http` 6, `warm` 4. Turnstile cleared on all
three runs of this list, in 1.5 s, 2.1 s and 1.6 s.

### Anti-bot: `vendors8`, four vendors named, two targets each

Two targets each behind **Kasada** (`twitch`, `hyatt`), **Akamai** (`newegg`, `homedepot`),
**DataDome** (`g2`, `idealista`) and **Cloudflare**'s managed challenge (`crunchbase`, `indeed`) —
the same ids the committed `bench/baseline/vendors8.json` uses. **Median 3/8, range 2..3.** It scores
worse than `hard12`, which is the point of publishing it. `hard12` and `public31` stay frozen, because a number only means something against
its own list.

Beyond the score:

- Kasada is passable: `twitch` clears at the `real` tier in all three runs —
  9.3 s, then 1.9 s, then 1.8 s. `hyatt`, behind the same vendor, fails in a way worth reading:
  28 s, then 63 s, then a timeout, across three runs minutes apart. The baseline reads that
  as the vendor's documented behaviour — the puzzle gets harder for an address it has seen
  repeatedly — and says so as a reading of the timings, not as something it measured inside the
  vendor.
- Akamai: the `homedepot` target answers `200` with its own error template — *"Oops!!
  Something went wrong. Please refresh page"*, 206 characters — after a day of benchmark runs
  against this address, and `403` with the same page over plain HTTP. That is a soft block wearing a
  success code. Svipall was returning those 206 characters *as the page*; it now treats a short
  "something went wrong, please refresh" as the stand-in it is.
- The two published records of the previous round disagree with each other, and rather than pick
  the flattering one, [`bench/baseline/README.md`](bench/baseline/README.md) says so and declines to
  assert any movement at all: *"until that is resolved, 'the median left the previous range' cannot
  be asserted."*
- A change that fixed real detection is explicitly not credited with the score. Vendor signs on
  headers and cookies had been declared and never read, so a whole class of wall was reported as
  "the page did not render". Fixing it renames a block; it cannot make a page arrive — and the
  baseline says exactly that.

### What Svipall does **not** get past, and why

These are honest failures. Each was investigated by hand rather than retried, and each is decided on
the server's side by something the tool cannot change from a single home connection.

| Site | What actually happens | Why Svipall cannot fix it |
|---|---|---|
| **g2.com**, **idealista.com** | The interstitial carries the verdict `'t':'bv'` — *blocked visitor* — in the top document. That is a hard block, not a challenge; no slider is ever offered | The verdict is **IP reputation**. It persisted with a clean Chrome for Testing, a fresh profile and a rotated machine identity, and both of these sites name this IP on the page. The same address gets a *solvable* challenge (`'t':'fe'`) only over bare HTTP, which cannot run the widget. Only another exit address changes the answer |
| **crunchbase.com** | Passed at six seconds early in the day; refuses the same code, on a fresh profile wearing a different machine, after fifteen visits within the hour | The decision is **per visit and per address**, taken server-side from traffic history. Svipall behaves correctly on the page and the outcome still depends on how the address has been scored that hour |
| **indeed.com** | Not a fixed answer at all: 0 of 3 on `hard12` (2026-09-04), 2 of 3 on `public31` the next day, and 1 of 3 in an earlier round | Same shape as crunchbase. `indeed`, `crunchbase` and `zillow` have swapped places across four measurement rounds: this is the noise band of a list run from one address, not a code change, which is why it is listed here rather than counted as a pass |

> Svipall's answer to all of these is `web_route` — send the domain through a
> residential proxy *you* supply — and that is the one thing a local-only tool cannot provide for
> itself. It will never bundle proxies, never call a captcha farm, and never report a block as a
> success.
>
> `evasion --exit URL` runs the same targets through an exit you supply, so *"Svipall cannot"* can be
> separated from *"this address cannot"*. **No committed baseline has ever used it** — every one
> reads `"exit": null` — so that qualifier applies to every number on this page.

### Automation tells — 160 of 160, offline, and it fails the build

`fingerprint` asks public detectors what they see, which needs the network, which keeps it out of
the build. So there is a second harness that asks the same question of a page the benchmark serves
itself on loopback, across **five browser passes** — `browser`, `browser (reused)`, `stealth`,
`real` and `warm` — 32 probes each, and it **fails the build**:

```bash
cargo run -p svipall-bench --release -- tells --assert
```

It opened at 22 of 52 clean, fourteen probes across four tiers, before the harness grew to 32
probes and five passes. Every failure was a real contradiction that had been shipping. A sample of
what a harness catches that a person does not:

| Probe | What it caught |
|---|---|
| `residue` | `window.__svipall_console` — a ring buffer under a name that spelled out the product. Walking `window`'s own property names is the cheapest check a detector runs |
| `dom_rect` | `getBoundingClientRect` jittered `x`/`width`/`height` and not `left`/`right`/`top`/`bottom`, so every rectangle disagreed with its own arithmetic |
| `host_object_brands` | `navigator.connection` and `performance.memory` replaced by object literals: `[object Object]` where `[object NetworkInformation]` belongs |
| `languages_shape` | **`en;q=0.9` inside `navigator.languages`** — an `Accept-Language` header where a list of tags belongs. Nobody predicted this one |
| `screen_plausible` | Headless reports an 800×600 display while the flags size the window to 1366×768: a window wider than its screen |
| `worker_realm` | A worker reporting the host's real 32 cores beside a document reporting the identity's 8. One `postMessage` to catch |
| `cross_realm_tostring` | A same-origin `about:blank` iframe is a realm of its own, so the `toString` mask had never seen the top realm's accessors. One line returned the patch *and* the value it hid, at every tier |
| `navigator_webdriver` | `navigator.webdriver` was deleted outright. Every Chrome since 89 carries the property and answers `false` — the deletion was the only thing producing a navigator no real browser has |
| `runtime_domain_unobservable` | A watchdog, not a defect: it fires only if Chrome reopens the `Runtime.enable` console leak the CDP client's design rests on |

All 32 pass at all five passes now. Two of the fixes were structural rather than cosmetic: the
console ring is gone from the page entirely and comes from `Runtime.consoleAPICalled` on the protocol
side, and workers are handed the identity in the window between attaching paused and resuming.

### Identity coherence — asserted offline, in CI

A fingerprint is rarely caught by one odd value. It is caught by a combination no real device
produces: a macOS user agent with a Windows GPU, a desktop with no taskbar, a Firefox emitting
Chrome's client hints. Camoufox, the leading patched-Firefox project, names exactly this in its own
documentation as the thing it keeps getting wrong — not the spoofing technique, the *coherence
between spoofed values*. The quotation and what it implies are in
[`docs/firefox.md`](docs/firefox.md).

```bash
cargo run -p svipall-bench --release -- fingerprint --engine chrome
```

checks all seven identities Svipall can wear (Chrome, Firefox and phone, across three operating
systems) plus a **sweep of 1,500 freshly drawn machines**, against themselves: engine ↔ user agent,
client hints ↔ engine, screen ↔ availHeight ↔ viewport, form factor ↔ platform, timezone ↔ language,
renderer ↔ engine, and the macOS OS-token spelling that differs between the two engines. No network,
no browser, and it **fails the build** on a contradiction. It runs in `qc` and in CI.

Run **without** the `--engine` flag, the same command adds a network half — the only part of it that
touches the wire — and asserts eight things against `tls.peet.ws`:

| What is asserted | Measured |
|---|---|
| Which engine actually ran | `wreq` — the emulating one, not the fallback |
| Negotiated protocol | `h2` |
| JA4 carries the h2 marker | `t13d1516h2_8daaf6152771_d8a2da3f94cd` |
| Cipher count is Chrome-shaped | **15**, where rustls sends 20 |
| Extension count is Chrome-shaped | **16**, where rustls sends 11 |
| GREASE values present | yes |
| User-Agent matches the emulation | Chrome 149 on Windows |
| Post-quantum key share | `X25519MLKEM768` offered **with a key share**, as Chrome 131+ does |

> An earlier version of this file called the post-quantum key share a known gap. **It was not:** the
> check was looking for it in `ja4_r`, which lists ciphers, extensions and signature algorithms and
> never supported groups. The engine had been offering it all along. That correction is in the
> baseline log too, because a benchmark that quietly deletes its own mistakes is a marketing page.

### CPU budgets — measured, not recalled

`cargo run -p svipall-bench --release -- micro` on a 195 KB generated news page. The fixture is
generated from a fixed seed rather than checked in, so two machines measure the same document.

**The `Measured` column is one run on one desktop and yours will differ; the `Budget` column is what
`--assert` actually enforces**, and it is the half that gates the build. Timing budgets carry
headroom for exactly that reason; the structural checks are exact and cannot flake on any machine.

| Check | Measured | Budget |
|---|---|---|
| `classify` a 200 KB page | 196 µs | 400 µs |
| `quality::assess` | 45 µs | 250 µs |
| `parse_page`, text + title | 2.28 ms | 14 ms |
| `parse_page`, everything | 6.01 ms | 20 ms |
| Markdown, voted | 5.34 ms | 8 ms |
| `template::strip` | 213 µs | 2 ms |
| `induce` a schema from a listing | 2.40 ms | 60 ms |
| `bm25_filter`, full page | 1.86 ms | 3 ms |
| `budget::take`, full page | 225 µs | 4 ms |
| `simhash`, full page | 698 µs | 5 ms |
| `cache::find_near` over 300 pages | 722 ns | 2 ms |
| **DOM parses** for text + title + markdown + links + metadata | **1** | exactly 1 |
| **Disk reads** across 10,000 domain-state lookups | **0** | exactly 0 |
| **Ledger writes** across 10,000 charges | **0** | at most 1 |
| Pruning kept the article, the code and the table; dropped the nav and the sidebar | pass | exact |

---

## Features

### Reading

- **LLM-ready Markdown from any URL** — main content only, boilerplate stripped, hidden text
  removed, optional JSON schema extraction, BM25 `query` filtering, pagination by `max_tokens` +
  `cursor`. A continuation names the whole heading path it resumes under (`Guide > Install >
  Windows`) and repeats the tail of the previous page, so a page read in parts is never picked up
  cold.
- **Tables as rows, documents as prose** — `tables=true` returns every data table as typed rows
  (CSV/JSON/JSONL to a file), and docx, xlsx, pptx, odt, epub, rtf, csv and pdf come back as
  Markdown, from the web or from `file://` under a declared root. `raw:<html>` extracts markup you
  already have, with no request at all.
- **The site's own API, for free** — `web_capture` returns the JSON the page fetched while loading,
  so you page through `?page=2` instead of re-scraping HTML.
- **Selectors that survive a redesign** — what a `schema` finds is fingerprinted per domain; a
  selector the next redesign breaks is relocated by structural similarity and reported as `healed`
  with the selector to switch to. Never guessed: an ambiguous match is an error, not data.
- **A schema for a listing you have never seen** — `schema: "auto"` reads the page's own repeated
  structure, names the columns for what they hold (`title`, `url`, `price`, `date`, …) and returns
  the rows in `extracted` with the schema that produced them in `induced_schema`, to keep and pass
  back next time. No model, no API, one parse. A page with no clear record set returns **neither**: a
  candidate that does not clearly beat the runner-up is refused, and a field missing from a quarter
  of the records is dropped, because a guessed row is worse than no row and stays wrong quietly.
- **Feeds that load as you scroll** — `scroll="auto"` scrolls with wheel input until the document
  stops growing, clicks one "load more", and reads the whole listing.
- **Cheaper variants when you want them** — `mobile=true` (phone layout, usually far less
  navigation), `text_only=true` (skip images, fonts, media), `css_selector`, and a page cache that
  revalidates with `If-None-Match`, so a repeat visit costs a `304`.
- **Cross-page boilerplate removal — built, measured, and shipped *off*.** This is the one thing a
  local tool can do that a stateless extractor cannot: trafilatura sees one page and must guess what
  its navigation is, while Svipall has a cache of what you actually fetched from that domain, so a
  block on most of a site's pages is *the site*, not the page. Templates are 40–50% of the data on
  the web (Alarte & Silva), and SIGIR-23 says no public benchmark can score cross-page methods
  because none ships the sibling pages. TeCo does, so it was scored there: at the shipping threshold
  it fired on 2 of 11 sites, saved 3.4% of the text — and removed **one word of human-labelled
  content on one site**. The gate on this feature is absolute, no threshold made it hold, and tuning
  the threshold until one corpus reports zero would be fitting to that corpus. So it is `false` by
  default and reachable by asking: `use_site_template: true`. Two guards apply even then — nothing
  is stripped until 16 pages of a domain are cached, and a strip that would leave under a fifth of
  the page is refused and reported instead.

### Judging what came back

Most tools tell you a request succeeded. Svipall tells you what *arrived*, and never withholds a
page over it. Nothing in this section removes a document, stops the ladder or subtracts from a
verdict. It labels; the caller decides.

A `200` is not an answer. Every response carries a `wall_kind`, and each one names the move it
actually implies rather than a generic retry. These are the wire values, verbatim:

| `wall_kind` | What it is | What happens next |
|---|---|---|
| `none` | The page arrived | done |
| `cloudflare` · `generic` · `hold` | A challenge still standing at this tier | climb, answer it on the page, or route the domain elsewhere |
| `vendor` | A fingerprinting wall, with `wall_vendor` and the `wall_evidence` that named it — a body string, a **response header**, or a **cookie name** | jump straight to the `real` tier; a residential exit usually helps |
| `empty` | Rendered no text at all | climb; or `web_act` with a wait, or a `css_selector` for the region you need |
| `status` | A hard HTTP block. The domain goes on a 15-minute cooldown | `web_route`, or clear the cooldown deliberately |
| `gate` | A **geo or consent** gate instead of the page — not a captcha | **stops the ladder**, because no tier dismisses a cookie banner: `web_act` to click through, or `web_route` to change country |
| `login` | A sign-in wall | jumps to `real`, then `web_login` once by hand; with a profile already supplied it stops |
| `paywall` | The article exists and is being withheld. Only a signed-in profile changes the answer — **a proxy does not**, and the note says so | same as `login` |
| `notfound` · `softnotfound` | A real 404, or **a `200` whose body says the page is not there** | both stop the ladder; no tier fixes either, and the second is the one that would otherwise reach a model as content |
| `timeout` | No tier answered inside the budget. Not a wall at all, and it is not dressed up as one | raise `timeout`, or lower `max_tier` |

`empty` and `status` are the only two verdicts a vendor sign found on a
header or cookie may rename — they mean "blocked, cause unknown", so naming the vendor is an upgrade,
and a wire sign never invents a wall anywhere else. And `softnotfound` matches on the whole trimmed
title, never a substring, because *"Understanding soft 404s"* is an article.

On top of the wall verdict, every page that *did* arrive carries what it is worth reading as:

- **Integrity verdict on every delivered page** — `full`, `partial` (cut off mid-thought) or `thin`
  (a husk), with `quality_reasons` naming why: `thin_text`, `low_text_ratio`, `truncated`,
  `not_prose`, `repetitive`, `landed_elsewhere` (you asked for an article and got the front page),
  and `mostly_boilerplate` — that last one only ever from a **crawl**, because only a caller that has
  seen the rest of the site can know it, and a single fetch says nothing rather than guessing. Every
  rule is
  **language-neutral on purpose** — a stop-word test written against English removes African-American
  English at 42% and Hispanic-aligned English at 32% on C4, so the tests here use only shape:
  length, symbols, alphabetic share, repetition. Never vocabulary.
- **How engineered the page is** — `optimization: high` with the traits behind it
  (`affiliate_heavy`, `headings_echo_the_body`, `link_dense`). Two traits are needed, because one
  alone is ordinary: plenty of honest pages carry a few referral links and a glossary is legitimately
  link-dense.
- **A page-substance classifier you train yourself** — `junk` / `thin` / `ordinary` / `substantive`
  from a hashed-bigram linear model, because DCLM's 416 controlled experiments put exactly that
  architecture *above* embeddings, perplexity filtering and prompting a language model per document.
  Four levels and not six, because FineWeb-Edu published the confusion matrix: recall 0.01 at the
  highest level. Pretending to more resolution than that is inventing precision.
  `svipall quality ask | export-training | train` fits it from your own history and your own ratings.
- **Percentiles, with the width of the claim attached** — a score with nothing to compare it against
  is a number you cannot act on. Below 30 observations it **refuses to answer** and says why; the
  answer carries a `wide` (±9 points) or `narrow` (±3 points) band, and it is per class, never pooled.
- **Provenance observations, never a score** — byline, publication date, outbound citations, and when
  this machine first saw the site. The W3C Credible Web group's own finding is why it stops at
  reporting: acting on signals like these produces a bias toward large professional publishers, and a
  page with no byline can be the best source on its subject.
- **Near-duplicate awareness** — `near_dup_of` asks the cache whether it has seen this page before,
  under any other name.
- **Corroboration and diversity ordering on `web_fetch_many`** — the result says how many *distinct*
  documents it actually holds, marks each duplicate with `same_text_as`, and moves the different ones
  up. A reordering only: nothing is dropped, the caller's first choice stays first, and
  `reordered_for_diversity` says when it happened. Cuconasu et al. (SIGIR 2024) measured that the
  document which degrades a generated answer is the high-scoring, on-topic, *answer-free* one — four
  copies of one wire story at the top of a list is exactly that shape — while adding distant
  documents *raised* accuracy by up to 35%. Corroboration is reported as corroboration, never as
  truth: it says the same thing was said N times independently.

Pass `include_quality: true` for the full `quality_detail` block. It is off by default: the compact
fields are on every response, and this is for a caller weighing a source rather than reading one.

### Getting in

- **Automatic anti-bot escalation** — plain HTTP first, then headless Chromium, then
  a stealth-patched browser, then a headful "real" browser with a persistent per-domain profile,
  then a patient `warm` tier that answers challenges. **Learned per domain and remembered between
  runs**, so the second visit starts where the first succeeded.
- **Chrome-accurate network fingerprint** — JA4, HTTP/2 SETTINGS order, header order and GREASE on
  BoringSSL, post-quantum key share included. The eight checks are
  [above](#identity-coherence--asserted-offline-in-ci). The emulation currently presents **Chrome
  149**, which is the newest profile the TCP engine offers; the provisioned browser is 152, and that
  gap is named as open in [`CHANGELOG.md`](CHANGELOG.md) rather than glossed over.
- **Firefox, coherently, on the http tier** — `http_firefox = true` and the http tier presents Gecko
  in TLS, headers and User-Agent *together*: Firefox's own emulation profile, its real header order
  with `User-Agent` first, its own `accept`, and **no `Sec-CH-UA` at all** — Firefox sends no client
  hints, and emitting one is the loudest way to be caught pretending. The browser tiers stay Chrome,
  because their protocol is Chrome's. [`docs/firefox.md`](docs/firefox.md) sets out what a
  patched-Gecko browser engine would add and what it would cost.
- **Stealth that goes far beyond `navigator.webdriver`** — coherent machine identities (screen, GPU,
  fonts, languages, timezone, memory, DPR), deterministic canvas/audio/text-geometry noise, WebRTC
  leak prevention behind proxies, and no automation residue in the DevTools client. Asserted at
  [160/160](#automation-tells--160-of-160-offline-and-it-fails-the-build).
- **Human-like behaviour** — Bézier pointer paths that land off-centre, typing cadence by digraph,
  wheel-notch scrolling with inertia, focus/visibility events, dwell time proportional to page
  length. **Never a bare `click()`, `scrollBy` or forged event.**
- **Sessions retired rather than reused** — a session is cookies + machine + exit. When a site turns
  on one, the profile is retired (the browser holding it closed first) and the next visit arrives as
  somebody else. `isolated=true` makes a profile that exists only for one fetch.
- **Pools of exits, used properly** — `web_route` takes several proxies per domain, each with its
  declared country; the domain keeps one (`sticky`) until it blocks it twice, then moves on, and a
  retired exit **heals with time** rather than staying dead. Pacing, strikes and latency are keyed by
  `(domain, exit)`, so a pool actually buys throughput instead of ten exits sharing one gap.
  **Authenticated proxies work on every tier**: `user:pass` goes to the browser over the protocol,
  never onto the command line, which is what Chrome cannot read from `--proxy-server`. The exit's
  locale and timezone travel with it, `dns_over_https` closes the DNS leak when there is no proxy to
  do it, and `web_route check` flags a `socks5://` that would resolve names on your machine.
- **Reputation, spent like a budget** — what each address has spent with each host, decaying with a
  half-life, so a benchmark or a crawl cannot quietly burn the one address you have.
- **HTTP/3, opt-in** — a vendored quiche on the same BoringSSL the http tier already links, emitting
  Chrome's QUIC ClientHello and Chrome's HTTP/3 SETTINGS frame, both asserted offline against a
  capture of a real Chrome.

### Acting

- **Real browser interaction** — `web_snapshot` returns the page as roles, accessible names and short
  refs instead of markup; `web_act` clicks, types, fills, scrolls and waits on those refs.
  `browser_open` / `browser_do` keep a session alive across calls. Deterministic — no vision model.
- **Search without an API key** — DuckDuckGo, Bing and Brave scraped directly, optionally merged by
  agreement.
- **A site's own search box** — `web_site_search` fills it once, learns the URL pattern it produces,
  and every later query is an ordinary fetch with no browser at all.

### Crawling

- **Crawls that survive interruption** — same-domain BFS or DFS, robots.txt, sitemaps and feeds,
  near-duplicate removal, `llms.txt` output, and a `crawl_id` you pass back to resume from the
  persisted frontier.
- **Crawls that know when to stop** — coverage of your query and novelty per page are measured
  lexically (no model, no download); a crawl that has stopped learning ends instead of spending the
  rest of its budget.
- **Crawls that only fetch what moved** — `--since-last` compares sitemap `<lastmod>` against the
  page cache. A URL with no `lastmod` is fetched, because silence is not "unchanged".
- **Politeness that adapts** — the gap between requests is tuned per domain from the host's own
  latency and refusals (100 ms–2 s on HTTP, 400 ms–5 s on browser tiers), `Retry-After` is honoured
  up to two minutes, and beyond that the domain gets a visible cooldown.
- **Concurrency sized to your machine** — browsers cost far more than HTTP requests, and a laptop
  asked to run six of them produces timeouts that look exactly like walls. Parallelism is tightened
  from core count and open browsers; it is never raised above what the config allows.
- **Page two, found** — a listing whose next page is a URL differing by a number is recognised from
  the URL alone, before the crawl decides it has finished.
- **Bulk output to files** — CSV, JSON, JSONL written to disk with `out_file`, so thousands of rows
  never pass through the model's context.

### Remembering, and staying safe

- **Memory across sessions** — `web_notes` key-value store, `web_watch` change monitoring (whole page
  or one CSS region), `web_diff`, and a queryable request log that says which tier answered and which
  wall appeared.
- **Secrets that never reach the model** — credentials referenced by name from `~/.svipall/secrets.env`
  and substituted on the way to the browser.
- **Origin policy** — allow/block lists, private-address refusal, and optional ad/tracker/consent-banner
  blocking with cached lists that degrade silently offline.
- **A CLI with the same brain** — published as an [Agent Skill](skill/SKILL.md) for agents that prefer
  a shell to a tool schema; a test keeps the skill in step with the CLI.
- **A local REST API for every other language** — [details below](#the-rest-api).

---

## How it works, in plain words

1. **Ask for the page the cheapest way first.** A direct HTTP request that looks exactly like
   Chrome's. Most pages stop here — 59 of the 93 `public31` cells did, every one of them in under
   two seconds.
2. **If the page needs JavaScript, open a browser.** Headless Chromium runs the scripts and hands
   back the rendered document.
3. **If the site checks for robots, wear a disguise.** The stealth tier patches every surface a
   bot-detection script inspects so the browser matches the identity the network layer already
   presented.
4. **If the site wants a real person, act like one.** The `real` tier is a visible-but-offscreen
   browser with a persistent profile, moving the pointer along curves and scrolling with a wheel.
5. **If there is a challenge, answer it or wait it out.** The `warm` tier runs the captcha strategy
   loop every turn — hold the button, solve the hash puzzle, drag the slider — and keeps perfectly
   still on self-verifying interstitials, because pointer activity there is exactly what gives a
   script away.
6. **Remember what worked.** The next request to that domain starts at the tier that succeeded.
7. **Tell the truth when it fails.** A blocked result carries `blocked_reason`, the wall kind, the
   vendor and the evidence that named it, and a note with the next move: `web_login` (do it by hand
   once, cookies are kept), `web_route` (send the domain through a proxy), or the captcha tools.

---

## MCP tools

Twenty-nine tools, all local.

| Tool | What it does |
|---|---|
| `web_fetch` | Fetch a page as Markdown or structured JSON. `mode=auto` climbs the ladder. `schema` (self-healing), `tables`, `scroll`, `query`, `max_tokens`/`cursor`, `cache`, `include_metadata`, `include_links`, `include_quality`, `use_site_template`, `robots`, `out_file`, `mobile`, `text_only`, `isolated`, `css_selector`, `profile`, `proxy`, `method`/`body`/`headers`. URLs may be `raw:<html>` or `file://` under `local_roots` |
| `web_fetch_many` | Bounded-parallel fetch of many URLs. Reports `corroboration` — how many *distinct* documents the set actually is — marks each duplicate with `same_text_as`, and moves the different ones up. It says `reordered_for_diversity` when it did, because a set that comes back in a different order without saying so is a surprise, not a feature |
| `web_search` | DuckDuckGo / Bing / Brave without an API key; `engine="all"` merges by agreement |
| `web_site_search` | Use a site's own search box, learn its URL pattern, then every later query is a plain fetch |
| `web_crawl` | Same-domain crawl with robots.txt, dedup, boilerplate removal, `strategy=dfs`, `scroll`, `llms.txt`, file export, a saturation stop, and a `crawl_id` to resume |
| `web_map` | A site's URLs without crawling it: robots.txt, sitemaps (nested indexes and `.gz` included), RSS/Atom feeds and homepage links — a few hundred tokens of structure instead of the thousands a crawl costs |
| `web_snapshot` | The page as roles, accessible names and short refs that `web_act` accepts. Deterministic, no vision model |
| `web_act` | click, type, fill, press, hover, select, scroll, wait, eval, goto, screenshot, hold, verify, console — all through human-like input |
| `web_capture` | The JSON/XHR responses the page fetched while loading: the site's real API |
| `browser_open` / `browser_do` / `browser_close` | Persistent session with cookies and page state across calls |
| `web_screenshot` | PNG of the rendered page |
| `web_diff` | What changed on a page since Svipall last saw it |
| `web_watch` | Monitor a page, or one `css_selector` region of it, on a schedule and report changes; the region survives a redesign |
| `web_notes` | Key-value memory that outlives the session |
| `web_log` | Which tier answered, which wall appeared, how long it took, per domain |
| `web_login` | Visible window for a manual login or challenge; cookies saved to a profile |
| `web_route` | Per-domain proxy, or a pool of `proxies` with `countries`; subdomains inherit; `exit_strategy` sticky or round-robin; `check=true` tests the exits (liveness, latency, DNS leak) with no third-party service |
| `web_profile` | Export/import an encrypted browser profile between machines |
| `web_status` | Learned tiers, cooldowns, routes, per-exit health and latency, profiles, open browsers, solver stats, which models answer and from where, whether the host has a real GPU, `h3_offered_by` |
| `browser_setup` | Download or manage Chrome for Testing |
| `solve_and_continue` | Solve the captcha **on the blocked page** and return what is behind it |
| `solve_image_captcha` / `solve_recaptcha_v2` / `solve_turnstile` / `solve_hcaptcha` / `captcha_status` / `report_captcha` | Local captcha solving with the classic `in.php` / `res.php` / `createTask` / `getTaskResult` HTTP shape, so existing clients work unchanged |

### The CLI

```
svipall fetch | crawl | snapshot | capture | search | map | log | notes | watch
        profile | browser | route | status | serve
        solver export-corpus
        quality ask | export-training | train
```

A test asserts the usage text names every command the binary answers to, and a second test keeps
[`skill/SKILL.md`](skill/SKILL.md) in step with both.

---

## Captcha solving (fully local)

**Nine automatic strategies**, ordered on each page by what has actually worked on that domain
before (`outcomes`). A strategy that declines costs no attempt, and there is never a cascade of
`if`s.

| Challenge | How it is solved | Fallback |
|---|---|---|
| Turnstile, reCAPTCHA v2/v3, hCaptcha | The real page loads in a stealth browser and the token is read when the widget clears | A visible window opens for a person (`SVIPALL_HUMAN_ASSIST=0` to disable) |
| Proof of work (hash puzzles) | A hash loop, computed locally — no model and nobody interrupted. It either solves the challenge or declines; it has no failing branch, and a decline costs no attempt. One attempt, because a nonce either verifies or was misparsed and a second try is pointless | — |
| Press and hold | Held on the real iframe button for the measured interval, with a real approach and press; two attempts, then one retry on a fresh profile — and the flagged profile is retired | Visible window |
| Slider / rotation | Classical vision on a screenshot: cross-correlation for the notch, edge-energy minimisation for the angle. Three attempts, since both have a tolerance | Human dashboard |
| Drag a piece into place | Geometry on the same screenshot | Human dashboard |
| Self-verifying interstitial ("Just a moment") | **Nothing** — the tool keeps perfectly still and extends the wait once when the page reports progress | Visible window |
| Image grid ("select all…") | Local classifier, tiles clicked as real pointer input, two attempts | The embedded detector, then a zero-shot pair, then a visible window |
| 4×4 single-picture grid | The **embedded segmenter** marks every cell its mask touches | Dashboard |
| "Click on the …" / "draw a box around the …" | The **embedded detector**: centres clicked or the strongest box traced, as fractions of the picture | Dashboard (two taps make a rectangle) |
| Image-to-text | Local OCR (`--features onnx-ocr`, CRNN/CTC model in `~/.svipall/models/`) | Dashboard shows the image |
| Audio | Local acoustic model (`--features onnx-audio`), clip fetched from inside the page, decoded in pure Rust | Dashboard plays the clip |
| Anything else | Recognised from the widget table — **fifteen widget families, eleven answer modalities** — or by the generic detector when it is a widget the table has never seen, then routed by modality | Dashboard |

**Models ship in the release binary.** A detector (SSDLite320-MobileNetV3, 13.8 MB) and a segmenter
(DeepLabV3-MobileNetV3, 44.1 MB), torchvision weights under BSD-3, running on the **CPU** — so an
image grid is answered out of the box with nothing to install and nothing fetched at run time. A
model you train from your own corpus and drop in `~/.svipall/models/` **wins over the embedded one
and is picked up without a restart.**

Those two weights are not a binary blob you have to trust: `tools/models/export.py` regenerates them
from torchvision's published weights — no account, no key, no service — and `docs/models.md` states
the contract each one has to keep.

Widgets are named by the host their challenge endpoint lives on, which is the stable, factual name
for a protocol. **Eight of the fifteen families need no model file at all**, because their modality
(proof-of-work, slide, rotate, hold, drag) is arithmetic and image geometry; three of those are
proof-of-work schemes, answered by a hash loop that has no failing branch. A conformance test walks
the table and fails if a row has
no fixture or names a modality nothing can answer — which is what keeps *"adding a widget is adding
a row"* true rather than aspirational.

A subject a grid model does not know is **never guessed at** — wrong tiles spend the attempt and
confirm what we are — so it goes straight to a person.

### The human dashboard

`http://localhost:8787/human`, and on your LAN address when `dashboard_bind` is not loopback. One
renderer per modality, and it works from a phone. **Every coordinate it sends is a fraction of the
image, never a pixel**, so a 1280-wide challenge answered on a 390-wide screen is still correct. The
answer is checked against the modality of the job it answers *before* it is stored, so a mismatch is
a rejection at the door with a reason rather than a wrong answer discovered a minute later by the
site. `Unknown` — *"I cannot read this"* — is a real answer, and the one that keeps the ranking
honest. An unsolved challenge expires after 30 minutes; a page-rating card, which nobody is waiting
on, does not.

### Training your own models

```bash
cargo build --release --features onnx-ocr,onnx-grid,onnx-audio,onnx-detect,onnx-segment,onnx-zeroshot
```

| Feature | Embedded? | Files in `~/.svipall/models/` |
|---|---|---|
| `onnx-detect` | **yes**, 13.8 MB | `detect.onnx`, `detect.json` |
| `onnx-segment` | **yes**, 44.1 MB | `segment.onnx`, `segment.json` |
| `onnx-grid` | no | `grid.onnx`, `grid.json` |
| `onnx-ocr` | no | `captcha.onnx`, `captcha.json` |
| `onnx-audio` | no | `audio.onnx`, `audio.json` |
| `onnx-zeroshot` | no | `clip_image.onnx`, `clip_text.onnx`, `clip.json`, `vocab.json`, `merges.txt` |
| page substance | no (not ONNX) | `substance.bin`, `substance.json` — fit by `svipall quality train` |

A detector output whose class axis does not equal `4 + classes.len()` is **refused, not reshaped**.
Every challenge answered — by a model, by zero-shot or by a person — stays in the local corpus for
`corpus_keep_days` (30). `svipall solver export-corpus --out ./corpus` writes the images and a
`manifest.jsonl` with prompt, answer, who answered and whether the page accepted it: training data
for your own models. Rows with `"source":"human","ok":true` are labelled by a person *and* verified
by the site. Full sidecar contracts in [`docs/models.md`](docs/models.md).

---

## The REST API

The same server, over HTTP, so any language can drive it — not only an MCP client or a shell.

```bash
svipall serve --port 8788        # the bearer key is printed once, and kept in ~/.svipall/api_key
curl -sH "Authorization: Bearer $KEY" -H 'content-type: application/json' \
     -d '{"url":"https://example.com","query":"pricing"}' localhost:8788/v1/fetch
```

`svipall-mcp` mounts the same router when `rest_port` is set, on its own listener, sharing one
browser pool, one page cache and one set of learned tiers with the MCP tools.

Nineteen routes, one per tool, each taking that tool's own JSON as the body:

| | |
|---|---|
| `POST /v1/fetch` `/v1/fetch_many` `/v1/crawl` | pages |
| `POST /v1/search` `/v1/site_search` `/v1/map` | finding things |
| `POST /v1/snapshot` `/v1/act` `/v1/capture` `/v1/screenshot` | a real browser |
| `POST /v1/solve_and_continue` | the captcha, answered on the blocked page |
| `POST /v1/diff` `/v1/watch` `/v1/notes` `/v1/log` | memory |
| `POST /v1/route` `/v1/profile` `/v1/browser_setup` | configuration |
| `GET`/`POST /v1/status` | what this installation has learned. `GET` is read-only by construction: the three clearing fields are reachable only by `POST` |
| `GET /v1/health` | the one route with no key, so a container healthcheck does not need one |

A blocked page is a `200`: the call ran, the *page* did not. `blocked_reason`, `wall_kind` and
`note` are in the body, exactly as they are over MCP. Only a malformed body (`400`), a bad key
(`401`), a browser `Origin` or a rebound `Host` (`403`), a body over 2 MB (`413`) or a broken
installation (`500`) is not a `2xx`. A client that read a wall as a `5xx` would retry forever against
something that is never going to move.

Every route needs the key, including on loopback. A local port is not a boundary: Svipall carries
logged-in profiles, cookies and your exit address, so an open one is a proxy wearing your identity.
Two more checks sit in front of the key, because binding to `127.0.0.1` does not stop a page in your
own browser being served a DNS answer of `127.0.0.1` and posting to it: any request carrying an
`Origin` header is refused, and on a loopback bind so is any `Host` that is not loopback. There is no
CORS layer and there will not be one — no browser page is a client of this API.

Ten tools are deliberately **not** routes, in three groups, and `rest.rs` records why next to each:
`browser_open`/`browser_do`/`browser_close`, because a session is a resource HTTP cannot bound and a
client that dies between open and close leaks a real browser; `web_login`, because an HTTP request
must not make a window appear on your desktop and hold the connection for an hour; and the six
`solve_*`/`captcha_status`/`report_captcha` tools, which already answer on the dashboard port in the
classic solver wire shape. Twenty-nine tools minus those ten is the nineteen routes above. A new
`#[tool]` **fails the test suite** until it is listed as a route or as a named exclusion.

A long crawl is a job rather than a held connection. `"async": true` answers `202` with an id; `GET
/v1/jobs/{id}` polls it, `GET /v1/jobs/{id}/stream` follows it as Server-Sent Events, `DELETE` stops
it. The id **is** the `crawl_id`, so there is one handle to learn and resuming is `{"crawl_id": "…"}`
— the same word the MCP tool and the CLI already use. A cancelled crawl stops between pages *after*
that page's links are queued, so its frontier is kept; it is never aborted, because that would leak a
browser page. A job whose process was killed becomes `interrupted`, and `interrupted` is resumable.
The first frame of a stream is always a snapshot from the store, so a subscriber that joins at page
forty is never told the job started at zero. And a queued job whose site already has one running is
held back: two crawls of one site would spend one address's reputation with that host twice as fast,
which is the scarcest thing a local-only tool has. Full contract in [`docs/rest.md`](docs/rest.md).

---

## Privacy and safety

- **Prompt-injection defence** — text a person cannot see (`display:none`, `opacity:0`, off-screen,
  zero-width characters) is removed before the content reaches the model. The rules are deliberately
  narrow: five of `sanitize.rs`'s seven tests exist to prove visible text is *not* dropped.
- **Credentials never enter the context.** `{"do":"type","ref":"e4","text":"${SHOP_PASSWORD}"}` is
  substituted from `~/.svipall/secrets.env` on the way to the browser. `web_status` lists names,
  never values.
- **Origin policy, checked before the request** — `allow_origins`, `block_origins` (blocking wins),
  and `block_ads` (cached lists, silent when offline). `refuse_private_addresses` stops an agent
  following a link to `169.254.169.254` — it is **off by default**, deliberately, because fetching
  `http://localhost` is an ordinary thing to ask a local-first tool to do; turn it on for an
  installation where an agent chooses its own URLs.
- **robots.txt** is reported by default and can be made binding with `robots=obey`.
- **No telemetry, no update checks, no background network.** Nothing is sent anywhere and nothing is
  fetched on a timer. Besides the pages you asked for, Svipall makes exactly two other kinds of
  outbound request, both only when you ask for them: `browser install` / `browser_setup` downloads
  Chrome for Testing from Google's public build server, and setting `block_ads = true` fetches the
  two lists `blocklist_sources` ships with — StevenBlack/hosts and EasyPrivacy — once, and caches
  them. `block_ads` is `false` by default, so on a stock install that request never happens.
- **No breaking of access controls.** Svipall evades bot detection on public pages. It does not crack
  passwords, bypass paywalls, or forge authentication. A login wall is passed by *you*, once, in a
  visible window, and the cookies are kept.

---

## Limits, stated on purpose

Most of these are permanent and deliberate; one is a build you have to ask for, and it says so.

- **No proxies, no IP rotation service.** You bring your own exit; Svipall keeps timezone, locale and
  languages consistent with it and closes the WebRTC and DNS leaks. It does not detect the proxy's
  country (that would require a geolocation service), so you declare it.
- **No paid or remote captcha solving.** Solving quality is bounded by the models and your hands,
  never by a vendor's quota. What no model can answer is parked and handed to a person at the
  dashboard, whose answer is replayed on the live page.
- **HTTP/3 is off by default, and that is a build choice, not a limit.** It works: a vendored quiche
  on the same BoringSSL the http tier already links, emitting Chrome's QUIC ClientHello — ALPS 17613,
  ECH GREASE, `compress_certificate`, `trust_anchors`, extension permutation, a GREASE transport
  parameter — and Chrome's HTTP/3 SETTINGS frame, both asserted offline against a capture of a real
  Chrome that `bench h3-ref` takes from a loopback QUIC server a real browser handshakes with. It is
  off because a QUIC stack is 37,000 vendored lines to carry for a transport most sites still do not
  offer, and because it can only ever be a *second* visit: `Alt-Svc` is how a site says it speaks h3,
  so the first fetch of any domain is TCP exactly as before. Build with `--features http3` and set
  `http3 = true`.
  **Measured:** four of twelve `hard12` targets advertise h3 at all, and the evasion median does not
  move — 8/12 either way, against a noise floor of 123–369 s per run. What *does* move is cost: on a
  site that offers h3 and walls the cheap tier over TCP, a page arrives in **950 ms at the http tier
  instead of 2,967 ms with a browser**, five runs each; the worst case for a site that advertises h3
  and does not deliver is one extra 568 ms, once per domain per six hours. Still not Chrome: the
  `trust_anchors` payload is empty where Chrome sends a list, and one extension Chrome sends
  (`0x12e0`) is not in this BoringSSL at all — so an h3 engine carries a Chrome version ceiling of its
  own, set by the age of the linked library. The whole record, including **the two reasons this
  project previously gave for not doing HTTP/3 and why both were wrong**, is in
  [`docs/http3.md`](docs/http3.md).
- **A browser that defends its own fingerprint contradicts the identity every other layer states**,
  and no stealth script can undo it, because it is the binary talking. Brave is the measured case:
  with it selected, a public detector saw `navigator.brave` and randomised plugin names next to a
  User-Agent claiming Chrome. Brave, Vivaldi and Opera are therefore sorted last among detected
  browsers. A build **two or more majors** behind the stable channel is flagged for the opposite
  reason — its user agent names a Chrome that no longer exists in the wild — and one major is not,
  because a rollout takes weeks and advice that always fires is advice nobody reads. `web_status` and
  the note on any blocked page say when either is true, and `browser_setup` installs or updates a
  dedicated Chrome for Testing.
- **A host with no usable GPU is a fingerprint problem no spoof can fix.** Without acceleration the
  WebGL renderer reads `SwiftShader` or `llvmpipe`, which is the signature of a server or a VM.
  Claiming a GPU the machine does not have is caught by the rendered-image hash, which a spoof cannot
  reproduce without owning that hardware. Svipall detects the case and says so in `web_status` rather
  than pretending otherwise. Everything else — the models included — runs fine on CPU.
- **A local security product that injects into pages is visible to every site.** On one measurement
  round, an antivirus was injecting a stylesheet and a script into every page, and on a vendor's
  silent device-check page the only traffic the page made went to that product. Svipall names this in
  the note of a blocked result and says what to exclude; it cannot remove it for you.

---

## Configuration

`~/.svipall/config.toml` (or `$SVIPALL_HOME/config.toml`). Every field has a default, so a missing or
partial file is fine. Everything Svipall remembers lives in that one directory, and deleting any of
it costs memory rather than function:

| | |
|---|---|
| `config.toml` | The settings below |
| `secrets.env` | Credentials referenced by name; the values never enter a tool call |
| `domain_tiers.json` | Which tier answered for each domain, so the next fetch starts there |
| `pools.json`, `exit_health.json` | Exits per domain, and what each one has done on each |
| `reputation.json` | What each address has spent with each host, decaying with a half-life |
| `svipall.db` | Page cache, crawl frontiers, notes, watches, quality histograms and the request log |
| `jobs.db` | Challenges seen, how they were answered, and the corpus |
| `profiles/`, `auto_profiles/`, `sessions/` | Named profiles, the per-domain ones the ladder makes, and the one-fetch isolated ones |
| `models/` | Models you installed, which win over the embedded ones |
| `browser/` | Chrome for Testing, when `svipall browser install` put it there |
| `in/`, `out/` | Where `file://` reads from and a relative `out_file` lands |
| `screenshots/` | What `web_screenshot` wrote |

<details>
<summary><b>The full <code>config.toml</code></b></summary>

```toml
# Browser and tiers
browser_path = ""            # wins over everything when set and the file exists. Order after that:
                             # SVIPALL_BROWSER / CHROME_PATH / CHROME_BIN /
                             # PUPPETEER_EXECUTABLE_PATH, then the one `browser install` put in
                             # ~/.svipall/browser, then auto-detection
max_tier = "warm"            # cap for mode=auto
browser_timeout_ms = 45000
warm_wait_ms = 20000         # how long `warm` waits for a challenge to clear
browser_idle_secs = 180
warm_keep_max = 2            # cleared pages held open between fetches; 0 disables holding entirely
warm_keep_secs = 120         # how long a held page may go unused. Above the proof-of-work token
                             # lifetime and below browser_idle_secs, and a test asserts both
http_engine = "auto"         # the emulating engine when built with `impersonate`, else reqwest
http_firefox = false         # present Gecko coherently on the http tier: TLS, headers, UA, no Sec-CH-UA
http3 = false                # speak HTTP/3 to sites that advertised it. Needs `--features http3`;
                             # a first visit is TCP either way, because Alt-Svc is what turns it on

# Identity and exits
locale = ""                  # empty = follow the exit's declared country
timezone = ""
exit_strategy = "sticky"     # or round_robin, for domains with a pool of exits
reputation_budget = 250      # what one address may have outstanding with one host; 0 = off
reputation_half_life_hours = 6   # how long until half of what was spent stops counting
dns_over_https = ""          # e.g. https://dns.example/dns-query; empty = off, unnecessary behind a proxy

# Crawling and output
parallelism = 4              # web_fetch_many / web_crawl; tightened further by machine load
max_tokens_per_fetch = 25000
max_tokens_total = 60000     # cap across a whole crawl
overlap_blocks = 1           # blocks of the previous page a `cursor` continuation repeats; 0 = none

# Policy
allow_origins = []
block_origins = []           # blocking wins over allowing
refuse_private_addresses = false  # OFF by default, and not because the risk is small: fetching
                             # http://localhost is an ordinary thing for an operator to ask for.
                             # Turn it on for an installation where an agent picks its own URLs.
local_roots = []             # directories file:// may read; empty = ~/.svipall/in only
block_ads = false            # a real trade: pages whose third parties all fail load differently
blocklist_sources = [        # only fetched when block_ads = true
  "https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts",
  "https://easylist.to/easylist/easyprivacy.txt",
]

# Solver and dashboard
corpus_keep_days = 30        # how long solved captchas keep their images for export-corpus; 0 = none
solver_workers = 4
dashboard_port = 8787
dashboard_bind = "127.0.0.1" # set to a LAN address to answer challenges from a phone
log_level = "info"

# REST API
rest_port = 0                # 0 = off. `svipall serve` starts it anyway; this is what makes
                             # svipall-mcp mount it too. It grants everything the MCP tools do.
rest_bind = "127.0.0.1"
api_key = ""                 # empty = ~/.svipall/api_key, generated on first use and printed once
max_jobs = 2                 # long jobs at once — not `parallelism`, which bounds one job's fetches
```
</details>

| Env var | Default | Effect |
|---|---|---|
| `SVIPALL_HOME` | `~/.svipall` | Config, cache, profiles, models, blocklists |
| `SVIPALL_BROWSER` | — | Path to a browser binary. Also honoured: `CHROME_PATH`, `CHROME_BIN`, `PUPPETEER_EXECUTABLE_PATH` |
| `SVIPALL_HTTP_ENGINE` | `http_engine` | Which http engine runs; beats the config file |
| `SVIPALL_HUMAN_ASSIST` | on | Open a visible window when a token captcha cannot be auto-solved |
| `SVIPALL_HUMAN_WAIT_SECS` | 180 | How long that window waits |
| `SVIPALL_DASHBOARD_PORT` | `dashboard_port` | Port the human dashboard listens on |
| `SVIPALL_REST_PORT` | `rest_port` | Port the REST API listens on inside `svipall-mcp`. The Docker knob |
| `SVIPALL_API_KEY` | — | Pin the bearer key, for a container whose home is not writable |

---

## How Svipall compares

Read from each project's own README on **2026-09-05**. Feature sets in this space move fast, so check
theirs before relying on a row. A dash means *the project does not advertise it* — absence of a claim
is not proof of absence, and no row here is a measurement of somebody else's code.

| | Svipall | [Firecrawl](https://github.com/firecrawl/firecrawl) | [Crawl4AI](https://github.com/unclecode/crawl4ai) | [Scrapling](https://github.com/D4Vinci/Scrapling) | [Playwright MCP](https://github.com/microsoft/playwright-mcp) |
|---|---|---|---|---|---|
| Language / runtime | Rust, single binary | TypeScript (Node) | Python | Python | TypeScript (Node) |
| Usable with no account or API key | ✅ | self-host only; the hosted API and its MCP server take a key | ✅ | ✅ | ✅ |
| HTTP API, callable from any language | ✅ `svipall serve`, local, one endpoint per tool | ✅ | ✅ | — | — |
| Browser-grade TLS fingerprint on plain HTTP | ✅ (BoringSSL) | — | — | ✅ | — |
| HTTP/3 | ✅ opt-in, Chrome-shaped QUIC handshake **and** SETTINGS frame, asserted offline | — | — | ✅ | — |
| Tier escalation learned and remembered per domain | ✅ | — | proxy / fetcher escalation | session routing you declare | — |
| Captcha answering with no third-party service | ✅ local models + human dashboard | — | — | stealth clears Turnstile and interstitials; a sponsored paid token API for other vendors is listed in its README | — |
| Accessibility-tree snapshot for agents | ✅ | — | — | — | ✅ |
| The page's own JSON API, captured | ✅ | — | — | ✅ | — |
| Resumable crawls | ✅ | ✅ | ✅ | ✅ | — |
| Selectors relocated after a redesign | ✅ | — | — | ✅ | — |
| Page-quality label on every document | ✅ integrity, optimisation, substance, provenance | — | — | — | — |
| Boilerplate stripped using the site's *other* pages | ✅ opt-in, scored on TeCo, off by default and the README says why | — | — | — | — |
| Training corpus from every captcha solved | ✅ | — | — | — | — |
| Publishes its own anti-bot benchmark — three lists, failures and raw logs in-repo | ✅ | — | — | — | — |

Scrapling is the closest project to this one and has plenty Svipall
does not: remote browsers over CDP, Scrapy-style spiders with streaming and ready-made templates, a
much larger ecosystem and far more users. Firecrawl and Crawl4AI are the better fit if you want a
managed or containerised service with an HTTP API in front of it. Playwright MCP is the right choice
if all you need is a browser your agent can drive and you have no anti-bot problem at all.

The rows this project actually cares about are the last three: labelling what came back, answering
challenges without paying anyone, and publishing the number it gets rather than the number it would
like.

---

## Architecture

Nine crates — seven of our own and two vendored — about **62,000 lines of its own plus 47,000
vendored**.

| Crate | What it is |
|---|---|
| `svipall-core` | Classification, identity and fleet, quality (integrity, optimisation, substance, calibration, provenance, diversity), pdf/document, budget, robots, sitemaps, throttle, capacity, saturation, policy, blocklists, exits, reputation, growth, export, widgets, answers, watches, SQLite cache and crawl state |
| `svipall-extract` | The extraction engine — schema, induction, heal, tables, sanitize, prune, meta, signals. Deliberately **MIT OR Apache-2.0** and re-exported by `core` |
| `svipall-cdp` | **Vendored** chromiumoxide 0.7.0 (MIT OR Apache-2.0), with the automation residue patched out. All nine deviations from upstream are listed in `crates/svipall-cdp/PATCHES.md` |
| `svipall-quic` | **Vendored** quiche 0.24.9 (BSD-2-Clause), patched so its QUIC ClientHello and HTTP/3 SETTINGS are Chrome-shaped and so it links the BoringSSL the http tier already carries rather than a second copy. All eleven deviations in `crates/svipall-quic/PATCHES.md` |
| `svipall-http` | The http tier. `impersonate` (default) emulates Chrome or Firefox through BoringSSL; `--no-default-features` falls back to reqwest, which `web_status` names under `http_engine` and which refuses outright if the emulating engine was asked for by name; `http3` (opt-in) adds the QUIC engine |
| `svipall-models` | The embedded ONNX weights |
| `svipall-solver` | Captcha job store and HTTP API |
| `svipall-dashboard` | The human panel |
| `svipall-mcp` | **The product**: MCP tools, browser pool, strategy loop, the HTTP API (`rest`), the job runner (`jobs`) and the progress sink both report through. Ships `svipall-mcp` (server) and `svipall` (CLI, which is also `svipall serve`) |

Three invariants hold the whole thing together, and each is enforced by a test rather than by
convention:

1. **One identity profile** drives TLS, headers, CDP overrides, the stealth script and every worker
   realm — so a Chrome version is never stated in two places.
2. **One DOM parse per response.** You ask via `ParseWants` and read from `PageParts`; the benchmark
   asserts the count is exactly 1.
3. **Nothing is ever withheld.** `quality` labels a page, never filters one.

### Documentation

| | |
|---|---|
| [`docs/install.md`](docs/install.md) | Every way to install it, per platform, written so an AI agent can execute it; the MCP registration for every client; and the failures that actually happen, with what each one means |
| [`GET-STARTED.md`](GET-STARTED.md) | The same thing with nothing assumed, for somebody who has never installed anything from a terminal |
| [`docs/bench.md`](docs/bench.md) | Every benchmark mode, the three target lists, and the rule each number is read under |
| [`docs/extraction.md`](docs/extraction.md) | How a fetched page becomes the text a model reads, how good that is against three published extractors, and how to measure it yourself |
| [`docs/exits.md`](docs/exits.md) | Proxy pools: sticky vs round-robin, the health arithmetic, healing, `(domain, exit)` pacing, and the four leaks |
| [`docs/models.md`](docs/models.md) | Which models are embedded, the sidecar contracts, hot-swap, and corpus export |
| [`docs/firefox.md`](docs/firefox.md) | The Gecko identity that ships, and the measured reason the browser-tier fork is not built |
| [`docs/http3.md`](docs/http3.md) | The QUIC engine: why h3 was declined twice on reasons that did not hold, the offline Chrome capture it is measured against, and what is still not Chrome |
| [`docs/rest.md`](docs/rest.md) | The HTTP API: routes, status codes, the key and the two checks in front of it, and how a long crawl becomes a job you can follow, stop and resume |
| [`bench/baseline/README.md`](bench/baseline/README.md) | The measurement journal: every round, including the ones that improved nothing and the ones where a number went down |
| [`CHANGELOG.md`](CHANGELOG.md) | What 1.0 is, every gate it passes with its number, and what is still open |

---

## Development

### Build from source

Only worth it to contribute, or on a platform with no published build. Needs a Rust toolchain plus
`cmake`, `nasm`, `perl` and `llvm` (BoringSSL).

```bash
git clone https://github.com/ilien-dev/svipall
cd svipall
cargo build --release
./target/release/svipall browser install     # optional, recommended: a dedicated Chrome for Testing
```

Three things a source build does differently from a release one, and `svipall doctor` reports all
three:

- On Windows, set a short `CARGO_TARGET_DIR` (e.g. `C:\t`) first: BoringSSL's build paths run into
  `MAX_PATH` and the failure is an unhelpful cmake error.
- `.cargo/config.toml` sets `target-cpu=native`, so what `--release` produces is **for this machine
  only** and can die with an illegal instruction on another. Release artefacts use `--profile dist`
  with an explicit baseline; never ship what `--release` builds here.
- A clean clone carries no models, so image captchas go to the human dashboard rather than being
  answered. `tools/models/export.py` reproduces them; the release workflow and the `full` container
  image both run it.

No BoringSSL toolchain at all? `cargo build --release --no-default-features` builds without the
browser-grade TLS fingerprint, falling back to reqwest; `web_status` reports which engine is live
under `http_engine`, and asking for the emulating one explicitly on such a build is a **hard error
rather than a silent downgrade**, because a silent downgrade is exactly the failure that is hard to
notice.

### The gate

**TDD: a test before every behaviour change**, and it must fail without the change. `cargo test
--workspace` must be green — **1,143 tests pass today**, plus 16 ignored by default because they need
the network or a real browser, and more behind `--features http3`. Four of them run a real ONNX
Runtime session over hand-built fixture graphs, so the inference paths are executed rather than only
lint-checked.

```powershell
pwsh scripts/qc.ps1        # fmt, clippy -D warnings across the whole feature matrix, tests,
                           # unused deps, CLAUDE.md size guard, perf budgets, extraction
                           # floors, automation tells, identity coherence
pwsh scripts/qc.ps1 -Fix   # fmt + clippy --fix
```

`scripts/qc.sh` is the bash equivalent. **CI** runs fmt, clippy across the feature matrix (including
`--no-default-features` and `http3`), the full test suite, the ONNX model tests, `micro --assert`,
`fingerprint --engine chrome`, unused-dependency and file-size guards, the plugin manifests, both
installer scripts end to end, and a container build — on **Linux, Windows and macOS**, on every push
and pull request. Tagged releases build five targets, smoke-test each binary they are about to
publish, attach `sha256sums.txt` with a GitHub build attestation, and push both container images to
`ghcr.io`.

Two steps are the ones that keep this project honest, and **both run offline**: `tells --assert`
opens a page on loopback at all five browser passes and fails the build if anything of ours is
readable from it, and `fingerprint --engine chrome` checks every identity against itself. Neither can
be satisfied by argument. **`fingerprint --engine chrome` runs in both `qc` and CI; `tells --assert`
runs in `qc` only**, because it needs a provisioned browser the CI runners do not have — so it gates
every local change and is not a green tick on a pull request. The extraction floors are likewise a
`qc` step and skip themselves, loudly, on a machine without the corpora.

```
cargo run -p svipall-bench --release -- \
  micro [--assert] | tells [--assert] | fingerprint [--engine E] | extract [--corpus DIR] |
  evasion [--set hard12|public31|vendors8] [--runs N] [--exit URL] | h3 | h3-ref | cache
```

Contributions are taken under the **DCO** — no CLA, no copyright assignment. See
[`CONTRIBUTING.md`](CONTRIBUTING.md).

---

## FAQ

<details>
<summary><b>Do I need an API key, an account, or a subscription?</b></summary>

No. There is nothing to sign up for and nothing to pay: no scraping API, no captcha farm, no
geolocation lookup, no telemetry, no update check. Two outbound requests exist besides the pages you
asked for, and you have to ask for both — downloading Chrome for Testing, and fetching the ad
blocklists if you turn `block_ads` on. See [Privacy and safety](#privacy-and-safety).
</details>

<details>
<summary><b>Which platforms does it run on?</b></summary>

Windows, macOS and Linux. CI runs the whole gate on all three on every push, and tagged releases
attach binaries for **Windows x86-64, macOS Intel, macOS Apple silicon, Linux x86-64 and Linux
arm64**, with a `sha256sums.txt` and a build attestation. Install them with a one-line script, with
Homebrew, Scoop, winget or the AUR, from a `.deb` or `.rpm`, through npm, or as a container image on
`ghcr.io` — [docs/install.md](docs/install.md) has all of it.

Four honest gaps, and `svipall doctor` reports whichever applies to the machine it is on.
**Linux arm64 has no browser tiers**: Chrome for Testing publishes no linux-arm64 build, so that
artefact is the http tier unless you point `browser_path` at your own Chromium. **The Linux and
macOS Intel binaries carry no local captcha models**: the ONNX Runtime builds they would need
reference glibc 2.38, which would restrict the Linux binary to distributions newer than Debian 12,
and none is published for x86-64 macOS at all — so image challenges go to the human dashboard on
those, and the container image is where Linux keeps its models. **Windows arm64 has no build**; the
x64 one runs under emulation. On Windows, keep `CARGO_TARGET_DIR` short when building from source
— BoringSSL's paths run into `MAX_PATH`.
</details>

<details>
<summary><b>Can I use it without an AI agent?</b></summary>

Yes, two ways. `svipall` is a plain CLI that prints one JSON object per command, so `| jq` works;
and `svipall serve` puts the same nineteen tools behind a local REST API that any language can
drive. MCP is one front end of three, not the product.
</details>

<details>
<summary><b>Will it get my IP blocked?</b></summary>

It can, and the tool is built around that being the scarce resource. Every request is paced per
domain from that host's own latency and refusals, `Retry-After` is honoured up to two minutes, a
hard block puts the domain on a 15-minute cooldown, and a reputation ledger tracks what each address
has spent with each host and decays it with a six-hour half-life. Two crawls of the same site cannot
run at once for exactly this reason. None of that makes you invisible — this project's own benchmark
has watched a home address get worse at three targets over a day of runs, and
[published it](#anti-bot-vendors8-four-vendors-named-two-targets-each).
</details>

<details>
<summary><b>Does my data leave my machine?</b></summary>

No. Everything — the page cache, crawl state, cookies, profiles, models, the captcha corpus — lives
in `~/.svipall`. The dashboard binds to loopback unless you change it. There is no account, no sync
and nowhere for it to go.
</details>

<details>
<summary><b>Will it get past Cloudflare / DataDome / Akamai / PerimeterX?</b></summary>

Sometimes, and the [Proof](#proof-every-number-with-the-command-that-reproduces-it) section says
exactly which and how often, with the raw logs committed. Turnstile clears on every run of both
lists that carries it — 1.5–2.1 s on `hard12`, 1.7–2.4 s on `public31`. Cloudflare managed challenges are decided per visit and per address and flip in both directions.
DataDome returns a *hard block* for a home address — no challenge is ever offered — and no local tool
can change that; the answer there is `web_route` with an exit you supply. Anyone claiming a flat "yes"
to this question is not measuring.
</details>

<details>
<summary><b>Is it a Firecrawl / Crawl4AI / Scrapling / Playwright MCP replacement?</b></summary>

Sometimes, and the [comparison table](#how-svipall-compares) is honest about where it is not.
Firecrawl and Crawl4AI are the better fit for a managed or containerised service. Scrapling has a
larger ecosystem, remote browsers over CDP and Scrapy-style spiders. Playwright MCP is the right
choice when you have no anti-bot problem at all. Svipall's case is: local-only, no keys, a labelled
page instead of a raw one, captchas answered without paying anyone, and published numbers.
</details>

<details>
<summary><b>Is scraping legal?</b></summary>

That depends on the site, the data and where you are, and it is your call rather than this project's.
Svipall grants you **no authorisation with respect to any system you point it at** — read
[`DISCLAIMER.md`](DISCLAIMER.md) before you run it against something that is not yours. It evades bot
detection on public pages; it does not crack passwords, bypass paywalls or forge authentication.
</details>

<details>
<summary><b>Do I need a GPU?</b></summary>

No. Every model ships and runs on the CPU. A GPU matters for a different reason: a host with no
usable one reports a `SwiftShader` or `llvmpipe` WebGL renderer, which is the signature of a VM, and
no spoof fixes that. `web_status` tells you if you are in that case.
</details>

<details>
<summary><b>Why Rust?</b></summary>

One binary with no runtime to install, a page parsed in single-digit milliseconds, and BoringSSL —
the library Chrome itself uses — linked directly so the TLS fingerprint is the real thing rather than
an approximation.
</details>

<details>
<summary><b>How do I say it?</b></summary>

*SVEE-pahl.* See below.
</details>

---

## About the name

In the Old Norse poem *Grímnismál*, Odin lists the names he has travelled under, and one of them is
**Svipall** — "the changeable one", from *svipa*, to shift, to flash past. It is the name he uses
when he walks the world in a different shape each time, so that he can see everything and nobody sees
him coming.

That is exactly what this tool does. For every site it wears one coherent face: one machine, one
browser, one network fingerprint, one way of moving a pointer, all agreeing with each other, and a
different face the next time if the last one was remembered. It never stands still long enough to be
pinned down, and it looks at the whole web from wherever you run it.

---

## License

**AGPL-3.0-only.** Free to run, study, modify and share, for any purpose including commercial use.
Two obligations come with it: a fork stays under the same licence with its source published, and
anyone who offers Svipall to others **over a network** must publish the complete source of what they
run (section 13). See [`LICENSE`](LICENSE).

`crates/svipall-extract`, the extraction engine, is deliberately **MIT OR Apache-2.0** so that
anything can depend on it: a library nobody can use is a library nobody reads. `crates/svipall-cdp`
keeps its upstream terms (chromiumoxide, MIT OR Apache-2.0) and `crates/svipall-quic` keeps its own
(quiche, BSD-2-Clause); the default build links BoringSSL under an explicit AGPL section 7 linking
exception. These are set out in [`NOTICE`](NOTICE) and
[`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md).

## Trademark

The name **Svipall** and the Svipall logo are trademarks of the author. They are **not** licensed
under the AGPL, and nothing in this repository grants a licence to them.

The licence gives you the code. It does not give you the name. Run it, study it, modify it, fork it
and redistribute it freely under the AGPL — but distribute a modified version under a **different
name and without the logo**, so that nobody who downloads it is misled about who produced it or what
is in it.

Nominative use needs no permission and never did: saying that your project uses Svipall, works with
Svipall, or is a fork of Svipall is fine.

## Disclaimer

Svipall is provided **as is, with no warranty and no liability**, and it grants you **no
authorisation with respect to any system you point it at**. Complying with the law, with
data-protection rules and with a site's terms is the operator's responsibility, not the author's.
Capability is not permission — read [`DISCLAIMER.md`](DISCLAIMER.md) before you run it against
something that is not yours.

---

<p align="center">
  <sub>
    <b>Keywords:</b> web scraping · web crawler · MCP server · Model Context Protocol · Claude Code ·
    Claude Desktop · Cursor · AI agent tools · LLM web browsing · LLM-ready markdown ·
    html to markdown · RAG data pipeline · anti-bot bypass · Cloudflare bypass · Turnstile · Akamai ·
    PerimeterX · DataDome · Kasada · captcha solver · local captcha solving · headless browser ·
    stealth browser · browser fingerprint · TLS fingerprint JA4 · Chrome impersonation · HTTP/3 QUIC ·
    proxy rotation · Rust · Playwright alternative · Firecrawl alternative · Crawl4AI alternative ·
    Scrapling alternative · local-first · self-hosted · privacy · no API key
  </sub>
</p>
