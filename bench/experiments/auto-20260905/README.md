# Automatic routing verification — 2026-09-06

Final verification passed: full QC, 1,177 test executions, 160 automation probes and eight targeted
integration/browser tests (including the manual fixtures). CLI default selection and persistent
configuration updates also passed. See [verification.json](verification.json),
[complete QC](results/qc-complete.txt) and [browser checks](results/manual-verified.txt).

This change prioritizes emulated identity over delivery scores. It adds one optional native
fallback, local route evidence, persistent visit admission and cooldowns. No public-site delivery
rate is claimed for this policy. The previous `local-20260905` figures describe different policies
and must not be presented as automatic-mode results. Historical baseline files are unchanged.

## Protocol

- Pure policy and SQLite tests cover route ranking, expiry, repeated failures, short delivered
  pages, context separation, concurrent reservations and persistence across reopened connections.
- Integration tests use an HTTP proxy fixture that only listens on loopback. Hostnames ending in
  `.test` exercise external-site admission; no external DNS or target is contacted.
- Manual browser fixtures verify emulated-first/native-last order, native opt-out, learning over
  three visits and preservation of the earlier page and privacy notice when native times out.
- Browser: the existing managed desktop build at version `152.0.7977.75`, Windows, the same host
  as the previous comparison. Host software can inject markup; the learning fixture intentionally
  uses a substantial synthetic document to avoid testing the host-dependent text/markup ratio.
- Full QC uses two build jobs and two test threads to bound compiler memory consumption. External
  extraction corpora are not selected; those optional corpus checks are not measured.

The learning fixture records two transport attempts on each of its first two visits, then one
emulated-browser attempt on the third. This demonstrates routing behavior, not a statistically
established speed improvement or a claim about real-world completeness. Quality remains a label;
short delivered pages neither become failed routes nor trigger native exposure on their own.

## Audit notes

The default-mode and launch-defense regression tests failed before implementation and passed after
the fixes. The repeated-failure policy test likewise failed before adding route backoff.

The first broad QC run hit Windows virtual-memory exhaustion while compiling many tests in
parallel; the owned QC process tree was stopped and verification resumed with two build jobs.
A later workspace rerun encountered stale cache-migration test databases because those existing
tests derive temporary paths from reusable Windows process IDs. Final reruns use a fresh temporary
directory for the test process. No cache migration implementation was changed.

A crawl regression test caught a refused visit being marked done when the new gate returned a
descriptive reason instead of the existing machine-readable cooldown shape. The final response
keeps the canonical reason, empty attempts when nothing was requested, and the explanatory note;
the passing test verifies that the page remains pending. Native timeouts are recorded as failed
route evidence and still disclose attempted exposure while retaining the earlier page.

Early learning fixtures were labelled `thin` because injected markup dominated their small body.
Those failures are retained in `results/manual-browser*.txt`; the fixture was enlarged without
relaxing the quality rules. The first passing controlled measurement recorded attempts `2, 2, 1`.

`results/` retains verification logs and exit-code records. Browser profiles and other run state
are excluded from version control. No installed user configuration, security software setting,
remote service or public benchmark state was changed by these checks.

Before publication, the machine's absolute home path was replaced by `<repo>` and `<home>` in the
committed logs. Only those display strings changed; the five source SHA-256 values recorded in
`verification.json` still verify against the files they name.
