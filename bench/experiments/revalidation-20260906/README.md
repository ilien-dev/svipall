# Local revalidation and README audit — 2026-09-06

Source revision: `dd8a304` (plus the README corrections in this working tree).
The audit changes documentation only; no Rust behavior or historical benchmark data was changed.

**Final QC passed (exit 0): 1,170 workspace tests, three additional HTTP/3 executions, four model
executions, 160/160 automation probes, identity coherence and both performance-budget variants.**
The workspace left 19 tests ignored; the HTTP/3 step left one network test ignored. The eight
targeted executions below include three normally ignored browser tests and repeat five default
tests. Counts are executions, not a sum of distinct tests across all commands.

See [verification.json](verification.json), [final QC](results/qc-final.txt),
[compiler and benchmark output](results/qc-final-stderr.txt), and
[initial failed QC](results/qc-initial.txt).

## Scope

Recheck automatic routing, native fallback, live configuration, local sessions, model execution
and Windows installation. Compare README claims with source, CI workflows, effective defaults
and committed measurement records. These checks do not establish a public-site success rate for
the current automatic policy, nor do they rerun Linux/macOS release jobs or the container build.

## Controlled browser checks

Eight test executions passed using the installed managed browser `152.0.7977.75` on Windows:

- `svipall-mcp/tests/automatic.rs`: six tests, including the normally ignored browser fixture.
- `automatic_learning.rs`: one normally ignored three-visit fixture.
- `automatic_timeout.rs`: one normally ignored timeout fixture.

They verify emulated-first/native-last ordering, native opt-out, the attempt cap, short-page
preservation, persistent visit limits and Retry-After, retained crawl work, local learning and
preservation of the earlier page plus privacy notice after native timeout. The learning fixture
again recorded attempts **2, 2, 1**. Timings are observations, not a speed-improvement claim.

The executables were produced by this audit's `cargo test --workspace` compilation and then
invoked with `--include-ignored --nocapture`, one test thread and an isolated temporary home.
Equivalent Cargo invocation after QC:

```powershell
cargo test -p svipall-mcp --test automatic --test automatic_learning --test automatic_timeout -- --include-ignored --nocapture --test-threads=1
```

All fixture connections terminate on loopback. `.test` names exercise external-origin policy
through a local proxy; they are not contacted through external DNS.

## Packaging and documentation checks

The current debug CLI and MCP binaries were copied into an isolated runtime directory. The
existing `stage-windows-runtime.ps1`, `test-local-install.ps1` and `smoke-local-mcp.ps1` scripts
passed, including `-RequireLocalRuntime`. The MCP handshake listed **29 tools**, applied a saved
configuration change on the next call, and loaded all four checked runtime DLLs from the fixture
directory. The installer persisted the browser opt-out; uninstall retained unrelated and modified
DLLs. This checks local packaging mechanics, not a newly built portable release artifact.

All **45 documented configuration fields** were checked against `svipall config show` from the fresh binary
(the API-key placeholder was excluded from value comparison). All **48 relative file references**
checked in the README existed. Historical `public31` JSON distinguishes **59 HTTP stopping cells**
from **44 HTTP cells scored `ok`**; neither count is a current automatic-mode delivery rate.

The README now distinguishes two-observation automatic learning from legacy tier memory;
documents native fallback in its main explanation; separates local storage from outbound traffic
and client data handling; labels duplicates without promising removal; names the unauthenticated
health endpoint; and describes platform-specific CI coverage. Historical experiments remain dated
and are explicitly separated from today's automatic policy.

The linked upstream project READMEs were spot-checked for the comparison table:
[Firecrawl](https://github.com/firecrawl/firecrawl),
[Crawl4AI](https://github.com/unclecode/crawl4ai),
[Scrapling](https://github.com/D4Vinci/Scrapling), and
[Playwright MCP](https://github.com/microsoft/playwright-mcp).
Their rows remain attributed feature descriptions, not comparative performance measurements.

## Initial QC failure

The first QC run reused an integrity-test executable compiled under the repository's former
`spivall` directory. Four fixture-loading tests failed because its embedded `CARGO_MANIFEST_DIR`
pointed there; the fixtures exist under the current `svipall` directory. Its Cargo dep-info file
confirmed the stale value. Source timestamps for the integrity, widget and TLS-stack test files
were refreshed to force recompilation; their contents were unchanged. Temporary test directories
are fresh for each run to avoid reusing SQLite fixtures keyed by Windows process IDs.

The same initial run had no valid explicit managed-browser path in its isolated home and
auto-detected Brave. Its automation check scored **150/160**, with failures in `brave_absent`,
`window_chrome_height` and `host_object_brands`. This is retained as a failed run, not counted as
160 clean probes. The final run explicitly selects the managed Chrome for Testing binary, just
as the targeted browser fixtures do. Probe results are specific to that browser and host setup;
they do not establish that every detected Chromium-family browser is a coherent emulation.

## Reproduction and limits

QC used `CARGO_TARGET_DIR=C:\t`, `CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=2`, fresh `TEMP`, `TMP`
and `SVIPALL_HOME` directories, and an explicit `SVIPALL_BROWSER` pointing at the existing managed
browser for the final run. The command was:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/qc.ps1
```

No external extraction corpus was selected, so the optional extraction-floor step was skipped.
No public-site evasion benchmark or network fingerprint test was rerun. This audit makes no new
claim about those historical scores, cross-platform runtime compatibility, anonymity or universal
detector acceptance. The comparison-table review is a source spot-check, not an execution of
competitors or a verification of every external research/legal assertion in the README.

The tracked Rust line counts were 65,882 owned and 47,753 vendored, including tests and benchmarks.
The README rounds those dated counts. Before saving logs here, absolute repository and user-home
prefixes were replaced with `<repo>` and `<home>`; results and measurements were not changed.
Installed user settings and historical baseline files were untouched.
