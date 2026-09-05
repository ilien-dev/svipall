---
name: Svipall
description: Fetch, crawl, search and extract any web page from the shell, getting past anti-bot walls and captchas locally. Use when a page is blocked, when a site needs crawling, when a page has to be read as structure rather than prose, or when a change needs watching over time.
---

# Svipall

Local-first web scraping. Every request, every browser and every captcha stays on this machine —
no API keys, no third-party solving service, nothing sent anywhere.

Every command prints one JSON object to stdout. Diagnostics go to stderr, so `svipall ... | jq` works.

## Start here

```bash
svipall fetch https://example.com/article
```

`mode=auto` is the default and the right answer: Svipall climbs a ladder — plain HTTP, then a browser,
then a stealth browser, then a real one, then one that waits out a challenge — stopping at the first
tier that works, and remembering it for that domain. Do not pick a tier by hand.

`svipall doctor` answers what this installation can actually do — which browser would run, which
captcha models are compiled in, whether the dashboard port is free — and names the command that
fixes anything that is wrong. Run it once before concluding a site is the problem.

## When a page comes back blocked

The result carries `blocked_reason`, `wall_kind`, `wall_vendor`, `widgets` and a `note` saying what
to do. Read it rather than retrying: a blind retry on a wall is how a domain earns a cooldown.
`wall_vendor` names the product guarding the domain by its own endpoint, and appears on a page that
arrived too — it says who is watching, not that anything was withheld.

- The note names a captcha widget → the MCP server's `solve_and_continue` solves it in place, or a
  person finishes it at the dashboard (`svipall status` prints the URL, and it works from a phone).
- A fingerprinting wall that never yields → route the domain through a proxy:
  `svipall route add shop.example --proxy socks5://… --country DE`, or a pool it moves through as
  exits get blocked: `--proxies A,B,C --countries DE,DE,NL`. Subdomains inherit. `svipall status`
  shows each exit's health and latency per domain; `svipall route check shop.example` tests them
  (liveness, latency, DNS-leak) with no third-party service — prefer `socks5h://` over `socks5://`,
  which resolves DNS on this machine.
- `blocked_reason: "address_budget"` → nothing was requested: this address has spent its standing
  with that host and is being rested. The result says how many seconds until it has not. Route the
  domain through a proxy, wait, or `web_status(clear_budget="shop.example")` if you mean to spend it
  anyway. `svipall status` shows what every address has spent where.
- A login wall → `web_login` once, by hand, and the profile keeps the cookies.

## Reading a page cheaply

| Want | Command |
|---|---|
| The prose | `svipall fetch URL` |
| Only what is relevant | `svipall fetch URL --query "shipping costs"` |
| Something to click | `svipall snapshot URL` — roles, names and refs, ~150 tokens for a whole page |
| The site's real API | `svipall capture URL` — the JSON the page itself fetched while loading |
| A lot of pages | `svipall crawl URL --out pages.csv` — writes a file, returns a path and a count |
| A table, as rows | `svipall fetch URL --tables --out rows.csv` — typed rows with their columns, not a markdown grid |
| A listing, as rows | `svipall fetch URL --schema auto` — reads the page's own repeated structure, names the columns for what they hold, and returns the schema it worked out in `induced_schema`. Keep that and pass it as `--schema '{…}'` next time. A page with no clear record set returns neither rather than guessing |
| A document, not a page | `svipall fetch https://x/report.docx` — docx, xlsx, pptx, odt, epub, rtf, csv and pdf read as markdown, from the web or from `file://` |
| Markup you already have | `svipall fetch raw: --stdin < page.html`, or `svipall fetch file:///…/page.html` (under `~/.svipall/in` or `local_roots`) |

`svipall capture` is the one people forget. Most sites render from an endpoint their own JavaScript
called a moment earlier, and that response is smaller, already typed and far more stable than the
HTML built from it. An endpoint that took `page=1` will take `page=2`.

## Crawling

```bash
svipall crawl https://docs.example/ --pages 50 --query "authentication"
svipall crawl https://docs.example/ --dfs            # one branch to its end: a manual, a listing
svipall crawl https://docs.example/ --since-last     # only what the sitemap says has changed
svipall crawl https://a.example/ --out rows.jsonl    # to a file, not through the context
```

A crawl returns a `crawl_id`. If it is interrupted, pass that id back and it continues from where it
stopped rather than starting over.

## Finding things

```bash
svipall search "rust async runtime" --engine all   # every engine, merged by agreement
svipall map https://example.com                    # the site's URLs, a few hundred tokens
```

## Remembering across runs

```bash
svipall notes set shop/last_id 4820
svipall notes get shop/last_id
svipall watch add https://example.com/changelog --every 3600
svipall watch add https://shop.example/item --selector ".price"   # one region; survives a redesign
svipall watch check                                # what changed since last time
svipall log --summary                              # which domains are slow or blocked, and why
svipall solver export-corpus --out ./corpus       # every captcha seen + answer, for training your own models
svipall quality ask --count 20                    # put pages in front of a person at the dashboard to rate
svipall quality export-training --out set.jsonl   # those ratings plus what the log implies, as training data
svipall quality train --in set.jsonl --out ~/.svipall/models
```

`svipall log --summary` is worth a look when a site starts failing: a domain that is half blocked and
slow is a domain whose learned tier is wrong.

## From another language

```bash
svipall serve --port 8788     # one endpoint per tool; the bearer key is printed once
curl -sH "Authorization: Bearer $KEY" -H 'content-type: application/json' \
     -d '{"url":"https://example.com","query":"pricing"}' localhost:8788/v1/fetch
```

Same objects as the CLI. A blocked page is a `200` carrying `blocked_reason`; only a bad request or
a broken installation is not.

## What a page says about itself

Every delivered page carries `quality`: `full` when there is nothing to report, otherwise `partial`
(cut off) or `thin` (a husk), with `quality_reasons` naming why. A page withheld behind a
subscription comes back as `wall_kind: paywall`, and a 200 that is really a missing page as
`softnotfound` — neither is content, and no tier fixes either. `optimization: high` appears only on
the far end of pages built for a ranking. `web_fetch_many` adds `corroboration`, which says how many
of the results are actually different documents rather than one story on five hostnames.

**None of it ever withholds a page.** They are labels: the odd, thin, heavily-optimised page that
happens to hold the answer is returned exactly like any other, and what to do about it is yours.

## Rules worth keeping

- **Never retry a blocked URL blindly.** Read `blocked_reason` and act on it.
- **Never set the tier by hand.** `auto` learns; a fixed tier is either slower or weaker.
- **Prefer `snapshot` to prose when the next step is a click**, and `capture` to parsing HTML.
- **Send bulk results to a file.** `--out` on `fetch` and `crawl` costs a path instead of the rows.
- **Credentials never go in a command.** Put them in `~/.svipall/secrets.env` and refer to them by
  name as `${SHOP_PASSWORD}`; the value is substituted on the way to the browser and never appears
  in the transcript.
