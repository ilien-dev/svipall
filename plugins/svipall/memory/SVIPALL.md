# Web access

This machine runs **Svipall**. Every web access goes through it — never the built-in `WebFetch` or
`WebSearch`. Svipall climbs a tier ladder past anti-bot walls, answers captchas locally, and reports
a block as a block instead of handing back a challenge page dressed as an article.

- page → `mcp__svipall__web_fetch` (omit `mode`; `css_selector` when the target is known;
  `query=` to filter a long page; PDFs and office documents read the same way) · rows instead of
  prose → `tables=true` or `schema="auto"` · anything large → `out_file`
- several known urls → `web_fetch_many` (`tables`/`schema` apply to each) · a site's URL list → `web_map` (cheap, first) · a site →
  `web_crawl` (`schema`/`tables` for rows) · search → `web_search` · a site's own search box → `web_site_search`
- the API behind a listing → `web_capture` (`page=2` beats following links) · something to click →
  `web_snapshot`, then `web_act` with `ref` (one shot) or `browser_open` → `browser_do` … →
  `browser_close` (several steps, cookies kept) · a picture → `web_screenshot` (`mobile` for a phone)
- a gate or a sign-in → `web_login` (once, by hand; the cookies are kept) · a fingerprint wall
  that never yields → `web_route(domain, proxy)` · a captcha named in `blocked_reason` →
  `solve_and_continue` (`solve_*` tokens only for a form you post yourself)
- across sessions → `web_notes` · what changed → `web_diff` once, `web_watch` on a schedule ·
  why a domain is slow or blocked → `web_log view=summary` · state and resets → `web_status`

**Rules.** Never retry a blocked URL blindly — read `blocked_reason` and report it. Never set a tier
by hand. Send anything large to `out_file` rather than into the context. Keep credentials in
`~/.svipall/secrets.env` and reference them as `${NAME}`. Human verification that needs a person:
stop and say so, do not loop.

`svipall doctor` says what this installation can actually do, and names the fix for anything that is
wrong. Run it before concluding a site is unreachable.
