# Automatic policy on public targets — 2026-09-06

**Completed: 459 calls, nine batches, 48 requested URLs across 51 target slots.**
The frozen `dd8a304` executable passed the existing delivery check on **348/459 calls (75.82%)**:
333 with emulated identity and 15 with native identity. This is a first-response delivery-check
rate, **not a complete-extraction or challenge-solving rate**. Of the 348 passes, 156 were explicitly
paginated; the [content audit](content-audit.md) documents head fragments, a login page, empty search
results and a title-only catalogue response that still pass the check.

## Results, including failures

| Set | Passes in rounds 1 / 2 / 3 | Median and range per round | Total passes / calls | Successful-call median | Fetch seconds per delivery |
|---|---|---|---:|---:|---:|
| public31 | 79 / 79 / 79 (93 calls each) | 79/93; 79..79 | 237/279 (84.95%) | 1.01 s | 5.94 s |
| hard12 | 25 / 28 / 19 (36 calls each) | 25/36; 19..28 | 72/108 (66.67%) | 1.31 s | 13.61 s |
| vendors8 | 13 / 14 / 12 (24 calls each) | 13/24; 12..14 | 39/72 (54.17%) | 3.50 s | 11.98 s |

These per-round counts include all three consecutive calls per target. [Visit-position tables](results.md)
separate positions 1, 2 and 3 and report their three-round medians/ranges and p95 latency.
Target slots overlap across sets; the lists include diagnostic pages and basic reference controls.
The historical vendor labels are not a fresh identification of every site's protection stack.

Total fetch time was **2,853.49 seconds (47.56 minutes)**, including failures. Dividing that cost
by 348 delivery-check passes gives **8.20 seconds per delivery**. The all-call median/p95 were
1.01/40.11 seconds; the successful-call median was 1.01 seconds. Controller wall time was
**63.95 minutes**, including eight fixed two-minute pauses and process overhead, from
09:09:11 to 10:13:08 UTC on 2026-09-06. Preparation, content review and later QC are outside that duration.

There were **66 confirmed local deferrals** (32 cooldowns, 34 address-budget refusals), nine
recorded timeouts, and 106 browser-launch errors across 53 calls. All failures remain in the
denominator. The third `vendors8` round delivered fewer results while its all-call median fell
to 0.17 seconds; its successful-call median was 2.79 seconds. Fast refusal is not fast delivery.

Native fallback was recorded on **28 calls**, all with a privacy notice; **15 delivered with native
identity**. Those include a paginated head and an empty-result UI. These are conditional outcomes
of reaching the fallback, not fifteen proven complete extractions or a causal gain over disabling
native. Recorded attempt arrays also show examples of the starting route changing after two useful
observations; see the audit for exact calls and their content limitations.

## Revision and protocol

The executable was copied before traffic and held at SHA-256
`6111b0567787e8b6942cb0b9d619be3a2bc619feee1a47f5be248a5b68681049`, from revision
`dd8a30485d7ae005aacdafbb5bc1f9fd832af7a5`. The managed browser was Chrome for Testing
`152.0.7977.75`. Machine: Windows 11 Pro 10.0.26100, Ryzen 9 7950X3D (16 cores/32 threads),
62.9 GiB RAM. This was an optimized local `--release` build with `target-cpu=native`, not a
distributable artifact or a peak-memory measurement.

During the run, a separate change advanced the workspace to `e60e10b`, fixing browser directory
ownership, shutdown and diagnostics, plus Linux CI display setup. **The public figures above do
not include that fix.** The [later local validation](latest-code-validation.md) identifies the
newer tested revision separately. The machine's background workload and security-software page
injection were not controlled, so this is an observed host/exit snapshot, not a clean-room comparison.

The [prespecified protocol](protocol.md) was saved before traffic and is unchanged. Its
[manifest](manifest.json), [target definitions](targets.json) and [machine/runtime metadata](machine.json)
record hashes, configuration, model assets and schedule. Key conditions:

- Three rounds per set, three consecutive calls per target, with one persistent isolated home.
  Route evidence, browser profiles, traffic, reputation and cooldowns persist across every batch.
  Initial reputation/traffic were snapshotted from the installed home; user credentials, cookies
  and settings were not imported. Later rounds are not cold starts.
- `browser_identity=auto`, native fallback enabled, 60-second total caller timeout, six-attempt
  cap, default traffic/reputation limits, cache bypass, human assistance disabled. No external
  solver, proxy service or geolocation lookup was added. Server-side history is uncontrolled.
- `public31` requests HTML; the other sets request Markdown. The default 25,000 estimated-token
  response budget remains active, and the harness does not follow continuation cursors.
- Delivery requires status 200..399, no `blocked_reason`, nonempty returned content, and at least
  one expected string where supplied. `public31` supplies no expected strings. Historical verdicts
  are retained separately; the audit does not retrospectively change this scoring rule.
- Set order rotates. The existing harness uses `seed | 1`, so recorded seeds 20260908 and 20260909
  yield the same within-set order in rounds 2 and 3. They are temporal repetitions, not independent
  random samples. Failed/partial batches are never silently replayed; this run completed all nine.

No contemporaneous competitor or explicit-native control arm was run. Earlier benchmarks use
different policies, timeouts and state handling. **This experiment does not establish a causal
speedup, superiority to another tool, universal detector acceptance, anonymity, or future reliability.**
The reduced third-round delivery counts are published alongside the shorter times. Historical
`bench/baseline/` records were not replaced.

## Files and offline verification

- [summary.json](summary.json), [calls.csv](calls.csv), [targets.csv](targets.csv): aggregates,
  all 459 calls and all 51 target slots, including failures.
- [results/](results/): nine compressed response records and corresponding stderr logs. Only
  repository/home prefixes and one observed exit IP were redacted; original file hashes are retained.
- [verification.json](verification.json): completed-batch, protocol/target and published-file hashes.
- [audit-verification.json](audit-verification.json): frozen executable/runtime checks, native privacy
  notices, explicit deferral reasons and supplementary content-flag counts.
- [content-audit.md](content-audit.md): representative content review and measurement limits.
- [offline-tests.json](offline-tests.json): ten controller/reporter checks; the initial failing
  controller test run is retained in [tests-before.txt](tests-before.txt).

Run from the repository root, without network access:

    python bench/experiments/automatic-public-20260906/verify.py
    python scripts/test_measure_automatic.py
    python bench/experiments/automatic-public-20260906/test_report.py

The verifier recomputes outcomes/timings aggregates from the published records and checks hashes,
459 distinct calls, 51 slots and per-round ranges. The frozen summary field
`refused_without_attempt` counts empty attempt logs; the reporter separately requires an explicit
policy reason for a confirmed deferral. All 66 empty logs in this dataset meet that stricter rule.
Native counts reflect recorded flags; an outer timeout can otherwise lose internal attempt details.

For a new measurement, use a new directory with a copy of `protocol.md` named `README.md`, then
run `scripts/measure_automatic.py prepare`, `run` and `analyze` with `--root` pointing there.
`prepare` also requires `--binary`, `--browser` and `--cli` paths. Stage the Windows runtime DLLs
beside the copied executable before `run`, using `scripts/stage-windows-runtime.ps1`. The exact
nine benchmark argument lists are defined by `manifest.json` and the controller. Reusing these
commands reproduces the procedure, not a promise that changing public sites return identical data.
