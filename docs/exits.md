# Exits

An exit is a proxy the operator supplies. svipall never ships one, never buys one, and never
resolves your address to a country — a country is *declared*, not detected, and an exit whose
country nobody declared simply has none.

This is also, honestly, the ceiling: the four targets the benchmark does not clear are decided by
IP reputation, and the answer to all four is an exit. A local-only tool cannot provide that for
itself.

## Declaring a pool

`web_route` takes a domain, one or more proxy URLs and, optionally, the country each one is in:

```
web_route domain=example.com proxies=["socks5h://127.0.0.1:9050"] countries=["de"]
```

Pools live in `~/.svipall/pools.json` and are inherited by subdomains: a pool on `example.com`
serves `shop.example.com` unless that name has one of its own.

Two strategies (`exits.rs`):

* **`sticky`** (default) — the domain keeps the exit it last used while that exit is usable. A site
  that watched one address build a session should keep seeing it; rotating on every request is the
  pattern a scoring panel is built to notice.
* **`round_robin`** — take the next usable exit after the last one. For throughput, when the site
  does not track sessions.

Either way a retired exit is skipped, and **when every exit is retired the healthiest is used
anyway**. Refusing to fetch is not a strategy.

## Health, strikes and healing

Health is kept per `(domain, exit)` in `~/.svipall/exit_health.json`, never per exit alone: an
address burnt on one site is untouched on the next, and treating it otherwise throws away most of
what a pool is worth.

The arithmetic (`session.rs`):

| | |
|---|---|
| Full health | 100 |
| Retired below | 40 |
| A block costs | 35 |
| A rate limit costs | 10 |
| A clean fetch gains | 3 |

So two blocks retire an exit for that domain (100 − 70 = 30). What counts as a block is
`Verdict::of`: an empty 200, a redirect from a deep URL back to the root, and a response eight
times slower than that exit's own average all count, because all three are how a site says no
without saying it.

**Health heals with time** — one point per 600 seconds idle, capped at full. A twice-blocked exit
is usable again after roughly ninety minutes of not being used, which is long enough that whatever
score retired it has likely moved on. A dead exit list that never recovers is a pool that shrinks
to nothing over a week.

## Pacing

`throttle.rs` keys pacing and strikes by `(domain, exit)` as well, so ten exits actually buy
throughput instead of ten addresses sharing one gap. Per-domain alone are the two things that
really are about the site rather than the address: `Retry-After` (honoured up to two minutes) and
the hard cooldown after that (fifteen minutes).

The gap between requests is tuned from the host's own latency and refusals, with floors and
ceilings per tier — 100 ms to 2 s on `http`, 400 ms to 5 s on the browser tiers — and a strike
backs off exponentially, `base * 2^min(strikes, 4)`.

A pool of one is deliberately not tracked *for health*: with nowhere else to go, a strike against
the only exit is just a strike against the domain. Spend is different, and is tracked for every
address including the machine's own — see below.

## The reputation budget

Health is what a site has said about an exit. Spend is what an address has done to a host, and it
accumulates whether or not anything has been said yet. Nothing counted it until now, and this
project's own benchmark lost a target to that: the lists were run against one residential address
several times in a day, and a cell that had been passing stopped.

`reputation.rs` keeps spend per `(domain, exit)` in `~/.svipall/reputation.json`, and **the machine's
own address is a key like any other** — which is the whole point, because the case that went wrong
is the one with no proxy at all.

A visit costs what its tier costs, doubled when the page comes back walled:

| `http` | `browser` | `stealth` | `real` | `warm` |
|---|---|---|---|---|
| 1 | 3 | 4 | 8 | 12 |

Not a count of visits: an HTTP GET and a headful browser waiting twenty seconds for a challenge are
not the same event to a host that scores addresses, and pricing them alike would either make
crawling impossible or make challenges free.

**Spend decays rather than resetting**, halving every `reputation_half_life_hours` (six by
default). A window would have a cliff — at midnight, eight visits become free again — and would
need a list of timestamps; a half-life is one number and one instant. It also means
`reputation_budget` (250 by default) is a **rate**, not a daily total: a steady spend settles at
`budget * ln 2 / half_life`, about 29 points an hour.

What it does:

* under 70% of the budget, nothing;
* from 70% to 100%, the pacer's gap is stretched continuously, up to four times — capped there
  rather than higher because the browser tiers already wait seconds and a crawl has a deadline;
* over the budget, the fetch is declined before anything goes out, with `blocked_reason:
  "address_budget"`, how many seconds until it is not, and the way out.

**This amends "refusing to fetch is not a strategy" above.** That rule is about exit *health*, and
it stands: a burnt pool still hands back its healthiest exit. A spent budget is a different claim —
not "this address is refused" but "we have asked enough for now" — and it is the one thing a
local-only tool can do about the scarcest resource it has. It stays a labelled answer with an
escape hatch, never a silence: `web_status(clear_budget="DOMAIN")` empties it, `web_route` moves to
another address, and `reputation_budget = 0` turns the whole mechanism off.

An exit that has spent its budget is passed over by the pool exactly as a retired one is, so a
domain with somewhere else to go goes there instead of being declined. The solver is the exception,
and deliberately: it keeps the sticky exit because the challenge it is answering is on the page that
exit was shown.

A crawl that meets the budget stops with `stopped_by: "over_budget"` and puts what it did not fetch
back on its frontier. A URL declined before a request is not a URL that has been fetched, and
recording it as one would lose the page for that crawl id forever.

## Leaks

* **DNS.** `socks5://` resolves names on *this* machine and sends the answer through the proxy,
  which puts every hostname you visit in your resolver's log and your ISP's. `socks5h://` resolves
  at the far end. `web_route check` flags the difference rather than silently fixing it, because
  the two are different requests and only the operator knows which was meant. With no proxy at all,
  `dns_over_https` closes the same hole at the browser (`--dns-over-https-mode=secure`, never
  `automatic`: a mode that falls back to plaintext is not a mode).
* **WebRTC.** Behind a proxy the browser is launched with non-proxied UDP disabled, so a STUN
  request cannot report the real address; without one it is limited to the default public
  interface.
* **Credentials.** `user:pass` in a proxy URL is split off and handed to the browser over CDP.
  Chrome cannot read userinfo from `--proxy-server` — it pops a 407 dialog instead — and the
  argument vector is readable by every other process on the machine.
* **Locale.** The exit's declared country sets the identity's timezone and language. A proxy in
  Frankfurt with a New York clock is a contradiction that costs nothing to avoid.

## What is not here

There is no exit svipall can create for itself: no Tor control port, no VPN interface binding, no
device on the LAN. A `socks5h://127.0.0.1:PORT` from an `ssh -D` tunnel or a local Tor daemon works
today as an ordinary proxy URL, but nothing supervises that tunnel, and `Health` cannot yet tell
"the tunnel died" from "the site blocked me" — both arrive as a failure and cost the exit 35 points.
