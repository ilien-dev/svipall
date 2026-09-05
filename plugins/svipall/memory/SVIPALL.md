# Web access

This machine runs **Svipall**. Every web access goes through it — never the built-in `WebFetch` or
`WebSearch`. Svipall climbs a tier ladder past anti-bot walls, answers captchas locally, and reports
a block as a block instead of handing back a challenge page dressed as an article.

- page → `mcp__svipall__web_fetch` (leave `mode` at `auto`; `css_selector` when the target is known;
  `main_content_only=false` for `<head>`; `query=` to filter a long page)
- several urls → `web_fetch_many` · a site → `web_crawl` · search → `web_search` ·
  a site's own search → `web_site_search` · a site's URL list → `web_map`
- structure instead of prose → `web_snapshot` · the JSON the page fetched → `web_capture`
  (usually the site's real API, and usually the right answer)
- click, type, scroll → `web_act`; cookies that must stay alive → `browser_open` → `browser_do` … →
  `browser_close` · screenshot → `web_screenshot`
- a gate or a sign-in → `web_login` (once, by hand; the cookies are kept) · a flagged address →
  `web_route(domain, proxy)` · a captcha with a sitekey → `solve_and_continue` or `solve_*`
- across sessions → `web_notes` · what changed → `web_watch`, `web_diff` · what it is doing →
  `web_status`

**Rules.** Never retry a blocked URL blindly — read `blocked_reason` and report it. Never set a tier
by hand. Send anything large to `out_file` rather than into the context. Keep credentials in
`~/.svipall/secrets.env` and reference them as `${NAME}`. Human verification that needs a person:
stop and say so, do not loop.

`svipall doctor` says what this installation can actually do, and names the fix for anything that is
wrong. Run it before concluding a site is unreachable.
