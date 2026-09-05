# Patches

Every deviation from upstream, why it exists, and the test that would catch its loss. Anchored to
named functions, not line numbers.

The reference every shape patch is written against is a real Chrome for Testing 152 QUIC Initial,
captured offline and written down in `docs/http3.md`. When Chrome moves, re-capture and re-measure;
do not adjust these from memory.

---

## 1. One library target, and no BoringSSL of its own

**Where** `Cargo.toml`.

**Was** `crate-type = ["lib", "staticlib", "cdylib"]`, `default = ["boringssl-vendored"]`, a
`build = "src/build.rs"`, and `windows-sys` only under upstream's own target table.

**Now** `crate-type = ["lib"]`, no default features, no build script, and `btls-sys` as an ordinary
dependency.

**Why** A cdylib is a complete shared object and must resolve every symbol by itself. This crate
deliberately builds no BoringSSL, so its cdylib cannot link — which is the whole of the "83
unresolved BoringSSL symbols" that stood on the record for two rounds as a reason HTTP/3 was
impossible here. The rlib has no such requirement, and the final binary resolves those symbols
against the BoringSSL `btls-sys` already put there.

**Also** `doctest = false` and `test = false`: upstream's doc examples and inline `#[cfg(test)]`
modules are upstream's tests of upstream behaviour and need dev-dependencies this copy does not
carry. What this crate is patched to hold is in `tests/`, and that still runs.

**Caught by** nothing compiling at all.

---

## 2. `c_void` returns on two FFI declarations

**Where** `src/crypto/boringssl.rs`, the `extern "C"` block: `AES_ecb_encrypt`, `CRYPTO_chacha_20`.

**Was** `) -> c_void;`

**Now** `);`

**Why** A C function returning `void` is not a function returning `c_void`, and rustc says so. It is
an upstream slip, harmless in practice and fatal under this workspace's `-D warnings`.

**Caught by** `cargo clippy --workspace --all-targets -- -D warnings`.

---

## 3. Application settings and ECH GREASE

**Where** `src/tls/mod.rs`, `Handshake::add_application_settings`,
`Handshake::set_alps_use_new_codepoint`, `Handshake::set_enable_ech_grease` (all new), called from
`Connection::with_tls` in `src/lib.rs` immediately after `handshake.init`.

**Why** Chrome's QUIC ClientHello carries ALPS (17613) and an ECH GREASE (65037). Upstream emits
neither — not because BoringSSL cannot, but because quiche never declared the calls. They are
per-`SSL`, not per-`SSL_CTX`, which is why they are set on the handshake and only for a client.

**Note on the codepoint.** ALPS has two extension numbers: 17513, the original, and 17613, the one
Chrome uses now. Emitting the old one beside a current user agent is its own contradiction, so the
new codepoint is set explicitly rather than left to BoringSSL's default, which is the old one.

**Caught by** `the_handshake_carries_the_two_extensions_no_other_rust_quic_stack_emits`.

---

## 4. Extension permutation

**Where** `src/tls/mod.rs`, `Handshake::set_permute_extensions` (new), called as above.

**Why** Chrome shuffles its extension order on every connection — three captured runs shared not a
single position. A fixed order is a constant across every connection a machine ever makes, and it is
the cheapest thing there is to key on.

**Caught by** `the_extension_order_is_permuted_per_connection_rather_than_fixed`.

---

## 5. Certificate compression and trust anchors

**Where** `src/tls/mod.rs`, `Context::new`, plus the new `brotli_decompress` callback beside it.

**Why** Chrome sends `compress_certificate` (27) and `trust_anchors` (0xca34); upstream sends
neither. Both are context-level, so they are set once when the context is built.

- **compress_certificate.** Only the *decompress* direction is registered: a client advertises what
  it can decompress, and BoringSSL treats a NULL direction as never configured. It has to actually
  work — advertising brotli and then failing on it would break every handshake with a server that
  takes us up on it, which is worse than not advertising — so `brotli` is a real dependency.
- **trust_anchors.** An empty list still sends the extension, which the draft names as the way for a
  client to signal support without requesting an anchor. **Chrome sends a populated list and ours is
  empty**; the extension is present but its payload is not Chrome's. That gap is deliberate for now
  and recorded in `docs/http3.md` rather than papered over.

**Caught by** `the_handshake_carries_every_extension_the_measured_chrome_sends`.

---

## 6. The transport parameters are shuffled, and carry a GREASE

**Where** `src/transport_params.rs`, the end of `TransportParams::encode`.

**Was** a fixed set written in a fixed order, with no GREASE.

**Now** the encoded buffer is walked back into `(id, value)` pairs, a GREASE parameter with a fresh
random reserved id is appended, `version_information` (0x11, RFC 9368) is appended for a client, and
the whole list is shuffled before being written out again.

**Why** Chrome does all three. Sending no GREASE is as much of a tell as sending a fixed one, and a
fixed parameter order is the same constant problem as a fixed extension order.

**Why at the end rather than in the writer** The shape is one concern and this keeps it in one
place: re-checkable against `docs/http3.md`, and cheap to drop if upstream ever grows it.

**Caught by** `the_transport_parameters_carry_a_grease_of_their_own` and
`the_transport_parameters_are_the_ones_chrome_declares`.

---

## 7. Default ack-delay parameters are omitted rather than restated

**Where** `src/transport_params.rs`, `TransportParams::encode`, the `0x000a` and `0x000b` branches.

**Was** written whenever non-zero. Upstream's defaults are the protocol's defaults, 3 and 25, so
they were always written.

**Now** skipped when they equal the protocol default.

**Why** Chrome omits both. Restating a default is redundant on the wire and different from a
browser, which is the only kind of difference this crate cares about.

**Caught by** `the_transport_parameters_are_the_ones_chrome_declares`, which compares against the
captured list.

---

## 8. `Config::chrome`

**Where** `src/lib.rs`, beside `Config::new`.

**Why** The patches above fix the *shape*; the transport parameter **values** are a caller's choice,
and a caller that picked its own would be distinguishable however good the extension list was. The
values were read off the same Chrome 152 capture. One constructor means one place to re-measure when
Chrome moves, instead of a number copied into every call site.

**Caught by** `the_transport_parameters_are_the_ones_chrome_declares`.

---

## 9. Upstream's inline test modules are stripped

**Where** every `#[cfg(test)] mod … { … }` block in `src/` — 39 of them, 15,843 lines.

**Why** They are upstream tests of upstream behaviour, they need `rstest` and `mio`, and
`clippy --workspace --all-targets` compiles them whatever `[lib] test = false` says. Carrying two
extra dev-dependencies and fifteen thousand lines so that a lint pass can typecheck tests nothing
runs is a poor trade.

**Consequence, and why the manifest says `dead_code = "allow"`** those modules were the only callers
of a handful of upstream's own helpers, so removing them leaves the helpers unused. Deleting the
helpers as well would be a second and much larger divergence from upstream for no gain.

**Refreshing** the strip is mechanical: delete each `#[cfg(test)]` immediately followed by
`mod <name> {` through its closing `}` at column zero, and each `#[cfg(test)] mod <name>;`.

**Caught by** `cargo clippy --workspace --all-targets -- -D warnings`, which is where the missing
dev-dependencies surface.

---

## 10. The HTTP/3 SETTINGS frame is Chrome's, in Chrome's order

**Where** `src/h3/mod.rs`, the new `Config::chrome`; `src/h3/frame.rs`, `Frame::Settings::to_bytes`
and the length computation beside it.

**Why** SETTINGS is the first thing a client says over HTTP/3 and it is a fingerprint in exactly
the way HTTP/2's SETTINGS is one — which this project already treats as a fingerprint on the TCP
side. It stayed unexamined because it cannot be read the way the ClientHello can: it travels on a
control stream at 1-RTT, so nothing can see it without completing a handshake. `bench h3-ref` now
completes one, against the Chrome svipall provisions, on a loopback QUIC server.

**Measured.** Chrome for Testing 152.0.7977.75, four runs, identical every time:

```
0x01  QPACK_MAX_TABLE_CAPACITY  65536
0x06  MAX_FIELD_SECTION_SIZE    262144
0x07  QPACK_BLOCKED_STREAMS     100
0x33  H3_DATAGRAM               1
      GREASE                    fresh id and value per connection
```

**Was** upstream, with `Config::new`, sent `[(0x276, 1), (0x33, 1), GREASE]` — two settings, one of
them a draft codepoint Chrome does not use, and none of the three QPACK or field-section values
Chrome sends. It also ordered `0x06` before `0x01`.

**Now** three things:

- `Config::chrome` carries the three values Chrome sets. `h3_datagram` is deliberately *not* in it:
  it is `Some(1)` iff the transport enabled datagrams, and stating it twice is how two settings
  come to disagree.
- The encoder writes `0x01` before `0x06`, which is Chrome's order. Unlike the TLS extension list —
  which Chrome permutes per connection, so its set is the fingerprint and its order is not — this
  order did not move across four connections, so it is asserted as given.
- The draft datagram codepoint `0x276` is no longer *written*. The parser still accepts it, because
  what a peer sends is not ours to narrow.

**Caught by** `crates/svipall-quic/tests/settings.rs`, offline: a client and a server in one
process exchange datagrams through a buffer and the server reads the client's frame with
`peer_settings_raw` — the same accessor `bench h3-ref` used on Chrome, so both halves of the
comparison are taken the same way.

---

## 11. `selfsigned`: an ephemeral certificate, made rather than committed

**Where** `src/selfsigned.rs`, a new module of ours. Upstream has nothing like it — upstream ships
`examples/cert.crt` and `examples/cert.key` in the repository, which is exactly what this replaces.

**Why a certificate is needed at all** the HTTP/3 SETTINGS reference (`bench h3-ref`) and the
offline test that asserts against it (`tests/settings.rs`) both need a QUIC **server**, because a
SETTINGS frame travels on a control stream at 1-RTT and cannot be read out of a datagram nobody
answered. A server needs a certificate.

**Why not commit one** rustls, hyper and quiche all do, so this is a real choice and not an obvious
one. The reason against is about operating a repository rather than about cryptography: a committed
`key.pem` is a private key in a public tree. It is inert — it authenticates a name that resolves
nowhere, for a server that lives for one measurement — and it will still be raised by every secret
scanner, blocked by push protection on some accounts, and re-triaged by every person who greps for
one. A committed certificate also expires, and that failure lands years later on somebody with no
idea why a QUIC test stopped handshaking.

**What it costs** nothing new. BoringSSL is already linked here — that is the whole reason this
crate is vendored in this form — so a P-256 self-signed certificate is a dozen of its calls:
`EC_KEY_generate_key`, `EVP_PKEY_set1_EC_KEY`, `X509_new`, a v3 version so it can carry a
`subjectAltName` (Chrome has rejected common-name-only certificates since 58), `X509_sign` with
SHA-256, and two `PEM_write_bio_*` into a temporary directory that is removed on `Drop`.

**One detail worth keeping** the SPKI pin Chrome is given
(`--ignore-certificate-errors-spki-list`) is SHA-256 over the **DER SubjectPublicKeyInfo**, so it is
taken with `i2d_X509_PUBKEY` and not with `X509_pubkey_digest`, which hashes the public key bits
alone and yields a value Chrome never matches. Computing it in the same run as the key is also what
makes it impossible for a pin and a certificate to drift apart, which is the other failure a
committed pair invites.

**Caught by** five tests in `tests/selfsigned.rs` (in `tests/`, because this crate sets `test = false`): quiche loads both files, nothing is left on disk
after the value drops, two runs are two different keys, the pin is 44 base64 characters, and the
base64 encoder is checked against a known vector rather than only through a value nothing else
knows. `tests/settings.rs` is the integration proof — it cannot complete a handshake without this.
