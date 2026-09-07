# Local validation after the public measurement

The public results in this directory belong to frozen revision `dd8a304`. While that run was in
progress, a separate change advanced the workspace to `e60e10b`, correcting browser directory
ownership, shutdown timing, error messages and Linux CI display setup. These local checks cover
`e60e10b` plus the test-isolation correction described below. They do not update the public-site
delivery figures or measure the performance of the newer browser implementation on public sites.

**Final QC passed: 1,175 workspace tests, zero failures, 19 ignored; three additional HTTP/3
executions, four model executions, 160/160 automation probes, identity coherence and both
performance-budget variants.** The three normally ignored automatic-routing browser fixtures
also passed. Their learning sequence again records two attempts, two attempts, then one attempt.
No Rust/configuration source changed during the final QC and fixture run.

See [final metadata and source hashes](latest-code-verification.json),
[QC stdout](validation/latest-qc-stdout.txt), [QC stderr](validation/latest-qc-stderr.txt),
[ignored-fixture stdout](validation/latest-ignored-auto-stdout.txt) and
[ignored-fixture stderr](validation/latest-ignored-auto-stderr.txt). Counts are test executions;
the five-session fixture rerun below overlaps the workspace suite rather than adding five unique tests.

## A failure retained and corrected

The first QC on `e60e10b` failed in
`browsers_without_a_profile_get_a_directory_each_and_leave_none_behind`: the shared sessions
directory contained one scratch directory after the test's own pools shut down, where it expected
zero. The concurrent native-identity case also creates scratch profiles in that same process-wide
fixture home. No scratch directory remained after all the cases had finished. The assertion was
counting another case's live browser as a leak.

The test suite now takes an asynchronous mutex around cases sharing that home and its mutable
configuration. The regression still uses `tokio::join!` to open its two pools concurrently and
still checks that both profiles disappear. No product behavior was changed by this audit.
With five Rust test-harness threads, all five `local_sessions` cases passed after the correction.
This is a fixture-isolation fix, not a relaxed cleanup assertion.

Evidence: [initial QC metadata](latest-code-initial-verification.json),
[initial stdout](validation/latest-qc-initial-stdout.txt),
[initial stderr](validation/latest-qc-initial-stderr.txt),
[fixture rerun](local-sessions-isolation-verification.json),
[fixture stdout](validation/local-sessions-isolation-stdout.txt), and
[fixture stderr](validation/local-sessions-isolation-stderr.txt).

## Reproduction and scope

QC uses a fresh isolated home and temporary directory, the explicit managed browser
`152.0.7977.75`, `CARGO_TARGET_DIR=C:/t`, two Cargo build jobs and two Rust test threads.
The fixture regression was also run with five harness threads. The commands are:

    cargo test -p svipall-mcp --test local_sessions -- --nocapture --test-threads=5
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/qc.ps1
    cargo test -p svipall-mcp --test automatic --test automatic_learning --test automatic_timeout -- --ignored --nocapture --test-threads=1

The last command runs the three normally ignored automatic-routing fixtures without repeating
their default tests. Fixture traffic is loopback; no new public-site measurements are made here.
The final metadata records the exact source hashes, the test-file modification relative to
`e60e10b`, command exits and whether any source changed during QC.

These are Windows checks. Linux/macOS CI execution, distribution builds and external extraction
corpora are outside this rerun. The earlier installation/MCP smoke verification remains separately
recorded under [the initial revalidation](../revalidation-20260906/README.md).
