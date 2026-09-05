<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="logo/svipall-lockup-dark.svg">
    <img src="logo/svipall-lockup.svg" alt="svipall" width="440">
  </picture>
</p>

<h3 align="center">A different face at every gate.</h3>

<p align="center">
  The whole web, readable by your AI agent — on your own machine.
</p>

<p align="center">
  <a href="https://www.rust-lang.org"><img alt="Rust" src="https://img.shields.io/badge/Rust-stable-A7472C?style=flat-square&labelColor=0B1A2B"></a>
  <a href="#license"><img alt="License: AGPL-3.0" src="https://img.shields.io/badge/License-AGPL--3.0-EAD9C4?style=flat-square&labelColor=0B1A2B"></a>
  <a href="https://modelcontextprotocol.io"><img alt="MCP" src="https://img.shields.io/badge/MCP-29%20tools-DF8D27?style=flat-square&labelColor=0B1A2B"></a>
  <a href="#development"><img alt="Tests" src="https://img.shields.io/badge/tests-1118%20passing-EAD9C4?style=flat-square&labelColor=0B1A2B"></a>
  <a href="#transparency-what-svipall-gets-past-and-what-it-does-not"><img alt="Benchmark" src="https://img.shields.io/badge/benchmark-published%2C%20failures%20included-A7472C?style=flat-square&labelColor=0B1A2B"></a>
</p>

---

**An MCP server and CLI, written in Rust, that gives Claude (or any LLM agent) a real window onto the internet: any page as clean Markdown, whole-site crawls, search without an API key, clicking and typing in a real browser, and the patience to get through the pages that check who is knocking. Everything runs on your own machine.**

No cloud. No API keys. No paid captcha services. No data leaves your computer.

---

## What is svipall?

svipall is a **web exploration toolkit for AI assistants**. You plug it into Claude Code, Claude Desktop, Cursor or any client that speaks the Model Context Protocol (MCP), and from that moment your assistant can browse the internet the way a person does: open a page, read what matters, follow links, fill a search box, wait for a "checking your browser" screen to clear, and hand you the result as text you can actually use.

If you are not technical, think of it like this: an AI model on its own is a very well-read person locked in a room with no window. svipall is the window — and a good one, because many websites slam the shutters when they notice a robot looking in. svipall is built to look, behave and wait like a real visitor, so the shutters mostly stay open.

If you are technical: it is a **Rust MCP server** — 29 MCP tools, nineteen HTTP routes, eight crates, about 62,000 lines of its own plus 47,000 vendored (a patched Chrome DevTools Protocol client and a patched QUIC/HTTP-3 stack) — with a tiered fetch ladder (`http → browser → stealth → real → warm`), a Chrome- or Firefox-accurate TLS/HTTP2 fingerprint via BoringSSL, a vendored Chrome DevTools Protocol client with the automation tells patched out, an opt-in HTTP/3 engine whose QUIC handshake is shaped like Chrome's, human-like pointer/keyboard/scroll behaviour, one-parse-per-page HTML extraction to LLM-ready Markdown, persistent crawls, a local captcha strategy engine, and a human-in-the-loop dashboard that works from a phone. It ships as an MCP binary, as a plain CLI that agents can call from a shell, and as a local REST API (`svipall serve`) that any language can drive.

## About the name

In the Old Norse poem *Grímnismál*, Odin lists the names he has travelled under, and one of them is **Svipall** — "the changeable one", from *svipa*, to shift, to flash past. It is the name he uses when he walks the world in a different shape each time, so that he can see everything and nobody sees him coming.

That is exactly what this tool does. For every site it wears one coherent face: one machine, one browser, one network fingerprint, one way of moving a pointer, all agreeing with each other, and a different face the next time if the last one was remembered. It never stands still long enough to be pinned down, and it looks at the whole web from wherever you run it. Say it *SVEE-pahl*.

## Who is it for?

| You are… | svipall gives you… |
|---|---|
| **A Claude Code / Claude Desktop user** who wants their assistant to research, read documentation, compare prices or monitor pages | A one-line MCP setup and tools the assistant picks on its own (`web_fetch`, `web_search`, `web_crawl`, `web_snapshot`, …) |
| **A developer building AI agents** (custom tool loops, autonomous research bots) | A local, deterministic, token-cheap web layer with structured output, file export and resumable crawls |
| **A data / growth / research person** who needs pages that sit behind "Just a moment…" walls | A fetch ladder that climbs to a real browser only when needed, and tells you honestly when a site cannot be opened from your address |
| **A privacy-conscious operator** | Zero third-party calls: no scraping API, no captcha farm, no geolocation lookup, no telemetry |
| **A security or QA engineer** testing your own site's bot defences | A reproducible benchmark (`svipall-bench`) whose raw run logs are committed in the repository, and a request log that says which tier answered and which wall appeared |

svipall is **not** a hosted scraping API and does not try to be one. If you want a URL you can `curl` from a serverless function, use a cloud service. If you want the web inside your own agent, on your own hardware, with nothing phoning home, this is that.

## Features

### Reading

- **LLM-ready Markdown from any URL** — main content only, boilerplate stripped, hidden text removed, optional JSON schema extraction, BM25 `query` filtering, pagination by `max_tokens` + `cursor`. A continuation names the whole heading path it resumes under (`Guide > Install > Windows`) and repeats the tail of the previous page, so a page read in parts is never picked up cold.
- **Tables as rows, documents as prose** — `tables=true` returns every data table as typed rows (CSV/JSON/JSONL to a file), and docx, xlsx, pptx, odt, epub, rtf, csv and pdf come back as markdown, from the web or from `file://` under a declared root. `raw:<html>` extracts markup you already have, with no request at all.
- **The site's own API, for free** — `web_capture` returns the JSON the page fetched while loading, so you page through `?page=2` instead of re-scraping HTML.
- **Selectors that survive a redesign** — what a `schema` finds is fingerprinted per domain; a selector the next redesign breaks is relocated by structural similarity and reported as `healed` with the selector to switch to. Never guessed: an ambiguous match is an error, not data.
- **A schema for a listing you have never seen** — `schema: "auto"` reads the page's own repeated structure, names the columns for what they hold (`title`, `url`, `price`, `date`, …) and returns the rows in `extracted` with the schema that produced them in `induced_schema`, to keep and pass back next time. No model, no API, one parse. A page with no clear record set returns neither: a candidate that does not clearly beat the runner-up is refused, and a field missing from a quarter of the records is dropped, because a guessed row is worse than no row and stays wrong quietly.
- **Feeds that load as you scroll** — `scroll="auto"` scrolls with wheel input until the document stops growing, clicks one "load more", and reads the whole listing.
- **Cheaper variants when you want them** — `mobile=true` (phone layout, usually far less navigation), `text_only=true` (skip images, fonts, media), `css_selector`, and a page cache that revalidates with `If-None-Match`, so a repeat visit costs a `304`.

### Getting in

- **Automatic anti-bot escalation** — plain HTTP first (about 100 ms), then headless Chromium, then a stealth-patched browser, then a headful "real" browser with a persistent per-domain profile, then a patient `warm` tier that answers challenges. Learned per domain and remembered between runs.
- **Firefox, coherently, on the http tier** — `http_firefox = true` and the http tier presents
  Gecko in TLS, headers and User-Agent *together*: Firefox's own emulation profile, its real header
  order with `User-Agent` first, its own `accept`, and **no `Sec-CH-UA` at all** — Firefox sends no
  client hints, and emitting one is the loudest way to be caught pretending. The browser tiers stay
  Chrome, because their protocol is Chrome's. See [`docs/firefox.md`](docs/firefox.md), which also
  sets out what a patched-Gecko browser engine would add and what it would cost.
- **Chrome-accurate network fingerprint** — JA4, HTTP/2 SETTINGS order, header order and GREASE all emulate current Chrome, post-quantum key share (`X25519MLKEM768`) included. 24 of 24 checks pass; the list is [below](#fingerprint--24-of-24).
- **Stealth that goes beyond `navigator.webdriver`** — coherent machine identities (screen, GPU, fonts, languages, timezone, memory, DPR), deterministic canvas/audio/text-geometry noise, WebRTC leak prevention behind proxies, no automation residue in the DevTools client.
- **Human-like behaviour** — Bézier pointer paths that land off-centre, typing cadence by digraph, wheel-notch scrolling with inertia, focus/visibility events, dwell time proportional to page length. Never a bare `click()`.
- **Sessions retired rather than reused** — a session is cookies + machine + exit. When a site turns on one, the profile is retired (the browser holding it closed first) and the next visit arrives as somebody else. `isolated=true` makes a profile that exists only for one fetch.
- **Pools of exits, used properly** — `web_route` takes several proxies per domain, each with its declared country; the domain keeps one (`sticky`) until it blocks it twice, then moves on, and a retired exit **heals with time** rather than staying dead. Pacing, strikes and latency are keyed by `(domain, exit)`, so a pool actually buys throughput instead of ten exits sharing one gap. **Authenticated proxies work on every tier**: `user:pass` goes to the browser over the protocol, never onto the command line, which is what Chrome cannot read from `--proxy-server`. The exit's locale and timezone travel with it, `dns_over_https` closes the DNS leak when there is no proxy to do it, and `web_route check` flags a `socks5://` that would resolve names on your machine.
- **Captcha answering, all local, models included** — nine automatic strategies (proof-of-work, press-and-hold, slide-puzzle, rotate-image, drag-piece, image-grid, image-segment, object-detect, audio-model), Turnstile / reCAPTCHA / hCaptcha token extraction from the real page, and a human dashboard for everything else. The release binary carries a detector and a segmenter (torchvision weights, CPU, no GPU needed), so an image grid is answered out of the box; a model you train from your own corpus, dropped in `~/.svipall/models/`, wins over the embedded one and is picked up without a restart. A challenge every model declines is parked, posted to the dashboard with its picture, and the person's answer is replayed on the live page. Fifteen widget families in one table, eleven answer modalities, plus a generic detector for widgets the table has never seen.

### Acting

- **Real browser interaction** — `web_snapshot` returns the page as roles, accessible names and short refs instead of markup; `web_act` clicks, types, fills, scrolls and waits on those refs. `browser_open` / `browser_do` keep a session alive across calls.
- **Search without an API key** — DuckDuckGo, Bing and Brave scraped directly, optionally merged by agreement.
- **A site's own search box** — `web_site_search` fills it once, learns the URL pattern it produces, and every later query is an ordinary fetch with no browser at all.

### Crawling

- **Crawls that survive interruption** — same-domain BFS or DFS, robots.txt, sitemaps and feeds, near-duplicate removal, `llms.txt` output, and a `crawl_id` you pass back to resume from the persisted frontier.
- **Crawls that know when to stop** — coverage of your query and novelty per page are measured lexically (no model, no download); a crawl that has stopped learning ends instead of spending the rest of its budget.
- **Crawls that only fetch what moved** — `--since-last` compares sitemap `<lastmod>` against the page cache. A URL with no `lastmod` is fetched, because silence is not "unchanged".
- **Politeness that adapts** — the gap between requests is tuned per domain from the host's own latency and refusals (100 ms–2 s on HTTP, 400 ms–5 s on browser tiers), `Retry-After` is honoured up to two minutes, and beyond that the domain gets a visible cooldown.
- **Concurrency sized to your machine** — browsers cost far more than HTTP requests, and a laptop asked to run six of them produces timeouts that look exactly like walls. Parallelism is tightened from core count and open browsers; it is never raised above what the config allows.
- **Page two, found** — a listing whose next page is a URL differing by a number is recognised from the URL alone, before the crawl decides it has finished.
- **Bulk output to files** — CSV, JSON, JSONL written to disk with `out_file`, so thousands of rows never pass through the model's context.

### Remembering, and staying safe

- **Memory across sessions** — `web_notes` key-value store, `web_watch` change monitoring (whole page or one CSS region), `web_diff`, and a queryable request log that says which tier answered and which wall appeared.
- **Secrets that never reach the model** — credentials referenced by name from `~/.svipall/secrets.env` and substituted on the way to the browser.
- **Origin policy** — allow/block lists, private-address refusal, and optional ad/tracker/consent-banner blocking with cached lists that degrade silently offline.
- **A CLI with the same brain** — `svipall fetch | crawl | search | snapshot | capture | map | watch | notes | route | profile | browser | status | log | serve | solver export-corpus`. Published as an Agent Skill (`skill/SKILL.md`) for agents that prefer a shell to a tool schema; a test keeps the skill in step with the CLI.
- **A local REST API for every other language** — `svipall serve` puts the same nineteen tools behind `POST /v1/fetch`, `/v1/crawl`, `/v1/search` …, on loopback, behind a bearer key it generates for you. Same objects the CLI prints and the model reads. [Details below](#the-rest-api).

## Quick start

### 1. Build

Requires a Rust toolchain plus `cmake`, `nasm`, `perl` and `llvm` (for BoringSSL).

```bash
git clone <this repository> svipall
cd svipall
cargo build --release
# No BoringSSL toolchain? This builds without the Chrome TLS fingerprint and says so at startup:
# cargo build --release --no-default-features
```

On Windows keep `CARGO_TARGET_DIR` short (MAX_PATH).

### 2. Get a browser (optional but recommended)

svipall auto-detects Chrome, Edge, Chromium or Brave. Browsers that ship their own anti-fingerprinting contradict the identity svipall presents and earn hard blocks, so the cleanest option is a dedicated Chrome for Testing:

```bash
./target/release/svipall browser install
```

### 3. Add it to Claude

**Claude Code**

```bash
claude mcp add svipall -- /absolute/path/to/target/release/svipall-mcp
```

**Claude Desktop / Cursor / any MCP client** — `claude_desktop_config.json` (or the client's equivalent):

```json
{
  "mcpServers": {
    "svipall": {
      "command": "/absolute/path/to/target/release/svipall-mcp"
    }
  }
}
```

That is the whole setup. Ask your assistant to "read this page", "find the pricing on that site" or "crawl these docs and summarise them" and it will pick the right tool. The server also serves the captcha dashboard on `http://localhost:8787/human` for anything that needs a human hand.

### 4. Or use it from a shell

```bash
svipall fetch https://example.com/article
svipall fetch https://shop.example/item --query "shipping costs"
svipall crawl https://docs.example/ --pages 50 --out pages.csv
svipall search "rust async runtime" --engine all
svipall snapshot https://news.ycombinator.com
svipall map https://example.com
svipall status
svipall log --summary
```

Every command prints one JSON object to stdout; diagnostics go to stderr, so `| jq` works.

### 5. Or run it in a container

```bash
docker build -t svipall .
claude mcp add svipall -- docker run -i --rm -v svipall-home:/data svipall
docker run --rm -v svipall-home:/data svipall svipall fetch https://example.com
```

The image carries both binaries and a Chrome for Testing of its own; everything it learns lives in the `svipall-home` volume. Publish `-p 8787:8787` to reach the dashboard. Release builds for Windows (x86-64), macOS (Intel and Apple silicon) and Linux (x86-64 and arm64) are attached to every tagged release with a `sha256sums.txt`, and the image is pushed to `ghcr.io`.

## How it works, in plain words

1. **Ask for the page the cheapest way first.** A direct HTTP request that looks exactly like Chrome's. Most pages stop here, in a tenth of a second.
2. **If the page needs JavaScript, open a browser.** Headless Chromium runs the scripts and hands back the rendered document.
3. **If the site checks for robots, wear a disguise.** The stealth tier patches every surface a bot-detection script inspects so the browser matches the identity the network layer already presented.
4. **If the site wants a real person, act like one.** The `real` tier is a visible-but-offscreen browser with a persistent profile, moving the pointer along curves and scrolling with a wheel.
5. **If there is a challenge, answer it or wait it out.** The `warm` tier runs the captcha strategy loop every turn — hold the button, solve the hash puzzle, drag the slider — and keeps perfectly still on self-verifying interstitials, because pointer activity there is exactly what gives a script away.
6. **Remember what worked.** The next request to that domain starts at the tier that succeeded.
7. **Tell the truth when it fails.** A blocked result carries `blocked_reason`, the wall kind, the widgets seen, and a note with the next move: `web_login` (do it by hand once, cookies are kept), `web_route` (send the domain through a proxy), or the captcha tools.

## Transparency: what svipall gets past, and what it does not

This project publishes its own benchmark and reports the number it gets, not the number it would like. The raw run logs and JSON are committed in [`bench/baseline/`](bench/baseline/), including the rounds of work that improved nothing.

```bash
cargo run -p svipall-bench --release -- fingerprint
cargo run -p svipall-bench --release -- evasion --set hard12   --runs 3
cargo run -p svipall-bench --release -- evasion --set public31 --runs 3
cargo run -p svipall-bench --release -- micro --assert
# What a real Chrome sends as HTTP/3 SETTINGS, read off a QUIC server this process runs on
# loopback. No network; needs the http3 feature and the provisioned browser.
cargo run -p svipall-bench --release --features http3 -- h3-ref
```

Every evasion figure is the **median of three runs with its range**, targets in a fresh random order each run, cooldowns cleared first, from **a single residential IP with no proxy**. A change counts as an improvement only when the median moves outside the previous range. The reputation spend is deliberately not cleared, and `bench evasion` refuses to start a list whose address has already spent past the line: the round below is what happens when a benchmark is allowed to burn the address it is measuring from.

**Measured — `public31` (2026-09-05): median 26/31 (range 25..26, up from 25..25), zero hard blocks; the one cell that moved is `indeed-jobs`, the flakiest on the list, and the entry beside it says so. `hard12` (2026-09-04): median 7/12 (range 7..8, down from 8..8). `vendors8` (2026-09-04): median 2/8 (range 2..2, down from 3..3). Fingerprint: 16/16, of which 7 are identity-coherence checks offline. Automation tells: 160/160 clean, offline and in the gate — thirty-two probes across five browser passes.**

Two of those three went down, and both losses are the same shape: `zillow` on `hard12` and the department-store target on `vendors8` are the two cells whose vendors score *addresses*, and this address ran the lists eight times in one day. Neither was retried until it came back. What the search for a cause turned up is in [`bench/baseline/README.md`](bench/baseline/README.md), including a defect it found in this round's own work and a suspicion that the re-measurement did not support.

Two things this section could not say before, and now can:

- **A third list, `vendors8` — 2 of 8** — two targets each behind the proof-of-work vendor, the
  edge vendor, the fingerprinting vendor and the managed challenge, with the vendors named.
  `hard12` and `public31` stay frozen, because a number only means something against its own list.
  It scores worse than `hard12`; that is the point of publishing it. The proof-of-work vendor is
  **passable** — twitch clears at the plain `browser` tier in under two seconds — and the target
  that does not pass fails in a way worth reading: 29 s, then 62 s, then a timeout across three
  runs minutes apart, which is the vendor making the puzzle harder for an address it has seen,
  exactly as its own documentation describes.
  It **went down by one this round**, and the cell is worth the paragraph: the edge vendor's
  department-store target now answers this address with `200` and its own error template — "Oops!!
  Something went wrong. Please refresh page", 206 characters — after a day of benchmark runs
  against it, and `403` with the same page over plain HTTP. That is a soft block wearing a success
  code. The number stays down rather than being retried until it comes back, and the finding it
  produced is the fix worth having: svipall was returning those 206 characters *as the page*, and
  now treats a short "something went wrong, please refresh" as the stand-in it is.
- **A proxied column** — `evasion --exit URL` runs the same targets through an exit you supply.
  Every figure above is from one residential address with no proxy, which this README has called
  its own ceiling for four rounds without measuring past it. Now both columns exist, so "svipall
  cannot do this" is separated from "this address cannot". The un-proxied column stays the
  headline, because it is the one anybody can reproduce without buying anything.

The proof-of-work vendor is the one wall that is not a page: no challenge, no widget, nothing to
answer. Its script earns a token that lives 60–180 seconds, so **passing it once is not passing
it** — a stateless HTTP client cannot hold it at all. The `warm` tier holds a live browser and
re-earns the token at 40 seconds, below the observed floor. That is a structural consequence of
running locally with a real browser, not a better spoof.

### Two lists, two rules, never one number

`hard12` is svipall's own list: twelve sites chosen *because* they have walls, scored by whether the expected text came back with no wall reported. `public31` is the list an independent benchmark published in May 2026 (seven stealth tools, 31 targets, 651 verdicts), scored with that benchmark's own four-way rule (`ok | gated | blocked | error`) ported verbatim into `bench/src/targets.rs` so a cell here means what a cell there means. Twenty-five of its 31 targets pass for every tool measured there, including unpatched automation; the signal lives in six cells.

A 9/12 and a 28/31 are therefore not the same kind of number, and quoting one against the other — in either direction — is reading noise as signal. Both are published, each with its list, so nobody has to.

### `hard12` — per target, over three runs

| Site | Protection | Passed | Tier that answered | Time |
|---|---|---|---|---|
| example.com | none | 3/3 | `http` | 0.0–0.2 s |
| en.wikipedia.org | none | 3/3 | `http` | 0.3 s |
| news.ycombinator.com | none, JS-light | 3/3 | `browser` | 1.6–1.8 s |
| nowsecure.nl | Cloudflare Turnstile | 3/3 | `browser` | 1.5–2.5 s |
| amazon.com search | Amazon's own detection + JS-rendered listings | 3/3 | `browser` | 2.4–4.0 s |
| newegg.com listing | Akamai Bot Manager | 3/3 | `browser` | 7.2–7.6 s |
| stackoverflow.com | Cloudflare (403 on plain HTTP) | 3/3 | `warm` | 1.9–9.6 s |
| zillow.com | PerimeterX / HUMAN "Press & Hold" | 3/3 | `warm` | 3.7–54.2 s |
| indeed.com | Cloudflare managed challenge | **1/3** | `real`, in the run that passed | 4.9 s |
| crunchbase.com | Cloudflare managed challenge | **0/3** | — | ~23 s to give up |
| g2.com | DataDome (`captcha-delivery.com`) | **0/3** | — | 25–32 s |
| idealista.com | DataDome (`captcha-delivery.com`) | **0/3** | — | 25–32 s |

Resolved by tier across the three runs: `browser` 12, `http` 6, `warm` 6, `real` 1.

Turnstile clears in under three seconds every run. The press-and-hold widget is held on its real iframe button, not the decoy `div` drawn over it, and the session that vendor once flagged is discarded instead of being carried into the next fetch — which is why zillow went from flaky to 3/3, and also why its time varies so much: the first hold sometimes fails and the retry runs on a fresh profile.

### The four svipall does not clear, and why

These are honest failures. Each was investigated by hand rather than retried, and each is decided on the server's side by something the tool cannot change from a single home connection.

| Site | What actually happens | Why svipall cannot fix it |
|---|---|---|
| **g2.com** | The interstitial carries the verdict `'t':'bv'` — *blocked visitor* — in the top document. That is a hard block, not a challenge; no slider is ever offered. | The verdict is **IP reputation**. It persisted with a clean Chrome for Testing, a fresh profile, a rotated machine identity and nine minutes of complete silence. The same address gets a *solvable* challenge (`'t':'fe'`) only over bare HTTP, which cannot run the widget. Only another exit address changes the answer. |
| **idealista.com** | Identical to g2: `'t':'bv'` on every browser tier. | Same vendor, same IP verdict. svipall recognises the hard block and reports it rather than spending the whole budget on it. |
| **crunchbase.com** | Passed at six seconds early in the day; refuses the same code, on a fresh profile wearing a different machine, after fifteen visits within the hour. | The decision is **per visit and per address**, taken server-side from traffic history. svipall behaves correctly on the page (keeps still, extends the wait when the page reports progress) and the outcome still depends on how the address has been scored that hour. |
| **indeed.com** | Cleared at the `real` tier in one run out of three; the other two sat at `warm` for half a minute on "Just a moment…". | The same shape as crunchbase. indeed and zillow have swapped places between measurement rounds: this is the noise band of a twelve-target list run from one address, not a code change. |

> Stated plainly: svipall's answer to all four is `web_route` — send the domain through a residential proxy you supply — and that is the one thing a local-only tool cannot provide for itself. It will never bundle proxies, never call a captcha farm, and never report a block as a success.

### `public31` — the independent list, its own rule

| | runs | median | range | `blocked` verdicts |
|---|---|---|---|---|
| `public31` | 25, 26, 26 | **26/31** | 25..26 | **0** |

Resolved by tier across the three runs: `http` 44, `real` 29, `warm` 4 — nearly half of that list never needs a browser at all. That histogram moves a lot between rounds (the previous one was `http` 36, `browser` 22, `warm` 15, `stealth` 2) and the reason is `domain_tiers.json`: the ladder starts each domain where it last succeeded, so the shape reflects what this machine has learned about these sites, not a change in the code. Cooldowns are cleared before a run; the learned tiers and the reputation spend deliberately are not, because both are the product's own state.

**Zero blocked is the number worth reading.** The benchmark this list comes from published a `blocked` column for all seven tools it measured, and only one of them reached zero:

| | OK | gated | **blocked** |
|---|---|---|---|
| nodriver | 28 | 3 | **0** |
| CloakBrowser | 26 | 3 | 2 |
| curl_cffi | 26 | 3 | 2 |
| Patchright | 25 | 3 | 3 |
| Camoufox | 25 | 3 | 3 |
| Playwright (vanilla) | 24 | 2 | 5 |
| rebrowser-playwright | 24 | 2 | 5 |
| **svipall** | **26** | 5 | **0** |

Different machine, different address, months apart — the OK counts are not comparable cell for cell and are not offered as if they were. The `blocked` column is the one that survives the comparison, because it is the failure that costs something: a gate is a retry, a hard block is an address the site has decided about.

The five that do not pass, and what each one actually is:

| Consistently gated | What it is |
|---|---|
| `bot.incolumitas.com`, `browserscan.net` bot page | Detection panels that score a visitor and print a verdict; there is no article to come back with. Gated for every tool the public benchmark measured, too |
| `sedarplus.ca` | WAF. Also gated for all seven |
| `medium.com`, `canadianinsider.com` | **Not walls.** Both answer `200` with their own titles and 45–50 KB of their own content. The public rule counts `cdn-cgi/challenge-platform` in the body as a gate, and every Cloudflare customer page carries that script whether or not a challenge was served |

`indeed.com` jobs used to be a sixth, and it is the cell that took this list from 25 to 26 — it now clears in **two runs of three** where it cleared in none. It is a real Cloudflare managed challenge and it is also the flakiest target here: this benchmark has watched it, `crunchbase` and `zillow` swap places across four separate rounds, decided per visit and per address on the server's side. What was different on the run that moved it is not the code but the address, which had been left alone for two hours. A number that moves when the address rests is a number about the address, and it is reported as an improvement only because the median left the previous range, which is this project's rule.

That last row is the ported rule being over-broad, measured directly rather than argued about. svipall's classifier is right and the imported one is wrong, and the cells are still reported as failures because the rule is the rule — moving a target or bending a scoring function to win two cells is how a benchmark stops meaning anything. What is *not* done is escalating those pages to a browser to satisfy it: opening a browser on a page already in hand would make the tool worse in exchange for a number.

`x.com/explore` moved from gated to passing this round, and for a reason worth stating: the impersonating HTTP engine followed no redirects at all. `wreq` follows none unless told to, `reqwest` was told to follow ten, and only the fallback engine had ever been configured — so on the build everybody ships, every URL that redirects came back as its 3xx stub. `x.com/explore` was returning seventy-four bytes reading "Found. Redirecting to /i/flow/login", and the classifier had no way to tell that from a page.

### Identity coherence — asserted offline, in CI

A fingerprint is rarely caught by one odd value. It is caught by a combination no real device
produces: a macOS user agent with a Windows GPU, a desktop with no taskbar, a Firefox emitting
Chrome's client hints. The leading patched-Firefox project names exactly this as the thing it keeps
getting wrong — not the spoofing technique, the coherence between spoofed values — because it has
no way to check a generated identity against itself before shipping it.

`cargo run -p svipall-bench --release -- fingerprint --engine chrome` checks all seven identities
svipall can wear (Chrome, Firefox and phone, across three operating systems) against themselves:
engine ↔ user agent, client hints ↔ engine, screen ↔ availHeight ↔ viewport, form factor ↔
platform, timezone ↔ language, renderer ↔ engine, and the macOS OS-token spelling that differs
between the two engines. No network, no browser, and it **fails the build** on a contradiction. It
runs in `qc` and in CI.
### Automation tells — 160 of 160, offline, in the gate

`fingerprint` asks public detectors what they see, which needs the network, which keeps it out of the build. So there is a second harness that asks the same question of a page the benchmark serves itself on loopback, at **all four browser tiers**, and **fails the build**:

```bash
cargo run -p svipall-bench --release -- tells --assert
```

Thirty-two probes per tier. **It opened at 22 of 52**, and the second round — eighteen probes added, and the two unasserted surfaces folded in — opened at 116 of 128. Every failure was a real contradiction that had been shipping, and two of them were things nobody had thought to look for until the harness printed them:

| Probe | What it caught |
|---|---|
| `residue` | `window.__svipall_console` — a ring buffer under a name that spelled out the product. Walking `window`'s own property names is the cheapest check a detector runs |
| `dom_rect` | `getBoundingClientRect` jittered `x`/`width`/`height` and not `left`/`right`/`top`/`bottom`, so every rectangle disagreed with its own arithmetic — and left three own properties on an object that has none |
| `host_object_brands` | `navigator.connection` and `performance.memory` replaced by object literals: `[object Object]` where `[object NetworkInformation]` belongs |
| `getter_names` | Accessors named `"get"`, where the engine names its own `get deviceMemory` |
| `languages_fresh` | One frozen array on every read, so `navigator.languages === navigator.languages` was true. It is false in every real browser |
| `languages_shape` | **`en;q=0.9` inside `navigator.languages`** — an `Accept-Language` header where a list of tags belongs. Not predicted |
| `screen_plausible` | Headless reports an 800×600 display while the flags size the window to 1366×768: a window wider than its screen. Not predicted |
| `screen_position` | `window.screenX = -32000` on the headful tiers — the offscreen parking position, readable by the page |
| `touch_matches_identity` | `maxTouchPoints = 1` and `ontouchstart` on a desktop identity |
| `scrollbar_present` | `--hide-scrollbars` made `innerWidth === clientWidth` on a page that must scroll |
| `worker_realm` | A worker reporting the host's real 32 cores beside a document reporting the identity's 8. One `postMessage` to catch |
| `runtime_domain_unobservable` | A watchdog, not a defect: it fires only if Chrome reopens the `Runtime.enable` console leak the CDP client's design rests on |

The second round asked what the first had not, and found six more:

| Probe | What it caught |
|---|---|
| `cross_realm_tostring` | A same-origin `about:blank` iframe is a realm of its own, so the `toString` mask — a registry of the functions *this* realm patched — had never seen the top realm's accessors. `iframe.contentWindow.Function.prototype.toString.call(getter)` returned `() => 8`: the patch and the value it hid, in one line, at all four tiers |
| `navigator_getters_are_native` | The script that keeps documents and workers agreeing had no mask at all, so on the `browser` tier and inside **every** worker the same call returned the source directly |
| `permission_state_is_valid` | `navigator.permissions.query` answered a `notifications` query with an object literal — `[object Object]` where `[object PermissionStatus]` belongs — carrying `Notification.permission`, whose third value is `default`, which is not a `PermissionState` at all. The unpatched tier in the same browser answered correctly on its own |
| `navigator_webdriver` | `navigator.webdriver` was deleted outright. Every Chrome since 89 carries the property and answers `false`, and the launch flags already delivered that — so the deletion was the only thing producing a navigator no real browser has |
| `iframe_realm_agrees` | `outerHeight` was defined over the top of the real value, and a spoof holds only in the realm that installed it. The window is sized to the viewport plus its chrome now, so every realm reads the same honest number and nothing is defined at all |
| `screen_plausible`, `window_chrome_height` | Both tightened to the rule `coherence` already applied offline, and both then failed on the `browser` tier: `availHeight == height` and `outerHeight - innerHeight == 0`. Neither was headless being candid — both came from a device-metrics override this code sends, with the available area and the window around it forgotten |

All thirty-two pass at all four tiers now. Two of the fixes are structural rather than cosmetic: the console ring is gone from the page entirely and comes from `Runtime.consoleAPICalled` on the protocol side, and workers are handed the identity in the window between attaching paused and resuming.

One fix was caught by the *older* harness while being written: shadowing `performance.memory` on the instance did nothing, because the property hands out a fresh object on every read. That is the argument for having both.


### Fingerprint — 24 of 24

Eight checks on the network layer against `tls.peet.ws`, sixteen inside a live browser. All pass.

| Layer | What is asserted |
|---|---|
| Network (8) | engine in use, negotiated `h2`, JA4 carries the h2 marker, Chrome-shaped cipher count (15, where rustls sends 20), Chrome-shaped extension count (16, where rustls sends 11), GREASE values present, User-Agent matches the emulation, `X25519MLKEM768` offered **with a key share** |
| Browser (16) | Runtime domain not observable via `console.debug` or `console.log`, `navigator.webdriver` gone, no `webdriver` accessor on the prototype, no duplicate navigator getters, no `navigator.brave`, plugins present, `userAgentData` present, `availHeight < height`, realistic window chrome, `navigator.languages` matches the identity, `navigator.connection` answers, heap limit is a platform ceiling rather than this machine's RAM, `devicePixelRatio` is the announced one, text geometry stable across reads, patched functions report native code |

The stability check on text geometry is there because it caught a real bug: the noise patch was redrawing per call, so one element measured twice gave two different widths — a tell no real browser has.

An earlier version of this file called the post-quantum key share a known gap. It was not: the check was looking for it in `ja4_r`, which lists ciphers, extensions and signature algorithms and never supported groups. The engine had been offering it all along; the check now reads `supported_groups` and `key_share`.

### CPU budgets — measured, not recalled

`cargo run -p svipall-bench --release -- micro` on a 195 KB generated news page. Timing budgets carry 25% headroom; the structural checks are exact and cannot flake.

| Check | Measured | Budget |
|---|---|---|
| `classify` a 200 KB page | 161 µs | 400 µs |
| `parse_page`, text + title | 2.29 ms | 14 ms |
| `parse_page`, everything | 4.28 ms | 20 ms |
| `bm25_filter`, full page | 1.79 ms | 3 ms |
| `budget::take`, full page | 247 µs | 4 ms |
| `simhash`, full page | 706 µs | 5 ms |
| **DOM parses** for text + title + markdown + links + metadata | **1** | exactly 1 |
| **Disk reads** across 10,000 domain-state lookups | **0** | exactly 0 |

The fixture is generated from a fixed seed rather than checked in, so two machines measure the same document.

### Limits by design

Beyond those four targets, there are things svipall does not do. Most are permanent and on purpose; one is a build you have to ask for, and it says so.

- **No proxies, no IP rotation service.** You bring your own exit; svipall keeps timezone, locale and languages consistent with it and closes the WebRTC and DNS leaks. It does not detect the proxy's country (that would require a geolocation service), so you declare it.
- **No paid or remote captcha solving.** The release binary carries a detector and a segmenter
  (torchvision weights, BSD-3, exported by `tools/models/export.py`) and runs them on the CPU, so
  image challenges are answered out of the box with nothing to install and nothing fetched at run
  time. OCR, audio and a fine-tuned tile classifier are still yours to train — and a model you drop
  in `~/.svipall/models/` wins over the embedded one, without a restart. What no model can answer is
  parked and handed to a person at the dashboard, whose answer is replayed on the live page.
  Solving quality is bounded by the models and your hands, never by a vendor's quota.
- **HTTP/3 is off by default, and that is a build choice, not a limit.** It works: a vendored quiche on the same BoringSSL the http tier already links, emitting Chrome's QUIC ClientHello — ALPS 17613, ECH GREASE, `compress_certificate`, `trust_anchors`, extension permutation, a GREASE transport parameter — all asserted offline against a capture of a real Chrome. It is off because a QUIC stack is 37,000 vendored lines to carry for a transport most sites still do not offer, and because it can only ever be a *second* visit: `Alt-Svc` is how a site says it speaks h3, so the first fetch of any domain is TCP exactly as before. Build with `--features http3` and set `http3 = true`. What is not yet Chrome's: the `trust_anchors` payload is empty where Chrome sends a list, and one extension Chrome sends (`0x12e0`) is not in this BoringSSL at all. The HTTP/3 SETTINGS frame **is** Chrome's now — measured by `bench h3-ref`, which runs a loopback QUIC server a real Chrome handshakes with, and asserted offline. So an h3 engine carries a Chrome version ceiling of its own, set by the age of the linked BoringSSL. Measured on `hard12`: four of twelve targets advertise h3 at all, and the evasion median does not move — 8/12 either way, against a noise floor of 123–369s per run. What does move is cost: on a site that offers h3 and walls the cheap tier over TCP, a page arrives in **950 ms at the http tier instead of 2967 ms with a browser**, five runs each, and the worst case for a site that advertises h3 and does not deliver is one extra 568 ms, once per domain per six hours. [`docs/http3.md`](docs/http3.md) has the whole record, including the two reasons this project previously gave for not doing it and why both were wrong.
- **No breaking of access controls.** svipall evades bot detection on public pages. It does not crack passwords, bypass paywalls, or forge authentication. A login wall is passed by *you*, once, in a visible window, and the cookies are kept.
- **A browser with built-in anti-fingerprinting** (Brave, hardened Firefox) contradicts the Chrome identity svipall presents and earns hard blocks. So does a browser left several majors behind the stable channel, since the user agent then names a Chrome that no longer exists in the wild. `web_status` and the note on any blocked page say when either is true, and `browser_setup` installs or updates a dedicated Chrome for Testing.
- **A host with no usable GPU is a fingerprint problem no spoof can fix.** Without acceleration the
  WebGL renderer reads `SwiftShader` or `llvmpipe`, which is the signature of a server or a VM.
  Claiming a GPU the machine does not have is caught by the rendered-image hash, which a spoof
  cannot reproduce without owning that hardware. svipall detects the case and says so in
  `web_status` rather than pretending otherwise. Everything else — the models included — runs fine
  on CPU; this is about what a site sees, not about speed.
- **A local security product that injects into pages is visible to every site.** On one measurement round, an antivirus was injecting a stylesheet and a script into every page, and on a vendor's silent device-check page the only traffic the page made went to that product. svipall names this in the note of a blocked result and says what to exclude; it cannot remove it for you.

## MCP tools

Twenty-nine tools, all local.

| Tool | What it does |
|---|---|
| `web_fetch` | Fetch a page as Markdown or structured JSON. `mode=auto` climbs the ladder. `schema` (self-healing), `tables`, `scroll`, `query`, `max_tokens`/`cursor`, `cache`, `include_metadata`, `include_links`, `robots`, `out_file`, `mobile`, `text_only`, `isolated`, `method`/`body`/`headers`. URLs may be `raw:<html>` or `file://` under `local_roots` |
| `web_fetch_many` | Bounded-parallel fetch of many URLs, order preserved |
| `web_search` | DuckDuckGo / Bing / Brave without an API key; `engine="all"` merges by agreement |
| `web_site_search` | Use a site's own search box, learn its URL pattern, then every later query is a plain fetch |
| `web_crawl` | Same-domain crawl with robots.txt, dedup, boilerplate removal, `strategy=dfs`, `scroll`, `llms.txt`, file export, a saturation stop, and a `crawl_id` to resume |
| `web_map` | A site's URLs from robots.txt, sitemaps and feeds in a few hundred tokens |
| `web_snapshot` | The page as roles, accessible names and short refs that `web_act` accepts. Deterministic, no vision model |
| `web_act` | click, type, fill, press, hover, select, scroll, wait, eval, goto, screenshot, hold, verify, console — all through human-like input |
| `web_capture` | The JSON/XHR responses the page fetched while loading: the site's real API |
| `browser_open` / `browser_do` / `browser_close` | Persistent session with cookies and page state across calls |
| `web_screenshot` | PNG of the rendered page |
| `web_diff` | What changed on a page since svipall last saw it |
| `web_watch` | Monitor a page, or one `css_selector` region of it, on a schedule and report changes; the region survives a redesign |
| `web_notes` | Key-value memory that outlives the session |
| `web_log` | Which tier answered, which wall appeared, how long it took, per domain |
| `web_login` | Visible window for a manual login or challenge; cookies saved to a profile |
| `web_route` | Per-domain proxy, or a pool of `proxies` with `countries`; subdomains inherit; `exit_strategy` sticky or round-robin; `check=true` tests the exits (liveness, latency, DNS leak) with no third-party service |
| `web_profile` | Export/import an encrypted browser profile between machines |
| `web_status` | Learned tiers, cooldowns, routes, per-exit health and latency, profiles, open browsers, solver stats, which models answer and from where, whether the host has a real GPU, `h3_offered_by` |
| `browser_setup` | Download or manage Chrome for Testing |
| `solve_and_continue` | Solve the captcha **on the blocked page** and return what is behind it |
| `solve_image_captcha` / `solve_recaptcha_v2` / `solve_turnstile` / `solve_hcaptcha` / `captcha_status` / `report_captcha` | Local captcha solving with the classic `in.php` / `res.php` / `createTask` / `getTaskResult` HTTP shape, so existing clients work |

## Captcha solving (fully local)

Six automatic strategies, ordered on each page by what has actually worked on that domain before. A strategy that declines costs no attempt.

| Challenge | How it is solved | Fallback |
|---|---|---|
| Turnstile, reCAPTCHA v2/v3, hCaptcha | The real page loads in a stealth browser and the token is read when the widget clears | A visible window opens for a person (`SVIPALL_HUMAN_ASSIST=0` to disable) |
| Proof of work (hash puzzles) | Computed locally; always succeeds. One attempt, because a nonce either verifies or was misparsed | — |
| Press and hold | Held on the real iframe button for the measured interval, with a real approach and press; two attempts, then one retry on a fresh profile — and the flagged profile is retired | Visible window |
| Slider / rotation | Classical vision on a screenshot: cross-correlation for the notch, edge-energy minimisation for the angle. Three attempts, since both have a tolerance | Human dashboard |
| Self-verifying interstitial ("Just a moment") | Nothing — the tool keeps still and extends the wait once when the page reports progress | Visible window |
| Image-to-text | Local OCR (`--features onnx-ocr`, CRNN/CTC model in `~/.svipall/models/`) | Dashboard shows the image |
| Image grid ("select all…") | Local classifier (`--features onnx-grid`), tiles clicked as real pointer input, two attempts | Zero-shot pair (`--features onnx-zeroshot`) when the subject is unknown, else visible window |
| "Click on the …" / "draw a box around the …" | Local detector (`--features onnx-detect`), centres clicked or the strongest box traced, as fractions of the picture | Dashboard (two taps make a rectangle) |
| Audio | Local acoustic model (`--features onnx-audio`), clip fetched from inside the page, decoded in pure Rust | Dashboard plays the clip |
| Anything else | Recognised from the widget table — fifteen widget families, eleven answer modalities — or by the generic detector when it is a widget the table has never seen, then routed by modality | Dashboard |

Widgets are named by the host their challenge endpoint lives on, which is the stable, factual name for a protocol. Eight of the fifteen families need no model file at all, because their modality (proof-of-work, slide, rotate, hold, drag) is arithmetic and image geometry; three of those are proof-of-work schemes that always succeed. A conformance test walks the table and fails if a row has no fixture or names a modality nothing can answer, which is what keeps "adding a widget is adding a row" true rather than aspirational.

The dashboard at `http://localhost:8787/human` (and on your LAN address when `dashboard_bind` is not loopback) has one renderer per modality and works from a phone. Every coordinate it sends is a fraction of the image, never a pixel, so a 1280-wide challenge on a 390-wide screen is answered correctly. Unsolved jobs expire after 30 minutes.

A subject a grid model does not know is never guessed at — wrong tiles spend the attempt and confirm what we are — so it goes straight to a person.

### Model files (all optional)

```bash
cargo build --release --features onnx-ocr,onnx-grid,onnx-audio,onnx-detect,onnx-zeroshot
```

| Feature | Files in `~/.svipall/models/` | Contract |
|---|---|---|
| `onnx-ocr` | `captcha.onnx`, `captcha.json` | `{"height":32,"width":128,"channels":1,"normalize":true,"charset":"-0123…z"}` (`charset[0]` = CTC blank) |
| `onnx-grid` | `grid.onnx`, `grid.json` | `{"height":224,"width":224,"channels":3,"normalize":true,"threshold":0.5,"classes":[…]}` |
| `onnx-audio` | `audio.onnx`, `audio.json` | `{"sample_rate":8000,"n_fft":256,"hop":128,"n_mels":40,"charset":"-0123456789"}` |
| `onnx-detect` | `detect.onnx`, `detect.json` | `{"height":320,"width":320,"channels":3,"normalize":true,"threshold":0.4,"iou":0.5,"classes":[…]}`; output `[1, 4+classes, N]` or `[1, N, 4+classes]` |
| `onnx-zeroshot` | `clip_image.onnx`, `clip_text.onnx`, `clip.json`, `vocab.json`, `merges.txt` | `{"height":224,"width":224,"mean":[…],"std":[…],"vocab":"vocab.json","merges":"merges.txt","context":77,"margin":0.15}` |

A detector output whose class axis does not equal `4 + classes.len()` is refused, not reshaped.

Every challenge answered — by a model, by zero-shot or by a person — stays in the local corpus for `corpus_keep_days` (30). `svipall solver export-corpus --out ./corpus` writes the images and a `manifest.jsonl` with prompt, answer, who answered and whether the page accepted it: training data for your own models. Rows with `"source":"human","ok":true` are labelled by a person *and* verified by the site. [`docs/models.md`](docs/models.md) has the full contracts.

## The REST API

The same server, over HTTP, so any language can drive it — not only an MCP client or a shell.

```bash
svipall serve --port 8788        # the bearer key is printed once, and kept in ~/.svipall/api_key
curl -sH "Authorization: Bearer $KEY" -H 'content-type: application/json' \
     -d '{"url":"https://example.com","query":"pricing"}' localhost:8788/v1/fetch
```

`svipall-mcp` mounts the same router when `rest_port` is set, on its own listener, sharing one browser pool, one page cache and one set of learned tiers with the MCP tools.

**Nineteen routes, one per tool**, each taking that tool's own JSON as the body:

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

**A blocked page is a `200`.** The call ran; the *page* did not. `blocked_reason`, `wall_kind` and `note` are in the body, exactly as they are over MCP. Only a malformed body (`400`), a bad key (`401`), a browser `Origin` or a rebound `Host` (`403`), a body over 2 MB (`413`) or a broken installation (`500`) is not a `2xx`. A client that read a wall as a `5xx` would retry forever against something that is never going to move.

**Every route needs the key, including on loopback.** A local port is not a boundary: svipall carries logged-in profiles, cookies and your exit address, so an open one is a proxy wearing your identity. Two more checks sit in front of the key, because binding to `127.0.0.1` does not stop a page in your own browser being served a DNS answer of `127.0.0.1` and posting to it: any request carrying an `Origin` header is refused, and on a loopback bind so is any `Host` that is not loopback. There is no CORS layer and there will not be one — no browser page is a client of this API.

Four tools are deliberately **not** routes, and `rest.rs` records why next to each: `browser_open`/`browser_do`/`browser_close`, because a session is a resource HTTP cannot bound and a client that dies between open and close leaks a real browser; `web_login`, because an HTTP request must not make a window appear on your desktop and hold the connection for an hour; and the `solve_*` token family, which already answers on the dashboard port in the 2captcha wire shape. Unknown body fields are ignored rather than rejected, so an MCP client and an HTTP one behave the same way — a typo'd field is silent.

**A long crawl is a job, not a held connection.** `"async": true` answers `202` with an id; `GET /v1/jobs/{id}` polls it, `GET /v1/jobs/{id}/stream` follows it as Server-Sent Events, `DELETE` stops it. The id **is** the `crawl_id`, so there is one handle to learn and resuming is `{"crawl_id": "…"}` — the same word the MCP tool and the CLI already use. A cancelled crawl stops between pages *after* that page's links are queued, so its frontier is kept and the id can be picked up again; it is never aborted, because that would leak a browser page. A job whose process was killed becomes `interrupted` — the fact `crawl.status` could never state, since it is written `running` per batch and never cleared — and `interrupted` is resumable. The first frame of a stream is always a snapshot from the store, so a subscriber that joins at page forty is never told the job started at zero. And a queued job whose site already has one running is held back: two crawls of one site would spend one address's reputation with that host twice as fast, which is the scarcest thing a local-only tool has. [`docs/rest.md`](docs/rest.md) has the whole contract.

## Safety and privacy

- **Prompt-injection defence.** Text a person cannot see (`display:none`, `opacity:0`, off-screen, zero-width characters) is removed before the content reaches the model. The rules are deliberately narrow; half the tests guard against false positives.
- **Credentials never enter the context.** `{"do":"type","ref":"e4","text":"${SHOP_PASSWORD}"}` is substituted from `~/.svipall/secrets.env` on the way to the browser. `web_status` lists names, never values.
- **Origin policy, checked before the request.** In `~/.svipall/config.toml`: `allow_origins`, `block_origins` (blocking wins), `refuse_private_addresses` (stops an agent following a link to `169.254.169.254`), and `block_ads` (cached lists, silent when offline).
- **robots.txt** is reported by default and can be made binding with `robots=obey`.
- **No telemetry, no update checks, no background network.** The only outbound traffic is the pages you asked for and, once, the blocklists if you enabled them.

## Configuration

`~/.svipall/config.toml` (or `$SVIPALL_HOME/config.toml`). Every field has a default, so a missing or partial file is fine.

Everything svipall remembers lives in that one directory, and deleting any of it costs memory rather than function:

| | |
|---|---|
| `config.toml` | The settings below |
| `secrets.env` | Credentials referenced by name; the values never enter a tool call |
| `domain_tiers.json` | Which tier answered for each domain, so the next fetch starts there |
| `pools.json`, `exit_health.json` | Exits per domain, and what each one has done on each |
| `reputation.json` | What each address has spent with each host, decaying with a half-life |
| `svipall.db` | Page cache, crawl frontiers, notes, watches and the request log |
| `jobs.db` | Challenges seen, how they were answered, and the corpus |
| `profiles/`, `auto_profiles/`, `sessions/` | Named profiles, the per-domain ones the ladder makes, and the one-fetch isolated ones |
| `models/` | Models you installed, which win over the embedded ones |
| `browser/` | Chrome for Testing, when `svipall browser install` put it there |
| `in/`, `out/` | Where `file://` reads from and a relative `out_file` lands |
| `screenshots/` | What `web_screenshot` wrote |

```toml
# Browser and tiers
browser_path = ""            # empty = auto-detect; `svipall browser install` is preferred when present
max_tier = "warm"            # cap for mode=auto
browser_timeout_ms = 45000
warm_wait_ms = 20000         # how long `warm` waits for a challenge to clear
browser_idle_secs = 180
http_engine = ""             # empty = the emulating engine when built with `impersonate`
http3 = false                # speak HTTP/3 to sites that advertised it. Needs `--features http3`;
                             # a first visit is TCP either way, because Alt-Svc is what turns it on

# Identity and exits
locale = ""                  # empty = follow the exit's declared country
timezone = ""
exit_strategy = "sticky"     # or round_robin, for domains with a pool of exits
reputation_budget = 250      # what one address may have outstanding with one host; 0 = off
reputation_half_life_hours = 6   # how long until half of what was spent stops counting
dns_over_https = ""          # e.g. https://dns.example/dns-query; empty = off, and unnecessary behind a proxy

# Crawling and output
parallelism = 4              # web_fetch_many / web_crawl; tightened further by machine load
max_tokens_per_fetch = 25000
max_tokens_total = 0         # 0 = no session cap
overlap_blocks = 1           # blocks of the previous page a `cursor` continuation repeats; 0 = none

# Policy
allow_origins = []
block_origins = []           # blocking wins over allowing
refuse_private_addresses = true
local_roots = []             # directories file:// may read; empty = ~/.svipall/in only
block_ads = false            # a real trade: pages whose third parties all fail load differently
blocklist_sources = []

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

| Env var | Default | Effect |
|---|---|---|
| `SVIPALL_HOME` | `~/.svipall` | Config, cache, profiles, models, blocklists |
| `SVIPALL_HUMAN_ASSIST` | on | Open a visible window when a token captcha cannot be auto-solved |
| `SVIPALL_HUMAN_WAIT_SECS` | 180 | How long that window waits |
| `SVIPALL_REST_PORT` | `rest_port` | Port the REST API listens on inside `svipall-mcp`. The Docker knob |
| `SVIPALL_API_KEY` | — | Pin the bearer key, for a container whose home is not writable |

## How svipall compares

Read from each project's own README on **2026-09-03**. Feature sets in this space move fast, so check theirs before relying on a row. A dash means *the project does not advertise it* — absence of a claim is not proof of absence.

| | svipall | [Firecrawl](https://github.com/firecrawl/firecrawl) | [Crawl4AI](https://github.com/unclecode/crawl4ai) | [Scrapling](https://github.com/D4Vinci/Scrapling) | [Playwright MCP](https://github.com/microsoft/playwright-mcp) |
|---|---|---|---|---|---|
| Language / runtime | Rust, single binary | TypeScript (Node) | Python | Python | TypeScript (Node) |
| Usable with no account or API key | ✅ | self-host only; the hosted API and its MCP server take a key | ✅ | ✅ | ✅ |
| HTTP API, callable from any language | ✅ `svipall serve`, local, one endpoint per tool | ✅ | ✅ | — | — |
| Browser-grade TLS fingerprint on plain HTTP | ✅ (BoringSSL) | — | — | ✅ | — |
| HTTP/3 | ✅ opt-in (`--features http3`), Chrome-shaped QUIC handshake, asserted offline | — | — | ✅ | — |
| Tier escalation learned and remembered per domain | ✅ | — | proxy / fetcher escalation | session routing you declare | — |
| Captcha answering with no third-party service | ✅ local models + human dashboard | — | — | stealth clears Turnstile and interstitials; its README points at a paid token API for the rest | — |
| Accessibility-tree snapshot for agents | ✅ | — | — | — | ✅ |
| The page's own JSON API, captured | ✅ | — | — | ✅ | — |
| Resumable crawls | ✅ | ✅ | ✅ | ✅ | — |
| Selectors relocated after a redesign | ✅ | — | — | ✅ | — |
| Training corpus from every captcha solved | ✅ | — | — | — | — |
| Publishes its own anti-bot benchmark — two lists, failures and raw logs in-repo | ✅ | — | — | — | — |

**Where the overlap is real.** Scrapling is the closest project to this one and has plenty svipall does not: remote browsers over CDP, Scrapy-style spiders with streaming and ready-made templates, a much larger ecosystem and far more users. Firecrawl and Crawl4AI are the better fit if you want a managed or containerised service with an HTTP API in front of it. Playwright MCP is the right choice if all you need is a browser your agent can drive and you have no anti-bot problem at all.

The rows this project actually cares about are the last two: answering challenges without paying anyone, and publishing the number it gets rather than the number it would like.

## Architecture

- `svipall-core` — classification, identity and fleet, extraction (schema, heal, tables, sanitize, prune), pdf/document, budget, robots, sitemaps, throttle, capacity, saturation, policy, blocklists, exits, growth, export, widgets, watches, SQLite cache and crawl state.
- `svipall-cdp` — vendored Chrome DevTools Protocol client; every deviation from upstream is listed in `crates/svipall-cdp/PATCHES.md`.
- `svipall-quic` — vendored quiche, patched so its QUIC ClientHello is Chrome's and so it links against the BoringSSL the http tier already carries rather than a second copy; every deviation is in `crates/svipall-quic/PATCHES.md`.
- `svipall-http` — the http tier; `impersonate` (default) emulates Chrome through BoringSSL, `--no-default-features` falls back to reqwest and says so at startup, and `http3` (opt-in) adds the QUIC engine for sites that advertised it.
- `svipall-solver` — captcha job store and HTTP API. `svipall-dashboard` — the human panel.
- `svipall-mcp` — the product: MCP tools, browser pool, strategy loop (the ladder itself lives in `svipall-core`), the HTTP API (`rest`), the job runner (`jobs`) and the progress sink both of them report through (`progress`). Ships `svipall-mcp` (server) and `svipall` (CLI, which is also `svipall serve`).

One identity profile drives TLS, headers, CDP overrides and the stealth script, so a Chrome version is never stated in two places. One DOM parse per response, asserted by the benchmark. All pointer, keyboard and wheel input goes through the behaviour module.
### Documentation

| | |
|---|---|
| [`docs/bench.md`](docs/bench.md) | The six benchmark modes, the three target lists, and the rule every evasion number is read under |
| [`docs/exits.md`](docs/exits.md) | Proxy pools: sticky vs round-robin, the health arithmetic, healing, `(domain, exit)` pacing, and the four leaks |
| [`docs/extraction.md`](docs/extraction.md) | How a fetched page becomes the text a model reads, how good that is, and how to measure it yourself |
| [`docs/models.md`](docs/models.md) | Which models are embedded, the sidecar contracts, hot-swap, and corpus export |
| [`docs/firefox.md`](docs/firefox.md) | The Gecko identity that ships, and the measured reason the browser-tier fork is not built |
| [`docs/http3.md`](docs/http3.md) | The QUIC engine: why h3 was declined twice on reasons that did not hold, the offline Chrome capture it is measured against, and what is still not Chrome |
| [`docs/rest.md`](docs/rest.md) | The HTTP API: routes, what a status code means, the key and the two checks in front of it, and how a long crawl becomes a job you can follow, stop and resume |
| [`CHANGELOG.md`](CHANGELOG.md) | What 1.0 is, every gate it passes with its number, and what is still open |


## Development

TDD: a test before every behaviour change; `cargo test --workspace` must be green. **1,118 tests pass today**, plus 16 that are ignored by default because they need the network or a real browser, and four more behind `--features http3`. Four of them run a real ONNX Runtime session over hand-built fixture graphs, so the inference paths are executed rather than only lint-checked.

```powershell
pwsh scripts/qc.ps1        # fmt, clippy -D warnings across the onnx feature matrix, tests,
                           # unused deps, CLAUDE.md size guard, perf budgets, extraction
                           # floors, automation tells, identity coherence
pwsh scripts/qc.ps1 -Fix
```

`scripts/qc.sh` is the bash equivalent. CI runs the same gate on Linux, Windows and macOS, plus `--no-default-features` and a container build, on every push and pull request. Tagged releases build five targets, publish `sha256sums.txt` and push the image to `ghcr.io`.

Two of the gate's steps are the ones that keep this project honest, and both run offline: `tells --assert` opens a page on loopback at every browser tier and fails the build if anything of ours is readable from it, and `fingerprint --engine chrome` checks every identity against itself. Neither can be satisfied by argument.

Benchmarks: `cargo run -p svipall-bench --release -- micro [--assert] | tells [--assert] | fingerprint [--engine E] | evasion [--set hard12|public31|vendors8] [--runs N] [--exit URL] | extract [--corpus DIR]`. [`docs/bench.md`](docs/bench.md) has the rules each number is read under.

## License

**AGPL-3.0-only.** Free to run, study, modify and share, for any purpose including
commercial use. Two obligations come with it: a fork stays under the same licence with
its source published, and anyone who offers svipall to others **over a network** must
publish the complete source of what they run (section 13). See [`LICENSE`](LICENSE).

`crates/svipall-cdp` keeps its upstream terms (chromiumoxide, MIT OR Apache-2.0) and
`crates/svipall-quic` keeps its own (quiche, BSD-2-Clause); the default build links
BoringSSL under an explicit AGPL section 7 linking exception.
Both are set out in [`NOTICE`](NOTICE) and
[`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md).

Contributions are taken under the DCO — no CLA, no copyright assignment. See
[`CONTRIBUTING.md`](CONTRIBUTING.md).

## Disclaimer

svipall is provided **as is, with no warranty and no liability**, and it grants you
**no authorisation with respect to any system you point it at**. Complying with the
law, with data-protection rules and with a site's terms is the operator's
responsibility, not the author's. Capability is not permission — read
[`DISCLAIMER.md`](DISCLAIMER.md) before you run it against something that is not yours.

---

*Keywords: web scraping, web crawler, MCP server, Model Context Protocol, Claude Code, Claude Desktop, AI agent tools, LLM web browsing, LLM-ready markdown, anti-bot bypass, Cloudflare bypass, Turnstile, Akamai, PerimeterX, DataDome, captcha solver, local captcha solving, headless browser, stealth browser, browser fingerprint, TLS fingerprint JA4, Chrome impersonation, Rust, Playwright alternative, Firecrawl alternative, Crawl4AI alternative, Scrapling alternative, local-first, privacy, no API key.*
