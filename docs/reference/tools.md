# MCP tools

The tools an MCP client lists, with every parameter, exactly as the server describes them. This page is generated from the code by `crates/svipall/tests/reference_docs.rs`; edit the tool definitions, not this file.

There are 12 tools.

## `web_fetch`

Read URL(s): page, PDF, office doc, JSON API, source file, download; an audio/video file as its transcript. Returns `content` (markdown; JSON and code as served), `title`, `tier_used`, `quality`. Tier climbs http > browser > stealth > real > warm on its own, learned per domain. github.com is read from its API. Fewer tokens: `query`, `css_selector`, `json_path`, `max_tokens` + `cursor`, `out_file`. Rows: `schema` or `tables`. `urls` reads several in parallel (results in input order, duplicates marked `same_text_as`); with `probe` it only checks they exist (status, type, length: a 404 is an answer). Same-site links come back as paths, join them to `url`. Blocked: read `blocked_reason` and `note`, never retry blindly.

Read-only: yes. Reaches the network: yes.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `body` | string |  | Request body |
| `cache` | string |  | auto (default) \| read \| write \| bypass \| refresh |
| `css_selector` | string |  | Keep only matching elements |
| `cursor` | string |  | Continue a cut result |
| `extraction` | string |  | markdown (default) \| text \| html |
| `fresh` | boolean |  | Try again a site that failed in the last 10 min |
| `headers` | object |  | Extra request headers |
| `include_links` | boolean |  | Add links and images |
| `include_metadata` | boolean |  | Add canonical, language, author, dates, OpenGraph, JSON-LD, feeds |
| `include_quality` | boolean |  | Add quality_detail |
| `isolated` | boolean |  | Throwaway browser profile |
| `json_path` | string |  | Fields of a JSON answer: items[:5].{name, owner.login} |
| `main_content_only` | boolean |  | Default true: drops nav, footer, sidebars |
| `max_tier` | string |  | Highest tier to climb to |
| `max_tokens` | integer |  | Content cap, default 25000. A cut result carries `cursor` |
| `max_tokens_total` | integer |  | urls: cap for all pages together, default 8000 |
| `method` | string |  | POST, PUT, DELETE, HEAD (http tier) |
| `mobile` | boolean |  | Ask as a phone |
| `mode` | string |  | Force one tier. Leave unset |
| `out_dir` | string |  | urls: each page whole, one file each |
| `out_file` | string |  | Write here, return the path. .md .txt .json .csv .html get text or rows; any other name, or a binary body, the bytes (call again to resume). Relative: ~/.svipall/out/. urls: one .json/.jsonl for the set |
| `probe` | boolean |  | urls: existence only, no body |
| `profile` | string |  | Cookies of a saved login profile |
| `proxy` | string |  | Proxy URL, this call only |
| `query` | string |  | Keep only blocks relevant to these words. Name the fact, not the topic |
| `robots` | string |  | warn (default) \| obey \| ignore |
| `schema` | string or object |  | Rows as `extracted`: "auto", or {name, base_selector, fields:[{name, selector, type, attribute}]}; type text\|attribute\|number\|exists\|list\|html\|markdown |
| `scroll` | string |  | "auto" or a round count, for a page that loads as you scroll |
| `tables` | boolean |  | Data tables as {caption, header, rows} |
| `text_only` | boolean |  | Skip images, fonts, CSS in browser tiers |
| `timeout` | integer |  | ms, default 60000 |
| `url` | string |  | Also raw:<html>, or file:///path under ~/.svipall/in |
| `urls` | array of string |  | Several, instead of url |
| `use_site_template` | boolean |  | Strip what the site repeats on every page; can drop content |
| `wait` | integer |  | Seconds to wait out a visit limit, max 900; 0 never waits |

## `web_search`

Search the web, no API key. Returns title, url, snippet per result. Zero results carry a `note`: change the words, `lang`/`region` or `engine` instead of repeating. `site` searches inside one site through its own search box and returns `results_url`, the pattern that makes later queries a plain web_fetch. Read results with web_fetch `urls`.

Read-only: yes. Reaches the network: yes.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `engine` | string |  | auto (default) \| all (merged) \| ddg \| ddg-html \| bing \| brave \| news |
| `fetch` | boolean |  | site: false returns only the URL pattern |
| `lang` | string |  | e.g. es. Default: the query's |
| `limit` | integer |  | Default 10, max 50 |
| `query` | string | yes |  |
| `region` | string |  | Country, e.g. MX |
| `site` | string |  | Any page of one site: search it with its own box |
| `timeout` | integer |  | site: ms, default 60000 |

## `web_crawl`

Read a site whose pages are not known: crawls from `url`, returns each page as markdown, deduplicated, robots.txt obeyed, with a `crawl_id` to resume. `map` only lists the site's URLs (robots, sitemaps, llms.txt, feeds, links) for a few hundred tokens: do that first. Known URLs: web_fetch `urls`. Many rows: `out_file`. Same-site links are paths, join them to the page's `url`.

Read-only: yes. Reaches the network: yes.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `also` | array of string |  | More sites; the budget is shared equally |
| `crawl_id` | string |  | Resume an interrupted crawl |
| `dedup` | boolean |  | Default true: skip near-duplicates, naming what they repeat |
| `extraction` | string |  | markdown (default) \| text \| html |
| `include` | string |  | Only URLs containing this, e.g. /docs/ |
| `limit` | integer |  | map: max URLs, default 1000 |
| `map` | boolean |  | List URLs only, fetch no page |
| `max_chars_per_page` | integer |  | Default 8000 |
| `max_depth` | integer |  | Default 2 |
| `max_duration_ms` | integer |  | Default 120000 |
| `max_pages` | integer |  | Default 20, max 200 |
| `max_tokens_total` | integer |  | Default 60000 |
| `mode` | string |  | Force one tier. Leave unset |
| `out_file` | string |  | .csv .json .jsonl: a path and a count come back |
| `output` | string |  | pages (default) \| llms.txt \| llms-full.txt |
| `query` | string |  | Rank pages by relevance, keep relevant blocks, stop when new pages add nothing |
| `robots` | string |  | obey (default) \| warn \| ignore |
| `schema` | string or object |  | Rows from every page, as on web_fetch |
| `scroll` | string |  | "auto" or a round count, on every page |
| `since_last_crawl` | boolean |  | Only what the sitemap's lastmod says changed |
| `sources` | array of string |  | map: robots, sitemap, llms, feeds, links (default all) |
| `stop_when_saturated` | boolean |  | Default true with query |
| `strategy` | string |  | best_first (default with query) \| bfs \| dfs (a manual, a paginated listing) |
| `tables` | boolean |  | Every page's data tables as rows |
| `timeout` | integer |  | ms per page, default 45000 |
| `url` | string | yes | Start; the crawl stays on its domain |

## `web_view`

Look at a page. Default: every button, link and field with role, name and a `ref` (e12) to act on with browser: no image, few tokens, the way to understand a page before clicking. `image`: a PNG screenshot (path, and inline) when the question is visual: layout, a chart, a picture.

Read-only: yes. Reaches the network: yes.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `find` | string |  | Only nodes whose name or role contains this, e.g. "add to cart", "button" |
| `full_page` | boolean |  | image: the whole scroll height |
| `image` | boolean |  | Screenshot instead of the node list |
| `inline` | boolean |  | image: false returns only the path |
| `limit` | integer |  | Max nodes, default 200 |
| `max_depth` | integer |  | 3 or 4 usually reaches the controls |
| `mobile` | boolean |  | image: phone viewport and user agent |
| `profile` | string |  | Cookies of a saved login profile |
| `proxy` | string |  | image: proxy URL, this call only |
| `tier` | string |  | browser \| stealth \| real (default) \| warm |
| `timeout` | integer |  | ms, default 60000 |
| `url` | string | yes |  |

## `browser`

Click, type and run steps in a real browser; returns each action's result and the resulting page. One shot: `url` + `actions`. Several calls with cookies and page state kept (log in then browse, cart then checkout): `open` (alone, or with the first `url`/`actions`) returns a `session_id`; pass it with `url` and/or `actions`, end with `close`. Take refs from web_view.

Read-only: no. Reaches the network: yes.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `actions` | array of object |  | Steps in order, e.g. [{"do":"type","ref":"e3","text":"shoes"},{"do":"press","key":"Enter"},{"do":"wait","selector":".results"}] |
| `close` | boolean |  | End session_id and release its browser |
| `extraction` | string |  | markdown (default) \| text \| html |
| `open` | boolean |  | Start a session |
| `profile` | string |  | Cookies of a saved login profile (one shot, open) |
| `proxy` | string |  | Proxy URL (one shot, open) |
| `query` | string |  | session: keep only blocks relevant to these words |
| `session_id` | string |  | From open |
| `tier` | string |  | One shot: browser \| stealth \| real (default) \| warm |
| `timeout` | integer |  | ms, default 90000 |
| `url` | string |  | Open or navigate first; omit in a session to stay on the page |
| `visible` | boolean |  | open: show the window |

## `web_watch`

Detect changes to a page. `diff`: compare it once with the cached copy (`changed`, `similarity`, `added`, `removed`). `add`: watch it on a schedule; `list`: what changed and when; `check`: look now; `remove`. Watches survive restarts and run while the server is up.

Read-only: no. Reaches the network: yes.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `action` | string |  | add (default) \| diff \| list \| check \| remove |
| `css_selector` | string |  | Watch only this region |
| `interval_secs` | integer |  | Default 3600, min 60 |
| `label` | string |  | Name shown in list |
| `refetch` | boolean |  | diff: false compares stored copies only |
| `url` | string |  | Needed for add, diff, remove, a single check |

## `web_status`

Why something is blocked or slow, and how to reset it. No arguments: a summary (cooldowns, visit limits, browser, solver), nothing changed; `sections` for more. `log`: the history per domain (tier that answered, wall met, time taken). The clear_* and forget_tier resets take a domain. The visit limit is per host and shared by every session on this machine, so clearing it spends what others are pacing.

Read-only: no. Reaches the network: no.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `clear_budget` | string |  | Domain: its reputation spend |
| `clear_cache` | any |  | true, or a domain |
| `clear_cooldown` | string |  | Domain: cooldown, holds, strikes, visit window, remembered failure |
| `configure` | any |  | Object: browser and session policy settings to save |
| `domain` | string |  | log: only this domain |
| `forget_tier` | string |  | Domain: its learned tier |
| `limit` | integer |  | log recent: default 50 |
| `log` | string |  | summary (half-blocked or slow domains) \| recent (last events) |
| `sections` | string |  | "all", or names from the summary's `sections`, comma-separated |
| `since_secs` | integer |  | log: default 3600 |

## `web_setup`

Set this machine up, by `do`. login: opens a visible window at `url` for a person to pass a login or a challenge no tier clears; cookies go to `profile` (default: the domain's own, used automatically afterwards). route: send `domain` through `proxy` or a pool from now on, for a fingerprint wall that never yields or a country lock (no other argument lists routes). profile: list, export or import saved logins as an encrypted archive. browser: status, install, update or remove the managed browser, when a fetch reports none usable.

Read-only: no. Reaches the network: yes.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `action` | string |  | profile: list (default) \| export \| import. browser: status (default) \| install \| update \| remove |
| `artifact` | string |  | browser: chrome (default) \| chrome-headless-shell |
| `check` | boolean |  | route: test the exits (alive, latency, DNS leak) |
| `check_url` | string |  | route: what check fetches |
| `countries` | array of string |  | route: country of each proxy, by position |
| `country` | string |  | route: ISO country of the exit, for a matching timezone and language |
| `do` | string | yes | login \| route \| profile \| browser |
| `domain` | string |  | route: subdomains inherit |
| `file` | string |  | profile: archive path; relative lands in ~/.svipall/out/ |
| `name` | string |  | profile: its name |
| `password` | string |  | profile: required to export or import |
| `profile` | string |  | login: name to save the cookies under |
| `proxies` | array of string |  | route: a pool; sticks to the first that works |
| `proxy` | string |  | route: e.g. socks5h://user:pass@host:1080 |
| `remove` | boolean |  | route: drop the domain's route |
| `timeout_s` | integer |  | login: default 300; closing the window ends it |
| `url` | string |  | login: the login form or the blocked page |

## `captcha`

Get past a captcha. `url`: solve the challenge on that blocked page in place and return the page behind it; use it when `blocked_reason` names a captcha (a widget that needs a person opens a window or the dashboard). A bare token, only for a form you post yourself from the same session and address: `kind` + `sitekey` + `page_url`. `image`: read a picture of distorted characters as text. `task_id`: poll a pending task; with `good`, report whether the site took the answer, which orders what is tried next time.

Read-only: no. Reaches the network: yes.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `extraction` | string |  | markdown (default) \| text \| html |
| `good` | boolean |  | With task_id: true accepted, false rejected |
| `image` | string |  | base64 or a URL |
| `invisible` | boolean |  | recaptcha_v2 without a checkbox |
| `kind` | string |  | turnstile \| recaptcha_v2 \| hcaptcha |
| `max_tokens` | integer |  | url: content cap, default 25000 |
| `note` | string |  | With good: what the site said |
| `page_url` | string |  | The page the widget is on |
| `profile` | string |  | url: profile whose cookies to use and update |
| `sitekey` | string |  | The widget's data-sitekey |
| `task_id` | string |  | The `taskId` a pending answer returned |
| `timeout_s` | integer |  | url: default 120 |
| `url` | string |  | The blocked page |

## `web_capture`

Find the JSON API behind a listing or pagination: records the XHR and JSON responses a page makes while it loads. Call without `pattern` to see the endpoints, then with `pattern` and `bodies` for the payloads. An endpoint that took page=1 takes page=2, which beats following links.

Read-only: yes. Reaches the network: yes.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `bodies` | boolean |  | Return the response bodies |
| `max_body` | integer |  | Chars per body, default 20000 |
| `pattern` | string |  | Only responses whose URL contains this, e.g. /api/ |
| `profile` | string |  | Cookies of a saved login profile |
| `settle_ms` | integer |  | Keep recording this long after load, default 3000 |
| `tier` | string |  | browser \| stealth \| real (default) \| warm |
| `url` | string | yes |  |

## `web_video`

What a video says and shows: captions and chapters as one [m:ss] timeline (speech transcribed when there are none), and timed contact sheets of the whole video, as files. Look closer with start/end, at, crop; find a moment with query. web_fetch on a video page returns only its description. An encrypted stream is reported as `drm`, never read.

Read-only: yes. Reaches the network: yes.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `at` | array of string |  | Exact moments |
| `crop` | string |  | Part of the frame: x,y,w,h in 0..1 |
| `cursor` | string |  | Continue a cut result |
| `end` | string |  | Its end |
| `frames` | integer |  | Moments to capture, max 120; 0 = no pictures |
| `lang` | string |  | Caption language to prefer, e.g. es |
| `max_tokens` | integer |  | Default 25000 |
| `out_file` | string |  | Write the timeline here, return the path |
| `profile` | string |  | Cookies of a saved login profile |
| `query` | string |  | Find where this is said or shown; English for pictures |
| `start` | string |  | Part to look at: seconds or m:ss |
| `url` | string | yes |  |
| `wait` | integer |  | As on web_fetch |

## `web_notes`

Remember a value across sessions: the last id seen, a learned URL pattern, a checkpoint.

Read-only: no. Reaches the network: no.

| Parameter | Type | Required | What it does |
|---|---|---|---|
| `action` | string |  | get (default) \| set \| list \| delete |
| `key` | string |  | Path-like, e.g. shop/last_id |
| `prefix` | string |  | list: keys starting with this |
| `value` | string |  | set: a string; JSON if it has structure |

