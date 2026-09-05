# The REST API

The same server over HTTP, so any language can drive it. Nineteen routes, one per tool, plus the
job routes that make a long crawl something you follow rather than something you hold a connection
open for.

Everything here runs on your machine. The API calls nothing external, binds loopback by default and
carries a key it generated itself.

```bash
svipall serve --port 8788
curl -sH "Authorization: Bearer $KEY" -H 'content-type: application/json' \
     -d '{"url":"https://example.com","query":"pricing"}' localhost:8788/v1/fetch
```

`svipall-mcp` mounts the same router when `rest_port` is set, on its own listener, sharing one
browser pool, one page cache and one set of learned tiers with the MCP tools. Two ways in, one
server behind them.

## What a status code means

> A page that came back blocked is a **200**. The call ran; the *page* did not.

This is the CLI's contract in its own words — *"the exit code … never depends on what a site said: a
page that was blocked is a successful report of a block"* — and REST inherits it verbatim. A client
that read a wall as a `5xx` would sit in a retry loop against something that is never going to move.

| Status | Means |
|---|---|
| **200** | the call ran, including every unhappy page: `blocked_reason`, `quality: "thin"`, `count: 0` |
| **202** | an asynchronous job was queued; `Location` names it |
| 400 | the body is not this endpoint's shape |
| 401 | missing or wrong bearer key |
| 403 | the request carried a browser `Origin`, or a `Host` that is not loopback on a loopback bind |
| 404 | unknown path, or a job id that does not exist |
| 413 | body over 2 MB |
| **500** | *this installation* could not carry the request out: no page cache, no browser, an unknown country code. Something the operator can fix — never a wall |

Unknown body fields are ignored rather than rejected, which is what MCP clients already get. A
typo'd field is therefore silent; that is deliberate, and it is the price of one behaviour across
both protocols.

## The key, and why loopback is not a boundary

Every route needs `Authorization: Bearer <key>`, including on loopback. svipall carries logged-in
browser profiles, cookies and your exit address, so an unauthenticated port is a proxy wearing your
identity, reachable by every process on the machine.

The key is resolved in this order:

1. `SVIPALL_API_KEY` — beats everything, and needs no writable home. The container and CI path.
2. `api_key` in `~/.svipall/config.toml`, if you pinned one.
3. `~/.svipall/api_key`, written by an earlier run.
4. Otherwise one is generated, written to `~/.svipall/api_key` at `0600` where the platform allows,
   and **printed once** on the run that made it.

It lives beside `config.toml` rather than in it because `Config` is not serialised: writing a
generated key back would mean rewriting a file you edit by hand, comments and all. `secrets.env`,
`pools.json` and `domain_tiers.json` are already the same pattern.

**Two checks run before the key**, and they are what makes a loopback bind mean something. A page
you have open at `evil.test` can be served a DNS answer of `127.0.0.1`; its script then posts to
`http://evil.test:8788/v1/route` and the *browser* delivers it to loopback — binding to `127.0.0.1`
does not help, because the request originates on your box. The key is the real defence, since a page
cannot read it, but a `POST` with a `text/plain` body is preflight-exempt and is therefore *sent*
regardless, which is enough for an endpoint with side effects. So:

- any request carrying an `Origin` header is **403**. No browser page is a legitimate client of this
  API, there is no CORS layer, and there will not be one;
- on a loopback bind, a `Host` that is not `localhost` / `127.0.0.1` / `[::1]` is **403**. The
  attacker's page has to send `Host: evil.test`, and that is the tell.

On a non-loopback `rest_bind` the `Host` check is skipped — your LAN address is a legitimate host —
and a warning is printed at startup saying the key is now the only thing between the network and
this machine's profiles.

## Routes

| Route | Body |
|---|---|
| `POST /v1/fetch` | `web_fetch`'s params |
| `POST /v1/fetch_many` | `web_fetch_many`'s |
| `POST /v1/crawl` | `web_crawl`'s, plus `"async": true` |
| `POST /v1/search` · `/v1/site_search` · `/v1/map` | finding things |
| `POST /v1/snapshot` · `/v1/act` · `/v1/capture` · `/v1/screenshot` | a real browser |
| `POST /v1/solve_and_continue` | the captcha, answered on the blocked page |
| `POST /v1/diff` · `/v1/watch` · `/v1/notes` · `/v1/log` | memory |
| `POST /v1/route` · `/v1/profile` · `/v1/browser_setup` | configuration |
| `GET` / `POST /v1/status` | `GET` is read-only by construction: `WebStatusParams` carries three *clearing* fields, and a `GET` is wired to defaults so it cannot reach them |
| `GET /v1/health` | the one route with no key, so a container healthcheck does not need one |

`/v1/screenshot` returns the JSON `web_screenshot` returns — including `path`, since the PNG is
written to disk either way — plus `image_base64` when the picture is small enough to be worth
carrying. That threshold is decided once, in the seam, so MCP and REST cannot disagree about it.

### Not routes, and why

`rest::NOT_IN_REST` records the reason beside each name, and a conformance test fails the build if a
new tool is neither a route nor listed there.

- **`browser_open` / `browser_do` / `browser_close`** — a session is a resource HTTP cannot bound.
  It would work: the browser pool is shared, so a session opened by one request is visible to the
  next. That is the problem. A client that dies between `open` and `close` leaks a real browser
  process with no TTL and no cap. `web_act` covers the single-shot case, which is what a REST caller
  almost always wants.
- **`web_login`** — opens a visible window on your desktop and waits up to an hour for a person. An
  HTTP request must not be able to do that.
- **the `solve_*` token family** — already answers on `dashboard_port` in the 2captcha wire shape.
  Two contracts for one queue, with two auth schemes and two error conventions, is how a caller ends
  up unsure which one it is talking to.

## Long crawls

A crawl of two hundred pages is minutes. Synchronous is still the default — it is what MCP does and
what a script expects — but `"async": true` hands back an id instead.

```bash
curl -sH "Authorization: Bearer $KEY" -H 'content-type: application/json' \
     -d '{"url":"https://docs.example/","max_pages":200,"async":true}' localhost:8788/v1/crawl
# 202, Location: /v1/jobs/9f2c1a4be7d0435a
# {"job_id":"9f2c…","kind":"crawl","state":"queued","stream":"/v1/jobs/9f2c…/stream"}
```

| Route | |
|---|---|
| `GET /v1/jobs?state=&limit=` | a listing. **Never carries `result`** — ten finished 200-page crawls would be tens of megabytes of pages nobody asked for twice |
| `GET /v1/jobs/{id}` | the row, plus `result` once it is terminal. While it runs, `pages_done` is the honest partial answer; you are never handed a fabricated result |
| `GET /v1/jobs/{id}/stream` | the same job as Server-Sent Events |
| `DELETE /v1/jobs/{id}` | ask it to stop |

### The id is the crawl id

For a crawl, `job_id` **is** `crawl_id`. There is one handle to learn, one id in every log line, and
resuming is the vocabulary the MCP tool and the CLI already use:

```bash
curl -sH "Authorization: Bearer $KEY" -H 'content-type: application/json' \
     -d '{"crawl_id":"9f2c1a4be7d0435a","max_pages":400,"async":true}' localhost:8788/v1/crawl
```

The server mints the id; `POST /v1/crawl` does not accept a new one. `resume_or_start` does not
validate a supplied id — an unknown one silently starts a fresh crawl under it — so letting a client
choose would be letting it choose this server's primary key.

### States

```
queued ──claimed──> running ──┬── finished     the frontier drained
                              ├── stopped      a budget: max_pages, time, tokens, saturation,
                              │                or over_budget — this address has spent its
                              │                standing with the site (docs/exits.md)
                              ├── cancelled    the crawl saw the flag
                              ├── interrupted  the process that owned it is gone
                              └── failed       the task panicked
```

**`interrupted` is the state this whole design exists for.** A crawl was always resumable —
`crawl_queue` has survived a kill since crawls were written — but `crawl.status` is set to `running`
per batch and *nothing ever clears it*, so there was no way to tell a crawl that died from one that
finished. The job row is that missing fact. It is detected by two things together: the row is owned
by another run, **and** its heartbeat has gone quiet for five minutes. Both halves are needed,
because two svipall processes can share one database, and because `crawl.updated_at` is written per
batch — a level of two hundred pages can be half an hour between writes. The heartbeat is written
per page.

An `interrupted` job is resumed like any other: submit its id again.

### Cancelling

`DELETE` sets a flag in the row — durable, survives a restart — and, if the job is running here,
flips the one the crawl reads. The crawl checks it at two points, both places it is already between
things: the top of a batch, and the end of a page *after that page's links have been queued*. It
then leaves through its ordinary exit, so the frontier is written and the id can be picked up again.

It never aborts the task. `web_act` and `web_capture` close their browser page on the normal path
only, so killing a fetch mid-flight would leak a CDP page into the pool.

`DELETE` answers **200** with the state the job was in, including for a job that had already
finished: you wanted to know whether you stopped a running crawl or reaped a finished one, and both
are answers.

### The stream

```
event: snapshot
data: {"id":"9f2c…","state":"running","pages_done":41,"pending":118, …}

event: progress
data: {"job_id":"9f2c…","kind":"page","pages_done":42,"total":200,"queued":117,
       "url":"https://docs.example/guide","tier":"http","status":200}

event: done
data: {"id":"9f2c…","state":"finished","pages_done":57,"pending":0}
```

The first frame is **always** a `snapshot` built from the store, never from the channel. That is the
one way this stream could lie: a subscriber joining at page forty and inferring "this started at
zero" from the first live event would draw a bar from the beginning of work that is nearly done.

It is a snapshot rather than a replay, deliberately. Full replay needs a durable event log written
per page and trimmed by the housekeeper — real cost for a consumer that is a progress bar — and what
a late subscriber actually needs is where the job is *now*, which is already durable and exact.
There is no `Last-Event-ID`.

A `progress` event never carries the page. A crawl of two hundred pages that put its markdown in
every event would stream its whole result twice; each event stays under half a kilobyte, and a test
enforces that.

A subscriber that falls behind gets `event: lagged` with how many it missed, not a closed
connection. And a listener can never slow or stop a crawl: the send is non-blocking and errors when
nobody is listening, which is the ordinary case.

Subscribing to a job that has already ended gives you `snapshot` then `done`, then the stream closes
— never a hang. An unknown id is a 404 before any stream is opened.

### One crawl per site

`max_jobs` (default 2) caps how many jobs run at once. Deliberately not `parallelism`, which bounds
requests *inside* one job: two crawls at parallelism 4 is eight in-flight fetches and, on a browser
tier, eight Chrome pages — more than `capacity::concurrency` would ever grant one crawl.

On top of that, **a queued job whose site already has a job running is not started.** The pacer
already prevents a burst — `throttle` keys `(domain, exit)` and concurrent callers each reserve the
next free slot — but what it cannot prevent is two jobs spending one address's reputation with one
host twice as fast. That is the scarcest thing a local-only tool has: this project's own benchmark
lost a target by running its lists eight times against one address in a day. The rule is decided by
the database, from the domain recorded when the job was queued, so a restart cannot forget it.

That incident now has a mechanism behind it rather than only a rule: `core::reputation` keeps what
each address has spent with each host, decaying with a half-life, and slows down before declining.
See [`docs/exits.md`](exits.md). `max_jobs` and the one-crawl-per-site rule still matter — they stop
two jobs spending the same budget twice as fast — but the budget is what bounds the total.

## Configuration

```toml
rest_port = 0                # 0 = off. `svipall serve` starts it anyway; this is what makes
                             # svipall-mcp mount it too.
rest_bind = "127.0.0.1"
api_key = ""                 # empty = ~/.svipall/api_key, generated on first use
max_jobs = 2
```

`SVIPALL_REST_PORT` and `SVIPALL_API_KEY` override the first and third.

## What `svipall serve` does not do yet

The 60-second housekeeping loop that reaps idle browsers, trims the request log and evicts old
profiles lives in `svipall-mcp`'s `main`, and has not been moved yet. Under `svipall serve` the job
runner keeps its own house — orphan adoption, job expiry — but browsers are not reaped and the log
is not trimmed. Run the REST API from `svipall-mcp` with `rest_port` set if that matters to you.

`solve_and_continue` under `svipall serve` also has no human dashboard behind it: the CLI opens no
solver database. Local models still answer; a challenge none of them can answer has nowhere to go.
