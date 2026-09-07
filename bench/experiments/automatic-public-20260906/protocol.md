# Automatic policy on public targets — protocol frozen 2026-09-06

Status: preparation. Results will be added after all scheduled calls have completed.

## Prespecified measurement

Measure the current default `browser_identity=auto`, with native fallback enabled, against the
unchanged `public31`, `hard12` and `vendors8` targets. Three consecutive calls per target, three
rounds per set: **459 scheduled calls across 51 target slots**. Slots overlap across sets and are
not 51 independent sites. The set order rotates each round; the existing Rust harness shuffles
targets with a recorded seed. No target will be removed for failing.

One isolated experiment home persists across every batch. Learned routes, browser profiles,
cooldowns, traffic windows and reputation remain in place. The initial reputation ledger and
transactional traffic ledger are snapshotted from the installed home if present; user cookies,
credentials and settings are not imported. No cooldown or budget will be cleared to obtain a
better score. Visit positions 1/2/3 mean consecutive calls within a batch; later batches are not
cold starts. Requests refused locally stay in the denominator and are reported separately.

Use the existing optimized benchmark executable built from `dd8a304`, frozen by SHA-256, and the
already installed managed browser, also hashed. No Rust product behavior is changed for the run.
The caller timeout is **60 seconds**, matching the product default, instead of the old comparison
harness's 90-second default. All policy limits retain their defaults. Cache is bypassed, as in
the earlier comparison. Human assistance is disabled so that the result measures unattended use;
no external solver, proxy service or geolocation lookup is used. The machine's actual network
exit and server-side history are uncontrolled; no IP address will be published.

The controller waits two minutes between completed batches. These pauses do not reset remote
reputation. It does not automatically rerun failures or discard locally refused calls. A partial
batch is preserved and stops the controller for investigation rather than being silently replayed.

## Outcomes to publish

- For each set and visit position: delivery-check count per round, median and complete range.
- Total fetch time including failures, median/p95 call latency, successful-call latency, local
  deferrals, and total fetch seconds per delivered result. Fast refusal is not fast delivery.
- Native attempted, native delivered, emulated delivered, timeout and quality-label counts.
- Per-target results and a content audit that distinguishes passing the frozen check from
  actually obtaining the requested information. Generic brand text alone does not prove completeness.
- Wall-clock controller duration and pauses, machine/build/browser/configuration metadata,
  exact commands, hashes, raw result records and verifier output.

The frozen delivery check is the existing harness: HTTP status in 200..399, no `blocked_reason`,
nonempty content, and at least one expected string where the target supplies expectations.
`public31` has no expected-content strings. Its historical verdict is retained separately.
Neither rule proves that every requested fact or catalogue arrived. A manual audit may flag weak
passes; it will not retroactively change the primary scoring rule.

These are repeated observations on one host and exit, not independent random samples of the web.
There is no contemporaneous competitor or explicit-native control arm. Historical figures may
provide context but do not establish a causal speed improvement for this policy; their timeouts
and state protocols differ. Improvements will not be claimed from overlapping ranges or from a
larger count of cheap local refusals. Null results and regressions will be published.
