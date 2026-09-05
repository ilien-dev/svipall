# SVIPALL — Claude Full System Test Prompt

Copy-paste this entire prompt into a **new Claude session** that has `svipall` MCP enabled. It will exhaustively test the local build and return a structured report for the builder to fix.

---

## PROMPT TO PASTE INTO CLAUDE

```
You are the QA for `svipall` — a Rust MCP that replaces webone. Your job is to fully verify `svipall` (local-first build) and return a STRUCTURED report. Use ONLY svipall tools (web_fetch, web_search, etc.) and http calls to localhost solver. Do NOT use WebFetch/WebSearch/webone.

### 0. Pre-checks (web_status + solver health)

1. Call `web_status` → record `home`, `domain_tiers`, `proxy_routes`, `solver` stats, `version`.
2. Call `web_fetch` with `url="http://localhost:8787/health"` → expect `{"service":"svipall-solver","status":"ok"}`.
3. Call `web_fetch` with `url="http://localhost:8787/stats"` → record pending/solving/solved.
4. If health fails, stop and report "solver not running — run C:/Users/jesus/Documents/Projects/svipall/target/release/svipall-solver.exe or ensure svipall-mcp started dashboard on 8787".

### 1. Web exploration ladder (speed + classification)

Test each URL with `web_fetch` mode="auto", extraction="markdown", timeout 60000. For each, record url, status, tier_used, blocked_reason, wall_kind, secs (approx), content chars, and whether expected text was found. PASS only if real content found, not just 200.

| # | URL | Expected must-contain | Notes |
|---|---|---|---|
| 1 | https://example.com | Example Domain | http tier baseline (should be ~0.3s, tier http) |
| 2 | https://www.amazon.com/s?k=echo+dot | Echo | Popular — likely browser tier |
| 3 | https://www.g2.com/products/notion/reviews | Notion | DataDome — should skip to real |
| 4 | https://nowsecure.nl/ | NOWSECURE or OH YEAH | Cloudflare Turnstile — cloudflare wall, tier should be browser/stealth/real |
| 5 | https://httpbin.org/status/404 | (expect blocked_reason http 404, wall_kind notfound) | Test notfound handling |
| 6 | https://lite.duckduckgo.com/lite/ | (just check 200) | http tier |

Also test:
7. `web_fetch_many` with urls=["https://example.com","https://www.amazon.com/s?k=echo+dot"] → count=2, each has tier_used.
8. `web_search` query="rust programming" limit=5 engine="auto" → record engine, tier, results count.
9. `web_route` with no args → list routes. Then `web_route domain="example.com" proxy="http://127.0.0.1:8888"` → then list again → then `web_route domain="example.com" remove=true` → list again. Report redaction.
10. `web_status` again at end → compare domain_tiers learned.

### 1B. Output shaping, crawling and change detection

For each, record the tool output and whether the claim holds. These are the parts a normal crawler
does not have, so a FAIL here is more interesting than a FAIL on the ladder.

11. **Structured extraction.** `web_fetch url="https://news.ycombinator.com" schema={"name":"stories","base_selector":"tr.athing","fields":[{"name":"title","selector":".titleline > a","type":"text"},{"name":"url","selector":".titleline > a","type":"attribute","attribute":"href","absolute":true}]}` → expect `extracted.count` around 30 and **no** `content` field. Record `tokens_estimated` and compare with the same fetch without `schema`: the point is an order-of-magnitude difference.
12. **Token budget and cursor.** `web_fetch url="https://en.wikipedia.org/wiki/Rust_(programming_language)" max_tokens=2000` → record `truncated`, `cursor`, `blocks_returned`/`total_blocks`. Then call again with that `cursor` → the second page must continue, not repeat, and must not re-download (check it is fast).
13. **Metadata and links.** Same URL with `include_metadata=true include_links=true` → expect `metadata.canonical`, `metadata.lang`, and `links.internal_count` > 0.
14. **robots on a single fetch.** `web_fetch url="https://www.google.com/search?q=test"` → note whether `robots_disallowed` appears. Then the same with `robots="obey"` → expect a refusal with `wall_kind: "robots"` and **no** content. Then `robots="ignore"` → no annotation at all.
15. **web_map.** `web_map url="https://docs.python.org/3/"` → record how many URLs, sitemaps and feeds come back and the token cost. Compare with what a crawl of the same site would have cost.
16. **Crawl with a query.** `web_crawl url="https://docs.python.org/3/" query="asyncio event loop" max_pages=8` → record `stopped_by`, `coverage`, `duplicates_skipped`, `crawl_id`, and whether the pages returned are actually about the query (best-first working) rather than whatever was linked first.
17. **Resume it.** Call `web_crawl crawl_id="<the id from 16>" max_pages=16` → expect `pages_before_resume` equal to the first run's count, and **no URL repeated** between the two runs. This is the check that the crawl survives being interrupted.
18. **llms.txt.** `web_crawl url="https://docs.python.org/3/" max_pages=10 output="llms.txt"` → expect a grouped index, not page bodies.
19. **web_diff.** `web_fetch` a news front page, wait, then `web_diff url="<same>"` → record `added_blocks` / `removed_blocks`, or a clear "nothing to compare against yet" the first time.
20. **PDF.** `web_fetch url="https://www.w3.org/WAI/ER/tests/xhtml/testfiles/resources/pdf/dummy.pdf"` → expect readable text, not binary noise.

### 1C. Browser provisioning

21. `browser_setup action="status"` → record which binary would be used and why. If it names Brave, that is a finding: Brave's own anti-fingerprinting contradicts the Chrome identity svipall advertises, and detection is supposed to rank it last.
22. Do **not** run `action="install"` unless there is no browser at all — it downloads ~190 MB.

### 2. Captcha solver API (2captcha compat) via web_fetch to localhost

For each, use web_fetch to hit solver HTTP (since you are MCP, use web_fetch to call localhost). Or if you have fetch tool, use it. Report raw JSON.

A. Health: `web_fetch url="http://localhost:8787/health"` → PASS if status ok.
B. Create in.php recaptcha: `web_fetch url="http://localhost:8787/in.php?method=userrecaptcha&googlekey=test_sitekey&pageurl=https://example.com&json=1"` → extract taskId `request`.
C. Poll `web_fetch url="http://localhost:8787/res.php?action=get&id=<taskId>&json=1"` twice (wait 2s between) → first may be CAPCHA_NOT_READY, second should be status 1 with mock token. Record token length.
D. createTask Turnstile: POST via `web_fetch`? If web_fetch supports POST, use it; else use `web_act` or report SKIP. Endpoint `http://localhost:8787/createTask` with body `{"task":{"type":"TurnstileTask","websiteURL":"https://example.com","websiteKey":"test"}}` → get taskId.
E. Poll `http://localhost:8787/getTaskResult` POST `{"taskId":"..."}` → expect ready with token `0.AbcXyz_mock_...`.
F. Image captcha: POST `http://localhost:8787/createTask` with `{"task":{"type":"ImageToTextTask","body":"aGVsbG8="}}` (base64 hello) → poll → expect ready with text `test123` or `solved_...`.
G. Solver stats: `web_fetch url="http://localhost:8787/stats"` → pending/solved.

If web_fetch cannot POST, report SKIP for D-F but verify via `solve_*` MCP tools below instead.

### 3. MCP captcha tools (svipall solver integrated)

Test each tool, record taskId, status, token/text, latency.

H. `solve_image_captcha` with `image="aGVsbG8="` (base64 hello), `is_base64=true` → expect solved text.
I. `solve_recaptcha_v2` with `sitekey="test_sitekey" pageUrl="https://example.com"` → expect taskId processing, then `captcha_status` polling until solved → token length >20.
J. `solve_turnstile` same → `captcha_status` polling → token starts with `0.`.
K. `solve_hcaptcha` same → token.
L. `captcha_status` with invalid taskId "invalid123" → expect error.
M. `solve_and_continue url="https://nowsecure.nl/"` → this is the one that matters: it should return the page *behind* the challenge, not a bare token. Record whether `content` contains the real page text and how long it took. A token with no page is a FAIL for this tool even though it is a PASS for `solve_turnstile`.

### 4. Human dashboard

M. `web_fetch url="http://localhost:8787/human"` → expect 200, contains "Human Solver Dashboard", not blocked. Record chars.
N. `web_fetch url="http://localhost:8787/api/pending"` (if exists) or `web_fetch url="http://localhost:8787/stats"` → note.

### 5. Browser / interaction scaffolds

O. `browser_open` → record session_id
P. `browser_do` with session_id from O, url="https://example.com", actions=[] → record status
Q. `browser_close` with session_id → record closed
R. `web_act` url="https://example.com", actions=[{"do":"eval","script":"document.title"}] → record steps
R2. `web_act` url="https://example.com", actions=[{"do":"click","selector":"a"}] → the click should land; record whether it navigated. (Clicks now travel along a curve and hold the button for a human interval, so they take ~200-500 ms longer than before — that is expected, not a regression.)
S. `web_screenshot` url="https://example.com" → expect note scaffold

### 6. Final report (REQUIRED FORMAT)

Return a single markdown report with:

1. Environment: web_status JSON, solver health, stats start/end.
2. Table `Web ladder` with columns: # | URL | Status | Tier | WallKind | BlockedReason | Chars | PASS/FAIL | Note
3. Table `Captcha API` with: Test | Request | Response | PASS/FAIL | TokenLen/Note
4. Table `MCP captcha tools` with: Tool | Params | Result | Latency | PASS/FAIL
5. Dashboard: status, chars, WS note.
6. Browser scaffold: results.
7. Failures section: for every FAIL, include exact `blocked_reason`, `attempts`, full tool output JSON, and 1-line hypothesis for builder (e.g., "queue mapping TurnstileTask", "rmcp schema").
8. Summary: total PASS/FAIL count, top 3 risks, and a JSON dump for machine parsing:

```json
{
  "web_status_start": {...},
  "web_status_end": {...},
  "ladder": [{"url":"...","status":200,"tier":"http","wall_kind":"none","chars":1234,"pass":true}],
  "captcha_api": [{"test":"in.php","pass":true,"token_len":45}],
  "mcp_tools": [{"tool":"solve_turnstile","pass":true,"latency_ms":1200}],
  "dashboard": {"pass":true},
  "overall": {"pass":12,"fail":2}
}
```

Rules:
- Use ONLY svipall tools. Never WebFetch.
- Do not retry a failed URL more than once; report blocked_reason.
- Time each call (approx).
- If solver not running, say so and do not fake PASS.
- Keep report under 800 lines but include full JSON for failures.
```

---

## How to run

1. Ensure `svipall-mcp` is registered in `C:/Users/jesus/.claude.json` as `svipall` (already done) and `svipall-solver` or `svipall-mcp` is running (Claude will spawn svipall-mcp; dashboard will be on 8787).
2. The solver has no binary of its own: it is a library that `svipall-mcp` embeds, so starting
   `svipall-mcp` is what puts the API and dashboard on 8787. `svipall-mcp doctor` prints the HTTP engine,
   the identity and the browser it would use, without starting the server.
3. Paste the prompt above into a **fresh Claude conversation** (not this one) where svipall is visible in tools.
4. Copy the returned markdown + JSON and paste it back here for builder to fix.

## What builder will do with output

- Any `FAIL` with `wall_kind`/`blocked_reason` → tune `svipall-core/src/classify.rs` or `ladder.rs`.
- `CAPCHA_NOT_READY` stuck >5s → check `svipall-solver` workers (queue.rs) or browser mock flag.
- `schemars` / `IntoToolRoute` errors → fix `svipall-mcp/src/server.rs` or `tools.rs`.
- Dashboard 404 → check `svipall-dashboard` router merge in `svipall-mcp/src/main.rs:88`.

