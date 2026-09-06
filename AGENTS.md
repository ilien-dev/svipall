# Svipall

Local-first Rust web scraping + captcha MCP server.
Evade anti-bot walls; never break access controls. Everything runs locally.

## Layout

`crates/svipall-*`; `bench/` is also a workspace member.

- `core`: classify, ladder, identity/coherence, quality, policy, exits, throttle, widgets,
answer, session, SQLite, config. `extract`: MIT/Apache extraction, re-exported.
- Vendored `cdp` and `quic` (quiche+h3): record every deviation in `PATCHES.md`.
- `http`: default `impersonate` provides Chrome/Firefox TLS+HTTP2 via BoringSSL.
Opt-in `http3` requires prior `Alt-Svc`; never on first visits or through proxies.
- `solver`: store/API; `dashboard`: human panel; `models`: ONNX; `mcp`: both binaries.

## Invariants

- `IdentityProfile` drives TLS, headers, CDP, stealth and every worker realm. Never hardcode
versions elsewhere. Check offline with `bench fingerprint --engine chrome`.
- Parse each response once: request `ParseWants`, read `PageParts`.
- Return every page: `quality` labels, never filters (`anti_discard.rs`).
- Fingerprint schema selectors per domain; heal redesigns by similarity (`healed`).
- Route pointer, keyboard and wheel through `behavior`; no bare clicks, `scrollBy` or forged events.
- Expose no named globals or shadowed host objects; keep realms consistent.
`bench tells` checks every tier offline in QC.
- Strip hidden text with `extraction::sanitize`. Reference secrets by name from
`~/.svipall/secrets.env`; never put values in calls.
- Operator-owned exits stay `sticky` until retired, then heal. Key `throttle`, `exit_health`
and `reputation` by `(domain, exit)`, including home. Browser proxy auth uses CDP, never argv.
- CLI, MCP and keyed loopback REST `serve` share one server. Keep `skill/SKILL.md`, its plugin
copy and `rest::ROUTES` synchronized through tests.
- Challenges = modality x widget: add a `WIDGETS` row and `core/fixtures/widgets/` fixture.
Tests keep `Modality::Text` unreachable from the live loop.
- `solve_loop` ranks strategies using route `outcomes`; declines cost no attempt.
No cascaded `if`s. Fall back to dashboard with parked page and answer replay.
- Use modality-checked `core::answer::Answer`; coordinates are asset fractions 0..1, never pixels.
- Schema changes require bumping `SCHEMA_VERSION` in `solver/src/db.rs`:
`IF NOT EXISTS` alone breaks older installations.

## Workflow

- TDD: write a test that fails without the behavior change, then implement.
- Quality gate: `scripts/qc.ps1` or `bash scripts/qc.sh`; `-Fix`/`--fix` applies fmt+clippy.
Integration tests: `<crate>/tests/`. Run ignored network/browser tests manually.
- Measure benchmarks; regenerate `bench/baseline/` exactly per its README.
Improvements count only when the median leaves the previous range. Publish null results too.
- No third-party services, remote deployment, API keys, paid solvers or geolocation.
No vendor brand names in code; name protocols by endpoint.
- English only. **AGENTS.md must never exceed 1000 tokens.** Trim before adding instructions.
QC/CI enforce the same token estimate as CLAUDE.md via `scripts/check-claude-md.{ps1,sh}`.

## Build / run / ship

`cargo build --release` needs cmake, nasm, perl, llvm; use a short `CARGO_TARGET_DIR` on Windows.
Release uses `target-cpu=native`: never ship it; ship `--profile dist` artifacts.
`svipall-mcp`: stdio + dashboard 8787; `svipall`: CLI; `svipall doctor`: install diagnostics.
Config: `~/.svipall/config.toml`; README documents `SVIPALL_*`.
Install via `install.{sh,ps1}`, package managers, Docker or `plugins/svipall/` (version = crate).
