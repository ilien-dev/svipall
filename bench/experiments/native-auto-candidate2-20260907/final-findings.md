# Native versus automatic: completed comparison and stopping decision

Automatic is the better efficiency choice in this measured workload. Useful delivery is nearly
tied: 130/351 for auto and 129/351 for native. This one-result difference does not establish a
quality winner or statistical equivalence. Native retains a small advantage in the secondary
measure that includes useful excerpts marked blocked: 136 versus 131. Neither mode dominates
every target or round, and neither provides complete extraction of the difficult workload.

## All measured versions

Each version contains 918 saved calls, 459 per arm, including 351 production calls per arm.
Diagnostics are excluded from the useful counts and production costs below. Every version has
an explicit content audit; the current version has 287 distinct reviewed fingerprints.

| Version | Useful auto / native | Useful content available auto / native | Production seconds per useful auto / native | Useful counts by round auto / native |
|---|---:|---:|---:|---|
| Baseline | 142 / 140 | 144 / 149 | 11.43 / 18.41 | 54,49,39 / 55,46,39 |
| Candidate 1: within-call classified-wall routing | 164 / 161 | 165 / 171 | 16.96 / 18.70 | 57,48,59 / 59,44,58 |
| Candidate 2: retained current implementation | 130 / 129 | 131 / 136 | 8.40 / 16.58 | 58,45,27 / 54,44,31 |

These are descriptive comparisons across different histories and times. Candidate 1 has the
highest observed useful count in both arms. Its round medians (57 auto, 58 native) exceed the
baseline ranges, but its 13.09-hour interruption, changing remote responses and simultaneous
native improvement prevent attributing that increase to the routing patch. Candidate 2's
medians (45,44) do not exceed either previous range. No repeatable causal public gain from the
final patch is established. The lower final counts are retained, not replaced with candidate 1.

The current run accumulated 1,174.42/2,473.16 fetch seconds including diagnostics and failures.
Production-only totals are 1,091.98/2,138.53 seconds. Calendar duration was 72.51 minutes;
recorded block intervals sum to 68.13 minutes, including the recovered controller interval.
Full QC and the corpus investigations are additional work, not part of this run duration.

Among the 109 production pairs where both arms returned useful content, auto accumulated
368.41 seconds and native 735.33 seconds: about 50% less time for auto. Medians on those same
calls were 1.02 and 3.02 seconds. This comparison contains no fast empty refusals. It is still
conditional on both succeeding and shares profile/history effects. Native was slightly faster
on the common successful subset in round 3 (54.81 versus 59.85 seconds); auto's aggregate
advantage does not mean it wins every round. See `final-sensitivity-details.json`.

## Admission, privacy and interruption sensitivity

Production calls include 171 auto and 155 native local deferrals. Across all endpoints, these
rose by round from 36/32 to 64/60 to 88/83. Both arms share accumulated accounting on one exit;
repeated requests are dependent and the leading arm can consume the next arm's opportunity.
Fresh profiles do not reset this accounting. Fast deferrals are not fast extraction, and a new
immediate workload on the exhausted ledger would mostly add refusals rather than isolate code.

Removing both sides of any production pair with a confirmed local deferral leaves 159 pairs:
126 useful auto versus 113 native, or 126 versus 119 for available useful content. This is a
conditional sensitivity analysis, not a replacement success rate or an unbiased treatment effect.

The round-1 controller interruption lasted 152.42 seconds after the existing child finished.
All three responses were recovered without replay, missing outcomes or unknown admissions.
Removing the affected target in both arms and all rounds removes 18 calls but changes neither
useful total nor the 21 auto-only / 20 native-only useful pairs. It also leaves auto's aggregate
time advantage. This cannot rule out elapsed-time effects on subsequent shared state.

The planned uninterrupted confirmation was not achieved. Another full run is not needed to
support the limited descriptive conclusion here: efficiency favored auto, useful delivery was
close, secondary availability favored native, and a causal patch gain remains unproven. An
uninterrupted run with a separately designed admission/history comparison would be needed for
stronger claims. We do not label this run uninterrupted or claim that the sensitivity restores
experimental independence.

Automatic attempted native fallback five times, delivered mechanically four times, and obtained
audited useful content once. Notices were retained. Native remains last and subject to eligibility,
attempt limits and remaining time; it is not guaranteed to run on every failed automatic visit.
Both arms had nine incomplete-content results. Completed pagination means the available cached
document was read, not that every field or record on a site was recovered.

## Remaining deficits and practical stopping point

All 20 primary pairs lost by auto have an observed disposition:

| Cause | Calls | Evidence and disposition |
|---|---:|---|
| Local cooldown or address budget before transport | 16 | Empty attempt lists and explicit local reasons. Retain these refusals; no budget enlargement, refund, alternate exit or cooldown reset |
| Deadline after emulated real/warm attempts | 1 | Saved sequence on the review-listing endpoint never reaches native. The fixed 60-second deadline and native-last policy can leave no fallback time; blanket early readiness acceptance failed its delayed-content control |
| Remote query quota returned as HTTP 200 | 3 | Saved text explicitly says a query can only run five times per minute. It has no requested records. Further identity escalation must not be used to bypass this restriction; mechanical delivery is not useful delivery |

Auto's 21 primary wins also have limits: native had nine mechanically delivered refusal pages
(six filing-page forbiddens and three query-quota responses), six blocked but useful job excerpts,
one actual challenge wall, one fetch failure and four local deferrals. The secondary endpoint
retains those useful job excerpts. Changing the score or silently removing blocked labels would
manufacture a win without recovering additional content. Full per-pair evidence is in
`remaining-deficits.json`.

Shared extraction still loses job titles, repository names and search-result destinations in
some layouts. Two broader heading-preservation candidates improved three saved pages but
regressed on the same 3,975-page corpus: 65 improvements/458 regressions for the unrestricted
version and 50/123 for its refinement. Both were rejected. The narrow retained correction fixes
its reproduced wrapper defect but does not solve these broader listings or demonstrate a
material corpus-wide gain. All nulls and regressions remain published.

The tested routing and extraction changes have reached a practical stopping point under this
workload, existing architecture and fixed privacy, admission and deadline constraints. The
remaining observed losses are accounted for; the tested additional extraction and readiness
shortcuts do not provide an acceptable repeatable gain. This is a boundary of this improvement
loop, not proof of an absolute technical ceiling. New readiness evidence, different extraction
algorithms or a differently designed experiment could improve future results. Similar totals
do not prove equivalence, and the final patch's public improvement remains unproven.

## Retained changes and validation

Retained: classification-aware within-call routing, expiring cross-call wall evidence,
managed-challenge transitions, narrow wrapper-heading preservation, and bounded controller
progress-write retries. Native order, opt-out, access controls, traffic budgets and caller
deadlines remain covered by the existing constraints. The independently developed tool-surface
and browser changes in the frozen tree were included in QC; see `loop-decisions.md`.

Full frozen-product validation passed 1,216 workspace tests (22 ignored by default), ten manual
automatic/learning/timeout tests, four additional local browser tests, feature-matrix Clippy,
ONNX and local HTTP/3 checks, browser probes, coherence, CPU budgets and corpus floors.
Six alternating CPU-gate invocations passed. The later controller-only fix passed all 18 Python
controller/report checks, separately from the earlier product QC. No claim is made that every
ignored public-network test ran. Evidence is preserved under `narrow-validation/`.

`independent-verification.json` confirms all 918 calls and 306 blocks, exact schedule order,
raw and compressed hashes, frozen source/browser/binary/runtime identity, and equality of
published responses to redacted originals. No known exit address remains in the published
response/stderr files. The final README update is documentation after the frozen run, not part
of the measured executable. Reproduce tables with `compare-variants.py` and `compare-deficits.py`;
recheck the published files with `verify-publication.py`.
