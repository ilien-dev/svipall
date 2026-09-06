# Local reliability improvements

Requested outcome: implement the local improvements identified in the review and publish a reproducible before/after comparison. No external solvers, proxy services, or additional automation frameworks.

## Completion requirements

- [x] Preserve a baseline executable built from the original product with the same comparison harness as the candidate.
- [x] Fix first-visit detection and stable-profile eligibility for kept pages.
- [x] Preserve a live document when reusing it; distinguish reuse from navigation and cache hits.
- [x] Add bounded, progress-aware challenge budgets and test renewal eligibility.
- [x] Add a configurable native-hardware browser mode, including local locale/timezone defaults.
- [x] Expose configuration within the CLI/MCP and integrate browser provisioning without manual file edits.
- [x] Verify bundled local model availability and report capability limits accurately.
- [x] Fix invalid-status and UTF-8 benchmark defects; retain the historical score beside delivery/error metrics.
- [x] Record all repeat positions, configuration, seed, state, latency and failures in measurements.
- [x] Run meaningful regression tests, browser checks and required quality gates.
- [x] Measure original and changed product with controlled local state and explicit network-history limitations.
- [x] Publish comparison and completion evidence, including regressions or unchanged results.

## Measurement protocol

The comparison harness is added before product changes and compiled into both executables. Original target lists remain frozen. Each sample records the public rule independently of product delivery. Repeated fetches are reported individually, never silently discarded. Both arms use the same browser, target order seed, timeout and repeat count, with separate state directories and the same initial identity seed. Live server/IP state cannot be reset locally; run order and local reputation spend are recorded, and elapsed-time effects must not be attributed to the implementation.

## Evidence

Baseline: product source `b291e45c68e41604546542d753f0808e738b4a48`, with the new shared measurement
harness and scoring fixes, before any product changes. Built using `cargo build --release -p
svipall-bench --features onnx` and preserved at `bench/experiments/local-20260905/bin/before.exe`.
Authoritative clean-reference SHA-256:
`1BB26A54B3ADF851891C2529BF1B9464846150468EB9B4FED1D8C45FC5CAFA32`.

The branch advanced independently to `cd2653c` during this work and included the measurement
harness in that commit. Its packaging changes and subsequent agent-instruction/QC edits are
preserved. The saved reference binary is the authoritative baseline, not the moving HEAD.

`cargo test -p svipall-core -p svipall-bench`: 38 benchmark tests passed (4 corpus tests ignored),
516 core unit tests passed, 11 core integration tests passed. Invalid-status and UTF-8 regressions
are covered. Configuration overlays validate before replacing the saved settings.

`cargo test -p svipall-mcp --test local_sessions -- --test-threads=1 --nocapture`: real Chrome
loopback tests passed for native/emulated worker agreement and both named/automatic profile reuse.
The fixture's SDK sends the returning request to a new endpoint; the response and hit counts prove
the old document survived and fresh data was fetched. A live-policy test was added afterward and
is included in the full QC run.

The default desktop build now enables embedded detector, segmentation and grid paths. Release and
container minimal builds explicitly opt out; no runtime model download was added. Local model
execution tests passed in QC. Full QC logs are under `bench/experiments/local-20260905/results/`.

Full `scripts/qc.ps1` printed `QC PASSED`: workspace and model tests, all feature lint arms,
HTTP/3 offline tests, manifest validation, instruction-size guards, CPU/structural budgets,
160/160 automation probes and identity coherence passed. Extraction-corpus checks were skipped
because no corpus path was configured. The script itself exited; its PowerShell `Start-Process
-Wait` wrapper continued waiting for a descendant and was stopped after confirming the script
was no longer present. Subsequent clippy and the new Rust summary aggregation test passed.

Live configuration is exposed through `web_status.configure` / `/v1/status` as well as CLI
settings. Its integration test confirms next-request application and stable in-flight policy.
Browser install/update now selects the installed executable for the next request automatically;
removal disables automatic reinstallation. Existing named sessions retain their generation.

Live comparison is complete: 27 runs and 918 samples passed the frozen-hash, target-order and
delivery-verdict audit. The measurement script rotates three arms across
three rounds, with first/returning visits separately reported, using the saved before binary and
the final candidate binary. Results are aggregated by `svipall-bench summarize` (no Python).

Final-artifact inspection caught a gap the earlier optional-model tests did not cover: `doctor`
reported inference enabled but no embedded models. The cached models build script used an
`env!("CARGO_MANIFEST_DIR")` path from the workspace's previous name (`hlid`). It now reads Cargo's
runtime environment, and a regression asserts that source assets present at build time are
actually embedded. All three model crate tests and four model execution tests passed after the
fix. The rebuilt CLI reports embedded `detect` and `segment` with inference enabled and no
separately installed models (`results/doctor.json`). Both final benchmark executables pass micro
budgets with actual embedded inference (`*-micro-final.txt`). The final MCP binary passes a stdio
initialization/configuration smoke test with 29 tools and next-call application of saved settings.

The first saved reference also lacked embedded models. To avoid crediting session changes for
that build-cache defect, a clean reference was built from an archive of `b291e45` with the
same four measurement files and the same model assets as the candidate. Its product sources are
unchanged. The first binary remains at `bin/before-cache-path.exe` for diagnosis; it is not the
reference for the live comparison. The clean archive is under `source-before/` (ignored), and the
comparison manifest records the actual executable hashes. Both reference and candidate
contain the same detector and segmentation assets. Candidate SHA-256:
`404677BB83A9A59E27A2AEEB1F659034A5A0A84D2782458B94EC5B5744BCD874`.

The initial complete reference run exposed a Windows PowerShell controller defect: `Refresh()`
discarded the exited process's status. The controller now retains its native handle and reads the
exit status without refreshing. The complete reference result was preserved on resumption, and the
shorter 54-second first pause is recorded in `deviations.json`. Later candidate/native runs completed
with verified zero process exit codes.

During measurement, upstream `8e94755` removed browser-side consumers that had been swept into
`4ae8ac8` without their uncommitted core/CDP fields. The complete browser implementation was
restored from `739eda1`, which also contains the independent sibling-directory version fix. The
working tree contains the matching core/CDP fields. Frozen measurement binaries are unaffected;
the final full quality gate passed after restoration (`results/qc-final.txt`, exit 0). The native
browser regression also passed explicit locale/timezone agreement with its worker, alongside
emulated and native defaults (three integration tests, 19.51 seconds).

An environment transition left the second public candidate round without a live controller,
benchmark or browser process. Its 52 recorded calls (44 delivered, 8 walls) and original state are
preserved under `interrupted/after-public31-2` and `state/interrupted-after-public31-2`; no complete
response JSON exists. The full round resumes with fresh state, and the extra remote history is
documented. The replacement controller is detached and its PID/start time are checked while waiting.

The detached controller also disappeared at the next continuation, during `before-public31-2`
after 12 progress records. They are archived separately with the original state. A temporary
interactive Windows scheduled task ran `scripts/run-local-comparison.ps1`; its benchmark
process was confirmed in desktop session 1, matching the earlier runs. Its successful completion
and verified removal are recorded in `controller-result.json` and `controller-cleanup.json`.
Aggregation and the completion audit passed. Total interrupted progress records: 64.

The current rc.2 CLI/MCP artifacts were rebuilt after the restored browser implementation. Their
stdio smoke passes initialization, 29 tools and next-call configuration. Windows import auditing
found four compiler runtime dependencies that were absent from release archives. Release packaging
now stages Visual Studio's release redistributables app-locally, and the PowerShell installer
copies their hash-checked manifest and DLLs. An actual loaded-module check proves the MCP process
uses all four from its own artifact directory. The offline installer fixture passes install,
browser opt-out persistence, uninstall, and preservation of unrelated/modified files. Installers
no longer ask an additional browser-download question. A Windows PowerShell diagnostic-stderr
issue exposed by the fixture was also fixed. Evidence: `results/install-smoke.json` and
`results/mcp-smoke.json`. These locally tuned test binaries are not portable distribution
artifacts; the release workflow continues to use `--profile dist`.

Runtime packaging fixtures are isolated under ignored `runtime/`. The frozen live benchmark
executables remain in `bin/` without app-local DLLs, preserving the runtime used in previous rounds.

The final manual network checks exposed an existing isolated-profile cleanup defect: the pooled
browser still held the directory open on Windows. The isolated branch now uses the existing
bounded retirement helper to close its dedicated browser before removal. The failing test passed
after the change; all eight browser tests, four HTTP tests and the live HTTP/3 test passed. The
full gate was rerun after this correction and returned `QC PASSED`, exit 0 (`qc-complete.*`).
No live comparison sample uses isolated mode, so the frozen fetching comparison is unaffected.

The [completed findings](../bench/experiments/local-20260905/findings.md) publish delivery medians,
full ranges, cumulative times, substantive recoveries, the empty Zillow reference, the minimal
Home Depot responses, six unsuccessful challenge renewals and zero observed live document reuses.
Scripts from the host's security software were observed in captured public HTML in all three
arms and are recorded as an environment condition, not attributed to the product.

The CLI/MCP binaries were rebuilt after that final correction, and their new hashes are recorded
in `current-artifacts.json`. Final startup checks confirm both embedded models, local inference,
29 MCP tools, app-local runtime loading and an actual configuration change from 21000 to 22000 ms
applied on the next call. `verification.json` records the completed checks and the four unrun
external-corpus diagnostics. The comparison executables retain their original frozen hashes.
