# Development

Lifted out of the README so that file stays readable. Everything here is the same text, with its links repointed.

### Build from source

Only worth it to contribute, or on a platform with no published build. Needs a Rust toolchain plus
`cmake`, `nasm`, `perl` and `llvm` (BoringSSL).

```bash
git clone https://github.com/ilien-dev/svipall
cd svipall
cargo build --release
./target/release/svipall browser install     # optional, recommended: a dedicated Chrome for Testing
```

Three source-build considerations; `svipall doctor` reports browser and model availability:

- On Windows, set a short `CARGO_TARGET_DIR` (e.g. `C:\t`) first: BoringSSL's build paths run into
  `MAX_PATH` and the failure is an unhelpful cmake error.
- `.cargo/config.toml` sets `target-cpu=native`, so what `--release` produces is **for this machine
  only** and can die with an illegal instruction on another. Release artefacts use `--profile dist`
  with an explicit baseline; never ship what `--release` builds here.
- A clean clone carries no model weights. Model-dependent challenges require compatible supplied
  weights or human assistance. `tools/models/export.py` reproduces the detector and segmenter;
  model-enabled release jobs and the `full` container build run it. ONNX Runtime availability also
  depends on the platform; `--no-default-features --features impersonate` omits local models.

No BoringSSL toolchain at all? `cargo build --release --no-default-features` builds without the
TLS emulation and the default local-model features, falling back to reqwest; `web_status` reports which engine is live
under `http_engine`, and asking for the emulating one explicitly on such a build is a **hard error
rather than a silent downgrade**, because a silent downgrade is exactly the failure that is hard to
notice.

### The gate

**TDD: a test before every behaviour change**, and it must fail without the change. `cargo test
--workspace` must be green. The
[2026-09-07 validation](../bench/experiments/native-auto-candidate2-20260907/narrow-validation/qc-execution.json)
passed **1,216 workspace tests**, with zero failures and 22 ignored by default. Separately, ten
automatic/learning/timeout tests and four local browser tests passed with their ignored fixtures
enabled. HTTP/3 passed three tests with one network test ignored, and all four ONNX model tests
passed. Full QC also passed format, the Clippy feature matrix, CPU/structural budgets, 160 browser
probes, identity coherence and the available SIGIR-23 corpus floors. Source, model and corpus
hashes were stable. This validation contains the retained narrow heading change. Two broader
isolated prototypes were rejected after corpus regressions and were not integrated.
The [2026-09-06 validation](../bench/experiments/automatic-public-20260906/latest-code-validation.md)
covers `e60e10b` plus a test-isolation correction and retains the initial shared-directory assertion
failure. The [earlier revalidation](../bench/experiments/revalidation-20260906/README.md) records the
1,170-test `dd8a304` run, installation/MCP checks and its initial cache-path/browser failures.

```powershell
pwsh scripts/qc.ps1        # fmt, clippy -D warnings across the whole feature matrix, tests,
                           # unused deps, CLAUDE.md size guard, perf budgets, extraction
                           # floors, automation tells, identity coherence
pwsh scripts/qc.ps1 -Fix   # fmt + clippy --fix
```

`scripts/qc.sh` is the bash equivalent. **CI** runs fmt, clippy across the feature matrix (including
`--no-default-features` and `http3`), the full test suite, the ONNX model tests, `micro --assert`,
`fingerprint --engine chrome` and unused-dependency checks on **Linux, Windows and macOS**.
Linux starts Xvfb for tests that open a browser window. Each platform runs its applicable installer;
file-size and plugin-manifest guards and the container
build run on Linux. CI is triggered by pushes to `main`, pull requests and manual dispatch.
Releases build five targets, smoke-test each binary they are about to
publish, attach `sha256sums.txt` with a GitHub build attestation, publish the workspace to
crates.io and the wrapper to npm, and push both container images to `ghcr.io`. See
[Releasing](#releasing) for what starts one.

Two steps are the ones that keep this project honest, and **both run offline**: `tells --assert`
opens a page on loopback at five browser passes and fails if a checked probe detects a known
automation tell, and `fingerprint --engine chrome` checks identity coherence. Neither can
be satisfied by argument. **`fingerprint --engine chrome` runs in both `qc` and CI; `tells --assert`
runs in `qc` only**; the CI workflow does not invoke it. It checks local browser behaviour when
run, skips when no browser is available, and is not a green tick on a pull request. The extraction floors are likewise a
`qc` step and skip themselves, loudly, on a machine without the corpora.

```
cargo run -p svipall-bench --release -- \
  micro [--assert] | tells [--assert] | fingerprint [--engine E] | extract [--corpus DIR] |
  evasion [--set hard12|public31|vendors8] [--runs N] [--exit URL] | h3 | h3-ref | cache
```

### Releasing

**A release is a merge to `main` that carries a new version.** Nothing else starts one, and no tag
has to be pushed by hand.

```bash
scripts/sync-version.sh 1.0.0-rc.4   # or scripts/sync-version.ps1
scripts/qc.sh                        # the version tests are part of it
git commit -am "release: v1.0.0-rc.4"
```

`[workspace.package] version` in the root `Cargo.toml` is the only place the number is written by
hand. `sync-version` copies it into every member manifest, every internal dependency line, the
plugin manifest and the npm wrapper; `crates/svipall/tests/release_version.rs` fails the build
when any of them drifts. Each of those files is a different way to be wrong: Claude Code silently
keeps a cached plugin when `plugin.json` names a version it has already seen, the npm postinstall
builds its download URL from its own version, and crates.io rejects a `path` dependency carrying no
`version`.

On a push to `main` the `version` job reads that number and asks one question: does a tag `v<it>`
already exist? If it does — which is every ordinary push — the whole workflow stops there and costs
nothing. If it does not, the release runs: five targets built and smoke-tested, the `.deb`, `.rpm`
and package manifests rendered, the GitHub release published (which is what creates the tag, so a
build that fails leaves none behind for the next run to trip over), then npm, crates.io and the two
container images.

npm and crates.io both publish over OIDC, with no stored secret: each registry was told on its own
site that this repository and this workflow file may publish, and trades the token GitHub mints for
a short-lived one. Both steps skip a version already on the registry, so re-running a release is
safe. Publishing to crates.io is not reversible — there is no unpublish, only `yank` — which is why
the crates go up one at a time, in dependency order, after the release itself exists.

Every member is published except `bench`. That includes the two vendored forks, `svipall-cdp` and
`svipall-quic`: crates.io resolves every dependency of a published crate, optional ones included,
so `svipall` and `svipall-http` cannot exist there while either fork is missing. Their
`UPSTREAM.md` is their readme for that reason.

Pushing a tag still works and is the recovery path when a release has to be re-run from a commit
that is no longer the head of `main`.

---

Contributions are taken under the **DCO** — no CLA, no copyright assignment. See
[`CONTRIBUTING.md`](../CONTRIBUTING.md).

---
