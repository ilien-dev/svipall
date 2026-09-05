# Local reliability improvements

Requested outcome: implement the local improvements identified in the review and publish a reproducible before/after comparison. No external solvers, proxy services, or additional automation frameworks.

## Completion requirements

- [ ] Preserve a baseline executable built from the original product with the same comparison harness as the candidate.
- [ ] Fix first-visit detection and stable-profile eligibility for kept pages.
- [ ] Preserve a live document when reusing it; distinguish reuse from navigation and cache hits.
- [ ] Add bounded, progress-aware challenge budgets and test renewal eligibility.
- [ ] Add a configurable native-hardware browser mode, including local locale/timezone defaults.
- [ ] Expose configuration within the CLI/MCP and integrate browser provisioning without manual file edits.
- [ ] Verify bundled local model availability and report capability limits accurately.
- [ ] Fix invalid-status and UTF-8 benchmark defects; retain the historical score beside delivery/error metrics.
- [ ] Record all repeat positions, configuration, seed, state, latency and failures in measurements.
- [ ] Run meaningful regression tests, browser checks and required quality gates.
- [ ] Measure original and changed product with controlled local state and explicit network-history limitations.
- [ ] Publish comparison and completion evidence, including regressions or unchanged results.

## Measurement protocol

The comparison harness is added before product changes and compiled into both executables. Original target lists remain frozen. Each sample records the public rule independently of product delivery. Repeated fetches are reported individually, never silently discarded. Both arms use the same browser, target order seed, timeout and repeat count, with separate state directories and the same initial identity seed. Live server/IP state cannot be reset locally; run order and local reputation spend are recorded, and elapsed-time effects must not be attributed to the implementation.

## Evidence

Pending implementation and measurement.
