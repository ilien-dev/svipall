# Upstream

| | |
|---|---|
| Source | `quiche` 0.24.9, from crates.io |
| Copied | 2026-09-04 |
| Licence | BSD-2-Clause — the text is kept in `COPYING` |

The package is renamed `svipall-quic` so nothing mistakes it for the published crate, but the
library keeps the name `quiche`: the vendored source is `use crate::` throughout, and callers read
better saying `quiche::` than `svipall_quic::`.

## Why it is vendored rather than depended on

Two reasons, and the first is not negotiable.

**One BoringSSL.** The http tier's engine reaches BoringSSL through `btls-sys`. Upstream quiche
reaches it either by building its own copy (`boringssl-vendored`, the default) or through the
`boring` crate. Either way a binary with both has two BoringSSLs, and two do not link:
`LNK2005 … already defined`, and `btls-sys`'s symbol prefixing is opt-in and skipped on Windows.
Removing quiche's own copy requires changing its manifest, which requires a copy of it.

**The shape.** Upstream is not trying to look like a browser. Its QUIC ClientHello is missing four
of the things a real Chrome sends, and the sites that offer HTTP/3 are exactly the ones that look at
the first packet. Those four changes are in `PATCHES.md`, and they are the whole reason this is a
patched copy rather than a repackaged one.

## What was dropped, and why it is safe

| Dropped | Why |
|---|---|
| `src/tests.rs` (10,986 lines) and every inline `#[cfg(test)]` module (15,843 more) | upstream tests of upstream behaviour. They need `rstest` and `mio`, and `clippy --all-targets` compiles them whatever `[lib] test = false` says. See `PATCHES.md` entry 9 |
| `src/ffi.rs` (2,203) | the C API, behind `feature = "ffi"`, which nothing here enables |
| `src/build.rs`, `deps/boringssl/` | the vendored BoringSSL build. This crate borrows the one `btls-sys` already put in the binary |
| `examples/`, `include/`, `Cargo.lock` | not source |
| the `qlog`, `sfv`, `boring`, `openssl` features | optional, and none of them are carried |

That leaves **36,849 lines**. It is a lot of third-party code — roughly half this workspace again,
and nearly four times the other vendored crate — and it is the price of a QUIC and HTTP/3
implementation that nobody here has to write, review or keep correct.

## Refreshing

1. Download the new release into a scratch directory and copy its `src/` over this one.
2. Delete `src/tests.rs`, `src/ffi.rs` and `src/build.rs` again.
3. `cargo fmt -p svipall-quic`. This workspace formats its vendored crates to house style, as
   `svipall-cdp` already is, so the copy will not match upstream byte for byte and is not meant to.
4. Reapply every entry in `PATCHES.md`, top to bottom. Each is anchored to a named function rather
   than a line number, so they survive ordinary churn.
5. `cargo test -p svipall-quic` — every patch has a test in `tests/handshake.rs` that fails loudly
   if one was missed.
6. `cargo test --workspace`, then re-measure against a live Chrome:
   the capture procedure is in `docs/http3.md`, and it is what the assertions were written from.
