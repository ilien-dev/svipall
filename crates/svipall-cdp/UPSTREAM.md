# Upstream

| | |
|---|---|
| Source | `chromiumoxide` 0.7.0, from crates.io |
| Upstream commit | `ef98533b26444164fe0a0cfa7ba84dd57d9dfea9` |
| Copied | 2026-09-02 |
| Licence | MIT OR Apache-2.0 — both texts are kept in this directory |

The generated protocol types are **not** vendored. `chromiumoxide_cdp` ships `src/cdp.rs`
pre-generated (99,145 lines, `build = false`), so this crate keeps depending on the published
version. Copying 99k lines of machine-written code in order to change none of it would be all cost
and no benefit.

## Refreshing

1. Download the new release into a scratch directory and copy its `src/` over this one.
2. Reapply every entry in `PATCHES.md`, top to bottom. Each is anchored to a named function rather
   than a line number, so they survive ordinary churn.
3. `cargo test -p svipall-cdp` — the patches have unit tests that fail loudly if one was missed.
4. `cargo test --workspace`, then the browser suites by hand:
   `cargo test -p svipall-mcp --test stealth -- --ignored`.
5. `cargo run -p svipall-bench --release -- fingerprint` and compare against `bench/baseline/`.

## What was removed from the copy

`examples/`, `.github/`, `Cargo.lock`, `.gitignore`, `Cargo.toml.orig`, and the upstream `README`.
The `Cargo.toml` is rewritten rather than edited: the `fetcher` feature and its crate are gone (svipall
provisions its own browser), and so is the `async-std` runtime, which upstream enables by default
and which dragged a second async runtime into the binary for nothing.
