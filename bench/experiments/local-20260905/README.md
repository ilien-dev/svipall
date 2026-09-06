# Local-only implementation comparison

The live experiment is complete: 27 runs, 918 paired-trial samples and 64 separately preserved
interrupted progress records. Read [findings.md](findings.md) for the before/after interpretation,
[comparison.md](comparison.md) for medians/ranges and [summary.json](summary.json) for aggregates.

## Protocol

- Three frozen sets: `public31`, `hard12`, and `vendors8`.
- Three arms: original product (`before`), changed default (`after`), and changed product with
  browser identity scripts disabled (`native`).
- Three rounds per arm and set, two successive fetches per target: 918 samples in total.
- A 90-second whole-fetch timeout; local cooldown refusals remain failed delivery samples even
  when they make no new request to the target.
- Identical target order within a round, separate fresh homes and the same initial machine seed.
  The recorded seeds are 20260906, 20260907 and 20260908. The shared harness normalizes seeds with
  `seed | 1`, so the first two rounds use the same target order; the third uses a different order.
- The arm order rotates each round. The controller waits at least 120 seconds between runs.
  `deviations.json` records the shorter first pause after a PowerShell controller restart; the
  completed reference was preserved, and no target requests were repeated for that restart.
  A later interruption stopped `after-public31-2` after 52 logged calls without a complete JSON
  response. OS inspection confirmed its processes were gone before resumption. Those logs and
  interruption metadata are preserved in `interrupted/after-public31-2/`, with the original profile
  retained under `state/interrupted-after-public31-2`. The complete round was repeated with fresh
  state. These 52 additional calls and the longer interruption are separate from the 918 planned
  complete samples; they add uncontrolled remote history. They are not hidden or scored as successes
  in a complete paired round, and their raw response contents cannot be recovered from the progress log.
  The detached controller later disappeared during `before-public31-2`, leaving 12 more progress
  records. These are archived under `interrupted/before-public31-2/`; the original state is retained
  separately. There are 64 additional recorded calls across the two interruptions, plus any
  unobserved in-flight call. Coordination ran as a temporary Windows Task Scheduler task in
  the same interactive desktop session, outside the conversation process. `scheduled-controller.json`
  identifies it, and `controller-result.json` records successful completion. The task was removed
  and its absence verified in `controller-cleanup.json`; this is test orchestration, not a product dependency.
  Source validation and the final CLI/MCP build also ran during a deliberate pause between
  `before-hard12-2` and `after-vendors8-2`, with live measurement disabled. Longer gaps therefore
  include both interruptions and source verification. `timeline.csv` records the gaps between
  complete runs; those gaps are not guaranteed request-free because of the archived partial runs.
- Same Chrome for Testing executable, machine, network connection and embedded model assets.
- The reference is source `b291e45c68e41604546542d753f0808e738b4a48` plus the identical measurement
  harness. The candidate includes the session, wait, identity and configuration changes described
  in `../../../docs/local-configuration.md`. `manifest.json` records executable hashes.

The page cache is bypassed. Every fetch, including timeouts and missing content, stays in the
denominator. First and returning visits are reported separately. Success means a valid 2xx/3xx
status, nonempty returned content, no reported wall, and expected text where the frozen set supplies
it. Public-page delivery is not proof that a JavaScript fingerprint detector considers the browser
human. The historical public scoring rule is recorded separately; it could mark status-zero failures
as `ok`, so the corrected valid-status score is also retained.

Repeated calls and overlapping targets are correlated. The 918 samples are not 918 independent
sites, and success rates are reported per set. `content-audit.md` records a limitation of the
frozen expected-text rules: a brand match can accept a minimal shell without the desired catalogue.

Local state separation cannot reset remote IP history or control changes at the target. Results
describe these runs from this machine; a median outside the reference range is the repository's
descriptive improvement criterion, not proof of statistical significance or implementation causality.
The comparison uses no external solving service, proxy purchase, remote browser or manual challenge
completion.

## Validation artifacts

- `results/qc-complete.txt`, its stderr log and `results/qc-complete.json`: final full quality
  gate after the isolated-profile correction, `QC PASSED` and exit 0, including 160/160 automation
  checks and identity coherence. Earlier gates are also retained. No extraction corpus was configured.
- `results/doctor.json`: CLI confirms embedded detector and segmentation models, inference
  enabled, no separately installed models, and `ok=true` in the isolated verification home.
- `results/mcp-smoke.json`: initialization, 29 exposed tools and a persisted setting applied on
  the next MCP call without restarting the server, plus app-local runtime module paths.
- `results/network-tests.json`: the eight manual browser tests, four HTTP fingerprint/redirect
  tests and live HTTP/3 test passed. The initial isolated-profile cleanup failure is preserved
  separately; closing its dedicated browser before removal fixed it.
- `results/install-smoke.json`: offline Windows install/uninstall, persisted browser opt-out,
  and preservation of unrelated or modified files.
- `results/before-micro-final.txt` and `results/after-micro-final.txt`: both binaries pass CPU and
  structural budgets, including actual embedded-model inference. These supersede the earlier
  `before-micro` diagnostic made before the build-cache defect was discovered.

The first reference build lacked model assets because a cached build script had captured an old
workspace path. It is retained as ignored `bin/before-cache-path.exe` for diagnosis and is excluded
from live results. The authoritative reference was rebuilt from a clean source archive with the
same model assets as the candidate. The product fix reads the build script's runtime manifest path;
a regression test checks that source assets present at build time are actually embedded.

Run `scripts/compare-local.ps1` to measure; it preserves completed runs and refuses to overwrite
incomplete ones. Aggregate with `bin/after.exe summarize --dir bench/experiments/local-20260905`.
Binary, profile and source-archive directories are ignored; raw measurement JSON and logs are kept.
`scripts/audit-local-comparison.ps1` checks all 918 samples and frozen hashes and exports CSV data.
The browser/network checks ran after the last measured response. No comparison sample uses the
isolated-profile branch corrected during those checks. The host's security software injected
scripts into captured pages in all three arms; this environment condition is documented in the findings.

Before publication, the machine's absolute home path was replaced by `<repo>` and `<home>` in the
committed logs and manifests. Only those display strings changed: no measurement, verdict, hash or
recorded value was touched, and the SHA-256 values in `manifest.json` and `verification.json` still
verify against the files they name.
