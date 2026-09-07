# Investigation journal

All 918 baseline calls are complete. These observations support candidate hypotheses, not yet
established public-workload improvements. The baseline executable and raw records stay frozen.

## First matched review-page block

The direct native arm delivered three responses in 18.45 total fetch seconds. The automatic
arm delivered one in 166.22 seconds. Native ran first in this block; the shared reputation
ledger therefore makes these observations conditional on the order and existing exit history.

The automatic call that delivered visited HTTP, browser, stealth, real, warm and finally native
warm. Its successful native stage took 10.38 seconds, after roughly 36 seconds of other work.
Its next call ran out of time while waiting for pacing after three emulated attempts. The third
call reached an emulated real browser, then encountered the address budget after further waiting.
These are not three independent samples of native versus automatic success probability.

Candidate investigations after the baseline:

- Can wall evidence and existing route failures avoid repeatedly spending on unproductive
  intermediate tiers while retaining an emulated attempt and keeping native last?
- Can remaining-time allocation give a permitted native fallback a usable opportunity without
  extending the caller's deadline, ignoring pacing or increasing the visit budget?
- Does the policy stop on a technically delivered shell where the native arm retrieves actual
  requested records? Only matched content evidence can support this change.

The first property-listing block repeated the same six-stage pattern: about 51 seconds to
deliver through native after five emulated attempts, then a pacing timeout. In that block auto
ran first and the direct-native block was locally refused. This is direct evidence of the
shared-ledger carryover described in the protocol. A local refusal after the other arm spent
the budget is not evidence that the site's native browser failed. Report those pairs separately
and use local matched fixtures and admission-aware follow-up measurements when evaluating a
candidate; changing the network history must not be mistaken for an algorithm improvement.

The requested product detail URL in the first public block returned an explicit missing-page
template in both arms. No product records were present. That response does not support trying
additional identity routes to manufacture a successful product extraction.

The first job listings returned relevant description fragments while omitting many titles,
employers and item links in both arms. They are labeled partial in the content audit. A higher
mechanical delivery count alone would not fix this shared extraction limitation.

## Measurement qualifications

The native control is the product's explicit native warm-browser mode, with its normal shared
classification and extraction. It is not an independently implemented stock-browser scraper.
Ancillary product probes may use HTTP even when the requested page uses a browser; the harness
checks that the requested native page never uses an HTTP ladder tier. Top-level visit counts
are not counts of every browser resource request or diagnostic probe.

Continuation chunks are preserved verbatim. The combined content field concatenates those
chunks, including any continuation preambles; it is an audit convenience, not a byte-identical
copy of the source document. A missing cursor is not independent proof that a site's entire
catalogue or article arrived. Blocked responses may already be capped by the product.

Content review inputs omit arm, tier, timing and attempts. The reviewer has also seen progress
and diagnostic logs, so this is assisted review with hidden arm labels, not an independent
double-blind evaluation. The review labels state the specific content evidence inspected.

The installed home's reputation was checked against the preceding experiment's final ledger
before launch. Entries exceeding that ledger were confined to unrelated development-resource
hosts; no target-domain budget was reduced by using the preceding experiment as the seed.

## Candidate 1: classification-aware automatic routing

The automatic loop ignored the fingerprint/hold classification when choosing its next route,
although the explicit emulated ladder already used it to prefer a headful session. The candidate
applies that evidence to automatic routing too: HTTP fingerprint walls can proceed to `real`,
then `warm`, with native still last. A promoted headful route cannot backtrack through weaker
routes after the same wall. Lower tier ceilings continue to require an emulated browser probe.
Neither visit limits, pacing, deadlines nor terminal login/paywall handling changes.

TDD: two new core tests initially failed to compile because `after_wall` was absent. The local
browser regression then failed on the old server with four attempts (HTTP, browser, stealth,
real), while delivering the requested document. With the candidate it passes with two attempts
(HTTP, real), the same requested content and no native fallback. All nine core automatic tests
pass. This establishes route reduction on the fixture, not a measured public success-rate gain.

Fixture qualification: the browser added a large stylesheet, making a body-only endpoint marker
an unreliable wall fixture under the classifier's bounded HTML scan. The final fixture exposes
the existing protocol cookie signal as well; its public landing seeds a separate cookie that
allows the document to render. This is a test-design correction. No classifier change has been
made, and possible loss of body signals after large injected styles remains an investigation.

Full candidate QC passed (1,179 workspace tests, 20 ignored, 160/160 browser probes); both manual
automatic browser tests also passed within the seven-test integration binary. The complete
918-call candidate/native confirmation is running in `native-auto-candidate1-20260907`.

## Completed baseline content audit

All 334 exact fingerprints have a label and reason. One hundred labels came from prespecified
diagnostic-target exclusion or exact-empty-content checks; other labels used content review,
with complete link inventories where missing record links affected the verdict. Review excerpts
are sufficient to establish useful records, but are not a full-document completeness audit.

The primary endpoint has auto 142 versus native 140 useful mechanical deliveries. The counts
by round are auto 54/49/39 and native 55/46/39. Neither median leaves the other's range. Some
blocked responses retain useful records: an explicitly secondary content-availability count
is auto 144 versus native 149, including nine native job-listing excerpts capped at 2000 chars.
This secondary endpoint was added after inspection; the prespecified primary remains unchanged.

Other shared limitations are now visible: search snippets lose source URLs; job listings lose
titles and item links; repository exploration loses repository names/links; the gold-news URL
delivers a generic newsroom with no extracted gold mention. These must not be called successful
requested-data extraction merely because the mechanical check passed. A missing post or product
page is also not evidence that another identity would recover the requested item.
