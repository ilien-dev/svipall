# Native versus automatic: baseline protocol

Frozen before the first public request of this comparison. This protocol does not change the
earlier automatic-only experiment or its results.

## Question and arms

Compare the product's default automatic policy with a direct native browser visit on the same
revision and managed browser. `auto` uses `browser_identity=auto`, `mode=auto`, the default
six-attempt ceiling and native last-resort fallback. `native` uses `browser_identity=native`,
`mode=warm`, the same browser tier selected by the automatic policy's default native fallback.
Merely changing browser identity while retaining `mode=auto` would still permit an HTTP-first
ladder; that is not the standalone native control requested here.

Both arms retain default policy limits, a 60,000 ms fetch deadline, human assistance disabled,
no operator-supplied proxy, no credentials and no external solver. Neither arm follows login,
payment or other access controls. Native exposure notices and all returned pages are retained.

## Workload and state

Use all unchanged public31, hard12 and vendors8 target slots: 51 slots, 48 unique URLs. Three
consecutive visits per target and arm, in three rounds: 459 calls per arm, 918 total. Each target
block runs in its own process, with the same executable for both arms. The order of target
blocks is shuffled with distinct Python PRNG seeds 2026090601, 2026090602 and 2026090603.
The leading arm is alternated by target slot and reversed in the next round. Two 120-second
pauses separate rounds. The exact schedule is part of the manifest.

Each arm has a separate, persistent home for profiles, cookies and route evidence. Both start
without imported cookies or route learning. Reputation, cooldowns and the SQLite traffic ledger
are shared serially: copy the latest canonical accounting into the next arm before its process
starts, then copy its final accounting back after shutdown. Never restore an older ledger, clear
budgets, remove holds or reset cooldowns. Initial accounting comes from the final home of the
preceding automatic public experiment. This is one physical exit with an existing history.

Native fallback within auto does not share its profile with the standalone native control.
Persistent route evidence and browser profiles survive all rounds; live processes and warm
pages survive only the three consecutive calls within a target block. Overlapping domains and
URLs across lists remain in place and are not independent samples. Remote reputation is shared
and cannot be isolated. Alternating order reduces ordering bias but cannot remove carryover.
Local refusals are counted and reported separately; results conditional on admission cannot be
presented as the unconditional success rate. Background host workload is not controlled.

## Content and timing

Request markdown and detailed quality from both arms. Each initial call uses `cache=write`:
always make a fresh visit, store the returned document, never serve an earlier visit's cache.
Default output token limits remain in force. Follow a continuation only while the private
in-memory store has a fresh copy. Every continuation must report a cache hit and must not
restart a stale cursor. Retain the original chunks and a reconstructed content field. Stop
after 64 chunks, a repeated cursor, no fresh cache or 15 seconds of continuation work, with an
explicit incomplete flag. A site that forbids caching can therefore remain incomplete.

Record first-response time and total content-retrieval time. Include failures, timeouts and
local refusals in total cost per delivery. Also report process wall time, successful-call
medians, per-round median and range, native attempts, content completeness, and stop reasons.
Do not interpret fast refusals as fast extraction. Three sequential calls measure within-block
reuse; the first call in later rounds may already have persisted route/profile evidence.

## Frozen scoring and audit

The mechanical delivery check is status 200..399, no `blocked_reason`, nonempty reconstructed
content and at least one original expected string where the target defines expectations.
This is a secondary endpoint, not proof of successful extraction.

The primary content assessment is an explicit audit of the requested page's function, using
saved responses only. Hide the arm label during content review. Record a reason and classify
each reviewed content fingerprint as useful, partial, shell, wall, unavailable, diagnostic-only
or uncertain. Unreviewed or ambiguous content never counts as proven useful. Review every
distinct outcome used to support a winner; publish the mapping back to all calls afterward.

For articles, useful means actual topical article paragraphs; for listings, identifiable
requested records with substantive fields, such as job title/employer, product/name/price,
property description/location, question title/excerpt, story title/link or hotel/name/location.
For organization or product pages, require subject-specific factual content beyond branding.
For landing pages, require the actual public landing content; this does not prove access to
downstream data. For a named social profile or post, a login screen is not the requested item.
An empty results interface is partial/uncertain unless independent evidence verifies emptiness.
The short example-domain control is useful when its intended explanatory text is present.
The short challenge-demo landing is only a delivery/control result. Diagnostic panels and TLS
endpoints are reported separately and do not establish useful production extraction or passing
active browser detectors. Challenge text, navigation-only content and head fragments are not
useful. A truncated document can provide useful records but is separately marked incomplete.

Compare useful content first, then total time per useful result and completeness. Report
tradeoffs when an arm wins one metric and loses another. Do not force a universal winner from
inconclusive variation. Changes count as measured improvements only if the median leaves the
previous measured range, as required by AGENTS.md. Retain null and regressive experiments.

## Improvement loop and stopping evidence

Freeze and finish the baseline before modifying product behavior. Inspect matched failures,
then write a failing local regression for each actionable defect before implementation. Keep
native last, identity coherent and every page returned with its quality label. No target-name
special cases or larger policy budgets may manufacture a win. Validate candidates locally and
on the affected public targets, retaining all attempts and the shared accounting. Confirm any
retained candidate against native again over the complete workload before final claims.

Maintain an experiment journal with the hypothesis, patch, tests, measured effect and decision
for every candidate. Stop only after observed deficits have an evidence-backed disposition and
remaining candidate changes cannot produce a repeatable gain within the tested architecture,
constraints and workload. A practical plateau in these tests is not proof that no future
algorithm, browser or network condition could improve the tool. If the evidence does not
establish a winner or a plateau, report that limitation and continue investigating.

## Reproduction and preservation

The manifest records revision, source hashes, executable/browser/runtime hashes and schedule.
The current baseline includes the previously validated browser scratch-profile fix from e60e10b
and the local test-isolation fix, plus a measurement-only Rust entry point. Product algorithms
are unchanged when the baseline is frozen. The optimized executable uses target-cpu=native and
is for local measurement, not distribution. Private profiles, raw responses and source archive
remain ignored. Publish sanitized response records, checksums, per-call tables and audit reasons.
No partial batch is silently rerun. Inspect its process and preserve partial evidence first.
