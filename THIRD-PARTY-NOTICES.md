# Third-party notices

svipall is licensed under AGPL-3.0-only (see [`LICENSE`](LICENSE)). It includes and
depends on material from third parties that stays under its own terms. Those terms are
not narrowed by this project's licence, and their notices must be preserved when you
redistribute a copy or a fork.

## Vendored source

**chromiumoxide** — `crates/svipall-cdp`
Copyright (c) 2020 Matthias Seitz. Licensed under **MIT OR Apache-2.0**.
This crate is a modified copy of chromiumoxide 0.7.0: the automation residue was
patched out. Every deviation from upstream is documented in
[`crates/svipall-cdp/PATCHES.md`](crates/svipall-cdp/PATCHES.md). The original terms
travel with it in `crates/svipall-cdp/LICENSE-MIT` and
`crates/svipall-cdp/LICENSE-APACHE`; both files must be kept in any redistribution.

**quiche** — `crates/svipall-quic`
Copyright (c) 2018-2019 Cloudflare, Inc. Licensed under **BSD-2-Clause**.
This crate is a modified copy of quiche 0.24.9: its QUIC ClientHello was shaped to match
a real Chrome, and it was changed to link against the BoringSSL this binary already
carries rather than build a second one. Every deviation from upstream is documented in
[`crates/svipall-quic/PATCHES.md`](crates/svipall-quic/PATCHES.md), and the original
terms travel with it in `crates/svipall-quic/COPYING`, which must be kept in any
redistribution. It also pulls in **brotli** (BSD-3-Clause OR MIT), for the certificate
compression a Chrome client advertises.

## Linked libraries

**BoringSSL** (via the `impersonate` feature, on by default)
Reached through `wreq` → `btls` → `btls-sys` (Rust bindings, MIT), which vendors and
builds BoringSSL itself. BoringSSL is distributed under the **Apache License 2.0**,
which combines cleanly into an AGPL-3.0 work; the only other terms in its LICENSE cover
TLS test-suite files under the Go BSD-3 licence that are not compiled into libcrypto or
libssl, so linking against it does not trigger them.

Historically BoringSSL carried the OpenSSL and SSLeay licences, which are read as
incompatible with the GNU (A)GPL. That is no longer the case for the version this
project builds. [`NOTICE`](NOTICE) still grants a section 7 linking exception as a
precaution for anyone who builds against an older or different OpenSSL-API library.

**ONNX Runtime** (via `ort`, only with the optional `onnx-*` features)
Copyright (c) Microsoft Corporation. MIT. The runtime binaries and any model files are
installed by the operator; no model ships with this repository.

## Cargo dependencies

Every other dependency in `Cargo.lock` is distributed under a permissive licence —
predominantly `MIT`, `Apache-2.0`, `MIT OR Apache-2.0`, `BSD-3-Clause`, `ISC` or
`Unicode-3.0` — all of which may be combined into an AGPL-3.0 work provided their
notices are preserved.

Regenerate the full, authoritative list before each release:

```powershell
cargo install cargo-about
cargo about generate --format json > third-party.json
```

CI should also fail on an incompatible licence entering the tree:

```powershell
cargo install cargo-deny
cargo deny check licenses
```

A dependency under GPL-3.0, AGPL-3.0 or a proprietary licence must be reviewed before
it is added: the first two are compatible with this project but would impose their
terms on downstream users of any binary, and the third cannot be distributed at all.

## Embedded models

The ONNX models compiled into the release binary by `crates/svipall-models` are exported by
`tools/models/export.py` from weights published by the torchvision project
(`ssdlite320_mobilenet_v3_large` and `deeplabv3_mobilenet_v3_large`), distributed under the
BSD-3-Clause licence. Inference uses ONNX Runtime (MIT) through the `ort` crate. Neither the
export nor the binary contacts any service at run time.
