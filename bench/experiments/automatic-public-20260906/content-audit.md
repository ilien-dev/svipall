# Content audit

All nine batches are complete: 459 calls on the frozen `dd8a304` executable.

Of 348 primary passes, **156 are explicitly paginated/truncated responses** (147 in `public31`,
nine in `hard12`). The supplementary flags identify 108 passing HTML fragments without a body
tag and 141 passes with fewer than 500 collapsed static-text characters. Flags overlap and are
review aids, not a replacement success rate. Short reference pages and JSON diagnostics can be
valid; head fragments, login pages and title-only responses do not prove receipt of the requested
main content. The raw records support a delivery-check rate, not a complete-extraction rate.

This supplementary audit does not change the frozen delivery check. It inspects saved responses
without issuing more requests. The offline reporter selects the shortest and longest passing
static-text samples per target slot (or the first failure when none passed). This is representative
inspection, not independent ground truth for every returned fact. HTML previews remove the head,
scripts, styles and templates; they do not execute JavaScript or reconstruct a rendered page.

## What the first response can establish

The harness requests HTML for `public31` and Markdown for the other sets. It retains the default
25,000 estimated-token response budget and does not follow continuation cursors. A returned HTML
fragment can therefore end inside the head. The underlying fetched document may have contained
more; the saved first response cannot establish that the caller received that content. This is
response pagination, not evidence that the remote server sent only a head. Likewise, a `thin`
label does not by itself prove a failed request: a valid JSON diagnostic can be short.

In the first `public31` batch, 49 of 79 passing responses were explicitly marked `truncated`.
These remain passes under the prespecified rule, but must not be described as complete documents.

| Target slots inspected in the first batch | Evidence in the saved first response | Limit on interpretation |
|---|---|---|
| `ceo-ca`, `canadianinsider`, `glassdoor`, `google-search`, `indeed-jobs`, `instagram-post`, `medium`, `nowsecure-cf`, `pixelscan-bot`, `pixelscan-fp`, `reddit`, `stackoverflow` | Passing samples have no static body text; metadata/head fragments and continuation cursors require further retrieval | Does not establish delivery of discussions, results, posts or rendered diagnostics |
| `github-explore` | Passing sample contains only the skip-navigation text outside the head | Does not establish delivery of the repository listing |
| `devto`, `linkedin-jobs`, `newsfilecorp` | Samples contain identifiable post, job or news entries; output is paginated | Supports delivery of some requested content, not the complete listing |
| `stockwatch` | Search interface explicitly reports zero news items for the requested last 24 hours | Valid empty-result UI; no independent check of whether the source should have had items |
| `crunchbase-cf`, `sedarplus` | Substantial public landing-page content and links | Does not establish access to downstream databases, searches or documents |
| `browserleaks-tls`, `tls-peet` | Structured TLS/HTTP diagnostic output | Transport observations only; not browser JavaScript or bot-acceptance results |
| `bot-incolumitas`, `browserleaks`, `browserscan-bot`, `creepjs`, `rebrowser-detector`, `sannysoft` | Detector instructions, labels and uncomputed/default fields | Fetching a detector page is not passing its active tests; no detector-acceptance rate is inferred |
| `amazon-product`, `booking-search`, `tiktok-user`, `x-explore` | No passing call in this batch | Failures remain in the denominator; no claim of content delivery |

The native success in this batch is a paginated `canadianinsider` response. It establishes that
the fallback ran and passed the frozen delivery check; it does not establish receipt of the
site's financial content in the first response.

## Markdown observations and attempt accounting

The first `hard12` batch uses Markdown and illustrates why format matters. Its passing samples
include product names/prices (`newegg`), question titles/excerpts (`stackoverflow`), property
listings (`idealista`), a company profile with some placeholder fields (`crunchbase`), review
content (`g2`), job-related snippets (`indeed`), news links (`hackernews`), the reference article
(`wikipedia`, paginated), and the complete short reference page (`example`). These observations
support partial task-relevant content, not independent verification of every item or field.
The `indeed` sample contains many empty list entries and omitted job labels, so its keyword pass
must not be treated as a complete structured job dataset. `nowsecure` returns a short branded
landing page; `amazon` and `zillow` have no passing call in this batch.

In the first `vendors8` batch, `akamai-newegg` contains SSD names/prices, `kasada-twitch` contains
live channels/categories, `cloudflare-indeed` contains job snippets with missing fields, and
`cloudflare-crunchbase` contains company information mixed with placeholder fields. By contrast,
all three passing `akamai-homedepot` responses contain only 236 collapsed characters: prefetch
configuration, a page title and an assistant label. They pass the keyword rule but do not contain
the requested appliance catalogue. `datadome-g2` and `datadome-idealista` are refused locally under
cooldowns inherited from the earlier set; `kasada-hyatt` returns HTTP 429 and then local cooldown
refusals. The set names are historical target labels, not a fresh identification of every site's
current protection stack.

The second `hard12` batch adds a passing `zillow` response, but that sample explicitly reports no
matching homes for the location selected by the site. It contains search advice rather than
property listings. That is a keyword-rule pass and an empty-result UI, not proof of a recovered
real-estate catalogue. The other newly selected second-round samples retain the same broad
content interpretation: job snippets with missing labels, identifiable property entries, and news
links whose scores/timestamps change.

In the second `vendors8` batch, the newly passing `datadome-g2` and `datadome-idealista` samples
contain review and property content consistent with the corresponding `hard12` samples. Updated
product, job and channel samples do not change the content limitations described above.

In `auto-public31-2`, the newly passing `booking-search` samples are paginated head fragments,
with no static body text in the saved first response. The newly passing `tiktok-user` sample has
the title `Log in | TikTok`, also with no body text in its paginated fragment. It does not establish
delivery of the requested public profile or posts. The equal 79/93 primary count in the first two
`public31` batches hides changes in which targets pass; it is not evidence of identical per-site
reliability. No other newly selected second-round sample adds more than 500 static-text characters
over its first-round maximum or changes the broad interpretations above.

The final `vendors8` batch retains the same content interpretation in its updated product and
channel samples. It returns fewer primary passes (12/24, after 13/24 and 14/24), while 11 calls
have empty attempt logs. Its all-call median is 0.17 seconds, but its successful-call median is
2.79 seconds. The low all-call median must not be presented as typical successful extraction
latency or as a delivery improvement.

The final `public31` batch again returns 79/93 primary passes. Its expanded LinkedIn sample still
contains identifiable jobs and still has a continuation cursor; it does not establish the full
listing. No newly passing target appears in that round, and no other selected sample changes
static-text size by more than 500 characters from the first two rounds' range. Native fallback
is not recorded in that batch. These observations retain the earlier content caveats.

The final `hard12` batch has 19/36 primary passes, after 25/36 and 28/36, with 15 confirmed local
deferrals. Updated selected news and job samples retain the same interpretation. Across the
complete dataset, the 66 empty attempt logs are all confirmed local deferrals: 32 cooldowns and
34 address-budget refusals. No empty-log timeout ambiguity occurs in this particular dataset.

An empty attempt log alone is not proof that nothing was requested: an outer timeout also returns
an empty log. The frozen controller's `refused_without_attempt` counter is therefore presented as
an **empty-attempt-log count**, alongside a stricter supplementary count of confirmed local
deferrals requiring an explicit policy reason. Native counts reflect recorded flags; an outer
timeout can lose internal attempt details. Primary delivery outcomes and elapsed times are not
changed by these audit distinctions.

## Host and execution limits

The existing harness initializes its shuffle with `seed | 1`. Recorded seeds 20260908 and
20260909 therefore produce the same effective shuffle state for rounds 2 and 3. Set order still
rotates, but within-set target order is not independently randomized three times. The protocol
and seeds remain unchanged; these are temporal repetitions, not independent random samples.

Route-learning examples in `auto-public31-1` are visible in the saved attempt arrays. For `ceo-ca`,
`stackoverflow` and `reddit`, visits 1 and 2 start at HTTP and end at `real`; visit 3 contains only
the `real` attempt. Their total call times are respectively 13.00/15.26/2.01,
16.34/27.34/3.64 and 6.78/8.45/2.38 seconds. This is direct evidence of a changed starting route
after two observations. It is not a controlled causal speed comparison: browser/profile state,
cooldowns and remote responses also change. All these first-response HTML examples are paginated,
so the route change does not establish complete content delivery.

Recorded attempts include **106 browser-launch errors across 53 calls**. They remain part of measured behavior and elapsed
time; this run does not estimate performance with those errors repaired. Some saved browser HTML
contains markup inserted by installed security software. The executable and managed browser are
frozen, but the host is not a clean-room browser environment. No anonymity claim follows from
these observations, and native mode deliberately exposes the browser's actual characteristics.

All 28 recorded native fallbacks include a privacy notice. Fifteen calls deliver with native
identity: one in `public31`, eleven in `hard12`, and three in `vendors8`. These include the
paginated Canadian Insider head and the Zillow empty-result UI described above. They are not
fifteen independently proven complete extractions, nor an estimate of a causal gain over disabling
native mode. Native success is conditional on reaching that fallback, not a random control arm.

While this measurement was running, a separate change advanced the workspace from `dd8a304` to
`e60e10b`, fixing browser directory ownership, shutdown timing and error diagnostics, and providing
a virtual display in Linux CI. The benchmark continues to use its original frozen executable;
these public measurements do not include that fix. Source activity and host workload are not
controlled experimental factors. Local verification of the newer revision is reported separately.
