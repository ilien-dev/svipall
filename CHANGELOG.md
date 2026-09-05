# Changelog

## 1.0.0 — 2026-09-05

The first release. Everything below is in the tree and measured; the numbers are the ones the
benchmark produced, including the ones that are not flattering.

### What it is

A local-first MCP server and CLI, in Rust, that gives an LLM agent a real window onto the web:
**29 MCP tools**, **19 REST routes**, **eight crates** (six of our own plus two vendored — a
patched Chrome DevTools Protocol client and a patched QUIC/HTTP-3 stack). No cloud, no API keys, no
paid captcha service, no telemetry. Nothing leaves the machine it runs on.

### Getting in

- **A tiered fetch ladder** — `http → browser → stealth → real → warm` — learned per domain and
  remembered between runs, that climbs only as far as a page requires.
- **Chrome- and Firefox-accurate TLS/HTTP2** on BoringSSL: JA4, SETTINGS order, header order,
  GREASE, and the post-quantum key share (`X25519MLKEM768`).
- **An opt-in HTTP/3 engine** (`--features http3`) on a vendored quiche, whose QUIC ClientHello
  carries twelve of Chrome's thirteen extensions, permutes them as Chrome does, and GREASEs a
  transport parameter as Chrome does. Triggered only by `Alt-Svc`, never on a first visit — which
  is Chrome's own rule — with a two-second handshake deadline of its own so a network that silently
  drops UDP costs seconds rather than the page budget.
- **One coherent identity per session** across TLS, headers, CDP, the stealth script and every
  worker realm, checked against itself offline in a gate that fails the build on a contradiction.
- **Human-like input**: Bézier pointer paths that land off-centre, typing cadence by digraph,
  wheel-notch scrolling. Never a bare `click()`.
- **Sessions retired rather than reused** when a site turns on one, and exits keyed by
  `(domain, exit)` with health that heals.
- **A local captcha strategy engine** that orders strategies by what has worked on this route, and
  a human-in-the-loop dashboard that finishes a live challenge from a phone.

### Reading

- Any URL as LLM-ready Markdown; tables as typed rows; docx, xlsx, pptx, odt, epub, rtf, csv and
  pdf as prose.
- `schema: "auto"` induces a schema from a listing's own repeated structure, and a schema's
  selectors are fingerprinted per domain so a redesign relocates them by similarity rather than
  breaking.
- `web_capture` returns the JSON the page fetched while loading — usually the site's real API.
- Hidden text never reaches the model.

### What is measured, and what it says

Every evasion figure is the **median of three runs with its range**, fresh order each run,
cooldowns cleared, from **one residential address with no proxy**. Raw logs are committed under
`bench/baseline/`, including the rounds that improved nothing.

| gate | result | network |
|---|---|---|
| `cargo test --workspace` | **1118 passing**, 16 ignored | no |
| `bench tells --assert` | **160/160** probes clean, five browser passes | no |
| `bench fingerprint --engine chrome` | **8/8** identities coherent | no |
| `bench micro --assert` | 11 CPU budgets + 4 structural checks | no |
| `bench extract --assert` | median F1 **0.920** (floor 0.900), content loss 11.8% (ceiling 15.0%), 3,975 pages | no, corpus on disk |
| `bench evasion --set public31` | **26/31** (25..26), zero hard blocks | yes |
| `bench evasion --set hard12` | **7/12** (range 7..8) | yes |
| `bench evasion --set vendors8` | **2/8** | yes |

`public31` was re-taken against this tree on 2026-09-05, once the reputation gate allowed it — it
refused for the better part of two hours first, and `--ignore-budget` was not used. It moved from
25/31 to 26/31, which by this project's rule counts as an improvement because the median left the
previous range. The annotation matters more than the number: **the one cell that moved is
`indeed-jobs`**, a Cloudflare managed challenge this benchmark has watched flip in both directions
across four rounds, and what was different was that the address had rested two hours. A number that
moves when the address rests is a number about the address.

`hard12` and `vendors8` still carry their 2026-09-04 figures and **predate the HTTP/3 SETTINGS work,
the CDP change and the window-geometry corrections in this release.** They were not re-taken because
running `public31` spends the same addresses they score, and taking all three back to back is the
exact thing that produced a round this project already published as a warning.

### What it does not do, stated plainly

The six `public31` cells and the walls in `vendors8` that do not open are decided by **IP
reputation**, not by fingerprint: the fingerprinting vendor returns `blocked visitor` for this
address with a clean browser, a fresh profile, a rotated machine identity and nine minutes of
silence. svipall's answer is `web_route` — send the domain through an exit you supply — and that is
the one thing a local-only tool cannot provide for itself. It will never bundle proxies, never call
a captcha farm, and never report a block as a success.

`evasion --exit URL` runs the whole set through an operator-supplied exit, so *"svipall cannot"*
can be separated from *"this address cannot"*. **No committed baseline has ever used it**; every
one reads `"exit": null`. Until somebody does, that qualifier applies to every number above.

### Closed in this release

- **The HTTP/3 SETTINGS frame.** `bench h3-ref` runs a QUIC server on loopback that a real Chrome
  completes a handshake with, so the frame can be read at all. Chrome for Testing 152.0.7977.75,
  four runs, sends `QPACK_MAX_TABLE_CAPACITY=65536`, `MAX_FIELD_SECTION_SIZE=262144`,
  `QPACK_BLOCKED_STREAMS=100`, `H3_DATAGRAM=1` and one fresh GREASE, in that order. We were sending
  upstream quiche's two — one of them a draft codepoint Chrome does not use. Now matched, asserted
  offline. `crates/svipall-quic/PATCHES.md` entry 10.
- **No private key in the repository.** The loopback server that reference needs a certificate for
  generates one in-process and deletes it when the run ends, on the BoringSSL already linked here.
  rustls, hyper and quiche all commit a test key; a key in a public tree is still a finding every
  scanner raises and some push protections block, and a committed certificate expires on somebody
  else's watch. `crates/svipall-quic/PATCHES.md` entry 11.
- **A log that misreported its own severity.** The CDP client raised `error!` for every event from a
  protocol domain newer than its pinned definitions — 55 lines per `tells` run, about a thousand per
  evasion baseline. An event has no `id` and nothing waits on it; a response does, and still errors
  loudly. `crates/svipall-cdp/PATCHES.md` entry 9.
- **The extraction gate, run rather than skipped.** `qc` has carried it for rounds and it printed
  *"skipped: set `SVIPALL_CORPUS`"* on a machine without the corpora. Run against the SIGIR-23 gold
  standard it reproduces the figures `docs/extraction.md` already publishes, exactly — including
  the three published extractors this project is **below** on median and says so.

### Still open, and named so it is not lost

- `trust_anchors` is sent with an empty payload where Chrome sends a populated list.
- Extension `0x12e0` is absent from the linked BoringSSL, which gives the h3 engine a Chrome
  version ceiling of its own, set by the age of that library rather than by a user agent.
- The QUIC Initial's own shape — connection id lengths, padding, version negotiation — is
  unmeasured.
- `MAX_EMULATED_CHROME` is 149, bounded by the newest emulation profile available to the TCP
  engine, while the provisioned browser is 152.

### Licence

AGPL-3.0-only. See `DISCLAIMER.md` for what this tool is and is not for.
