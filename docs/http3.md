# HTTP/3

svipall speaks HTTP/3 when it is built and turned on. It was declined twice before that, and this
page is the whole record: the reasons that were on file, why neither of them held, what a real
Chrome actually sends, and what the engine does and does not match.

The short version: **it is built.** `crates/svipall-quic` is a vendored quiche on the same BoringSSL
the http tier already links, emitting Chrome's QUIC ClientHello, and `svipall-http` fetches real
pages over it. It is off by default and opt-in at build time, for reasons in `README.md`. What is
still not Chrome is at the bottom of this page, measured rather than estimated.

## What was on record

Two entries in `bench/baseline/README.md`, and a bullet in the README that summarised them.

1. **A linking conflict** (first look). `quiche` reaches BoringSSL through `boring-sys`; the http
   tier's engine reaches it through `btls-sys`; both declare `links = "boringssl"` and Cargo permits
   one package per graph with a given `links` key.
2. **A shape argument** (second look, 2026-09-03), which retracted the first: *"the engine's binding
   builds BoringSSL with prefixed symbols, so two copies can share a binary"*, and the real obstacle
   is that Chrome's QUIC ClientHello carries application settings (ALPS, 17513) and an ECH GREASE
   (65037) *"that no Rust QUIC stack's TLS API can produce"*.

The second entry closed with a criterion, and it is the reason this page exists: *"when a Rust TLS
binding exposes ALPS and ECH GREASE for QUIC, the gate opens."*

## The spike (2026-09-04)

Four experiments, all local, none of them in the workspace. Nothing from them was merged; only what
they found.

### The linking reason was retracted for the wrong reason, and the conclusion still stood — until it did not

`quiche` and `wreq` now **resolve** together. Not because of symbol prefixing: because `quiche`
0.24.9's default feature is `boringssl-vendored`, which builds BoringSSL inside quiche's own build
script and declares no `links` key at all. The Cargo rule the first entry hit is simply not reached
any more.

They still do not **link**:

```
libbtls_sys-….rlib(windows.obj) : error LNK2005: CRYPTO_sysrand already defined in
                                  libquiche-….rlib(windows.obj)
step0_links.exe : fatal error LNK1169: one or more multiply defined symbols found
```

Prefixing would have fixed that, and the retraction assumed it was on. It is not: `prefix_symbols`
is an opt-in feature of `btls-sys`, and its build script prints *"the `prefix_symbols` feature is not
supported on macOS/iOS or Windows targets"* and skips it. So on this platform two BoringSSL copies
cannot share a binary, and the retraction's sentence is wrong even though the conclusion it replaced
was right.

None of which matters, because the route below needs only one BoringSSL.

### `SSL_CTX_add_custom_ext` does not exist here

The idea was to register 17513 and 65037 as arbitrary extensions. Two reasons that is not available:

- `SSL_CTX_add_custom_ext` is an OpenSSL 1.1 API, not a BoringSSL one.
- Modern BoringSSL has **no custom-extension API at all**. `btls-sys` runs bindgen over the whole of
  BoringSSL's headers with no allowlist, and the generated bindings contain **zero** occurrences of
  `custom_ext` — neither `SSL_CTX_add_custom_ext` nor the older `SSL_CTX_add_client_custom_ext`.

It would also have been the wrong tool. BoringSSL implements both extensions natively, an ECH GREASE
payload has to match plausible HPKE sizes or it is itself a tell, and a hand-rolled ALPS gets no
settings negotiation and no codepoint handling.

### The gate: both extensions, in QUIC mode, from the binding already in this binary

BoringSSL in QUIC mode writes nothing to a socket. It hands the handshake to an `SSL_QUIC_METHOD`
the caller installs. So one `SSL_do_handshake` on a client that will never hear back still produces a
complete TLS 1.3 ClientHello, in a buffer we own — no server, no socket, no packet capture. On
`btls`, the binding `wreq` uses:

```
SSL_set_quic_method            -> 1
SSL_set_quic_transport_params  -> 1
add_application_settings(h3)   -> Ok(())
set_enable_ech_grease(true)    -> (void)
first flight                   -> 1682 bytes

      0  0x0000    16 bytes  server_name
  65037  0xfe0d   250 bytes  <-- ECH GREASE
     10  0x000a    12 bytes
     16  0x0010     5 bytes  ALPN
     13  0x000d    20 bytes
     51  0x0033  1258 bytes  key_share
     45  0x002d     2 bytes
     43  0x002b     3 bytes  supported_versions
     57  0x0039    20 bytes  quic_transport_parameters
  17613  0x44cd     5 bytes  <-- ALPS (new codepoint)
```

Extension 57 is what proves this really is a QUIC handshake and not a TCP one. Flipping
`set_alps_use_new_codepoint` moves ALPS to **17513 (0x4469)**, the number the record names. Both
codepoints are reachable; the record's claim that neither is, is false.

`cloudflare/boring` — the crate `quiche` binds — is the one that cannot. It wraps
`SSL_set_enable_ech_grease` but has no ALPS wrapper, and BoringSSL has no context-level ALPS API to
wrap: `SSL_CTX_add_application_settings` does not exist, ALPS is per-`SSL`, and quiche creates its
`SSL` internally with no public accessor. That is a fact about one crate pairing, and it was
generalised into a claim about every Rust stack.

### What Chrome's QUIC ClientHello actually contains

There is still no public service that reports a QUIC fingerprint — the endpoint the bench uses has
`tls`, `http1` and `tcpip` sections and no QUIC one. So the reference was produced here, from the
Chrome svipall provisions, with no capture tool and no privileges: Chrome is told to force QUIC to a
name that resolves to a UDP socket this process owns, and an Initial packet is readable by anyone
holding the datagram, because its keys derive from a salt in RFC 9001 and the connection id in the
clear header.

**Chrome for Testing 152.0.7977.75, three runs.** Cipher suites: `1301 1302 1303`, and **no GREASE
cipher**. `legacy_session_id` is empty. Thirteen extensions, the same thirteen every run:

| | |
|---|---|
| `0x0000` | server_name |
| `0x000a` | supported_groups |
| `0x000d` | signature_algorithms |
| `0x0010` | ALPN (`h3`) |
| `0x001b` | compress_certificate |
| `0x002b` | supported_versions |
| `0x002d` | psk_key_exchange_modes |
| `0x0033` | key_share, 1258 bytes — a post-quantum share |
| `0x0039` | quic_transport_parameters |
| `0x12e0` | not named by this BoringSSL; two bytes of zeroes |
| `0x44cd` | **ALPS, new codepoint (17613)** |
| `0xca34` | trust_anchors (`TLSEXT_TYPE_trust_anchors`, `tls1.h:138`) |
| `0xfe0d` | **ECH GREASE (65037)** |

Three things worth having before anyone builds against this:

- **Chrome uses ALPS 17613, not 17513.** The number on record is the original codepoint; Chrome has
  moved to the new one. Emitting 17513 would be as wrong as emitting neither.
- **The order is permuted per connection.** Three runs produced three different orders, sharing not a
  single position. So the *set* is the fingerprint and the order is not — which means one less thing
  to reproduce, and one more reason a fixed order would stand out.
- **There is no GREASE extension and no GREASE cipher, but there is a GREASE transport parameter**,
  with a fresh random 62-bit id on every connection.

The transport parameters, stable across runs apart from that GREASE id:

```
0x01 max_idle_timeout            0x07 initial_max_stream_data_uni
0x03 max_udp_payload_size        0x08 initial_max_streams_bidi
0x04 initial_max_data            0x09 initial_max_streams_uni
0x05 initial_max_stream_data_bidi_local   0x0f initial_source_connection_id
0x06 initial_max_stream_data_bidi_remote  0x11 version_information (RFC 9368)
0x20 max_datagram_frame_size     0x3128 a Google parameter, ASCII "ORIGNOIP"
```

## The engine, as built

**`crates/svipall-quic` is a vendored quiche, linked against the BoringSSL already in the binary.**

The first attempt reported that quiche with `default-features = false` *"fails at link time — 83
unresolved BoringSSL symbols — because there is then no TLS library under it at all"*. That was a
reading of the wrong link. quiche declares `crate-type = ["lib", "staticlib", "cdylib"]`, and it is
the **cdylib** that fails: a cdylib is a complete shared object and has to resolve every symbol by
itself. The rlib does not, and the final binary resolves them against `btls-sys`'s BoringSSL.

So one copy of BoringSSL, no `links` conflict to have, and `quiche::h3` comes along with it — HTTP/3
itself is not separate work. Every deviation from upstream is in `crates/svipall-quic/PATCHES.md`,
each anchored to a named function and each with the test that catches its loss. In summary:

| | |
|---|---|
| the manifest | one library target, no BoringSSL of its own, `btls-sys` as a dependency |
| `tls/mod.rs` | ALPS with the new codepoint, ECH GREASE, extension permutation, `compress_certificate` with a real brotli decompressor, `trust_anchors` |
| `transport_params.rs` | a GREASE parameter with a random id, `version_information`, the whole list shuffled, and the two ack-delay defaults omitted as Chrome omits them |
| `lib.rs` | `Config::chrome`, whose values were read off the capture above |

`crates/svipall-quic/tests/handshake.rs` asserts all of it the way the reference was taken: a client
connection is opened, its first flight is taken straight out of `send()` rather than a socket, and
decrypted with the RFC 9001 salt. No network, no server, nothing to be flaky.

## The tier

`svipall-http`'s `H3Fetcher` implements the same `HttpFetcher` the TCP engine does, so nothing above
it knows which one answered except through `http_version`, which finally has a reader.

**h3 is not a new rung on the ladder.** The ladder escalates evasion effort; h3 is a transport choice
inside the `http` tier, and it is chosen the way Chrome chooses it:

> A browser never opens a first connection over QUIC. It learns from `Alt-Svc` that a site offers
> h3 and uses it next time.

`core::altsvc` has recorded exactly that since before any of this existed, so the trigger needed
no new state — but the *ladder* did, and that is the next section.

The engine decodes `content-encoding` itself. quiche returns a body exactly as the server sent it,
and the identity advertises `gzip, deflate, br, zstd` — trimming that header to what was convenient
would have been a difference from Chrome in the one place this project refuses to have them.

It carries **no cookie jar**, unlike the TCP engine, which keeps one inside its client. The caller's
headers are authoritative and `set-cookie` comes straight back. A tier that needs a session across
requests uses a browser, which is where sessions live.

```bash
cargo build --release --features http3
# ~/.svipall/config.toml
http3 = true
```

## When h3 is actually used

Three facts have to line up, and `web_status.http3` reports each one separately so a caller looking
at a TCP fetch with an `Alt-Svc` in hand can see which said no: the binary was built
`--features http3`, `http3 = true` in the config, and **this domain advertised h3**.

That last one is Chrome's rule — a browser never opens a first connection over QUIC — and it is what
keeps a first visit indistinguishable from what svipall did before HTTP/3 existed.

But the advertisement alone is not enough, because of how the ladder remembers. `domain_tiers`
learns that a walled site needs `browser`, so the next fetch starts there and the http tier — the
only place h3 is spoken — is never asked again. The domains with a learned tier above `http` are
exactly the walled ones, which are exactly the ones where h3 could help. So:

**One `http` attempt goes in front of the learned tier when the domain advertises h3 and has not
already refused to deliver over it.** What was learned was learned over TCP; QUIC is a different
request, not a repeat of a known-failed one.

Three things keep that from becoming a tax:

- **The outcome is remembered.** `core::altsvc::{verdict, remember_result}`: a probe that did not
  deliver is not paid again. It expires after six hours, because a dropped UDP port is usually the
  network — a laptop moves, a firewall changes, a captive portal ends — and remembering "no" for
  ever would let one bad café decide this machine never speaks h3 again.
- **The handshake has a deadline of its own**, two seconds, separate from the page budget. A network
  that refuses UDP says so at once; one that silently *drops* it says nothing, and this is the only
  thing that bounds that case. Once the connection is up the page gets the full budget: a slow large
  page is a different failure and must not be punished for someone else's firewall.
- **`Alt-Svc` is read from every tier**, not just http. Otherwise a domain learned at `browser`
  makes no http request, the advertisement expires after a day, and h3 is off for that domain for
  ever.

And h3 never costs a page: the engine is built with the TCP one behind it, and any transport failure
falls back. `svipall-http/tests/h3.rs` is that assertion, including the dropped-UDP case against
TEST-NET-3.

## What it is worth, measured

Full tables in `bench/baseline/README.md`. Three numbers matter.

**The ceiling.** Four of twelve `hard12` targets advertise h3 at all; thirteen or fourteen of
thirty-one on `public31`. Two thirds never offer it, so no engine of any quality could touch them.

**The evasion number does not move.** `hard12`, median of three, learned state cleared before each
condition: **8/12 with h3, 8/12 without**. Individual runs spread 123–369 seconds on identical code,
which is a noise floor far larger than anything h3 does. By this project's rule that is neither an
improvement nor a regression.

**Cost per page, which is not noise.** Five consecutive steady-state fetches of one h3-capable,
http-walled target, through the product, cache bypassed:

| | tier | per page | browsers |
|---|---|---|---|
| h3 off | `browser` | **median 2967 ms** | one per page |
| h3 on | `http` | **median 950 ms** | **none** |

3.1x faster at +/-3% spread. A hundred pages of that site is 297s against 95s; five hundred is eight
minutes against twenty-five, and zero Chrome page loads against five hundred.

**The worst case**, four visits to a site that advertises h3 and does not deliver over it: one extra
**568 ms**, once, on the second visit. From the third the memory has it and the two are the same
tool.

So: for a caller pulling a few pages, h3 is invisible except for at most half a second once per
domain per six hours. For a caller pulling many pages from a site that walls the cheap tier over
TCP, it is the difference between a browser per page and no browser at all.

## What is not Chrome yet

Measured, not estimated. Ten extensions of Chrome's thirteen were there before any of this; the
patches take it to twelve.

| | |
|---|---|
| `trust_anchors` payload | ours is empty, Chrome sends a populated list. The extension is present; its contents are not Chrome's |
| `0x12e0` | Chrome sends it and **this BoringSSL does not have it**. Chrome 152 ships a newer one |
| ~~HTTP/3 SETTINGS~~ | **closed.** Measured against a real Chrome and matched; see "The SETTINGS frame" below |
| the QUIC Initial itself | connection id lengths, padding, version negotiation — unmeasured |

That second row is this project's own doctrine arriving somewhere new. `identity.rs` caps the Chrome
major svipall claims because *"TLS is the one layer that cannot lie"*; an h3 engine has a ceiling of
its own, set by the age of the linked BoringSSL rather than by a user agent string. It has to be
measured, it may be lower than the TCP ceiling, and until it is measured `http3` stays off by
default and the TCP tier is the one with numbers behind it.

It has now been measured against targets: see "What it is worth" above. The short version is that
the evasion median does not move, that two thirds of these targets never offer h3 at all, and that
where it does apply the page arrives three times faster with no browser opened — which is a cost
result, not an evasion one, and is the honest claim to make for it.

## The SETTINGS frame, measured (2026-09-05)

The row above stayed open longer than the others for a structural reason, and it is worth stating
before the numbers: **a SETTINGS frame cannot be read the way a ClientHello can.** The ClientHello
is readable by anything holding the datagram, because its keys derive from a salt in RFC 9001 and a
connection id in the clear header — which is why the reference for it needed no server. SETTINGS
travels on an HTTP/3 control stream at 1-RTT. Nothing sees it without completing a handshake first.

So `bench h3-ref` completes one. A certificate for the name `quic.test` is generated in the process
that serves it and deleted when the run ends (`quiche::selfsigned`), Chrome is told to resolve that
name to a UDP socket this process owns and to force QUIC to it, and `peer_settings_raw` hands back
the frame in the order it arrived — the half a decoded struct would throw away.

The certificate is made rather than committed on purpose. A `key.pem` in a public repository is
inert here — it authenticates a name that resolves nowhere — and it is still a private key that
every secret scanner will flag, some push protections will block, and every reader will have to
triage. It would also expire one day and break a QUIC test for somebody with no idea why. BoringSSL
is already linked into this binary, so generating a P-256 self-signed certificate is a dozen of its
calls and no new dependency.

```
cargo run -p svipall-bench --release --features http3 -- h3-ref
```

**Chrome for Testing 152.0.7977.75, four runs, identical every time:**

| id | name | value |
|---|---|---|
| `0x01` | QPACK_MAX_TABLE_CAPACITY | 65536 |
| `0x06` | MAX_FIELD_SECTION_SIZE | 262144 |
| `0x07` | QPACK_BLOCKED_STREAMS | 100 |
| `0x33` | H3_DATAGRAM | 1 |
| GREASE | reserved (`0x1f·N + 0x21`) | fresh id **and** fresh value per connection |

Three things in that table were not what a reasonable guess would have produced, which is the
argument for measuring it rather than reading Chromium's source:

- **The order does not move.** Four connections, one order. That is the opposite of the TLS
  extension list, which Chrome permutes per connection — so there the *set* is the fingerprint and
  the order is not, and here the order is part of it.
- **`ENABLE_CONNECT_PROTOCOL` is absent.** A plain fetch does not send it, and an extra setting is
  as visible as a missing one.
- **The GREASE value is random too**, not just its identifier.

### What we were sending

`svipall-http` built its connection with `quiche::h3::Config::new()` — upstream's defaults — which
put this on the wire:

```
0x276  H3_DATAGRAM (draft 00)   1
0x33   H3_DATAGRAM              1
       GREASE
```

Two settings against Chrome's four, **none** of the three QPACK or field-section values Chrome
sends, and one codepoint — the draft datagram identifier — that Chrome does not send at all. A pair
of `0x276` beside `0x33` is a constant that no browser produces, free for any server that logs raw
settings.

Fixed in `crates/svipall-quic/PATCHES.md` entry 10: `h3::Config::chrome()`, Chrome's encode order,
and the draft codepoint written no longer (still parsed, because what a peer sends is not ours to
narrow). `crates/svipall-http/src/h3_engine.rs` builds with it.

**Asserted offline, in `qc`.** `crates/svipall-quic/tests/settings.rs` runs a client and a server in
one process, passes datagrams between them through a buffer, and reads our own client's frame with
`peer_settings_raw` — the same accessor `bench h3-ref` used on Chrome. Both halves of the comparison
are taken the same way, which is the only reason comparing them means anything.

**What this does not say.** No target was measured. Nothing here changes an evasion number and none
is claimed: this closes a shape gap that was on the wire of every h3 fetch, and shape gaps are
worth closing before anybody can point at them, not because a benchmark moved.
