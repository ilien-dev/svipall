# Baseline — 2026-09-02

The historical numbers below retain their original protocol and raw logs. The separate
[2026-09-05 local comparison](../experiments/local-20260905/README.md) records every first and
returning visit, invalid-status failures and delivered content. It uses saved before/after binaries
with the same harness and model assets. Its scores must not be substituted into these old tables:
the older verdict could label a status-zero response `ok`, and repeat handling also differs.

Regenerate this historical suite with:

```
cargo run -p svipall-bench --release -- fingerprint                > baseline/fingerprint.json 2> baseline/fingerprint.txt
cargo run -p svipall-bench --release -- tells --assert             > baseline/tells.json       2> baseline/tells.txt
cargo run -p svipall-bench --release -- evasion --set hard12   --runs 3 > baseline/evasion.json     2> baseline/evasion.txt
cargo run -p svipall-bench --release -- evasion --set public31 --runs 3 > baseline/public31.json   2> baseline/public31.txt
```

## Two lists, two rules, never one number

`hard12` is Svipall's own list: twelve sites chosen because they have walls, scored by whether the
expected text came back with no wall reported. `public31` is the list an independent benchmark
published in May 2026 (seven stealth tools, 31 targets, three sweeps, 651 verdicts), scored with
that benchmark's own four-way rule (`ok | gated | blocked | error`), ported verbatim into
`bench/src/targets.rs` so a cell here means what a cell there means.

Twenty-five of the 31 public targets pass for every tool including unpatched automation; the signal
lives in six cells. Nine of the twelve targets here are walls. A 9/12 and a 28/31 are therefore not
the same kind of number, and quoting one against the other — in either direction — is reading noise
as signal. Both are published, each with its list, so nobody has to.

Every evasion figure below and from now on is the **median of three runs with its range**, targets
in a fresh random order each run, cooldowns cleared first. A change counts as an improvement only
when the median moves outside the previous range.

The mistake this section corrects: an earlier baseline reported the post-quantum key share as a
known gap because the fingerprint check looked for the group in `ja4_r`, which lists ciphers,
extensions and signature algorithms and never supported groups. The engine had been offering
`X25519MLKEM768` all along; the check now reads `supported_groups` and `key_share` and asserts it.

## Fingerprint — 24/24

Every check passes against `tls.peet.ws` and a real browser. Grew from 16 during Phase 3: the
Runtime-domain watchdog, `navigator.languages`, `connection`, the heap ceiling, `devicePixelRatio`,
and a stability check on text geometry that caught a real bug in the noise patch — it was redrawing
per call, so one element measured twice gave two different widths. The post-quantum key share was
listed here as a known gap; see "Two lists, two rules" above for why that was the check, not the
engine.

## Evasion — 5/12 (three runs: 6, 5, 5)

The first time this was ever measured. It is the number the competition publishes and Svipall did not.

| outcome | targets |
|---|---|
| passes every run | example.com, news.ycombinator.com, en.wikipedia.org, **nowsecure.nl** (Turnstile, cleared at the `browser` tier), indeed.com (Cloudflare, cleared at `real`) |
| fails every run | g2.com and idealista.com (both the same fingerprinting vendor, ~25 s each at `warm`), amazon.com, newegg.com, crunchbase.com, stackoverflow.com |
| flaky | zillow.com — cleared once at `real`, held twice at `warm` |

### What the failures actually say

**Two of them are a shell-page gap, not a wall.** amazon and newegg come back `200` with a full page
of navigation — 13,218 and 40,437 characters of real text — and `blocked_reason: null`. They are not
challenge pages; the classifier is right that no wall is present. What is missing is the *content*:
product listings that only exist after JavaScript runs. The ladder sees a healthy page and stops at
the `http` tier, so it never escalates to a tier that would render them.

Svipall cannot currently tell "I got the chrome but not the article". The existing shell-page rule
(`classify.rs:253`) needs `body < 3_000` chars and a text/HTML ratio under 2%, and a navigation
shell of 13k characters clears both. Closing that is the cheapest available gain and belongs in the
classifier work, not in stealth.

**stackoverflow is a genuine block**: `403` at the `http` tier, and the higher tiers return a page
without the expected content.

**The vendor walls are honest failures.** Both fingerprinting-vendor targets are detected correctly
and never yield, after burning ~25 s each at the top tier. That is ~75 s per run spent on walls that
were never going to open.

**Turnstile is a real strength.** It clears at the `browser` tier, in under two seconds, every run.

## Two bugs this run found in the bench itself

- Success required `body.len() > 200`, so example.com — whose entire markdown is 167 characters —
  was scored as a failure while being fetched perfectly. The criterion is now "the expected text is
  there and the classifier did not call it a wall".
- A run put blocked domains on a 15-minute cooldown, so the next run skipped them without making a
  request and scored better for a reason unrelated to evasion. Cooldowns are now cleared at the
  start of every run.

---

## Re-measured after the stealth work

Same twelve targets, same machine, three runs each time.

| | runs | median |
|---|---|---|
| before | 6, 5, 5 | 5/12 |
| after | 6, 5, 6 | 6/12 |

**That is not an improvement.** It is one target inside a spread this benchmark already showed
between two runs minutes apart, on a sample of twelve. Reporting it as a gain would be reading noise
as signal.

What did move, and is measured: the fingerprint checks went from 16 to 23, all passing, including
two that did not exist before — a watchdog on the automation protocol, and a stability check on text
geometry that caught a real bug in the noise patch.

### Why the stealth work did not show up here

Worth writing down, because it is the useful part:

- **Six of the twelve failures are not about stealth.** Two are the same fingerprinting vendor,
  which never yields at any tier. Three come back `200` with a full page of navigation and no
  article — a rendering problem, not a wall. One is a plain `403`.
- **The bench runs from one address with no proxy.** The largest pieces of work — WebRTC no longer
  leaking the real address, identities drawn per session, sessions retired when a domain turns on
  them, the country moving with the exit node — only do anything once there is more than one
  identity and more than one exit. This benchmark exercises none of that.
- **Twelve targets is too few to see a small change.** Distinguishing 42% from 50% at this sample
  size needs many more runs than three.

So the honest reading is: the fingerprint is measurably better, the session and identity work is
untested by this benchmark, and the evasion rate is unchanged on these twelve sites. Widening the
target list and adding a proxied variant is what would make the next number mean something.

---

## After the widget, captcha and platform work

Re-measured with everything from the second plan in place: the widget table and its generic
detection, the strategy loop, the audio path, the restructured dashboard, the ad and origin
policies, export, notes, and depth-first crawling.

| | baseline | after stealth | now |
|---|---|---|---|
| fingerprint | 18/18 | 23/23 | **23/23** |
| evasion | 5/12 (runs of 6, 5, 5) | 6/12 | **7/12** |

**7/12 is one target better than the best previous run and three above the worst.** On twelve
targets, over single runs, that is still inside the spread this benchmark showed between two
executions minutes apart. It is recorded as the number it is, not as a trend.

The composition of the failures has not changed and is still the honest story:

- `captcha-delivery.com` on two sites: never yields, at any tier, from one address.
- Amazon and Newegg return a real page with a `200` and a navigation shell; the products are
  rendered by script the extractor does not run. That is a rendering gap, not a wall — the fix is
  `web_snapshot` or `web_capture`, both of which now exist.
- StackOverflow is a genuine `403`.

Nothing in this stretch was aimed at the evasion number, which is why it is reported flat. What did
change is what the tool can do once it is past a wall, and that is not something this benchmark
measures at all.

---

## HTTP/3: measured, and abandoned on the evidence

The plan carried an explicit abandonment criterion for HTTP/3 — *"if `bench fingerprint` does not
confirm a Chrome QUIC fingerprint, the engine is not published"* — on the assumption that the cost
was effort. It is not. It is a link conflict, and it is total.

`quiche` reaches BoringSSL through `boring-sys`. `wreq`, the engine that gives the http tier its
Chrome TLS and HTTP/2 fingerprint, reaches BoringSSL through `btls-sys`. Both declare
`links = "boringssl"`, and Cargo permits exactly one package per graph to do that:

```
package `boring-sys` links to the native library `boringssl`, but it conflicts with a previous
package which links to `boringssl` as well: package `btls-sys v0.5.6`
```

Building `quiche` with `default-features = false` gets past resolution and fails at link time
instead — 83 unresolved BoringSSL symbols — because there is then no TLS library under it at all.

So the three ways forward were: drop `impersonate` (trade a fingerprint measured at 23/23 for one
that cannot be measured), use `quinn` over rustls (ship a QUIC fingerprint that is demonstrably not
Chrome's, which the plan itself calls worse than not announcing h3), or stop.

**Stopped.** Recorded here rather than left as an open item, because the constraint is a property of
the dependency graph and will not change by trying harder.

One more measurement worth keeping: the fingerprinting endpoint the bench uses reports `tls`,
`http1` and `tcpip` sections and **no QUIC section at all**. Even with an engine in hand, the
abandonment criterion could not have been evaluated against it without first finding a service that
reports QUIC fingerprints — which is a second dependency the project does not want.

### Final run, and why the evasion number is reported as a range

Re-measured after phases 9 and 10: fingerprint **23/23**, evasion **6/12**.

The run before it, on the same code, was 7/12. That is the whole argument for not reading this
benchmark as a trend line: two runs of identical code, minutes apart, differ by one target out of
twelve. The honest statement is that the evasion rate on these twelve sites sits at **5–7 of 12**
and has not moved measurably across the entire second plan.

What did move, and is stable across every run: the fingerprint, from 18 checks to 23, all passing.

---

## Two bugs in the extractor, and the first real movement in the evasion number

Asked whether the evasion rate could go higher, the honest way to answer was to look at what the
five failures actually were rather than assume they were five anti-bot problems. Three of them were
not about anti-bot at all.

**A page the extractor emptied out was reported as a successful fetch.** A shop listing arrived
intact — `200`, correct title, 1.4 MB of markup — and came back as *zero characters*. A listing has
no prose, so its items look to the density pruner exactly like the navigation that pruner exists to
remove; with no `<main>` it trusted, everything was dropped. Reproduced offline against the saved
page: `main_content_only=true` gave 0 characters, `false` gave 45,551. `extraction::markdown_from`
now falls back to the whole document when the heuristics leave nothing, and only then.

**A response with no document at all was classified as a delivered page.** A page with an empty
`<body>` was caught by the classifier; a response with *no bytes whatsoever* fell through every
check and returned `(None, WallKind::None)` — a success. That stopped the ladder from climbing and
handed the caller zero characters with no reason given. Two real sites did this: a `302` with an
empty body, and a `200` from a tier that had been quietly refused.

Both were the same shape of failure: **the tool was reporting success for pages it had not got.**

| | before | after |
|---|---|---|
| fingerprint | 23/23 | 23/23 |
| evasion | 5–7 of 12 | **8–9 of 12** (three runs: 9, 8, 8) |

Three targets converted and they stayed converted across all three runs: the shop search page and
the component listing now escalate to a browser tier and return their products, and the developer
Q&A site now climbs `stealth -> real -> warm` instead of stopping at the first tier that handed it
nothing.

**What did not move, and will not from here.** The two `captcha-delivery.com` sites fail in every
run, at every tier, after 24 seconds. That is a fingerprinting wall combined with IP reputation, and
this benchmark runs from a single residential address with no proxy. The remaining variance —
Crunchbase and Zillow each failing in some runs and passing in others — is the same noise band the
whole file has been documenting, not a regression.

---

## The warm tier learns to act, and to keep still

Asked to push the evasion number as far as it would go, the three remaining failures were looked
at one by one rather than as "anti-bot".

**The strategy loop was never running during a fetch.** Everything built to answer challenges —
press-and-hold, the hash puzzles, the audio path — ran only from `solve_and_continue`. A plain
`web_fetch` at the warm tier nudged the page and hoped. Now every turn of the warm wait is a turn
of the strategy loop, so a press-and-hold is answered in the fetch that met it.

**On a challenge that verifies the visitor by itself, the right move is no move.** A managed
interstitial that reads "Just a moment" or "verifying your session" is watching the visitor; a
person reading it waits. The pointer movement and scrolling the tool made on every turn of that
wait was the script's tell, not the person's. The warm tier now keeps still on such pages, and
extends its deadline once when the page reports progress ("verification successful, waiting for
the site to respond"), because a pass already earned is not worth throwing away for a timer.

**A vendor's hard block was costing the whole budget.** The fingerprinting vendor's interstitial
carries its verdict in the top document — `'t':'bv'`, blocked visitor — while the words explaining
it sit in a frame in another process that this session cannot read. Reading the verdict ends the
wait in four seconds instead of twenty-four, with the honest reason: the address is refused, and
another exit is the only move.

Also tried and measured: a retry of the last rung as a fresh visitor — new profile, a different
machine from the fleet. It doubled the cost on the target it was built for and did not change the
answer, so it was removed from the ladder. The machine rotation stays, tied to `isolated`, where
"nothing carried in" should include the hardware.

| | before | after |
|---|---|---|
| fingerprint | 23/23 | **23/23** |
| evasion | 8–9 of 12 | **9–10 of 12** (runs: 9, 9, 10) |
| time spent on a hard block | 24 s | **4 s** |

The developer Q&A site and the property-listing shop now pass in every run. The managed-challenge
site passes in some runs and not others, and the pattern is the address, not the code: it passed
at six seconds early in the day and refused a fresh profile wearing a different machine after
fifteen visits in an hour. The two fingerprinting-vendor sites name this IP on the page. From a
single residential address without a proxy, 10 of 12 is the ceiling this benchmark can show; the
tool's own answer to the last two — `web_route` — is the one thing it cannot supply for itself.

### Polish, measured

`bm25_filter` counted every token of every block into two hash maps per block, for a query of
three words. It now counts only the query's terms in one pass: 3.3 ms to 1.8 ms on the full-page
fixture, output identical. `once_cell` is gone from `svipall-core` in favour of the standard
library's `LazyLock`; the markdown walker no longer allocates a `String` per link, image and
list item on the way to appending it; and `cargo build --release` now uses thin LTO, since that
profile is what the CLI and these benchmarks run.

---

## The environment was in the way

Asked once more to push the number, the two fingerprinting-vendor sites were opened by hand and
read rather than retried. What came out was not about the tool.

**The browser was the tell.** The machine's default browser is one that ships its own
anti-fingerprinting, and the pool had been warning about it on every start: under automation it
contradicted the Chrome identity Svipall advertises, and the vendor answered every browser tier with a
hard block (`'t':'bv'`) while answering the bare http tier with a solvable challenge (`'t':'fe'`).
`svipall browser install` now fetches Chrome for Testing from the shell, and the pool prefers it once
it is there.

**The antivirus was in the page.** With that browser, every page carried a stylesheet and a
script injected by a local security product, and on the vendor's silent device-check page the
only network traffic the page ever made went to that product — the vendor's own check never left
the machine. Any site's script sees an injection like that. Svipall now names it in the note of a
blocked result and tells the operator what to exclude. With the browser and Svipall excluded from the
product's web protection, the injection was gone in the next measurement.

**The version was wrong.** The managed browser's version sits in an ancestor directory and
`--version` prints nothing on Windows, so the identity fell back to a default major and the user
agent named a Chrome five versions older than the engine running. Fixed, with a test.

**And three bugs of Svipall's own, found on the way.** A fetch could hang after printing its answer,
because the CLI never shut the browser pool down. A blocked page ignored `out_file`, so the one
document an operator most wants to read was the one they could not get whole. `web_snapshot` died
entirely when one node on a verification wall threw inside its walk. All three fixed and tested.

**What the strategies learned.** The press-and-hold vendor shows a styled `div` reading "Press &
Hold" that does nothing while its real button — an iframe — is still hidden; the hold landed on
the decoy, spent the only attempt, and the loop gave up. The strategy now waits for the frame, the
hold budget is two, and the loop distinguishes "nothing handles this" from "nothing can yet". A
slider strategy exists now too, read entirely from a screenshot because the vendor's frame lives in
another process.

| | before this round | after |
|---|---|---|
| fingerprint | 23/23 | **23/23** |
| evasion | 8–10 of 12 | **8–10 of 12** |
| identity vs engine | Chrome 147 on a 152 engine | **consistent** |
| local injection | present on every page | **gone, and detected when present** |

The number did not move, and the honest reason is that what remains is decided on the other end.
The fingerprinting vendor still refuses this address (`bv`) with a clean browser and no injection —
the address earned that today, one benchmark run at a time. The press-and-hold vendor's collector
answers `"do":null` and keeps its button hidden; that is a verdict on the session, taken
server-side. The managed-challenge site passes and fails by the visit. From one residential
address with no proxy, this benchmark cannot show more than it does, and the tool's own answer to
that — `web_route` — is the one thing it cannot supply for itself.

### The profile the wall remembered

The press-and-hold vendor keeps the session it once flagged in the cookies of the persistent
profile. Opened on a profile nobody had seen, the same page cleared twice out of two; on the kept
one it never did. So a hold that will not clear on a persistent profile now earns one retry on a
fresh one, and when that works the flagged profile is retired — the browser holding it closed
first, because on Windows the directory is locked while it runs. Measured end to end:
`hold on the kept profile → retry on a fresh one → OK in five seconds → profile retired`.

And the page behind it was a second extractor lesson: for this address the listing carried no
listings, its trusted main region was the map, and what came back was the map's attribution line
— a tenth of a page that was itself a few hundred characters. On a page that small the whole of it
is the answer, so a fragment under four hundred characters on a page under four thousand now
yields the whole page. A short notice on a large site stays a short notice; that case is tested.

| | before | after |
|---|---|---|
| evasion | 8–10 of 12 | **9 of 12**, the press-and-hold site now passing in every run |

What remains is decided on the other end: the fingerprinting vendor's `bv` for this address, and
the managed-challenge site passing or failing by the visit.

---

## HTTP/3, examined a second time (2026-09-03)

The reason on record was a linking one: `quiche` and the emulating TLS engine both link
BoringSSL and Cargo permits one. That is no longer true — the engine's binding builds BoringSSL
with prefixed symbols, so two copies can share a binary — and it was never the real reason.

The real one is shape. A QUIC connection's ClientHello has to look like Chrome's as much as the
TCP one does, and Chrome's carries two extensions that no Rust QUIC stack's TLS API can produce:
application settings (ALPS, 17513) and an ECH GREASE (65037). Cipher list, curves, signature
algorithms, GREASE values and extension permutation are all settable; those two are not. A QUIC
handshake with two extensions fewer than Chrome's is a QUIC handshake that says "not Chrome" in
the first packet, on every site that looks — and the sites that offer h3 are exactly the ones
behind CDNs that look.

So h3 stays off, and this time the file says why in terms that can be re-checked: when a Rust
TLS binding exposes ALPS and ECH GREASE for QUIC, the gate opens. Until then Svipall records
which domains offer h3 (`Alt-Svc`, `core::altsvc`, shown by `web_status`) so that the day an
engine exists there is a list to measure it against.

---

## Re-measured after the integration work (2026-09-03)

Everything from the third plan in place: typed tables and documents, self-healing selectors,
scroll-until-stable, exit pools, the corpus and the two new solver strategies, `raw:`/`file://`.
Same machine, one residential address, no proxy, three runs per list, targets shuffled, cooldowns
cleared.

| list | runs | median | range | hard blocks |
|---|---|---|---|---|
| `hard12` | 9, 8, 8 | **8/12** | 8..9 | — |
| `public31` (independent list, its own verdict rule) | 23, 24, 24 | **24/31** | 23..24 | **0** |

**hard12.** Same composition as before: `captcha-delivery.com` on two sites never yields from one
address, crunchbase is per-visit-per-address, and indeed is now the flaky one (1 of 3) where
zillow used to be — zillow passed every run. Inside the spread this list has always shown; not a
change in either direction.

**public31, for the first time.** Twenty-four of thirty-one with zero hard blocks; the seven that
do not pass are all `gated` (an interstitial or a panel that scored us), never `blocked`. On the
published matrix that is the profile only one of seven tools had — the one that drives Chrome over
CDP with no automation shim — and the count sits with the Playwright forks. Two of the seven
gates are a `Just a moment…` the `http` tier was scored on before the ladder climbed; the public
rule takes the final response of one attempt, and Svipall's own rule would have escalated. That is
a real difference between the two rules, recorded here rather than adjusted away.

Both numbers are the numbers they are. Neither is the other.

---

## Three lists now, and the one number that was never measured (2026-09-04)

Everything from the fourth plan in place: models compiled into the binary, the strategy loop
learning per route, the dashboard finishing a live challenge, exits keyed by `(domain, exit)` with
health that heals, browser-tier proxy authentication, a Firefox identity for the http tier, and an
offline identity-coherence gate.

Regenerate with:

```
cargo run -p svipall-bench --release -- fingerprint                      > baseline/fingerprint.json 2> baseline/fingerprint.txt
cargo run -p svipall-bench --release -- evasion --set hard12   --runs 3  > baseline/evasion.json     2> baseline/evasion.txt
cargo run -p svipall-bench --release -- evasion --set public31 --runs 3  > baseline/public31.json    2> baseline/public31.txt
cargo run -p svipall-bench --release -- evasion --set vendors8 --runs 3  > baseline/vendors8.json    2> baseline/vendors8.txt
# Not a baseline file: the Chrome reference that `crates/svipall-quic/tests/settings.rs` asserts
# against. Offline, but it starts a browser, so it is regenerated by hand like the rest of this list.
cargo run -p svipall-bench --release --features http3 -- h3-ref
```

### Measured, 2026-09-04

| list | runs | median | range |
|---|---|---|---|
| `hard12` | 8, 8, 8 | **8/12** | 8..8 (was 8..9) |
| `vendors8` | 3, 3, 3 | **3/8** | 3..3 — first measurement |

`hard12` did not move, and its **range closed from 8..9 to 8..8**: the same eight targets passed in
all three runs, where previously indeed and zillow traded places between runs. Stability is not a
higher number and is not reported as one, but on a twelve-target list it is the only thing three
runs can honestly show.

`vendors8` — 3 of 8, the same three every run:

| vendor | target | result |
|---|---|---|
| proof-of-work | twitch | **passes every run**, `browser`, 1.8–5.3 s |
| proof-of-work | hyatt | fails; 29 s, then 62 s, then a 90 s timeout across the three runs |
| edge | newegg | **passes every run**, `browser`, 6–9 s |
| edge | homedepot | **passes**, `stealth`, 3.8 s — it escalates now |
| fingerprinting | g2, idealista | fail every run, ~32 s each |
| managed challenge | crunchbase, indeed | fail every run, ~21–32 s each |

**The proof-of-work vendor is passable, and one target shows exactly why it is hard.** Twitch clears
at the plain `browser` tier in under two seconds. Hyatt does not, and the way it fails is the
interesting part: 29 s, 62 s, then a timeout, across three runs minutes apart. That is the vendor's
documented behaviour — the puzzle gets harder for an address it has seen repeatedly — not a
regression in svipall. It was checked: an uncapped re-navigation loop would have produced the same
curve, so the reissue is now capped at one per wait, and the numbers were the same with and
without the cap.

The four that never pass are the two already named in `hard12` (a fingerprinting vendor that
refuses this address, and a managed challenge that decides per visit) plus their second targets.
Nothing here is new information about them; that is why they are in a separate list.

### `vendors8`: two targets each, four vendors, named

`hard12` and `public31` are frozen — their numbers only mean anything against their own lists, so
a new target goes in a new list. `vendors8` answers a different question: how does Svipall do
against each *vendor*, with the vendor named, two targets each so a single site cannot carry the
row. It is expected to score worse than `hard12`, which is the point of publishing it.

The vendor that had never been measured is the proof-of-work one. It is the only wall that is not
a page: no challenge, no widget, nothing to answer. Its script burns CPU to earn a token that lives
60–180 seconds, and **passing it once is not passing it** — the token has to be re-earned for the
life of the session. A stateless HTTP client cannot hold it at all. Svipall's warm tier holds a live
browser, so it can, and now does: `classify::warm_needs_reissue` re-navigates at 40 seconds, below
the observed floor with room to spare, rather than waiting out the budget and collecting a 403.

> **Wrong as published; see 2026-09-05.** It could not and did not, for two independent reasons.
> `is_proof_of_work_wall` only read the page body, and that vendor's tell is a header, so it never
> fired on the target at all. And the warm wait's budget is 20 seconds, so a clearance can never be
> judged 40 seconds old inside one. The measurement is in the later section; this paragraph is left
> standing because a baseline is a log of what was believed when, not a record to be tidied.

That is a structural advantage of the local-first design rather than a better spoof, and it is worth
naming as such.

### The proxied column

`evasion --exit URL` runs the same targets through an operator-supplied exit. Every number in this
file above was measured from **one residential address with no proxy**, which this README has named
as its own ceiling for four rounds without ever measuring past it. Now it can be: publish both
columns, and "Svipall cannot do this" is separated from "this address cannot".

The un-proxied column stays the headline, because it is the one anybody can reproduce without
buying anything.

### Identity coherence, offline and asserted

`fingerprint --engine chrome` checks every identity Svipall would wear against itself — engine ↔
user agent, client hints ↔ engine, screen ↔ availHeight ↔ viewport, form factor ↔ platform,
timezone ↔ language, renderer ↔ engine, and the macOS OS-token spelling that differs between the
two engines. Seven identities, no network, and it **fails the build** on a contradiction. It runs
in `qc` and in CI.

This is the check the leading patched-Firefox project says it keeps failing — not the spoofing
technique, the coherence between spoofed values. Here it is a test suite.

### Models, measured on the CPU

`micro --features onnx` times the embedded models on a 320 px picture, with the same 25% headroom
every other budget carries:

| check | measured | budget |
|---|---|---|
| detect / 320px picture (embedded) | 7.7 ms | 120 ms |
| segment / 320px picture (embedded) | 25.2 ms | 250 ms |

No GPU. A challenge takes seconds of network and animation on its own; tens of milliseconds of
inference is not what anybody waits for.

---

## The tells become a test, and a redirect that was never followed (2026-09-04)

Two of these numbers moved and one is new. The new one is the reason the others moved.

### `tells`: what a detector reads, asserted offline

`fingerprint` asks public detectors what they see, which needs the network, which keeps it out of
the gate. So there is now a second harness that asks the same question of a page the benchmark
serves itself on loopback, at **all four browser tiers**, and fails the build:

```
cargo run -p svipall-bench --release -- tells --assert
```

Fourteen probes × four tiers. **It opened at 22 of 52 clean.** Every failure was a real
contradiction that had been shipping, and the two most useful were ones nobody had thought to look
for:

| Probe | What it found |
|---|---|
| `residue` | `window.__svipall_console` — a ring buffer under a name that spelled out the product. Enumerating `window`'s own property names is the cheapest check a detector runs, and this was a direct hit |
| `dom_rect` | `getBoundingClientRect` jittered `x`, `width` and `height` but not `left`/`right`/`top`/`bottom`, so every rectangle disagreed with its own arithmetic — and the jitter was installed as three own properties on an object that has none |
| `host_object_brands` | `navigator.connection` and `performance.memory` had been replaced with object literals: `[object Object]` where `[object NetworkInformation]` belongs |
| `getter_names` | Accessors named `"get"` or `""`, where the engine names its own `get deviceMemory` |
| `languages_fresh` | One frozen array handed out on every read, so `navigator.languages === navigator.languages` was true. It is false in every real browser |
| `languages_shape` | `en;q=0.9` **inside `navigator.languages`** on the `browser` tier — an `Accept-Language` header passed where a list of tags belongs. Not predicted; the harness printed it |
| `screen_plausible` | Headless reports an 800×600 display while the launch flags size the window to 1366×768: a window wider than the screen holding it. Also not predicted |
| `screen_position` | `window.screenX = -32000` on `real` and `warm` — the offscreen parking position, readable by any page |
| `touch_matches_identity` | `maxTouchPoints = 1` and `ontouchstart` on a desktop identity, from a `setTouchEmulationEnabled(true)` the vendored CDP client hard-coded |
| `scrollbar_present` | `--hide-scrollbars` made `innerWidth === clientWidth` on a page that must scroll |
| `worker_realm` | A worker reported the host's real 32 cores and 32 GB beside a document reporting the identity's 8 and 8. One `postMessage` to catch |

All fourteen pass at all four tiers now, and `tells --assert` runs in `qc`. Two fixes are worth
naming because they are structural rather than cosmetic: the console ring is gone from the page
entirely and comes from `Runtime.consoleAPICalled` on this side of the protocol, and workers are
handed the identity in the window between attaching paused and `runIfWaitingForDebugger`, which is
four new entries in `crates/svipall-cdp/PATCHES.md`.

One of the fixes was itself caught by the older harness: shadowing `performance.memory` on the
instance did nothing at all, because the property hands out a fresh `MemoryInfo` on every read, and
`fingerprint` reported the machine's real heap ceiling. It is patched on the prototype now, and
`tells` has a probe that fails if it ever silently stops applying again.

### `public31`: 24 → 25, and why

| | runs | median | range | `blocked` |
|---|---|---|---|---|
| before | 23, 24, 24 | 24/31 | 23..24 | 0 |
| **now** | 25, 25, 25 | **25/31** | **25..25** | **0** |

The median moved outside the previous range, which is this file's own rule for calling something an
improvement. The cell that moved is `x.com/explore`, and the cause is worth the paragraph:

**The impersonating HTTP engine followed no redirects.** `reqwest` was configured with
`Policy::limited(10)`; `wreq` — the default, the one every release build uses — was configured with
nothing, and its default is to follow none. So on the build everybody actually ships, every URL
that redirects came back as its 3xx stub: `http` to `https`, a bare host to `www`, a trailing
slash, a login gate. `x.com/explore` was returning seventy-four bytes reading "Found. Redirecting
to /i/flow/login", and `classify` — reasonably — called that a delivered page and stopped the
ladder from ever opening a browser.

This was not a benchmark cell. It was every redirecting URL on the default build, and the benchmark
is simply where it became visible. `classify` now also refuses a 3xx that carries only a notice,
so a redirect that arrives anyway (a loop, a chain past ten) is escalated rather than returned.

### Two cells that are not failures

`medium.com` and `canadianinsider.com` were investigated by hand rather than retried. Both answer
`200` with their own titles and 45–50 KB of their own content: **the page is delivered.** The
public rule counts the string `cdn-cgi/challenge-platform` in the body as a gate, and every
Cloudflare customer page carries that script whether or not a challenge was served.

They stay in the failure column. The imported rule is scored as it was published, because a
benchmark whose scoring function bends to the tool it is measuring is not measuring anything. What
is *not* done is escalating those two to a browser to win them back — that would spend a browser
launch on a page already in hand, making the tool worse for the sake of the number.

That leaves the six as: two detection panels and one WAF that are gated for every tool the public
benchmark measured, two rule artefacts, and `indeed.com` — one genuine unclear wall.

### `vendors8`: 3 → 2, and what the lost cell actually was

| | runs | median | range |
|---|---|---|---|
| before | 3, 3, 3 | 3/8 | 3..3 |
| now | 2, 2, 2 | **2/8** | 2..2 |

The median left the range, so by this file's own rule it is a real move and not noise. The cell is
`akamai-homedepot`, and it was investigated by hand rather than retried or explained away.

What the site returns to this address, reproduced by hand at the `stealth` tier:

```
200 OK, 206 characters
"#1 Home Improvement Retailer / Oops!! Something went wrong. Please refresh page / Refresh"
```

and `403` with the same template on the http tier, which also put the domain on a cooldown. That is
a soft block: no challenge, no wall, no status code on the browser tier — the site's own error page,
served to an address it has decided about after a day of benchmark runs against it.

Two things follow, and only one of them is a number.

**The number stays 2/8.** Adjusting it, retrying until it comes back, or moving the target would all
be ways of not reporting what was measured. A re-measure after the address has rested is the way to
find out whether the cell returns; it is not a reason to hold the current figure back.

**The classifier was wrong about it, and that is fixed.** Svipall was returning those 206 characters
as the page with a `thin` quality label. A short `200` whose whole message is "something went wrong,
please refresh" is a stand-in, not content, and it now escalates like every other stand-in — behind
the same short-page gate that keeps an article *about* an outage from being caught by it. So the
next run either gets the page from a higher tier or reports a wall, instead of handing back an error
template and calling it a fetch.

That fix is the useful output of the round. The cell was not lost to a regression, and looking for
one is what found something real.

### Two Cloudflare walls, two tiers

`public31`'s tier histogram gained a rung this round: `real` answers two cells that used to be
reached the long way round. "Just a moment…" is a script that lets you through and a stealth-patched
headless browser clears it in seconds; the *managed* challenge scores the visitor instead, and
headless has never once passed one here. They were both `WallKind::Cloudflare` and both escalated to
`stealth`, so every managed challenge spent an attempt — and taught the site something — before
arriving at the headful tier that had a chance all along.

The discriminator has to be the challenge page's own markup (`cf_chl_opt`, `challenge-form`,
`orchestrate/chl_page`) and not `cdn-cgi/challenge-platform`, which sits on every Cloudflare
customer page whether or not a challenge was served. Keying on that would send half the web to the
headful tier — and is the same mistake the ported public rule makes when it calls those pages
`gated`.

### `hard12`: 8 → 7, `zillow`, and a defect found while looking for the cause

`hard12` moved to a median of 7/12 (range 7..8). The cell is `zillow`, and it went 5.7 s passing,
then 63.3 s and 63.0 s failing.

Looking for the cause turned up a real defect, which is worth separating from the number it did not
explain. Replacing the forged `visibilitychange` with a genuine one meant
`Page.setWebLifecycleState`, frozen then active — and `setWebLifecycleState` does what it says: it
**stops the page's JavaScript**. That call sat in a function the warm loop runs *while a challenge
is on screen*, so a widget measuring how long a button was held had its own timers frozen
underneath it, mid-hold. Whatever else is true, that is wrong, and it is gone.

**It did not fix `zillow`.** With the freeze removed the cell passed in 4.8 s on one run and failed
at 60.9 s on the next, which is where it was before. So the cause is not established, and the
honest reading is the one this file already recorded for that target two rounds ago: the
press-and-hold vendor scores addresses, `hard12` was run four times against it in one day, and the
target sits in a noise band that a twelve-target list cannot resolve.

What is left in that function is real pointer and wheel input, which costs the page nothing, plus
the focus emulation that fixes the contradiction actually worth fixing — a window parked off-screen
otherwise reports `document.hasFocus() === false` for the entire life of the session. A forged
event is not an alternative: `isTrusted: false` beside a `document.hidden` that never moved is a
page telling on itself.

Two things recorded rather than smoothed over: a change that was *more honest at the protocol
level* was worse in practice and had to come out, and removing it did not move the number it was
suspected of.

---

## HTTP/3, examined a third time — and both reasons on file were wrong (2026-09-04)

This file has declined h3 twice, and the second entry closed with a criterion that could be
re-checked: *"when a Rust TLS binding exposes ALPS and ECH GREASE for QUIC, the gate opens."* Taken
literally, it opens.

**The linking reason.** `quiche` and the http engine now resolve together — not because of symbol
prefixing, as the 2026-09-03 entry assumed, but because `quiche` 0.24.9 defaults to
`boringssl-vendored`, which builds BoringSSL in its own build script and declares no `links` key.
They still do not link: `LNK1169: one or more multiply defined symbols found`. Prefixing would have
fixed it, and `btls-sys` can prefix — but the feature is opt-in and its build script prints *"the
`prefix_symbols` feature is not supported on macOS/iOS or Windows targets"* and skips it. So the
retraction was wrong, the original conclusion was right, and neither matters for the route below.

**The shape reason.** `btls` — the binding this binary already links, through `wreq` — emits both
extensions in QUIC mode. Offline, with no server and no socket, because BoringSSL in QUIC mode hands
the ClientHello to an `SSL_QUIC_METHOD` the caller installs:

```
SSL_set_quic_method -> 1   add_application_settings(h3) -> Ok(())   first flight: 1682 bytes
  65037 0xfe0d  250 bytes  ECH GREASE
  17613 0x44cd    5 bytes  ALPS, new codepoint
     57 0x0039   20 bytes  quic_transport_parameters
```

Flipping the codepoint flag moves ALPS to 17513, the number this file names. The crate that cannot
do it is `boring`, which `quiche` binds: it has no ALPS wrapper, and BoringSSL has no context-level
ALPS API to wrap. That is one crate pairing, and it was written down as a fact about every Rust
stack.

**A QUIC reference now exists, and it is offline.** The measurement objection was the one that
survived — the fingerprinting endpoint has `tls`, `http1` and `tcpip` sections and no QUIC one. So
the reference was made here: Chrome is pointed at a UDP socket this process owns, and its Initial is
decrypted with the RFC 9001 salt and the connection id in its own clear header. Chrome for Testing
**152.0.7977.75**, three runs, same thirteen extensions every time, and:

* Chrome uses **ALPS 17613**, not the 17513 on file.
* The extension **order is permuted per connection** — three runs, three orders, no shared position.
* **No GREASE cipher and no GREASE extension**, but a GREASE transport parameter with a fresh random
  62-bit id each connection. The cipher list is `1301 1302 1303` and `legacy_session_id` is empty.

Nothing was measured on any target, so no number in this file moves. What changed is that the row
this project has published against Scrapling twice now rested on an argument that does not hold, and
the argument has been replaced by a reference measurement. The whole of it is in
[`docs/http3.md`](../../docs/http3.md), including what is still not proved: two extensions are not a
fingerprint, and the QUIC Initial around them is where the rest of the work is.

**Then the route was built, the same day.** The first look reported that `quiche` with
`default-features = false` fails at link time with 83 unresolved BoringSSL symbols. That was a
reading of the wrong link: quiche declares `crate-type = ["lib", "staticlib", "cdylib"]`, and a
**cdylib** has to resolve every symbol by itself. The rlib does not. A local copy of quiche 0.24.9
with `crate-type = ["lib"]`, `default = []`, three `extern "C"` declarations and about thirty-five
lines compiles and links against `btls-sys`'s BoringSSL — one copy in the graph, no `links` conflict
to have — and brings `quiche::h3` with it. Its first flight, taken out of `send()` and decrypted the
same way Chrome's was:

```
legacy_session_id: 0 bytes   cipher suites: 1301 1302 1303   key_share 1258 bytes
  65037 0xfe0d  ECH GREASE      17613 0x44cd  ALPS, new codepoint      57 0x0039  transport params
```

Against the Chrome reference that is **ten extensions of thirteen**, with the three matching values
that are hardest to get right — cipher list, empty session id, post-quantum key share — already
matching. What is missing is `compress_certificate` (`SSL_CTX_add_cert_compression_alg`),
`trust_anchors` (`SSL_CTX_set1_requested_trust_anchors`), extension permutation
(`SSL_set_permute_extensions`), a GREASE transport parameter and the transport parameter values —
plus one extension, `0x12e0`, that is **not in this BoringSSL at all**, because Chrome ships a newer
one. That last is `identity.rs`'s own rule arriving somewhere new: an h3 engine has a Chrome version
ceiling set by the age of the linked BoringSSL, and it will have to be measured like the others.

**And then it was built, the same day.** `crates/svipall-quic` is a vendored quiche 0.24.9 on
btls-sys's BoringSSL — one copy, no `links` conflict, `quiche::h3` included. Ten patches, all in its
`PATCHES.md`, take its QUIC ClientHello from ten of Chrome's thirteen extensions to twelve, add the
extension permutation Chrome does, and give the transport parameters a GREASE and a shuffle. Every
one is asserted offline in `crates/svipall-quic/tests/handshake.rs`, by decrypting the client's own
first flight with the same RFC 9001 derivation that read Chrome's.

`svipall-http` gained an `H3Fetcher` behind `--features http3`, triggered by `core::altsvc` — the
second visit to a domain that advertised h3, never the first, which is Chrome's own rule. It fetched
`https://cloudflare-quic.com/` end to end: **200, 125,959 bytes, `text/html`, `HTTP/3.0`**, with a
fallback wired to fail loudly so a pass could only mean QUIC carried it.

**No target was measured.** `bench evasion` has not been run with h3 on, so no number in this file
moves and none is claimed to. What is left, and written down in `docs/http3.md` rather than left to
be discovered: the `trust_anchors` payload is empty where Chrome sends a list, the HTTP/3 SETTINGS
frame has not been compared, and one extension Chrome sends (`0x12e0`) is not in this BoringSSL at
all — which means an h3 engine has a Chrome version ceiling of its own, set by the age of the
linked BoringSSL. That is `identity.rs`'s rule arriving in a new place, and it is the reason `http3`
stays off by default until it has been measured like everything else here.

---

## HTTP/3, measured (2026-09-04)

The engine works and the evasion number did not move. Both halves are the finding.

### The ceiling: how many of these sites even offer h3

`bench h3` reads `Alt-Svc` off an ordinary TCP fetch — the same thing `core::altsvc` reads at run
time — and then fetches the same URL over QUIC with a fallback wired to fail, so a page that comes
back can only have come over h3.

| set | advertise h3 | fetched over it |
|---|---|---|
| `hard12` | **4 of 12** | 4 |
| `public31` | **13–14 of 31** | 13–14 |

That is the ceiling. Two thirds of these targets never offer h3, so no h3 engine of any quality
could change their cell.

### Where it makes a difference

Five runs of `hard12`, and one with the order reversed to rule out the obvious confound — these
vendors score an address, so whichever request goes second is asking a server that has already seen
us:

| target | TCP | h3 |
|---|---|---|
| **amazon** | `503`, 2,671 bytes, no expected text | **`200`, ~816 KB, expected text present** — 5 runs of 5 |
| **indeed** | `403`, 28 KB | `200`, ~2 MB in 4 runs of 5; `403` in the fifth |
| nowsecure, g2 | identical both ways | identical both ways |

Reversing the order changed nothing, so this is the transport and not the ordering.

`public31` adds no clear cell: `medium` and `indeed-jobs` each flipped in one direction in one run
and back in the next, `amazon-product` returns a `404` over both, and `tls-peet` advertises h3 and
then does not answer us over it.

### And the evasion number did not move

`hard12`, median of three, cooldowns cleared, fresh order, one residential address, no proxy:

| | median | range | resolved by tier |
|---|---|---|---|
| h3 off | **7/12** | 7..8 | browser 12, http 6, warm 4 |
| h3 on (`Alt-Svc` primed first, so all three runs use it) | **7/12** | 7..8 | browser 12, http 6, warm 4 |

Identical, down to the tier histogram. By this file's own rule that is not an improvement, and it is
not a regression either.

### Why, and it is not what it looks like

`RUST_LOG=svipall_mcp=debug` on a run says it outright: *"amazon.com has not advertised Alt-Svc, so
this visit is TCP"* — on a run where amazon had already been fetched. The store was not empty; the
http tier was never asked.

**The learned ladder and h3 are wired past each other.** `domain_tiers` remembers that amazon needs
`browser`, so the next fetch starts there and `tier_http` — the only place h3 is consulted — never
runs. And the domains with a learned tier above `http` are exactly the ones with walls, which are
exactly the ones where h3 was measured to win. The feature is real, the trigger is real, and the
ladder routes around both.

Verified end to end with the CLI on a clean home: after one fetch, `web_status` reports
`h3_offered_by: [["amazon.com", 443]]`, `http3: {built, enabled, in_use}` all true, and
`domain_tiers: {"amazon.com": "browser"}`. The second fetch goes straight to `browser`.

**What that costs, in the one cell where it is measurable.** amazon over h3 at the `http` tier is a
`200` with the expected text in **1.0 s**; the ladder resolves it at `browser` in **2.5–3.5 s**.
Same verdict, three times the work. For `indeed`, which the ladder resolves at `warm` in 25–32 s and
still fails, four runs of five say the http tier over h3 would have returned a 2 MB `200`.

**Not fixed here, because the fix is a change to the ladder and belongs to whoever owns that
decision:** a domain that advertises h3 and whose learned tier was set by a *TCP* failure has not
actually been tried, and one h3 attempt at the http tier before escalating is a different request,
not a repeat. That costs an extra round trip on exactly the slowest domains, which is why it is a
decision and not a patch.

One thing this measurement also turned up and did not act on: `indeed`'s expected-text rule is
`"jobs"`, which appears in the Cloudflare interstitial too, so that cell scores `OK` on a body that
is a wall. That is a target definition to tighten, unrelated to h3.

---

## HTTP/3, wired to the ladder and measured properly (2026-09-05)

The previous entry found that h3 worked, won two of twelve walled cells at the http tier, and
changed no number — because `domain_tiers` learns a higher tier for exactly those domains and the
http tier, the only place h3 is spoken, is never asked again. This entry closes that, and reports
what it was worth.

### What changed

Four things, and three of them are bugs found by measuring rather than features:

1. **The probe.** `build_ladder` takes `h3_probe`; when a domain advertises h3 and has not already
   refused to deliver over it, one `http` attempt goes in front of whatever was learned. What was
   learned was learned over TCP, and QUIC is a different request rather than a repeat.
2. **A memory of the outcome**, `core::altsvc::{verdict, remember_result}`. Without it the probe is
   paid on every fetch for ever. It expires after six hours, because a dropped UDP port is usually
   the network — a laptop moves, a firewall changes — and remembering "no" permanently would let one
   bad café decide this machine never speaks h3 again.
3. **A handshake deadline of its own**, two seconds, separate from the page budget. This is the
   number that decides whether h3 can make the tool *slower*: a network that refuses UDP says so at
   once, but one that silently **drops** it says nothing, and without this the attempt sat there for
   the whole 45-second navigation budget before falling back. Asserted offline against TEST-NET-3.
4. **`Alt-Svc` is now read from every tier, not just http.** A trap with a long fuse: a domain
   learned at `browser` makes no http request, so once the advertisement expired — a day, by the
   specification's default — it could never be re-learned and h3 was off for that domain for ever.

Two bugs the measurement exposed on the way, both now fixed: the probe outcome was recorded on the
ladder's success path only, so a probe that *threw* was never remembered and was paid again every
time; and this benchmark gave the h3 arm an in-memory page cache and the control arm the operator's
real one, which measured the caches rather than the transports.

### The evasion number: unchanged, and the noise is larger than any effect

`hard12`, median of three, cooldowns **and** `domain_tiers.json` cleared before each condition, h3
arm run first this time to invert the previous order.

| | median | range | median seconds |
|---|---|---|---|
| h3 off | **8/12** | 8..8 | 291.0s |
| h3 on, probe wired | **8/12** | 7..9 | 281.7s |

Individual runs were 123 / 291 / 369s for the control and 187 / 344 / 282s for h3. A spread of that
size across three runs of the same code is the whole reason this file reports medians with ranges,
and it swamps anything h3 does. **By this file's rule, no improvement and no regression.**

### The number that is not noise: cost per page

Five consecutive steady-state fetches of one h3-capable, http-walled target (`amazon`), through the
product, cache bypassed, after one ordinary first visit — the shape a caller pulling many pages
actually has:

| | tier | per page | browsers opened |
|---|---|---|---|
| h3 off | `browser` | 2967 / 2864 / 2944 / 3279 / 2978 ms — **median 2967 ms** | one per page |
| h3 on | `http` | 953 / 950 / 945 / 960 / 808 ms — **median 950 ms** | **none** |

**3.1x faster, and no browser at all**, with a spread of ±3% rather than the ±50% the evasion runs
show. Arithmetic on those two medians, and nothing more: a hundred pages of that site is 297s
against 95s; five hundred is 25 minutes against 8, and five hundred Chrome page loads against zero.

### And the worst case, for the caller who only wants a few pages

Four consecutive visits to a target that advertises h3 and does **not** deliver over it
(`nowsecure`):

| visit | h3 off | h3 on |
|---|---|---|
| 1 | 3919 ms | 4571 ms |
| 2 | 3138 ms | 3781 ms — one extra 568 ms attempt |
| 3 | 3050 ms | 2874 ms — probe remembered as failed, not repeated |
| 4 | 2951 ms | 2843 ms |

**One extra half-second, once per domain per six hours.** From the third visit the two are the same
tool, and the memory is what makes that true.

### What this does not say

Nothing here measured a target the h3 engine reaches and the TCP one does not. `indeed` looked like
one in the previous entry — 2 MB over QUIC against a 403 over TCP, four runs of five — and did not
reproduce as a changed *verdict* in any run here. Four of twelve `hard12` targets advertise h3 at
all, so the ceiling on this was always low, and one of those four is where the whole measured gain
sits.

### Two things this session turned up that are not about HTTP/3

**The benchmark refused to run, and it was right to.** After a day of measuring, `bench evasion`
stopped with *"this address has already spent its standing with g2 (2h03m), idealista (1h44m),
zillow (2h42m), crunchbase (1h45m)"*. The reputation gate did exactly what it exists for. It also
means the evasion figures above were taken while several targets were already near or over that
line, which is the likeliest source of the 90-second timeouts and the 123–369s spread — so treat
them as "no difference measurable today" rather than as a clean 8/12 on either arm. `amazon`, where
the cost result was measured, sat at pressure 0.20 and is unaffected.

**`bench tells` was passing on a third of its probes.** It reported `56/56 probes clean` all day and
now reports `116/128` — the same binary, the same command. The eighteen probes per tier that were
missing are the ones that read the injected surface (`cross_realm_tostring`,
`navigator_getters_are_native`, `iframe_realm_agrees`, `permission_state_is_valid`, and others), and
twelve of them now fail. This reproduces in a build with **no** `http3` feature at all, so it is not
the QUIC work; something was causing the probe page to skip most of its checks and report the
remainder as a clean sweep. A gate that passes on a partial probe set is worse than one that fails,
and this needs its own investigation.

---

## The tell surfaces become one, and the gate stops being the loosest of the three (2026-09-04)

`tells` went from **fourteen probes to thirty-two** and from **56/56 to 128/128**. Six real
contradictions were shipping in between, four of them found by probes written for this round and
two by tightening a rule the gate was already applying more loosely than the offline linter beside
it. `fingerprint` went from 24/24 to **16/16** because sixteen of its checks moved into the gate
rather than because anything regressed.

### First, a correction to the entry above

The previous entry records *"`bench tells` was passing on a third of its probes… something was
causing the probe page to skip most of its checks and report the remainder as a clean sweep."*
**There was no such bug.** The observation was real and the inference was wrong: two sessions were
working in this tree at once, and the probe page was being *added to* between those two runs.
`56/56` was the fourteen-probe fixture; `116/128` was the thirty-two-probe one, mid-round, with
twelve genuine failures that are itemised below and fixed. The same binary was reading a different
fixture, because `PAGE` is `include_str!`'d and a rebuild picks up whatever is on disk.

It is worth keeping rather than deleting, because the reason it was hard to tell apart from a bug is
now fixed. `bench/src/tells.rs` carries a frozen `PROBES` list, `missing()` counts a probe that
never reported as a failure, and two offline tests fail if the list and the page disagree in either
direction. Before that, a probe lost to a JavaScript error before its `put(...)` lowered `total` and
failed nothing — which is the shape of bug the entry above thought it was seeing, and which could
have happened.

### What the strict rule caught, and why it was not headless being candid

`bench/fixtures/tells/index.html` tested `screen.availHeight <= screen.height`.
`crates/svipall-core/src/coherence.rs` treats `availHeight >= height` as a **violation** and
`fingerprint` used the strict form too: three surfaces, two rules, and the gate held the loosest.
The committed baseline had been recording the tell in plain sight for two rounds, at the `browser`
tier:

```
ok   screen_plausible   screen=1920x1080 avail=1920x1080 outer=1366x768
```

availHeight equal to height is a desktop with no taskbar, dock or menu bar — exactly what
`stealth_js` was written to remove at the other three tiers. Tightening the probe to `<` failed that
cell, and `window_chrome_height` — new this round — failed the same tier with
`outerHeight - innerHeight = 0`.

The cause is the part worth recording. **Neither number was the host's.** Every tier launches with
`Viewport { screen: Some((identity.screen.width, identity.screen.height)), .. }`, which becomes a
`setDeviceMetricsOverride`, and Blink then reports the whole of that display as available. The
`browser` tier was not being honest about a headless default; it was wearing an override this code
sends, with the available area and the window around it forgotten. Correcting them is arithmetic on
numbers this session already chose, not a stealth surface — the same argument `identity_core_js`
already makes for that tier — and the line stays where it was: no canvas, audio or text-geometry
noise, no plugin list, no WebGL spoof and no timezone override reach `browser`.

### The four the new probes found

| Probe | What it found |
|---|---|
| `cross_realm_tostring` | The `toString` mask is a registry of the functions **this realm** patched. A same-origin `about:blank` iframe is a realm of its own, so `iframe.contentWindow.Function.prototype.toString.call(topGetter)` returned `() => 8` — the patch and the value it hid, in one line, **at all four tiers**. Registry is by source text now as well as by object identity, because every realm builds the same accessors from the same identity and their sources match where their objects do not |
| `navigator_getters_are_native` | `identity_core_js` — the script that keeps documents and workers agreeing — had no mask at all. On the `browser` tier and inside **every worker on every tier**, one `getOwnPropertyDescriptor` and one `toString` read `() => 8` directly |
| `permission_state_is_valid` | `navigator.permissions.query` answered a `notifications` query with an object literal: `[object Object]` where `[object PermissionStatus]` belongs, carrying `Notification.permission`, whose third value is `default` — not a `PermissionState` at all. Three defects in five lines. The **unpatched** `browser` tier in the same pooled browser answered correctly on its own, so the wrapper only ever made the three patched tiers worse than plain Chrome. Deleted |
| `iframe_realm_agrees` | `outerWidth`/`outerHeight` were defined over the top of the real values, and a definition holds only in the realm that installed it: the top realm said 874, the iframe said 768. Fixed by sizing the **OS window** to the viewport plus its chrome, so `outerHeight - innerHeight` is 106 natively and nothing is defined at all. Strictly better than the spoof it replaces |

`navigator_webdriver` is the fifth, and it is the one that argues for measuring before deciding.
`stealth_js` deleted the property outright, with a comment explaining that there is then no accessor
for the "WebDriver Advanced" probe to find. The four tiers, before the change:

| tier | `navigator.webdriver` | `'webdriver' in navigator` |
|---|---|---|
| `browser` (no stealth script) | `false` | `true` |
| `stealth`, `real`, `warm` | `undefined` | `false` |

Every Chrome since 89 carries the property and answers `false`, and
`--disable-blink-features=AutomationControlled` in `BASE_ARGS` was already delivering exactly that
on the unpatched tier. The deletion was the only thing producing a navigator no real browser has.
Removed; all four tiers now read `value=false in navigator=true descriptor="get webdriver"`.

### One asserted surface instead of three

`fingerprint::browser()` (175 lines, sixteen checks, `stealth` tier only, network required, never
asserted) is deleted, and four tests in `crates/svipall-mcp/tests/stealth.rs` with it. None of them
needed the network: the `https://example.com/` navigation was a "have a real document" requirement,
and the loopback page is a real document that also runs at three tiers those checks never saw.
`canvas_noise_is_deterministic_within_a_page` survived, because it was not a duplicate — it computed
`same: a === b` and then asserted only that the canvas produced bytes, so it never tested the thing
it is named for. It asserts it now, and the same question is a `tells` probe (`canvas_noise_stable`)
with a third draw after an intervening `getImageData`, which is what would catch a counter that
advances per call rather than a seed that does not move.

### The probes that found nothing, published because they found nothing

`focus_and_visibility`, `input_modality`, `ua_string_self_agreement`, `canvas_noise_stable`,
`connection_coherent`, `device_pixel_ratio`, `text_geometry_stable`, `plugins_present`,
`brave_absent`, `no_duplicate_navigator_getters` and `patched_functions_are_native` passed at all
four tiers on the first run. They are watchdogs on fixes that already shipped and whose regression
would otherwise be silent — `SetFocusEmulationEnabled` on the parked headful tiers is the clearest
of them, since a window that reports `hasFocus() === false` for the life of a session makes every
challenge that waits for interaction wait forever, and nothing asserted it until now.

Three candidates were considered and dropped rather than written: `speechSynthesis.getVoices()`
(empty is a real headless signature, but the fix is a synthesised per-OS voice list, which is a new
spoofed surface whose wrong answer would be worse than the empty one), `screen.orientation` (follows
the same device-metrics override `screen_plausible` already checks), and
`Intl.DateTimeFormat().resolvedOptions().timeZone` against `getTimezoneOffset()` (both read the same
ICU default and cannot disagree unless something patched one in JS, and nothing does — the timezone
is a CDP override, which moves both together).

### What was not re-measured, and what still has to be

`tells` and `fingerprint` are regenerated. The evasion sets are **not**: the window is 106px taller
than it was and the `browser` tier now corrects two geometry values, which is a change a live site
can see, so `public31` in particular is owed a fresh three runs before its 25/31 can be quoted
against this tree. It is not in this entry because this address is over the reputation line on
several of its targets, which is the gate working as designed.

Still open, and named here so it is not lost: `http_firefox = true` has never been measured against
`public31`. It moves the TLS shape, headers and User-Agent of the tier that serves **34 of the 93
cells**, and `docs/firefox.md` used to price a `dev.to` regression against the *unbuilt* Gecko
browser tier — a cell that resolves at the http tier in about 100 ms and never escalates, so no
browser tier could lose it. That doc is corrected, and the measurement it points at is the section below.

### `http_firefox = true`, measured at last — and it costs nothing here

The entry above named this as owed, so here it is, taken the same afternoon. One run per arm, not
three, so this is a smoke test against a regression and **not** a new baseline figure: read it as
"no difference visible", not as a median.

| arm | public31 | `devto` | resolved by tier |
|---|---|---|---|
| default (`http_firefox = false`) | **25/31** | `ok` at `http`, 0.4 s | `{http 15, real 8, warm 2}` |
| `http_firefox = true` | **25/31** | `ok` at `http`, 0.1 s | `{http 14, real 10, warm 1}` |

Same total, same failing cells (`bot-incolumitas`, `browserscan-bot`, `sedarplus`, `medium`,
`canadianinsider`, `indeed-jobs`), and `devto` — the cell `docs/firefox.md` argued was at risk from
a Firefox TLS shape — passes on the first rung in both arms. The Gecko arm was in fact *faster*
(109 s against 148 s for the run), which is one run's worth of noise and is reported rather than
claimed.

Two things this does not say. It does not say `http_firefox` is free in general: one address, one
afternoon, one run, and `public31` is a list where twenty-five of thirty-one pass for every tool
ever measured on it. And it does not say the tier histogram is stable — `domain_tiers.json`
remembers where each domain was last served, and this address had been run repeatedly that day, so
`browser` does not appear in either arm where the committed baseline shows twenty-one of its cells.
That drift belongs to the memory, not to either arm, and it is the reason the committed baseline
stays the headline until a proper three-run round is taken from a rested address.

Committed as `baseline/public31-firefox.{json,txt}` so the default configuration keeps the number
anybody can reproduce.

### A note on the noise in these artifacts

Both runs emitted about a thousand lines of `svipall_cdp` WS errors — *"Failed to deserialize WS
response: data did not match any variant of untagged enum Message"* — one per CDP event Chrome 152
sends that the pinned protocol definition in the vendored client does not know. The client logs each
one at `error` and carries on, and every page still came back, so it is log noise rather than a
fault. It is stripped from the committed `.txt` with a header saying so, and it is worth a
`PATCHES.md` line of its own eventually: an `error!` for a message the client is designed to ignore
is a log that trains its reader to skip it.

---

## The wall that could not be seen at the tier that fights it (2026-09-05)

A vendor announces itself on one of three channels: the page body, a response header, or a cookie it
sets. `classify` declared all three kinds of sign and then searched for every one of them **in the
body**. A header name never appears in a body and a cookie name rarely does, so two of the three
channels were declared and never read. On top of that, `tier_browser` returned `headers: Vec::new()`
— the browser tiers had no response headers to give the classifier even if it had known to ask.

The proof-of-work vendor's whole tell is a header. So its wall was reported as
`near-empty body (unrendered SPA, interstitial or silent wall)`, `is_proof_of_work_wall` never fired
on the live target, and the 40-second reissue written for it was dead code exactly where it was
meant to work. The published `vendors8.json` recorded that for three runs without anyone being able
to read it as anything but "the page did not render".

What changed: one `VENDOR_SIGNS` table where every row declares its channel; a `PageView` that can
be given the response headers and the cookie names; the document's real headers captured at the
browser tiers from the CDP subscription that was already running; and a warm wait that emits one
structured event saying why it stopped.

**A wire sign never invents a wall.** It renames one already found — only `Empty` and `Status`, the
two verdicts that mean "blocked, cause unknown", are upgraded. Everything else in the cascade is
untouched, and `wall_vendor` is reported on a page that arrived whole as readily as on one that did
not. It says who is watching the domain; it never withholds anything.

Regenerate with the commands in the section above.

### Measured, 2026-09-05

| list | runs | median | range |
|---|---|---|---|
| `vendors8` | 2, 3, 3 | **3/8** | 2..3 |
| `tells` | — | **160/160** | was 128/128; a fifth row was added, see below |

**The headline number is not attributable to this change, and is not claimed as one.** Detection
renames a block; it cannot make a page arrive. Two other things moved underneath this run: the
reputation/budget work landed in the same tree, and the tier distribution changed completely —
`{"http": 5, "real": 3}` where the previous file recorded `{"browser": 6}`. Cost also rose to a
median of 399.6s per run of 8. None of that is separable from this change with one run, so none of
it is credited to it.

**The two published records of the previous `vendors8` disagree.** The table above in the 2026-09-04
section reads `3, 3, 3 → 3/8, range 3..3 — first measurement`, while `baseline/vendors8.json` as
committed holds `per_run [2,2,2]`, `median 2`, `by_tier {"browser": 6}`. Against the JSON, this
median left the previous range; against the README table, it did not. Until that is resolved,
**"the median left the previous range" cannot be asserted at all**, and it is not asserted here.

### What the run does say, and it is the point of it

`kasada-hyatt`, both runs that reached the warm tier:

```
wall_kind: "vendor"   wall_vendor: "kpsdk.io"   wall_evidence: "header x-kpsdk-ct"
warm: {"ended": "deadline", "iterations": 6, "secs": 20.6, "reissued": false, "reissue_changed": false}
```

and `akamai-homedepot`, on the channel that had never been read at all:

```
wall_vendor: "akamaihd.net"   wall_evidence: "cookie _abck"
```

### The reissue is unreachable, and now there is a number saying so

`reissued: false`. Not because the clearance was fresh — because it can never be judged stale.
`POW_TOKEN_LIFETIME_SECS` is 40 and `warm_wait_ms` is 20_000, so a wait that ends at its deadline
after 20.6 seconds can never see a clearance 40 seconds old. Even the single 15-second extension
reaches 35. **The reissue was written for a budget that does not exist**, and fixing the detection
only revealed the second reason it never ran.

Widening the budget is not done here, and deliberately. The one-reissue-per-wait cap and the 20s
wait were bought with a measurement recorded in `classify.rs`: an uncapped loop turned a 28-second
honest failure into a 90-second timeout. Changing either is its own change with its own run of this
list, not a line folded into this one. The paragraph in the 2026-09-04 section saying the warm tier
"can, and now does" re-earn the token at 40 seconds is **wrong as published**: it could not, and did
not, on either count.

### `tells` gained a fifth row

`browser (reused)` probes a page that was parked and taken back, so it has been navigated more than
once. That is the state a held page is in when the next fetch gets it, and it is the only way to
catch the largest risk in holding pages at all: `prepare` installs the identity script with
`evaluate_on_new_document`, which persists across navigations, so a reused page that were
re-prepared would carry two copies of the patches — readable from the page, and invisible to every
probe that only ever looks at a fresh tab. 160/160 clean, `residue`,
`no_duplicate_navigator_getters` and `patched_functions_are_native` among them.

### Held pages: built, guarded, **not yet measured**

A clearance that lives in a page's JavaScript runtime dies with the tab, so `warm_keep_max` (2) and
`warm_keep_secs` (120) hold a cleared page between fetches when — and only when — the clearance is
one a cookie cannot carry. The policy is `core::warm`, tested against numbers rather than Chromium.

The number that would prove or disprove it is the **second** fetch of a domain, which is why
`--repeat N` and `SVIPALL_WARM_KEEP` exist:

```
SVIPALL_WARM_KEEP=0 cargo run -p svipall-bench --release -- evasion --set vendors8 --runs 3 --repeat 2 > baseline/vendors8-repeat-off.json 2> baseline/vendors8-repeat-off.txt
SVIPALL_WARM_KEEP=2 cargo run -p svipall-bench --release -- evasion --set vendors8 --runs 3 --repeat 2 > baseline/vendors8-repeat-on.json  2> baseline/vendors8-repeat-on.txt
```

**Neither arm has been run.** The budget gate refused both — this address had already spent its
standing with four of the eight targets, about two and a quarter hours from recovering — and
`--ignore-budget` was not used, because a forced run's numbers are not comparable with the baseline,
which is the whole reason that gate exists.

So held pages ship measured only in the offline sense: the policy is unit-tested, the reuse is
tested against a real browser, and `bench tells` asserts that a reused page gives nothing away. The
claim that it is *worth* anything is unmade. Three outcomes were named before the arms are run, so
the write-up cannot avoid them: if the second fetch is fast anyway, the clearance was cookie-borne
and the predicate is wrong; if it is *slower*, a reused page is a tell and the change is reverted
rather than tuned; and if no cell exercises the path — which this run makes likely, since
`kasada-hyatt` never clears and so can never park anything — the change is unmeasured and should not
be counted as a win.

---

## The HTTP/3 SETTINGS frame, and a log that had been lying about its own severity (2026-09-05)

Two items this file had been carrying as open. Both are closed, both are offline, and **neither
moves an evasion number** — nor is claimed to.

### `docs/http3.md` had one row that could never be checked, and now can

The row read *"HTTP/3 SETTINGS — not compared against Chrome's. Its contents and order are a
fingerprint exactly as HTTP/2's are."* It stayed open for a structural reason worth writing down,
because it is the same reason it was easy to leave open: **a SETTINGS frame cannot be read the way
a ClientHello can.** An Initial packet is decryptable by anything holding the datagram — the keys
come from a salt in RFC 9001 and a connection id in the clear header — which is why the ClientHello
reference in `docs/http3.md` needed no server at all. SETTINGS travels on an HTTP/3 control stream
at 1-RTT. Nothing sees it without finishing a handshake first.

So `bench h3-ref` finishes one. A certificate for the name `quic.test` is generated in the process
that serves it and deleted when the run ends, Chrome is told to resolve that name to a UDP socket
this process owns and to force QUIC to it, and `peer_settings_raw` hands back the frame in arrival
order.

**The certificate is made, not committed, and that was a second decision.** The first version of
this committed a `cert.pem` and a `key.pem`, which is what rustls, hyper and quiche itself all do.
It is defensible and it was still wrong here: a private key in a public repository is a finding
every secret scanner will raise and some push protections will block, whatever it authenticates —
and a committed certificate expires, so the failure lands years later on somebody with no idea why
a QUIC test stopped handshaking. BoringSSL is already linked into this binary, so the alternative
cost a dozen of its calls and no new dependency (`crates/svipall-quic/PATCHES.md` entry 11). The
SPKI pin Chrome is given is computed from the certificate in the same run, so the two cannot drift
— which is the other failure a committed pair invites.

**Chrome for Testing 152.0.7977.75, four runs, identical every time:**

```
0x01  QPACK_MAX_TABLE_CAPACITY  65536
0x06  MAX_FIELD_SECTION_SIZE    262144
0x07  QPACK_BLOCKED_STREAMS     100
0x33  H3_DATAGRAM               1
      GREASE                    fresh id and fresh value per connection
```

**What we were sending.** `svipall-http` built its connection with `quiche::h3::Config::new()` —
upstream's defaults:

```
0x276  H3_DATAGRAM (draft 00)   1
0x33   H3_DATAGRAM              1
       GREASE
```

Two settings against four, none of the three QPACK or field-section values, and a draft codepoint
Chrome does not send at all. A `0x276` beside a `0x33` is a constant no browser produces, free to
any server that logs raw settings — and it had been on the wire of every h3 fetch since the engine
was built.

Three things in the capture were not what a reasonable guess would have produced, which is the
whole argument for measuring rather than reading someone's source:

- **The order does not move.** Four connections, one order — the opposite of the TLS extension
  list, which Chrome permutes per connection. There the set is the fingerprint and the order is
  not; here the order is part of it.
- **`ENABLE_CONNECT_PROTOCOL` is absent** on a plain fetch. An extra setting is as visible as a
  missing one.
- **The GREASE value is random too**, not just its identifier.

Fixed in `crates/svipall-quic/PATCHES.md` entry 10, asserted offline in
`crates/svipall-quic/tests/settings.rs` — a client and a server in one process, datagrams passed
through a buffer, and our own frame read back with `peer_settings_raw`, the same accessor the
reference was taken with. Three tests, and they were **red before the patch and green after**: the
recorded red state is `[(630, 1), (51, 1), <grease>]`.

`cloudflare-quic.com` still answers `200`, 125,959 bytes, over HTTP/3, with the fallback wired to
fail — so the new frame did not cost the engine a page.

### `bench tells` was emitting fifty-five errors that were not errors

The entry two rounds above noticed this and left it: *"an `error!` for a message the client is
designed to ignore is a log that trains its reader to skip it."* One `tells` run emitted **55** of
them, and about a thousand had to be scrubbed out of the committed evasion baselines before they
were readable.

The discriminator turned out to be `id`, and it is the one that matters:

| | |
|---|---|
| a message **with** `id` | a response to a call we made, something is waiting on it, a parse failure is a real fault. Unchanged: still `error!`, still an `Err` |
| a message with **no** `id` and a `method` | an event, from a domain newer than the protocol definitions compiled in. Skipped the way `Ping`/`Pong` already are |
| anything else | untouched. Not recognising a shape is not a reason to swallow it |

`crates/svipall-cdp/PATCHES.md` entry 9, with three tests over the classifier. **55 lines to 0, and
160/160 probes still clean.**

### What was not re-measured, and why it is not in this entry

`public31` and `vendors8` are **owed three fresh runs each** and did not get them. The reason is
the gate, working:

```
refusing to measure public31: this address has already spent its standing with
crunchbase-cf (1h44m), indeed-jobs (0h09m)
the numbers would not be comparable with the baseline.
```

`--ignore-budget` exists and was not used. A forced run is exactly the thing this file spent a
round learning not to publish, and the alternative — quoting the committed `25/31` against a tree
that is 126 source files newer — is the thing the entry above already called out. So the honest
state is recorded rather than patched over: **the committed evasion figures predate the h3 SETTINGS
work, the CDP change and the geometry corrections, and none of those has been measured against a
target.** Neither is expected to move a cell; that expectation is not a measurement either.

### And the one column this file still cannot fill by itself

`evasion --exit URL` has existed for two rounds. Every committed baseline still reads
`"exit": null`. It is not a code gap and never was: it needs an exit address the operator supplies,
and this project will not bundle one. Until somebody runs it, *"Svipall cannot do this"* and
*"this address cannot do this"* stay the same sentence in every number here — which is the single
largest qualifier on everything above.

### The extraction gate, run rather than skipped

`qc` has carried an extraction step for several rounds that says *"skipped: set `SVIPALL_CORPUS`"*
on any machine without the corpora, and this one was such a machine. The corpus is on disk now and
the gate ran:

```
ok   extraction: median F1 0.920 (floor 0.900), content loss 11.8% (ceiling 15.0%)
extract finished in 901.5s
```

3,975 gradable pages, 15 minutes, **exactly the figures `docs/extraction.md` already publishes** —
0.920 median against readability 0.963, trafilatura 0.958 and resiliparse 0.936, all three of which
this project is below and says so. Nothing moved; that is the point. A gate that has never been run
on a tree is not a gate, and now this one has been.

The one number worth re-reading beside it is `content reachable and then dropped: 11.82% of gold
words, on 1,940 of 3,975 pages`. It is under its ceiling and it is not small, and it is the
measurement that would catch the class of bug this file found twice by accident — a page fetched
successfully whose *answer* never reached the caller.

---

## `public31` re-measured against this tree — 26/31, and the cell that moved is the flakiest one (2026-09-05)

The entry above recorded that the committed `public31` figure predated 126 source files and could
not honestly be quoted against this tree. It has now been re-taken, on the same address, once the
reputation gate allowed it — it refused for the better part of two hours first, and
`--ignore-budget` was not used.

| | runs | median | range | `blocked` |
|---|---|---|---|---|
| before | 25, 25, 25 | 25/31 | 25..25 | 0 |
| **now** | 25, 26, 26 | **26/31** | **25..26** | **0** |

**By this file's own rule that is an improvement**: the median left the previous range. And the rule
is worth applying to itself here, because of *which* cell moved.

### One cell, and it is `indeed-jobs`

Six cells failed every run before: `bot-incolumitas`, `browserscan-bot`, `sedarplus`, `medium`,
`canadianinsider`, `indeed-jobs`. Five of them still fail every run, for the reasons already on
file — two detection panels with no article to return, a WAF, and two pages that answer `200` with
their own content and are scored `gated` by the ported rule for carrying a script every Cloudflare
customer page carries.

The sixth, `indeed-jobs`, went from **0 of 3 to 2 of 3**. That is the entire difference between 25
and 26.

**So the honest reading is not that anything got better.** `indeed` is a Cloudflare managed
challenge, and this file has recorded it flipping in both directions across four separate rounds —
it and `crunchbase` and `zillow` are the three cells this list has always shown swapping places,
decided per visit and per address on the server's side. What is different about this run is not the
code: it is that the address had been left alone for two hours, which is longer than it had been
left alone all day. A number that moves when the address rests is a number about the address.

Reported as an improvement because the rule says so, and annotated because the rule is not the whole
story. If the next round takes it from a rested address and 26 holds, that is when it means
something.

### What did not change, and is the number worth reading

**Zero `blocked`, again** — 77 `ok` and 16 `gated` across 93 cells, no cell where a site made a
decision about this address rather than about a request. That is the column the public benchmark
published for all seven tools it measured, where only one of the seven reached zero.

### The tier histogram moved a lot, and that is memory rather than code

`{http 44, real 29, warm 4}` where the previous baseline recorded `{http 36, browser 22, warm 15,
stealth 2}`. `browser` does not appear at all. That is `domain_tiers.json` — this machine has
fetched these domains many times today and the ladder starts each one where it last succeeded. The
protocol clears cooldowns before a run and deliberately does not clear the learned tiers or the
reputation spend, because both are the product's own state. Worth stating so nobody reads a
histogram shift as a stealth result.

### Still owed

`vendors8` and `hard12` were **not** re-taken. Running `public31` spends the same addresses they
score — `crunchbase`, `indeed` — so taking all three back to back is the exact thing that produced
the round this file already published as a warning. They keep their committed figures, and those
figures still predate this tree.
