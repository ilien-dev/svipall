# Candidate 1 observations

The full comparison is complete and audited; see [results](results.md). The observations below
were recorded during execution. A 13.09-hour interruption between rounds two and three limits
attribution; these observations do not establish retention or a winner.

## Complete-workload discordances

The offline analysis in `compare-deficits.py` verifies the saved responses and applies the
completed audit before grouping matching target/round/position pairs. It changes neither the
endpoint nor any response. See `remaining-deficits.json`.

Of 41 primary discordant pairs, 22 favor automatic and 19 favor native. The automatic losses
are ten confirmed local deferrals, seven timeouts and two HTTP 503 responses that stop on
server-directed backoff. Six of the seven timeouts did not reach native; one did. The secondary
content-available endpoint has 13 automatic-only and 19 native-only pairs, reflecting useful
records retained in responses that the product labels blocked.

The primary native losses include nine mechanically delivered short forbidden pages from the
filing portal; the content audit correctly excludes them. Nine job-list responses contain useful
records but carry a blocked verdict, and are therefore included only in the secondary endpoint.
These classification limitations must remain visible rather than being converted into successful
content retrieval by changing the score. They also show why native is not a universal upper bound:
the automatic arm can receive useful content through a different route on the same target.

The routing prototypes address unnecessary pre-native exploration. They do not waive the ten
local refusals or two server-backoff responses. The paired native result is not proof that native
would have succeeded at the automatic call's earlier/later admission time. Full uninterrupted
confirmation is still required after the local and corpus checks.

The first review-page block shows the intended route change: HTTP, real, warm, then native
warm. The prior baseline also attempted browser and stealth. Both automatic blocks delivered
one of three calls. Baseline fetch time was 166.22 seconds; candidate time was 103.92 seconds,
but the candidate's final call was an immediate local cooldown refusal. That reduction is
not evidence of faster extraction. Successful first-call time was 46.83 versus 44.00 seconds,
and the native stage itself was slower in the candidate observation (11.99 versus 10.38 seconds).
The two runs have different prior exit history and elapsed-time decay.

The candidate's second automatic call still exhausted its deadline before native: after HTTP
and real, warm consumed the remaining time. The existing constraints therefore still prevent
automatic from reproducing the direct native arm's three successful calls on this block. A
time-allocation change is a remaining hypothesis, not a reason to enlarge the caller's timeout
or bypass pacing. It would need to preserve the opportunity for emulated challenges to resolve.

The first property-search block returned no mechanically successful calls in either arm.
Automatic's very short refusals must remain failures in all efficiency denominators.

Some candidate content labels are transferred from the baseline only when the complete
URL/status/content fingerprint is identical. The label's method records that transfer.

## First property-listing block and remaining learning hypothesis

The candidate delivered two of three automatic calls on the Madrid listing, versus one of
three in the prior baseline block. Both candidate deliveries required native fallback. Their
fetch times were 45.08 and 50.24 seconds; the third call timed out at 59.90 seconds. The native
control then had three immediate local cooldown refusals. This block therefore cannot support
a claim that automatic outperformed an admitted native browser, and one block does not meet
the median-outside-range improvement rule.

The third automatic call exposes a further routing issue: after two HTTP/real fingerprint
failures, the planner suppresses those particular routes but starts at the previously untried
`browser` tier. It spends another 4.65 seconds on the same wall, then 22.59 seconds at `warm`
and expires during pacing before native. The first two calls had already skipped this weak
probe using the classified fingerprint wall.

Candidate hypothesis: retain expiring, context-specific fingerprint-wall evidence so subsequent
plans can avoid backtracking through weaker untried routes. Current samples retain generic
success/failure counts only, so transport failures cannot safely justify this inference. Any
implementation needs distinct classified-wall evidence, reset/expiry tests, compatibility with
existing saved samples, and the strongest allowed emulated probe before native. It must not
fabricate failure samples for routes that were never attempted, generalize a fingerprint wall
to ordinary network errors, or promote native ahead of permitted emulated exploration.

The [candidate-2 prototype](../native-auto-candidate2-20260907/README.md) now implements this
expiring evidence and passes its core regressions and all seven MCP automatic integration tests,
including the manual browser fixtures. Public confirmation remains pending; the current
comparison's frozen executable does not include it.

## Other observed costs: challenge waits and pacing

The first company-profile block delivered all three native requests but no automatic request.
Its first automatic call attempted HTTP, browser, stealth, real and warm in about 39.1 seconds
of transport time, then exhausted its 60-second deadline during pacing before native. The next
call returned an HTTP wall and then expired during pacing; the third returned no response.
The first insider-data automatic block similarly reached native once but ran out of time there,
then timed out during pacing on subsequent calls. These are recorded losses, not improvements
from the classification-specific candidate, which does not skip ordinary challenge routes.

Code inspection distinguishes the configured 1-second minimum from the adaptive gap: recent
latency, consecutive blocks and reputation pressure can require a much longer wait. Pacing also
reserves a future slot and charges the admitted attempt before its wait completes; cancellation
can therefore leave a reservation and a charge without a transport response. The frozen protocol
retains that accounting. Simply shortening waits or refunding these charges would change the
comparison's constraints and is not an accepted explanation of better extraction.

Time allocation across unresolved emulated attempts remains a hypothesis to assess after the
current route-memory candidate. Any change must keep required pacing, the total fetch deadline,
the caller's attempt ceiling, and the emulated probe before eligible native fallback. Native
and automatic both failed the first social-exploration block, which must not be described as an
automatic-only deficit.

## First round: complete content review

All 306 first-round calls are saved and every distinct first-round response is labeled. Of 153
calls per arm, automatic passed 116 mechanical checks and native passed 117. Primary useful
production deliveries were 57 for automatic and 59 for native; the secondary content-availability
counts were 58 and 62. There are 117 production-content calls per arm in this round, with the
remaining 36 being diagnostic/control calls excluded from the useful production endpoint.

Both arms returned more useful content than in the preceding baseline's first round (54 and 55).
The underlying pages changed too, including a news listing that now contains records. One round
does not establish a candidate improvement, a winner or a plateau; the remaining two rounds,
their audit and the shared-accounting sensitivity remain necessary.

## Shutdown recovery and two-round review

All 612 calls saved before the shutdown are now content-audited. Primary useful deliveries
were 105 for automatic (57/48 by round) and 103 for native (59/44). Secondary available useful
content counts were 106 and 109. Mechanical delivery counts were 205 and 210. These small
differences and overlapping round ranges do not establish a winner or an accepted improvement.

The interruption occurred between rounds, with no partial block or extra admission. The pause
before round 3 lasted 13.09 hours instead of 120 seconds. The same frozen executable, browser,
profiles and accounting are retained, with normal elapsed-time decay. The report now separates
the pre-interruption and resumed cohorts; all eleven paired-controller/report tests pass,
including paired sensitivity around this round boundary. An uninterrupted final confirmation
remains necessary for retained product changes.

The [matched loss breakdown](pre-interruption-auto-losses.json) isolates the 13 pairs in those
two rounds where native delivered useful content and automatic did not. Six automatic calls
timed out, six were locally deferred without a transport attempt, and one stopped after an
HTTP 503 with server backoff. Only one of the six timeouts reached native. Four had confirmed
pacing/route exhaustion before native and one has an empty timeout log, which is insufficient
to establish its exact cause. These observations keep deadline allocation and repeated managed
challenge probes on the investigation list; fingerprint-wall memory alone does not address
every deficit. Server backoff and admission limits must continue to be respected.
