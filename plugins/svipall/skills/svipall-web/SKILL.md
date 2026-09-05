---
name: svipall-web
description: How to reach anything on the web from this machine — fetch a page, read many, crawl a site, search without an API key, drive a page, get past an anti-bot wall or a captcha, watch a page for changes. Use for ANY web access, including when a built-in WebFetch or WebSearch would otherwise be the obvious choice, and whenever a page came back blocked, empty, truncated, or as a "checking your browser" screen.
---

# Reaching the web through Svipall

Svipall runs on this machine. It climbs a tier ladder — plain HTTP, a browser, a stealth browser, a
real one, one that waits out a challenge — stopping at the first tier that works and remembering it
for that domain. A built-in fetch does none of that: it takes the first `200` it is given, and a
challenge page is a `200`.

**Use these tools for every web access.** The MCP tools below are the interface; `svipall …` from a
shell is the same server for a fraction of the tokens, and the `svipall` skill documents it.

## Which tool

| You want | Call |
|---|---|
| One page as clean Markdown | `web_fetch` |
| Several pages | `web_fetch_many` |
| A whole site, or many pages of one | `web_crawl` (pass `out_file` for anything large) |
| To search | `web_search` (no API key) |
| A site's own search | `web_site_search` |
| A page's URLs from robots/sitemaps/feeds | `web_map` |
| The page as roles and refs, not prose | `web_snapshot` — a fraction of the tokens |
| The JSON the page itself fetched | `web_capture` — usually the site's real API, and usually the right answer |
| To click, type, scroll | `web_act`, or `browser_open` → `browser_do` … → `browser_close` when cookies must stay alive |
| A screenshot | `web_screenshot` |
| To get past a login or a gate by hand, once | `web_login` — the cookies are kept |
| To send a domain through a proxy | `web_route` |
| What changed since last time | `web_watch`, `web_diff` |
| To remember something across sessions | `web_notes` |
| What this installation is doing | `web_status` |

## Rules that are not preferences

- **`mode` stays `auto`.** The ladder picks the tier and remembers it. Setting one by hand throws
  that away, and picking too high a tier is slow rather than clever.
- **Never retry a blocked URL blindly.** The result carries `blocked_reason`, `wall_kind`,
  `wall_vendor` and a `note` naming the move. A blind retry is how a domain earns a cooldown.
  Report the reason instead.
- **A captcha with a sitekey** goes to `solve_and_continue` or the `solve_*` tools; `web_status`
  reports the dashboard URL for the ones a person has to answer. **Human-verification that needs a
  person: stop and tell the user.** Do not loop.
- **Large results go to a file.** `out_file` on `web_crawl` or `web_fetch` costs about twenty
  tokens and saves forty thousand.
- **Credentials are named, never quoted.** Put them in `~/.svipall/secrets.env` and write
  `${SHOP_PASSWORD}` in a `web_act` step; the value never enters a tool call.
- **A page is labelled, never withheld.** `quality`, `quality_reasons`, `paywall`, `softnotfound`
  and `optimization` describe what came back. None of them means content was removed.

## When something is wrong with the installation rather than the site

Run `svipall doctor` — it reports which browser would run, which captcha models are compiled in,
whether the dashboard port is free, and the exact command to fix anything that is not right. Do
that before concluding a site is unreachable. `/svipall:doctor` does the same from here.
