### Licence of contributions

Svipall is licensed under **AGPL-3.0-only** and will stay that way. You keep the copyright in what
you write. The maintainer may also offer the same code under other terms, such as a commercial or
hosted service built on a private fork, and that only works if one party holds the rights to the
whole codebase. [`CLA.md`](CLA.md) is the agreement that allows it: you grant the maintainer a
broad licence over your contribution, you keep every right you have in it, and it is published
under the AGPL exactly as before.

The pull request template ends with a **Contributor Licence Agreement** section. Keep its line
unchanged and add your sign-off under it (`Signed-off-by: Your Name <you@example.com>`), which
certifies the [Developer Certificate of Origin](https://developercertificate.org/). Pull requests
are squash-merged with their description as the commit message, so both lines end up in the
history. Nothing has to be printed, scanned or mailed.

If your contribution includes code you did not write, say so in the pull request and name its
licence. Code that is not permissively licensed will not be merged, and neither will code you
cannot license under `CLA.md`.

### Tests first

The whole gate runs in CI on Linux, Windows and macOS, so these rules hold for every change:

1. Write or update the test before you change behaviour. A behaviour change needs a test that
   would have caught the old behaviour, or the pull request is not ready.
2. `scripts/qc.sh` (or `pwsh scripts/qc.ps1`) has to pass: fmt, clippy `-D warnings` across the
   feature matrix, tests, `cargo-machete`, and the size and perf budgets. `-Fix` / `--fix` fixes
   what is mechanical.
3. No dead code, unused dependencies or unused fields.
4. Code, comments, documentation and identifiers are in English.

Read [`CLAUDE.md`](CLAUDE.md) first. It is the short version of how the pieces fit together, and
it may show that the architecture already answers your idea another way.

### Where things go

- A new captcha widget is a row in `WIDGETS` plus a fixture under
  `crates/svipall-core/fixtures/widgets/`. `tests/widgets.rs` checks both.
- Anything under `crates/svipall-cdp/` or `crates/svipall-quic/` needs a `PATCHES.md` entry
  that explains the deviation from upstream and names the test that would catch its loss.

### Reporting a bug

Say which tier answered (`web_log` tells you), give the URL or a reduced reproduction, the
`blocked_reason` if there was one, your OS, and whether the build has default features. A
failing test is worth more than a paragraph.

### Scope

Svipall evades anti-bot walls. It does not break access controls that protect someone else's
private data. Contributions aimed at unauthorised access, credential attacks, denial of service
or defeating protections on systems the operator has no right to reach will be declined. See
[`DISCLAIMER.md`](DISCLAIMER.md).
