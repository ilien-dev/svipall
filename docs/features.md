# Features

Lifted out of the README so that file stays readable. Everything here is the same text, with its links repointed.

### Reading

- **Markdown from supported pages and documents** — heuristic main-content extraction and
  hidden-text sanitization, optional JSON schema extraction, BM25 `query` filtering, pagination by `max_tokens` +
  `cursor`. A continuation names the whole heading path it resumes under (`Guide > Install >
  Windows`) and repeats the tail of the previous page, so a page read in parts is never picked up
  cold.
- **Tables as rows, documents as prose** — `tables=true` extracts detected HTML data tables as typed rows
  (CSV/JSON/JSONL to a file), and docx, xlsx, pptx, odt, epub, rtf, csv and pdf come back as
  Markdown, from the web or from `file://` under a declared root. `raw:<html>` extracts markup you
  already have, with no request at all. Document conversion is bounded and format-dependent;
  PDF extraction reads embedded text, not OCR of scanned pages. Default PDF limits are 50 MiB
  and 100 pages, so a large, malformed or image-only file can fail or yield incomplete text.
- **Observe JSON responses** — `web_capture` records matching responses during a bounded browser
  capture. Results may reveal an endpoint worth investigating; they do not guarantee a public,
  reusable or paginated API, and authentication and site restrictions still apply.
- **Selector recovery after some redesigns** — schema matches are fingerprinted per domain and
  structural similarity can propose a replacement reported as `healed`. Weak or ambiguous matches
  are refused; a confident heuristic can still select the wrong element, so validate the fields.
- **A schema for a listing you have never seen** — `schema: "auto"` reads the page's own repeated
  structure, names the columns for what they hold (`title`, `url`, `price`, `date`, …) and returns
  the rows in `extracted` with the schema that produced them in `induced_schema`, to keep and pass
  back next time. No model, no API, one parse. A page with no clear record set returns **neither**: a
  candidate that does not clearly beat the runner-up is refused, and a field missing from a quarter
  of the records is dropped, because a guessed row is worse than no row and stays wrong quietly.
- **Feeds that load as you scroll** — `scroll="auto"` watches document growth, tries one detected
  "load more" control and stops at stability or its round/time budget. Virtualized, delayed or
  endless feeds can remain incomplete; stopping does not prove the whole listing was read.
- **Cheaper variants when you want them** — `mobile=true` (phone layout, usually far less
  navigation), `text_only=true` (skip images, fonts, media), `css_selector`, and a page cache that
  can revalidate with `If-None-Match` when the server supplies a usable ETag. A later response may
  still be a full download; validators, cache policy and server behavior determine the result.
- **Cross-page boilerplate removal — built, measured, and shipped *off*.** Cached sibling pages
  help identify repeated site text, which can also be useful content. This heuristic can remove
  the wrong material. In the recorded TeCo evaluation, at the shipping threshold
  it fired on 2 of 11 sites, saved 3.4% of the text — and removed **one word of human-labelled
  content on one site**. The gate on this feature is absolute, no threshold made it hold, and tuning
  the threshold until one corpus reports zero would be fitting to that corpus. So it is `false` by
  default and reachable by asking: `use_site_template: true`. Two guards apply even then — nothing
  is stripped until 16 pages of a domain are cached, and a strip that would leave under a fifth of
  the page is refused and reported instead.

### Judging what came back

<p align="center">
  <img src="assets/readme/classify-what-arrived.png" alt="Three HTTP 200 responses contain an article, a login form and a missing-page message. Svipall distinguishes wall_kind none, login and softnotfound. Delivered content is assessed separately as full, partial or thin; quality labels never discard a page." width="880" loading="lazy">
</p>

Wall classification controls escalation and can stop the ladder. Content-quality assessment is
separate: its labels do not discard returned pages. Neither mechanism proves that the requested
information is complete or correct.

A `200` is not an answer. Classified fetch results can carry `wall_kind`; early policy, admission
or tool errors may have a different shape. The classification is heuristic. The values and usual
automatic-routing behavior are:

| `wall_kind` | What it is | What happens next |
|---|---|---|
| `none` | No wall was detected | return the response; check content separately |
| `cloudflare` · `generic` · `hold` | A challenge still standing at this tier | climb, answer it on the page, or route the domain elsewhere |
| `vendor` | A detected fingerprinting wall; `wall_vendor` and `wall_evidence` identify a recognized sign | prefer permitted headful emulation; fallback depends on the remaining plan and budgets |
| `empty` | Rendered no text at all | climb; or `web_act` with a wait, or a `css_selector` for the region you need |
| `status` | A recognized HTTP refusal status | obey reported backoff; status and policy determine whether further attempts are allowed |
| `gate` | A **geo or consent** gate instead of the page — not a captcha | **stops the ladder**, because no tier dismisses a cookie banner: `web_act` to click through, or `web_route` to change country |
| `login` | A detected sign-in wall | stop escalation; use `web_login` for an authorized manual sign-in if appropriate |
| `paywall` | Detected subscription or restricted content | stop escalation; an entitled session may be required, and a proxy does not grant access |
| `notfound` · `softnotfound` | A real 404, or **a `200` whose body says the page is not there** | both stop the ladder; no tier fixes either, and the second is the one that would otherwise reach a model as content |
| `timeout` | No tier answered inside the budget. Not a wall at all, and it is not dressed up as one | raise `timeout`, or lower `max_tier` |

`empty` and `status` are the only two verdicts a vendor sign found on a
header or cookie may rename — they mean "blocked, cause unknown", so naming the vendor is an upgrade,
and a wire sign never invents a wall anywhere else. And `softnotfound` matches on the whole trimmed
title, never a substring, because *"Understanding soft 404s"* is an article.

Supported page results also carry quality observations. These are heuristics, not completeness
proofs; early errors and other tool response types may omit them:

- **Integrity verdict on assessed pages** — `full`, `partial` (possible truncation) or `thin`
  (a husk), with `quality_reasons` naming why: `thin_text`, `low_text_ratio`, `truncated`,
  `not_prose`, `repetitive`, `landed_elsewhere` (you asked for an article and got the front page),
  and `mostly_boilerplate` — that last one only ever from a **crawl**, because only a caller that has
  seen the rest of the site can know it, and a single fetch says nothing rather than guessing. Every
  rule uses structural signals such as length, symbols, alphabetic share and repetition rather
  than a vocabulary filter. This design does not establish equal accuracy across languages.
- **How engineered the page is** — `optimization: high` with the traits behind it
  (`affiliate_heavy`, `headings_echo_the_body`, `link_dense`). Two traits are needed, because one
  alone is ordinary: plenty of honest pages carry a few referral links and a glossary is legitimately
  link-dense.
- **A page-substance classifier you train yourself** — `junk` / `thin` / `ordinary` / `substantive`
  from a hashed-bigram linear model. Its four labels reflect the configured training data;
  this repository does not establish superiority over embedding or language-model classifiers.
  `svipall quality ask | export-training | train` fits it from your own history and your own ratings.
- **Percentiles from local history** — below 30 observations, no percentile is returned. The band
  is `wide` from 30 observations and `narrow` from 200, per class. These are rough sample-size
  categories, not validated confidence intervals or a representative sample of the web.
- **Provenance observations, never a score** — byline, publication date, outbound citations, and when
  this machine first saw the site. Presence or absence of these signals does not establish credibility.
- **Near-duplicate awareness** — `near_dup_of` asks the cache whether it has seen this page before,
  under any other name.
- **Corroboration and diversity ordering on `web_fetch_many`** — the result says how many *distinct*
  documents it actually holds, marks each duplicate with `same_text_as`, and moves the different ones
  up. A reordering only: nothing is dropped, the caller's first choice stays first, and
  `reordered_for_diversity` says when it happened. These are text-similarity observations:
  distinct-document counts do not establish independent authorship, factual agreement or an
  improvement in a downstream model's answers.

Pass `include_quality: true` for the full `quality_detail` block. It is off by default: the compact
fields accompany supported assessed page results; this option adds their detailed evidence.

### Getting in

<p align="center">
  <img src="assets/readme/shapeshifter-identities.png" alt="Svipall takes three forms at docs.example, shop.example and news.example. Each keeps the amber eye and interwoven beard, illustrating a coherent identity across TLS, headers, browser and behavior." width="880" loading="lazy">
</p>

*Different domains, different faces. Within a session, cookies and identity stay together;
a spent session is retired rather than changing its face on every request.*

- **Automatic anti-bot escalation** — on a new route, plain HTTP first, then headless Chromium, then
  a stealth-patched browser, then a headful "real" browser with a persistent per-domain profile,
  then a patient `warm` tier that answers challenges. **Two supporting full-quality deliveries can
  promote an emulated route** for the same origin, path family, exit and browser environment.
  Fingerprint and hold walls can skip headless probes for an allowed headful session; a failed
  promoted headful route does not backtrack through weaker routes for those wall types.
  One optional native fallback stays last; [eligibility, expiry and limits](#automatic-routing-privacy-and-practical-limits)
  constrain the plan.
- **Browser-like network fingerprint** — selected JA4, HTTP/2 SETTINGS, header-order and GREASE checks on
  BoringSSL, post-quantum key share included. The eight checks are
  [above](#identity-coherence--asserted-offline-in-ci). The emulation currently presents **Chrome
  149**, which is the newest profile the TCP engine offers; the provisioned browser is 152, and that
  gap is named as open in [`CHANGELOG.md`](../CHANGELOG.md) rather than glossed over.
- **Firefox, coherently, on the http tier** — `http_firefox = true` and the http tier presents Gecko
  in TLS, headers and User-Agent *together*: Firefox's own emulation profile, its real header order
  with `User-Agent` first, its own `accept`, and **no `Sec-CH-UA` at all** — Firefox sends no client
  hints, and emitting one is the loudest way to be caught pretending. The browser tiers stay Chrome,
  because their protocol is Chrome's. [`docs/firefox.md`](firefox.md) sets out what a
  patched-Gecko browser engine would add and what it would cost.
- **Stealth that goes far beyond `navigator.webdriver`** — coherent machine identities (screen, GPU,
  fonts, languages, timezone, memory, DPR), deterministic canvas/audio/text-geometry noise, WebRTC
  mitigations behind proxies, and fixes for known DevTools automation traces. The tested surfaces passed
  [160/160](#automation-tells--160-of-160-offline-and-it-fails-the-build).
- **Human-like behaviour** — Bézier pointer paths that land off-centre, typing cadence by digraph,
  wheel-notch scrolling with inertia, focus/visibility events, dwell time proportional to page
  length. Built-in pointer, keyboard and wheel actions use this layer; caller-supplied `eval`
  JavaScript is outside that guarantee.
- **Sessions retired rather than reused** — a session is cookies + machine + exit. When a site turns
  on one, the profile is retired (the browser holding it closed first) and the next visit arrives as
  a fresh tool-managed profile. The same exit or other characteristics can still link visits.
  `isolated=true` makes a profile that exists only for one fetch.
- **Pools of exits, used properly** — `web_route` takes several proxies per domain, each with its
  declared country; the domain keeps one (`sticky`) until it blocks it twice, then moves on, and a
  retired exit **heals with time** rather than staying dead. Pacing, strikes and latency are keyed by
  `(domain, exit)`, so a pool actually buys throughput instead of ten exits sharing one gap.
  Supported browser proxy authentication uses CDP rather than credentials in command-line
  arguments. Declared locale/timezone and optional DNS-over-HTTPS configure supported browser
  paths; they are not a network-wide leak guarantee. `web_route check` flags a `socks5://`
  configuration that would resolve names locally. HTTP/3 is not used through proxies.
- **Reputation, spent like a budget** — what each address has spent with each host, decaying with a
  half-life, to reduce repeated attempts. This cannot prevent a site from blocking that address.
- **HTTP/3, opt-in** — a vendored quiche on the same BoringSSL the http tier already links, emitting
  Chrome's QUIC ClientHello and Chrome's HTTP/3 SETTINGS frame, both asserted offline against a
  capture of a real Chrome.

### Acting

- **Real browser interaction** — `web_snapshot` returns the page as roles, accessible names and short
  refs instead of markup; `web_act` clicks, types, fills, scrolls and waits on those refs.
  `browser_open` / `browser_do` keep a session alive across calls. Deterministic — no vision model.
- **Search without an API key** — DuckDuckGo, Bing and Brave scraped directly, optionally merged by
  agreement.
- **A site's own search box** — `web_site_search` can learn a reusable query-URL pattern and fetch
  later queries through it. Sites without a discoverable form or reusable URL can fail; the fetch
  may still require browser rendering.

### Crawling

- **Crawls that survive interruption** — same-domain BFS or DFS, robots.txt, sitemaps and feeds,
  near-duplicate labels, `llms.txt` output, and a `crawl_id` you pass back to resume from the
  persisted frontier.
- **Crawls that know when to stop** — coverage of your query and novelty per page are measured
  lexically (no model, no download); a crawl that has stopped learning ends instead of spending the
  rest of its budget.
- **Crawls that only fetch what moved** — `--since-last` compares sitemap `<lastmod>` against the
  page cache. A URL with no `lastmod` is fetched, because silence is not "unchanged".
- **Politeness that adapts** — the gap between requests is tuned per domain from the host's own
  latency and refusals, with a default one-second minimum. HTTP 429/503 stops escalation;
  the full `Retry-After` or the configured cooldown, whichever is longer, is persisted.
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
- **Secret references in action arguments** — values from `~/.svipall/secrets.env` are substituted
  locally when supported actions execute. This avoids putting values in the original tool call;
  it does not redact a site that echoes them in content, screenshots or other returned data.
- **Origin policy** — allow/block lists, private-address refusal, and optional ad/tracker/consent-banner
  blocking with cached lists that degrade silently offline.
- **A CLI with the same brain** — published as an [Agent Skill](../skill/SKILL.md) for agents that prefer
  a shell to a tool schema; a test keeps the skill in step with the CLI.
- **A local REST API for every other language** — [details below](#the-rest-api).

---
