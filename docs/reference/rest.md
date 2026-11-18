# REST routes

Every route `svipall serve` answers, with the body it takes. This page is generated from the code by `crates/svipall/tests/reference_docs.rs`; edit the routes, not this file. How to start the server and authenticate is in [the REST API guide](../rest.md).

There are 21 operation routes. Each takes `POST` with a JSON body.

## `POST /v1/act`

Runs the `web_act` operation.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `actions` | array of object | yes | Steps, in order, e.g. `[{"do":"type","ref":"e3","text":"shoes"},{"do":"press","key": "Enter"},{"do":"wait","selector":".results"}]`. |
| `extraction` | string |  | markdown (default), text, or html for the final page. |
| `profile` | string |  | Profile saved by web_login whose cookies to use. |
| `proxy` | string |  | Proxy URL for this run only; web_route sets one per domain. |
| `tier` | string |  | Browser tier: browser, stealth, real (default), warm. |
| `timeout` | integer |  | Timeout in ms for the whole interaction. Default 90000. |
| `url` | string | yes | The page to open first. |

## `POST /v1/browser_setup`

Runs the `browser_setup` operation.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `action` | string |  | status (default) \| install \| update \| remove. |
| `artifact` | string |  | chrome (default, supports the headful real/warm tiers) \| chrome-headless-shell (smaller, but cannot run headful and is more detectable). |

## `POST /v1/capture`

Runs the `web_capture` operation.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `bodies` | boolean |  | Return the response bodies too, once `pattern` names the endpoint. Default false. |
| `max_body` | integer |  | Per-body character cap. Default 20000. |
| `pattern` | string |  | Only responses whose URL contains this, e.g. "/api/". Omit it the first time. |
| `profile` | string |  | Profile saved by web_login whose cookies to use. |
| `settle_ms` | integer |  | How long to keep recording after the page loads, in ms. Default 3000. |
| `tier` | string |  | Browser tier: browser, stealth, real (default), warm. |
| `url` | string | yes | The page whose requests to record. |

## `POST /v1/crawl`

Runs the `web_crawl` operation.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `also` | array of string |  | More sites to crawl alongside `url`; each domain gets an equal share of the budget. |
| `crawl_id` | string |  | Resume an interrupted crawl by the `crawl_id` it returned; web_status lists those with work left. |
| `dedup` | boolean |  | Skip near-duplicate pages, naming what they duplicate. Default true. |
| `extraction` | string |  | markdown (default), text, or html. |
| `include` | string |  | Only follow URLs containing this substring (e.g. "/docs/"). |
| `max_chars_per_page` | integer |  | Per-page content cap in chars. Default 8000. |
| `max_depth` | integer |  | Link depth from the start URL. Default 2. |
| `max_duration_ms` | integer |  | Give up after this long regardless. Default 120000. |
| `max_pages` | integer |  | Maximum pages to fetch. Default 20, max 200. |
| `max_tokens_total` | integer |  | Whole-crawl token cap. Default 60000. |
| `mode` | string |  | Leave unset: auto learns the tier per domain. |
| `out_file` | string |  | Write the pages to this .csv, .json or .jsonl file; a path and a count come back. |
| `output` | string |  | Output shape: pages (default), llms.txt, llms-full.txt. |
| `query` | string |  | Rank pages by relevance to these words, keep the relevant blocks, stop when new pages add nothing. |
| `robots` | string |  | robots.txt policy: obey (default for crawling), warn, ignore. |
| `schema` | any |  | Rows from every page, as on web_fetch; with `out_file`, one row per item with its `url`. |
| `scroll` | string |  | Scroll every page until it stops growing ("auto" or a round count). Implies browser tiers. |
| `since_last_crawl` | boolean |  | Fetch only what the sitemap's `lastmod` says changed since the last crawl; undated pages are always fetched. |
| `stop_when_saturated` | boolean |  | Stop once new pages add nothing about `query`. Default true with a query. |
| `strategy` | string |  | best_first (default with a query), bfs, or dfs: one branch to its end, for a manual or a paginated listing. |
| `tables` | boolean |  | Every page's data tables as typed rows, as on web_fetch. |
| `timeout` | integer |  | Timeout in ms per page. Default 45000. |
| `url` | string | yes | Start URL. Crawl stays on this domain. |

## `POST /v1/diff`

Runs the `web_diff` operation.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `refetch` | boolean |  | Fetch a fresh copy first. Default true; false compares stored versions only. |
| `url` | string | yes | URL to compare against its previously cached copy. |

## `POST /v1/fetch`

Runs the `web_fetch` operation.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `body` | string |  | Request body for POST/PUT (http tier only). |
| `cache` | string |  | auto (default: serve a fresh copy, revalidate a stale one), read, write, bypass, refresh. |
| `css_selector` | string |  | Keep only the elements this CSS selector matches, e.g. "article" or "#prices". |
| `cursor` | string |  | The `cursor` of a truncated result, to continue it. |
| `extraction` | string |  | markdown (default), text, or html (the raw markup, many times the tokens). |
| `fresh` | boolean |  | Walk the ladder again though it failed on this site in the last 10 minutes. |
| `headers` | object |  | Extra request headers (http tier only). |
| `include_links` | boolean |  | Add `links` (internal and external, absolute) and images. |
| `include_metadata` | boolean |  | Add `metadata`: canonical URL, language, author, dates, OpenGraph, JSON-LD, feeds. |
| `include_quality` | boolean |  | Add `quality_detail`: verdict with reasons, near-duplicates, provenance. |
| `isolated` | boolean |  | A throwaway browser profile: no cookies in, none left behind. |
| `json_path` | string |  | Only these fields of a JSON answer, e.g. `items[:5].{name, owner.login}` (`.a`, `[0]`, `[1:5]`, `[*]`, `{a, b}`). |
| `main_content_only` | boolean |  | Drop navigation, footers and sidebars. Default true. |
| `max_tier` | string |  | Highest tier to climb to: http, browser, stealth, real, warm (default). |
| `max_tokens` | integer |  | Cap on the content, cut on block boundaries. Default 25000; a cut result carries a `cursor`. |
| `method` | string |  | GET (default), POST, PUT, DELETE, HEAD (http tier only). |
| `mobile` | boolean |  | Ask as a phone. Most sites send the same bytes, and it costs a browser page of its own. |
| `mode` | string |  | Leave unset: auto learns the tier per domain. A forced tier is slower or weaker. |
| `out_file` | string |  | Write to this file and return the path, not the content. A .md .txt .json .csv .html name gets the text (rows with schema/tables); any other name, or a binary body, gets the bytes as served, resumed by calling again. Relative paths land in ~/.svipall/out/. |
| `profile` | string |  | Profile saved by web_login whose cookies to use. Implies a browser tier. |
| `proxy` | string |  | Proxy URL for this fetch only; web_route sets one per domain. |
| `query` | string |  | Keep only the blocks relevant to these words, e.g. "shipping costs": the cheapest way to one fact. Name the fact, not the topic: a specific query left 6% of a long page, a broad one 88%. |
| `robots` | string |  | robots.txt: warn (default: fetch and say if disallowed), obey, ignore. |
| `schema` | any |  | Rows instead of prose, as `extracted`. "auto", or e.g. `{"name": "products", "base_selector": "div.product", "fields": [{"name": "title", "selector": "h2 a"}, {"name": "url", "selector": "a", "type": "attribute", "attribute": "href"}]}`; types text, attribute, number, exists, list, html, markdown. "auto" returns the schema it used as `induced_schema`; a selector a redesign broke is relocated and reported as `healed`. |
| `scroll` | string |  | Scroll a page that loads as you go: "auto" until it stops growing, or a number of rounds. Implies a browser tier. |
| `tables` | boolean |  | The page's data tables as typed rows, `{caption, header, rows}`, instead of prose. |
| `text_only` | boolean |  | Skip images, fonts, stylesheets and video in browser tiers. Faster; some anti-bot scripts notice it. |
| `timeout` | integer |  | Timeout in ms for the whole ladder. Default 60000. |
| `url` | string | yes | The page. Also `raw:<html>` (no request) or `file:///path` under `~/.svipall/in` or `local_roots`. |
| `use_site_template` | boolean |  | Strip what this site repeats on every page, learned from earlier fetches. Off by default: it can take real content with it. |
| `wait` | integer |  | Seconds to wait out a full visit window or a hold, up to 900. Unset: up to half the timeout; 0 never waits. |

## `POST /v1/fetch_many`

Runs the `web_fetch_many` operation.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `extraction` | string |  | markdown (default), text, or html. |
| `json_path` | string |  | Only these fields of every JSON answer, as on web_fetch. |
| `max_tier` | string |  | Highest tier the ladder may climb to. Default warm. |
| `max_tokens` | integer |  | Cap per page. Default: an equal share of max_tokens_total. |
| `max_tokens_total` | integer |  | Cap on all pages together. Default 8000; a cut page carries a cursor for web_fetch. |
| `mode` | string |  | Leave unset: auto learns the tier per domain. |
| `out_dir` | string |  | Write each page whole to a file in this directory; return paths. |
| `out_file` | string |  | Write the whole set to this .json/.jsonl file; return a summary. |
| `query` | string |  | Keep only the blocks relevant to these words, on every page. |
| `schema` | any |  | Rows instead of prose from every page, as on web_fetch. |
| `tables` | boolean |  | Every page's data tables as typed rows, as on web_fetch. |
| `timeout` | integer |  | Timeout in ms per URL. Default 60000. |
| `urls` | array of string | yes | The pages to fetch, in the order the results come back. |
| `wait` | integer |  | As on web_fetch, per URL. |

## `POST /v1/log`

Runs the `web_log` operation.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `domain` | string |  | Only this domain. |
| `limit` | integer |  | Maximum lines for `recent`. Default 50. |
| `since_secs` | integer |  | How far back to look, in seconds. Default 3600. |
| `view` | string |  | recent (default) or summary. |

## `POST /v1/map`

Runs the `web_map` operation.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `include` | string |  | Only URLs containing this substring. |
| `limit` | integer |  | Maximum URLs to return. Default 1000. |
| `sources` | array of string |  | Sources to use: robots, sitemap, llms, feeds, links. Default: all of them. |
| `url` | string | yes | Any URL on the site; the whole origin is mapped. |

## `POST /v1/notes`

Runs the `web_notes` operation.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `action` | string |  | get (default), set, list or delete. |
| `key` | string |  | The note's name. A path-like key ("shop/last_id") groups notes for `list`. |
| `prefix` | string |  | For `list`: only notes whose key starts with this. Empty lists everything. |
| `value` | string |  | What to remember. Required for `set`; a string, so pass JSON if it has structure. |

## `POST /v1/probe`

Runs the `web_probe` operation.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `timeout` | integer |  | Timeout in ms per URL. Default 15000. |
| `urls` | array of string | yes | The addresses, answered in this order. |

## `POST /v1/profile`

Runs the `web_profile` operation.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `action` | string |  | list (default), export or import. |
| `file` | string |  | Where the archive goes, or comes from. Relative paths land in ~/.svipall/out/. |
| `name` | string |  | The profile name, as used by web_login and `profile` on web_fetch. |
| `password` | string |  | Required for export and import. The archive is the session; there is no unencrypted form. |

## `POST /v1/route`

Runs the `web_route` operation.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `check` | boolean |  | Test `domain`'s exits instead of changing them: answered, latency, DNS leak. |
| `check_url` | string |  | The URL a `check` fetches. Default: a small neutral page. |
| `countries` | array of string |  | ISO country of each entry in `proxies`, by position; `country` covers the rest. |
| `country` | string |  | ISO country the proxy exits from, e.g. "DE", for a matching timezone and language. |
| `domain` | string |  | The domain to route; subdomains inherit. Omit everything to list the routes. |
| `proxies` | array of string |  | Several exits: the domain sticks to the first that works, moving on when it is blocked twice. |
| `proxy` | string |  | Proxy URL, e.g. "socks5h://user:pass@host:1080" (socks5h resolves DNS at the exit). |
| `remove` | boolean |  | Remove the route for `domain`. |

## `POST /v1/screenshot`

Runs the `web_screenshot` operation.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `full_page` | boolean |  | Capture the whole scrollable page. Default false (viewport only). |
| `inline` | boolean |  | Return the PNG inline as image content too. Default true, skipped above 3 MB. |
| `mobile` | boolean |  | Render the page as a phone would: phone user agent and viewport. |
| `profile` | string |  | Profile saved by web_login whose cookies to use. |
| `proxy` | string |  | Proxy URL for this run only; web_route sets one per domain. |
| `tier` | string |  | Browser tier: browser, stealth, real (default), warm. |
| `timeout` | integer |  | Timeout in ms. Default 60000. |
| `url` | string | yes | The page to render. |

## `POST /v1/search`

Runs the `web_search` operation.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `engine` | string |  | auto (default: the first engine that answers), all (every engine, merged by agreement), or one of ddg, ddg-html, bing, brave, news (recent articles). |
| `lang` | string |  | Result language, e.g. es. Default: the query's. |
| `limit` | integer |  | Results to return. Default 10, max 50. |
| `query` | string | yes | What to search for, as you would type it into a search box. |
| `region` | string |  | Country, e.g. MX. |

## `POST /v1/site_search`

Runs the `web_site_search` operation.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `fetch` | boolean |  | Fetch the results too, rather than only reporting the pattern. Default true. |
| `query` | string | yes | What to search for. |
| `timeout` | integer |  | Timeout in ms. Default 60000. |
| `url` | string | yes | Any page of the site — usually the home page. Its search box is what gets used. |

## `POST /v1/snapshot`

Runs the `web_snapshot` operation.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `find` | string |  | Only nodes whose name or role contains this, e.g. "add to cart" or "button". |
| `limit` | integer |  | Cap on nodes returned. Default 200. |
| `max_depth` | integer |  | How deep to look; 3 or 4 usually reaches the controls. |
| `profile` | string |  | Profile saved by web_login whose cookies to use. |
| `tier` | string |  | Browser tier: browser, stealth, real (default), warm. |
| `timeout` | integer |  | Timeout in ms. Default 60000. |
| `url` | string | yes | The page to read. |

## `POST /v1/solve_and_continue`

Runs the `solve_and_continue` operation.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `extraction` | string |  | Extraction for the unblocked page: markdown (default), text, html. |
| `max_tokens` | integer |  | Cap on the content returned, as in web_fetch. Default 25000. |
| `profile` | string |  | Profile whose cookies to reuse and update. Defaults to the domain's automatic profile. |
| `timeout_s` | integer |  | Seconds to wait for the challenge to clear. Default 120. |
| `url` | string | yes | The blocked page. The challenge is solved on this very page, not a copy of it. |

## `POST /v1/status`

Runs the `web_status` operation.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `clear_budget` | string |  | Domain whose reputation spend should be forgotten, for every exit that spent on it. |
| `clear_cache` | any |  | Empty the page cache: `true` for everything, or a domain name for just that site. |
| `clear_cooldown` | string |  | Domain to clear on every exit: hard-block cooldown, server holds and strikes, visit window, remembered failure. Not `clear_budget`'s standing. |
| `configure` | any |  | Save browser and session policy settings for later calls; open sessions keep theirs. |
| `forget_tier` | string |  | Domain whose learned tier should be forgotten. |
| `sections` | string |  | More of the report: "all", or names from the summary's `sections`, comma-separated. |

## `POST /v1/video`

Runs the `web_video` operation.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `at` | array of string |  | Exact moments to capture. |
| `crop` | string |  | The part of each frame to return, as fractions of it: x,y,w,h. |
| `cursor` | string |  | The `cursor` of a truncated result. |
| `end` | string |  | Where it ends. |
| `frames` | integer |  | Moments to capture from the video itself, spread over the part asked for, up to 120. Left out, the number follows the length; 0 turns the picture off. |
| `lang` | string |  | Caption language to prefer, e.g. "es". |
| `max_tokens` | integer |  | Cap on the timeline returned. Default 25000. |
| `out_file` | string |  | Write the timeline here and return the path. |
| `profile` | string |  | Profile saved by web_login whose cookies to use. |
| `query` | string |  | Find where this is said, written on screen, or shown. |
| `start` | string |  | Where the part to look at starts: seconds, or m:ss. |
| `url` | string | yes | A video page, a page embedding a player, or a player address. |
| `wait` | integer |  | As on web_fetch. |

## `POST /v1/watch`

Runs the `web_watch` operation.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `action` | string |  | add (default), list, remove or check. |
| `css_selector` | string |  | Watch only the part of the page this CSS selector finds. Changes elsewhere are ignored, and if a redesign breaks the selector the region is relocated by fingerprint. |
| `interval_secs` | integer |  | How often it is worth looking, in seconds. Default 3600, floor 60. |
| `label` | string |  | A name to recognise it by in the list. |
| `url` | string |  | The page. Required for add, remove and a single check. |

## Job routes

How a caller follows a long operation without holding a connection open.

- `/v1/jobs`
- `/v1/jobs/{id}`
- `/v1/jobs/{id}/stream`

## Operations that are not routes

| Operation | Why |
|---|---|
| `browser_open` | a session is a resource HTTP cannot bound: a client that dies between open and close leaks a real browser with no TTL and no cap. web_act covers the single-shot case |
| `browser_do` | belongs to a session; see browser_open |
| `browser_close` | belongs to a session; see browser_open |
| `web_login` | opens a visible window on the operator's desktop and waits up to an hour for a person; an HTTP request must not be able to do that |
| `solve_image_captcha` | the solver API on dashboard_port already answers this in the 2captcha wire shape |
| `solve_recaptcha_v2` | the solver API on dashboard_port already answers this in the 2captcha wire shape |
| `solve_turnstile` | the solver API on dashboard_port already answers this in the 2captcha wire shape |
| `solve_hcaptcha` | the solver API on dashboard_port already answers this in the 2captcha wire shape |
| `captcha_status` | the solver API on dashboard_port already answers this in the 2captcha wire shape |
| `report_captcha` | the solver API on dashboard_port already answers this in the 2captcha wire shape |
