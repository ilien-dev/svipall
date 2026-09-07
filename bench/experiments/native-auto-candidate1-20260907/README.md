# Classification-aware automatic routing versus native

Status: all 918 calls are complete and all 376 distinct content fingerprints are audited.
The 306 published records were independently checked for hashes, call counts and recorded exit-IP
redactions. See [recorded results](results.md), [full summary](summary.json) and
[verification](verification.json). No causal public improvement, winner or technical plateau is
established by this interrupted run.

| Endpoint | Automatic | Native warm |
|---|---:|---:|
| Mechanical deliveries / 459 calls | 327 | 331 |
| Useful production deliveries / 351 production calls | 164 | 161 |
| Available useful content, including blocked responses | 165 | 171 |
| Total fetch seconds, including failures and diagnostics | 2,891 | 3,545 |
| Useful deliveries by round | 57 / 48 / 59 | 59 / 44 / 58 |

Automatic returned three more primary useful deliveries; native returned six more secondary
available-content responses. Their round ranges overlap. Both arms returned more useful content
than in the preceding baseline, while both spent more total fetch time. The long pause before
round 3, changed page contents and retained shared admission history prevent attributing the
difference to routing. A faster local refusal is not faster extraction.

A computer shutdown extended the pause after the first 612 saved calls to 13.09 hours,
instead of the planned 120 seconds. No block was partial. Round 3 resumes with the same
verified executable, browser, profiles and traffic accounting; normal time decay remains
active. [Recovery evidence](interruption-between-rounds.json) records the deviation.
The generated summary separates rounds 1–2 from round 3. Completing this workload does
not substitute for an uninterrupted final confirmation of retained changes.

The candidate reduces fingerprint-wall routing from HTTP/browser/stealth/real to HTTP/real
on the local regression, preserving its content and emulated identity. Native remains last.
Nine core planner tests and all seven MCP automatic tests, including both manual browser
tests, pass. Full QC passed: 1,179 workspace tests, no failures, 20 ignored, and 160/160 browser
probes clean. See [QC summary](qc-summary.json) and [captured output](qc.txt). No extraction
corpus was configured for that QC, so those floors were explicitly skipped. A later local
inspection found the SIGIR-23 corpus; it will be used in the separate extraction investigation.

The workload, call deadlines, policy limits and arm order match the preceding 918-call
[baseline](../native-auto-20260906/README.md). The same candidate executable runs both arms;
its route-selection change applies only to automatic identity in automatic mode. Native
continues to request the page directly through its warm browser.

Shared traffic/reputation/cooldown accounting continues from the completed baseline, with
normal time decay. Separate arm profiles and route learning start fresh as they did at the
start of the baseline experiment, then persist throughout this comparison. No accounting
is reset. Because elapsed time and exit history differ, a before/after success difference is
descriptive and cannot by itself establish that the patch caused an improvement. Within-run
native/auto comparisons also retain the shared-ledger order and local-deferral qualification.

The complete workload includes the affected review and property targets, and retains negative
and null outcomes. The next candidate additionally investigates remembered fingerprint walls,
managed-challenge transitions and listing-heading preservation. Those changes are absent from
this executable. Its retained narrow version has since passed corpus/QC review; the separate
candidate-2 comparison is complete with its own documented controller interruption. See its
[final findings](../native-auto-candidate2-20260907/final-findings.md) for all measured versions,
remaining deficits and the qualified practical stopping decision.

After the executable/source snapshot was frozen, concurrent workspace changes appeared in
the MCP tool descriptions and schemas (`server.rs`, `tools.rs`, and a new `tool_surface.rs`
test). They are outside this candidate. The frozen source archive does not contain the schema
slimming helper or new descriptions, and the frozen executable does not contain the new
description text. This comparison and its QC claim do not validate those later changes.
