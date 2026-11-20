# Contributing to Svipall

Thanks for wanting to help. A contribution does not have to be a pull request: a clear issue
that reports a bug or proposes an improvement is a contribution too.

Svipall lets an AI agent read the web from the operator's own machine: it fetches, crawls and searches pages, gets past anti-bot walls by evading them, never by breaking access controls, and answers captchas locally. **A contribution that drifts away from that purpose will be rejected**,
however well it is made.

<!-- This file is generated from .github/repo-rules.json by Nabu (https://github.com/ilien-dev/nabu).
     Project-specific notes go in .github/contributing-notes.md, which is copied below. -->

## Nothing is guaranteed

Opening an issue or a pull request does not guarantee that it will be accepted or merged.
Every change is reviewed before it goes in, the maintainer's own included. A change can be
declined for its scope, its approach or the cost of maintaining it.

## Before you start

- **Search first.** Look through the [pull requests](https://github.com/ilien-dev/svipall/pulls?q=is%3Apr)
  and the [issues](https://github.com/ilien-dev/svipall/issues?q=is%3Aissue), open and closed. If someone
  already works on it, or it was declined, add to that conversation instead.
- **For anything bigger than a small fix, open an issue first** and wait for an answer. It is the
  cheapest way to find out whether the change fits before you write it.

## Issues

Use the forms: **Bug report** or **Feature request**. One issue per problem or idea. A bug report
says what you did, what you expected and what happened; a feature request says the problem
first and the solution after. Never paste tokens, keys or private data.

## One change per pull request

Each pull request does **one thing**. A new feature comes alone: no settings change, no visual
tweak elsewhere, no unrelated cleanup. Those go in their own pull requests. The smaller and more
focused a PR is, the faster it can be reviewed and merged; a PR that mixes changes will be asked
to split, or closed.

## Branch name

```
<type>/<1 to 5 words>
```

`<type>` is the Conventional Commit type of the change, the same one the title starts with:
`feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci`, `chore`, `style` or `revert`. Then 1 to 5 words of lowercase letters and digits joined by single
hyphens.

- `feat/export-cover-image` ✓
- `fix/progress-bar-width` ✓
- `feat/Export-Cover` ✗ (uppercase)
- `feature/export-cover-image` ✗ (the type is `feat`)
- `fix/add-a-new-progress-bar-width` ✗ (more than 5 words)

## Title and versions

The title is a [Conventional Commit](https://www.conventionalcommits.org/) of at most
72 characters, with the same type as the branch: `feat: export the cover image`. Pull
requests are squash-merged, so the title becomes the commit on `main`, and it decides
the next version:

| Title | Version |
|---|---|
| `feat: ...` | new minor version (1.4.0 → 1.5.0) |
| `fix: ...`, `perf: ...`, `revert: ...` | new patch version (1.4.0 → 1.4.1) |
| `feat!: ...` or any type with `!` | new major version (1.4.0 → 2.0.0) |
| `docs`, `test`, `ci`, `chore`, `refactor`, `build`, `style` | no new version |

**Nobody writes a version number.** A workflow publishes the version, the changelog and the
release after every merge. Do not edit version numbers or `CHANGELOG.md`.
A change to what users install (`crates/*/src/**`, `crates/*/Cargo.toml`, `crates/*/build.rs`, `crates/*/models/**`, `crates/*/assets/**`, `Cargo.toml`, `Cargo.lock`, `plugins/**`, `skill/**`, `skills/**`, `integrations/**`, `packaging/npm/**`, `packaging/templates/**`, `tools/**`, `server.json`, `Dockerfile`, `Dockerfile.slim`, `docker-entrypoint.sh`, `install.sh` and `install.ps1`) has to publish a version, or they never get it:
its title must be `feat`, `fix`, `perf` or `revert`. The check rejects any other type for those files.

## Every change to what users get is measured

A new feature, a fix or a change to how something works goes in only when it is measured and the
numbers show it adds value, or at least that nothing gets worse. A change that cannot be measured
is declined, however good it looks. Put the result on `main` and on your branch in the
**Measurement** section of the PR. How to measure here:

- A fix: the test that fails on `main` and passes on your branch, and the command that runs just it (`cargo test -p <crate> --test <file>`).
- A change to what a page returns, how often a wall stops it, or how fast it answers: `bench` on `main` and on your branch, run exactly as `bench/baseline/README.md` says. It counts only when the median leaves the previous range; publish the runs that improved nothing too.

For `feat`, `fix` or `perf` changes to what users get, Measurement cannot say "not applicable". For any
other change it can, with the reason (`Not applicable: docs only`).

## Description

Start from the template GitHub fills in
([`.github/pull_request_template.md`](.github/pull_request_template.md)). It is short on purpose:
it holds what a reviewer needs to decide quickly, and nothing else.

| Section | Limit | What goes in it |
|---|---|---|
| **What** | 280 characters | The one change this PR makes. |
| **Why** | 280 characters | The problem it solves. Link the issue (`Closes #12`). |
| **How** | 500 characters | The approach, and the main files it touches. |
| **Risk** | 200 characters | What could break, and how to undo it. |
| **Measurement** | 500 characters | The numbers before and after (see above). |
| **Evidence** | 300 characters of text | How you checked it, how a reviewer can check it; screenshots. |
| **Checklist** | — | Every box ticked. |

**Screenshots** are optional and always welcome, one or as many as it takes to make the change
easy to understand; for a visual change, before and after. Take them yourself
or ask your agent to; capture only that window or page, never the whole screen. Drag the files
into the description box on GitHub. Images, GIFs and links do not count toward the limit.

A check verifies the branch, the title and the description. Run it before opening the PR:

```
node .github/scripts/check-pr.cjs --branch feat/export-cover-image \
  --title "feat: export the cover image" --body-file pr.md --files-from-git main
```

## Before you push

```
cargo test --workspace
scripts/qc.sh
```

Every command must pass; CI runs them again on the pull request.

## Review

A pull request is merged after an approval from someone with write access to the repository and
with every check green. A new push dismisses an earlier approval. Answer review comments in the
PR; keep extra changes for another PR.

## Working with an AI agent

The rules are the same. Agents that read `.claude/skills` or `.agents/skills` have an
`uncontributing` skill that walks through them step by step and checks the result.

## Project notes

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

