# Native versus automatic: recorded results

Status: complete measurement; 918/918 calls.

Delivery is the mechanical check. Useful content requires a recorded audit reason.
No winner is established while the content audit or matched rounds are incomplete.

| Arm | Calls | Delivery checks passed | Audited useful | Fetch seconds / delivery | Successful median (s) |
|---|---:|---:|---:|---:|---:|
| auto | 459 | 327 | 164 | 8.84 | 1.01 |
| native | 459 | 331 | 161 | 10.71 | 3.11 |

Content audit: 376/376 exact content fingerprints labeled.

All failures remain in the denominators. See calls.csv and summary.json for matched outcomes.

The prespecified primary useful endpoint also requires mechanical delivery. A separate
secondary count retains useful records present on responses labeled blocked by the tool:
auto 165; native 171.
This does not imply those responses are complete or unblocked. The secondary endpoint was
added after review found real job records inside blocked responses; it does not replace
the prespecified primary endpoint. Counts remain provisional until the audit is complete.

Diagnostic panels, TLS endpoints and the challenge-demo landing are excluded from the
production-content cohort (702 calls). summary.json reports that cohort and
a sensitivity analysis excluding both calls of any pair with a confirmed local deferral.
The latter is conditional on admission history, not a randomized causal estimate.

A computer shutdown extended the pause before round 3 to 13.09 hours, exceeding the planned 120 seconds.
Saved calls, profiles and traffic accounting were retained. The extended pause allows
reputation and cooldown state to decay. summary.json separates results before and after
the interruption for all calls and the production cohort. Completing the workload does
not constitute an uninterrupted confirmation. See interruption-between-rounds.json.
