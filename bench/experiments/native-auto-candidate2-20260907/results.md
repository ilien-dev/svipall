# Native versus automatic: recorded results

Status: complete measurement; 918/918 calls.

Delivery is the mechanical check. Useful content requires a recorded audit reason.
No winner is established while the content audit or matched rounds are incomplete.

| Arm | Calls | Delivery checks passed | Audited useful | Fetch seconds / delivery | Successful median (s) |
|---|---:|---:|---:|---:|---:|
| auto | 459 | 251 | 130 | 4.68 | 1.00 |
| native | 459 | 258 | 129 | 9.59 | 2.62 |

Content audit: 287/287 exact content fingerprints labeled.

All failures remain in the denominators. See calls.csv and summary.json for matched outcomes.

The prespecified primary useful endpoint also requires mechanical delivery. A separate
secondary count retains useful records present on responses labeled blocked by the tool:
auto 131; native 136.
This does not imply those responses are complete or unblocked. The secondary endpoint was
added after review found real job records inside blocked responses; it does not replace
the prespecified primary endpoint. Counts remain provisional until the audit is complete.

Diagnostic panels, TLS endpoints and the challenge-demo landing are excluded from the
production-content cohort (702 calls). summary.json reports that cohort and
a sensitivity analysis excluding both calls of any pair with a confirmed local deferral.
The latter is conditional on admission history, not a randomized causal estimate.

An operator-reported computer shutdown interrupted one block. Its partial evidence is archived;
the block is repeated with recovered traffic accounting and retained profiles. One additional
completed call has no saved response, and one additional admission has an unknown outcome.
These are reported separately, not scored as product failures. summary.json also compares the
arms with the affected target excluded across all rounds. See interruption-recovery.json.
