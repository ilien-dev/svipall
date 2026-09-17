# Contributing to Svipall

Contributions are welcome. You keep the copyright in what you write — there is no
copyright assignment — but contributions are taken under a Contributor Licence
Agreement as well as the Developer Certificate of Origin.

## Licence of contributions

Svipall is licensed under **AGPL-3.0-only** and stays that way. The maintainer may also
offer the same code under other terms, including a commercial or hosted service built on
a private fork, which is only possible if one party holds the rights to the whole
codebase. [`CLA.md`](CLA.md) is the agreement that permits it: you grant the maintainer a
broad licence over your contribution, you keep every right you have in it, and it is
published to the public under the AGPL exactly as before.

Two things are required for a pull request to be merged:

1. **Accept the CLA.** Keep this line in the pull request description, unchanged — the
   template already contains it:

   ```text
   I have read CLA.md and I agree to it for this and my future contributions to Svipall.
   ```

2. **Sign off each commit**, certifying its origin with the
   [Developer Certificate of Origin](https://developercertificate.org/):

   ```powershell
   git commit -s -m "fix: relocate a healed selector when the anchor moves"
   ```

   `-s` appends a `Signed-off-by:` line with the name and email from your git config.

Nothing to print, scan or mail; nothing leaves the pull request.

If your contribution includes code you did not write, say so in the pull request and
name its licence. Anything that is not permissively licensed will not be merged, and
anything you cannot license under [`CLA.md`](CLA.md) cannot be merged either.

## Before you open a pull request

This repository is test-driven. The rules are not negotiable, because the whole gate
runs in CI on Linux, Windows and macOS:

1. **Write or update a test before you change behaviour.** A pull request that changes
   behaviour without a test that would have caught the old one is not ready.
2. `cargo test --workspace` must be green.
3. `pwsh scripts/qc.ps1` (or `scripts/qc.sh`) must pass: fmt, clippy `-D warnings`
   across the feature matrix, tests, `cargo-machete`, size and perf budgets.
   `scripts/qc.ps1 -Fix` fixes what is mechanical.
4. No dead code, no unused dependencies, no unused fields.
5. English only, in code, comments, documentation and identifiers.

Read [`CLAUDE.md`](CLAUDE.md) first: it is the short version of how the pieces fit
together, and it will save you from proposing something the architecture already
answers differently.

## What makes a change easy to merge

- One concern per pull request, with a title that says what changed.
- The test you added, and how to run just it.
- What you measured, when the change touches a budget in `bench/`.
- For a new captcha widget: a row in `WIDGETS` plus a fixture under
  `crates/svipall-core/fixtures/widgets/` — `tests/widgets.rs` checks both.
- For anything under `crates/svipall-cdp/`: a note in `PATCHES.md` explaining the
  deviation from upstream and the test that would catch its loss.

## Reporting a bug

Include the tier that answered (`web_log` says), the URL or a reduced reproduction, the
`blocked_reason` if there was one, your OS, and whether the build has default features.
A failing test is worth more than a paragraph.

## Security

Do not open a public issue for a vulnerability. Mail <code@ilien.dev> instead, with
enough detail to reproduce it, and give the maintainer reasonable time to ship a fix
before disclosing.

## Scope

Svipall evades anti-bot walls; it does not break access controls that protect someone
else's private data. Contributions aimed at unauthorised access, credential attacks,
denial of service or defeating protections on systems the operator has no right to
reach will be declined. See [`DISCLAIMER.md`](DISCLAIMER.md).
