# Svipall

Local web scraping + captcha MCP server in Rust. Defeat anti-bot walls **by evading them**,
never by breaking access controls.

## Layout
`crates/svipall-*`, `bench/` (workspace member), `site/` (website submodule).
- `core` — classify, ladder, identity+`coherence`, `quality`, policy, `exits`, throttle, widgets,
  `answer`, `session`, SQLite stores, config. `extract` — extraction, MIT/Apache, re-exported.
- `cdp` and `quic` (quiche+h3) are **vendored**: every deviation needs a `PATCHES.md` entry.
- `http`: `impersonate` (default) = Chrome/Firefox TLS+HTTP2 on BoringSSL. `http3`
  (opt-in) uses QUIC only where `Alt-Svc` said so: never a first visit, never past a proxy.
- `solver` store+API, `dashboard` human panel, `models` ONNX, `mcp` both binaries.

## How it works
- One identity (`IdentityProfile`) drives TLS, headers, CDP, the stealth script and every worker
  realm. Never state a version elsewhere; `bench fingerprint --engine chrome` checks it offline.
- One DOM parse per response: ask via `ParseWants`, read from `PageParts`.
- **Nothing is ever withheld.** `quality` labels a page, never filters one (`anti_discard.rs`).
- Schema selectors are fingerprinted per domain; a redesign relocates by similarity (`healed`).
- Pointer, keyboard, wheel go through `behavior`. No bare click, `scrollBy` or forged event.
- Nothing of ours is readable from the page: no named global, no shadowed host object, no realm
  disagreeing with another. `bench tells` asserts it offline in `qc`.
- Hidden text never reaches the model (`extraction::sanitize`). Secrets are named from
  `~/.svipall/secrets.env`; the value never enters a call.
- Exits are the operator's; a pool keeps one (`sticky`) until retired, then heals.
  `throttle`/`exit_health`/`reputation` key by `(domain, exit)`, home included; browser proxy
  auth is CDP, not argv.
- CLI, MCP and `serve` (REST, keyed, loopback) are one server; tests keep `skill/SKILL.md` (and
  the plugin's copy) and `rest::ROUTES` in step.
- A challenge is a *modality* x a *widget*: a new one is a row in `WIDGETS` plus a fixture in
  `core/fixtures/widgets/`. `Modality::Text` is unreachable from the live loop, by test.
- `solve_loop` orders strategies by what worked on this route (`outcomes`); a decline costs no
  attempt. Never a cascade of `if`s. Then the dashboard: parked page, answer replayed.
- Answers are `core::answer::Answer`, checked against the modality. **Every coordinate is a fraction
  0..1 of its asset, never a pixel.**
- Bump `SCHEMA_VERSION` (`solver/src/db.rs`) on any schema change (statements are `IF NOT EXISTS`).

## Rules
- **TDD.** A test first, failing without the change.
- **Quality gate** `scripts/qc.ps1`/`qc.sh` (`--fix` = fmt+clippy); `#[ignore]` tests
  (network/browser) by hand.
- **Benchmarks are measured, not asserted.** Baselines: `bench/baseline/` (its README). **A change counts only when the median leaves the previous range.** Publish
  the runs that improved nothing.
- **No third-party services, no remote deploy.** No API keys, no paid solvers, no geolocation.
  No vendor brand names in code; name protocols by endpoint.
- **Reader-facing Markdown: run the `quiron` skill on every edit** (READMEs, `docs/`, root
  guides; not agent files).
- **Docs move with the code**: a changed tool, argument, route, config key or behaviour updates
  `docs/` (new page: `docs/README.md`) and `CHANGELOG.md`. `docs/reference/` is generated:
  `SVIPALL_BLESS_DOCS=1 cargo test -p svipall --test reference_docs`.
- **English only.** **Size-guarded file**; trim, don't expand.

## Build / run / ship
`cargo build --release` (cmake, nasm, perl, llvm; short `CARGO_TARGET_DIR` on Windows) is
`target-cpu=native`: **never ship it**, artifacts are `--profile dist`. `svipall-mcp` over stdio +
dashboard 8787; `svipall` is the CLI (`doctor` checks an install). Config
`~/.svipall/config.toml`; `SVIPALL_*` in `docs/configuration.md`. Release = a `main` merge bumping
`[workspace.package] version`.
