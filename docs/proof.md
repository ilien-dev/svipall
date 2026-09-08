# Proof: every number, with the command that reproduces it

Lifted out of the README so that file stays readable. Everything here is the same text, with its links repointed.

This project publishes its own benchmarks and reports the number it gets, not the number it would
like. Raw run logs and JSON are committed in [`bench/baseline/`](../bench/baseline/) — including the
rounds of work that improved nothing, and the rounds where a number went *down*.

| Gate | Result | Needs network? | Command |
|---|---|---|---|
| Test suite | **1,216 passing**, 0 failing, 22 ignored (2026-09-07) | no | `cargo test --workspace` |
| Automation tells | **160 / 160** probes clean — 32 probes × 5 browser passes | no, but needs a browser | `bench tells --assert` |
| Identity coherence | **8 / 8** — 7 identities plus a 1,500-machine sweep | no | `bench fingerprint --engine chrome` |
| Network fingerprint | **8 / 8** wire checks against `tls.peet.ws` | yes | `bench fingerprint` |
| CPU budgets | **11 timed budgets + 4 structural checks**, all inside budget | no | `bench micro --assert` |
| Extraction quality | median ROUGE-LSum F1 **0.920** over 3,975 pages | no, once the corpus is fetched | `bench extract --corpus DIR` |
| Historical evasion, independent list | **26 / 31**, range 25..26, **zero hard blocks** | yes | `bench evasion --set public31 --runs 3` |
| Historical evasion, our own hard list | **7 / 12**, range 7..8 | yes | `bench evasion --set hard12 --runs 3` |
| Historical evasion, four named vendors | **3 / 8**, range 2..3 | yes | `bench evasion --set vendors8 --runs 3` |

**The rule for those three historical evasion rows:** median of three runs with its range, targets in a
fresh random order each run, cooldowns cleared first, from **a single residential address with no
proxy**. A change counts as an improvement only when the median leaves the previous range. The
reputation spend is deliberately *not* cleared, and `bench evasion` refuses to start a list whose
address has already spent past the line.

> These are historical results: `public31` was re-taken on 2026-09-05; **`hard12` and `vendors8`
> carry their 2026-09-04 and 2026-09-05 figures. All three predate the current automatic policy.**
> The latter two were not re-taken in that round
> because running `public31` spends the same addresses they score, and taking all three back to back
> is the exact thing that produced a round this project already published as a warning.

The `browser_identity=auto` policy has [controlled local verification](../bench/experiments/auto-20260905/README.md)
and a [2026-09-06 revalidation](../bench/experiments/revalidation-20260906/README.md),
including native-last order, opt-out, learning and timeout handling. Running the commands above now measures the current code and effective configuration;
it does not recreate the historical policy. Use the recorded revisions and configurations for those comparisons.

### Automatic-policy snapshot — first-response delivery, 2026-09-06

The [complete public measurement](../bench/experiments/automatic-public-20260906/README.md) recorded
**459 calls across 48 URLs: three rounds, three consecutive calls per target slot**, with persistent
learning, profiles, cooldowns and reputation, a 60-second timeout, cache bypass and unattended
operation. **348/459 (75.82%) passed the existing delivery check.** The executable was frozen at
`dd8a304`; these figures **predate the browser directory/shutdown fix in `e60e10b`** and are not
measurements of that newer build.

| Set | Total passes / calls | Median passes per round (range) | Successful-call median | Fetch seconds per delivery |
|---|---:|---|---:|---:|
| public31 | 237/279 (84.95%) | 79/93 (79..79) | 1.01 s | 5.94 s |
| hard12 | 72/108 (66.67%) | 25/36 (19..28) | 1.31 s | 13.61 s |
| vendors8 | 39/72 (54.17%) | 13/24 (12..14) | 3.50 s | 11.98 s |

Each round above includes all three visits; [tables by visit position](../bench/experiments/automatic-public-20260906/results.md)
separate their medians and ranges. The last column includes time spent on failures, divided by
delivery-check passes. The run took **63.95 minutes including pauses** and retained **66 local
deferrals and nine timeouts**. Native fallback was recorded on **28 calls**, all with a privacy
notice; **15 delivered with native identity**. These are conditional fallback outcomes, not a
controlled estimate of its gain over disabling native.

**A passing delivery check is not proof of complete extraction.** It requires status 200..399,
no reported block, nonempty content and at least one expected string where supplied; `public31`
has no expected strings. **156 of the 348 passes were explicitly paginated**, and the harness did
not follow their cursors. The [content audit](../bench/experiments/automatic-public-20260906/content-audit.md)
also identifies title-only catalogue responses, empty-result and login pages. Fetching a detector
page does not prove passing its active tests. The lists contain mixed page types and basic controls;
their overlapping targets and persistent state are not independent samples of the web.

This is an observed snapshot on one host and exit. The report retains browser-launch errors,
documents the repeated shuffle order in rounds 2 and 3, and separates background/source changes
from the frozen executable. It establishes neither a causal speedup nor superiority to another
tool or future reliability. [Raw records, hashes and offline verification](../bench/experiments/automatic-public-20260906/README.md#files-and-offline-verification)
allow the reported calculations to be checked without contacting the sites again.

### Native versus automatic — paired baseline, 2026-09-06

The [paired baseline](../bench/experiments/native-auto-20260906/README.md) saved **918 calls** and
reviewed all **334 distinct content fingerprints**. The control requests pages directly through
Svipall's native `warm` mode; it is not a separate stock-browser implementation. Both arms use
the same frozen executable, deadlines and shared traffic/reputation accounting.

| Baseline endpoint | Auto | Native warm |
|---|---:|---:|
| Mechanical deliveries / 459 calls | 293 | 306 |
| Useful production deliveries / 351 production calls | 142 | 140 |
| Useful content available, including responses marked blocked | 144 | 149 |
| Total fetch time, including failures and diagnostics | 1,717 s | 2,986 s |

The useful-delivery counts by round are **54/49/39 for auto** and **55/46/39 for native**.
These overlapping ranges do not establish a content winner. The content-availability row is a
secondary analysis added after review found useful job records inside blocked responses; those
responses can still be incomplete. Shared budgets also mean one arm can leave the next locally
deferred. The report retains those calls and publishes a sensitivity analysis; fast refusals
must not be mistaken for faster extraction. Review used arm-hidden content excerpts and recorded
reasons, not independent double-blind review or exhaustive completeness checks.

A [classification-aware routing candidate](../bench/experiments/native-auto-candidate1-20260907/README.md)
passed its local regressions and full QC, then completed 918 calls with all 376 content fingerprints
audited. It returned **164/161 useful deliveries** (auto/native), or **165/171** when including useful
content inside blocked responses, in **2,891/3,545 total fetch seconds**. Its pause before round 3
was extended to 13.09 hours by a computer shutdown. The report separates results before and after
that interruption; both arms improved their counts and both spent more time than in the baseline.
That interrupted run does not establish a causal routing benefit. The older automatic-only
snapshot above uses a different executable and protocol.

The [completed current comparison](../bench/experiments/native-auto-candidate2-20260907/final-findings.md)
measures remembered fingerprint walls, managed-challenge routing and narrow listing-heading
preservation: **918 calls**, all **287 content fingerprints audited**, and independently verified
published records. Results across the three measured versions are:

| Version | Useful production deliveries auto / native (351 calls each) | Useful content available auto / native | Production seconds per useful result auto / native |
|---|---:|---:|---:|
| Baseline | 142 / 140 | 144 / 149 | 11.43 / 18.41 |
| Candidate 1 | 164 / 161 | 165 / 171 | 16.96 / 18.70 |
| Current candidate 2 | 130 / 129 | 131 / 136 | 8.40 / 16.58 |

**Auto had the better aggregate efficiency; useful delivery was nearly tied.** On the same 109
pairs where both returned useful content, auto accumulated **368.41 seconds** versus native's
**735.33 seconds**, so the time difference is not only fast refusals. Native was slightly faster
on that subset in round 3 and retained more useful content under the secondary blocked-excerpt
measure. The one-result primary difference does not establish a quality winner or statistical
equivalence. Neither arm consistently dominates all sites or rounds.

Current useful counts by round were **58/45/27 auto** and **54/44/31 native**. Production local
deferrals numbered **171/155** as shared accounting accumulated; the lower final totals and
overlapping ranges do not establish a public improvement from the final patch. No limits or
cooldowns were reset. Removing both sides of locally deferred pairs leaves 126/113 useful
deliveries in 159 pairs, a conditional sensitivity result rather than a replacement success rate.
The run took **72.51 minutes**, including planned pauses and a recorded **152.42-second controller
recovery**. No calls were repeated or lost in that recovery; excluding the affected target leaves
the useful totals unchanged. It was not uninterrupted, and elapsed-time/history effects remain.

The improvement loop stopped at the documented practical limit of its tested hypotheses and fixed
constraints. Remaining auto losses were 16 local deferrals, one deadline before native and three
remote query-quota responses. This does not prove an absolute technical ceiling. Two isolated
extractor expansions recovered more records in three saved documents but reduced corpus precision;
both were rejected, and broader listing omissions remain. Full retained-product QC passed 1,216
workspace tests, ten automatic/learning/timeout checks, four additional local browser tests and
the SIGIR-23 corpus floors. Six alternating CPU gates passed; the later controller-write fix
separately passed 18 Python checks. See the linked report for changes, failed hypotheses, costs,
audit criteria, interruption sensitivity and validation limits.

### Extraction quality — measured against public corpora, including where it loses

ROUGE-LSum F1, median over the 3,975 gradable pages of the **SIGIR-23 gold standard**, scored by
`svipall-bench extract` against the study's own published extractions:

The SIGIR-23 values below were rechecked in the
[2026-09-07 validation](../bench/experiments/native-auto-candidate2-20260907/narrow-validation/qc-execution.json).
The later isolated extractor prototype is not included in these figures.

| | median | mean | IQR |
|---|---|---|---|
| readability | **0.963** | 0.861 | 0.881 – 0.987 |
| trafilatura | **0.958** | 0.877 | 0.870 – 0.986 |
| resiliparse | **0.936** | 0.826 | 0.810 – 0.980 |
| **Svipall** | **0.920** | 0.831 | 0.773 – 0.976 |
| Svipall, boilerplate removal off | 0.732 | 0.696 | 0.551 – 0.887 |

Three published extractors are above Svipall on median. In this corpus, boilerplate removal
adds about **0.19 median F1** over the disabled variant. F1 measures extraction agreement;
it does not measure token cost or guarantee that a particular answer survived.

<details>
<summary><b>The ensemble vote, and the router that was tried and retired</b></summary>

`svipall-extract` offers a vote of several heuristics reading one page. Under unanimity,
a block is removed only when *every* voter condemns it. This is a local implementation;
results from other ensemble extractors are not evidence of its accuracy.

One voter cannot remove a block on its own under unanimity. Several voters can still agree on
the wrong removal, so this does not guarantee preservation of every answer. Keeping additional
boilerplate can also reduce precision and increase tokens. The two-thirds rule is still available as
`Rule::Majority` for a caller who wants precision over recall; it is not the default and the module
says it never will be.

Both paths round to 0.920 on median in this validation. The vote raises the mean from 0.831 to
0.846 and the lower quartile from 0.773 to 0.804. These aggregate gains do not imply that every
individual page improves or remains unchanged.

A model to classify page type was tried here and retired, because the cheap structural signal beat it: the posting types the forum detector reads have
precision 1.000 on both halves of WCXB, against a model that named forums right about a third of
the time. When the cheaper signal is the more reliable one, it is the only one left — and it costs
one pass over a tree that is already parsed.

</details>

F1 cannot see whether the sentences a person marked as *required* survived extraction, and a page
scoring 0.92 that dropped the one sentence carrying the answer is still a failure. WCXB ships those
phrases, written by the corpus author:

| | required kept | boilerplate leaked | F1 |
|---|---|---|---|
| WCXB held-out, 505 pages | **93.3%** | 11.3% | 0.870 |
| WCXB dev, 1,476 pages | **86.3%** | 13.1% | 0.806 |

0.870 on the held-out set, over 505 pages. Five languages on DAnIEL, the remaining losses traced phrase by phrase, and the three
experiments that were tried against them and *rejected* are all in
[`docs/extraction.md`](extraction.md) — with the reason each one stayed out.

`bench extract --assert` enforces the following floors when the corresponding corpora are
supplied. WCXB and DAnIEL values below are historical measurements documented in
[`docs/extraction.md`](extraction.md); those corpora were not selected in the 2026-09-07 run.

| what is held | floor | measured |
|---|---|---|
| SIGIR-23 median F1 | ≥ 0.90 | **0.920** |
| WCXB development F1 | ≥ 0.78 | **0.806** |
| Worst DAnIEL language | ≥ 0.55 | **0.608** (Chinese) |
| Required snippets kept | ≥ 0.84 | **0.863** (dev) |
| Boilerplate leaked | ≤ 0.15 | **0.131** (dev) |
| Reachable gold words dropped | ≤ 0.15 | **0.118** |

The floors sit a little below the measurements on purpose — at the measured number, ordinary
variation turns into a red build; far below it, the gate stops being one. Raising one after an
improvement is the intended use; lowering one has to be argued for in the commit that does it.

None of these corpora are vendored — they are other people's data and
hundreds of megabytes of it — so four scripts fetch them, each naming its paper and its licence:

```bash
scripts/fetch-extraction-corpus.sh   # SIGIR-23 gold standard (Bevendorff et al.), Apache-2.0
scripts/fetch-wcxb.sh                # WCXB (Foley, 2026) — the required/forbidden phrases
scripts/fetch-daniel.sh              # DAnIEL (Lejeune et al.) — the five-language arm
scripts/fetch-teco.sh                # TeCo (Alarte & Silva), BSD — sibling pages, for template detection
cargo run -p svipall-bench --release -- extract --corpus ./extraction-corpus
```

`.ps1` equivalents sit beside each. The SIGIR tarballs are Git LFS pointers, so `git-lfs` has to be
installed first — without it a clone silently yields 133-byte text files where the pages should be,
and the script says so rather than letting the benchmark score an empty corpus.

### Anti-bot: `public31`, the independent list, scored by its own rule

`public31` is the list an independent benchmark published in May 2026 — seven stealth tools, 31
targets, 651 verdicts — scored with **that benchmark's own four-way rule** (`ok | gated | blocked |
error`) implemented in `bench/src/targets.rs`. The historical figures below retain their original
rule. Current scoring rejects missing or invalid HTTP statuses; the new comparison records both
rules and content delivery separately in [the local experiment](../bench/experiments/local-20260905/README.md).

The completed [local before/after comparison](../bench/experiments/local-20260905/findings.md) contains
918 samples across three configurations. Native mode raises `hard12` delivery from 9/12 first and
8/12 returning visits to 11/12 on both. That experiment's emulated default has mixed results, including a Zillow
delivery regression and longer difficult-set waits. Content limitations, ranges and null results
are reported alongside the gains. Its native arm is an explicit native override, and neither arm
measures today's automatic fallback. The historical table below remains unchanged.

| | runs | median | range | `blocked` verdicts |
|---|---|---|---|---|
| **Svipall** | 25, 26, 26 | **26 / 31** | 25..26 | **0** |

Those runs recorded 77 `ok`, 16 `gated` and zero `blocked` labels across 93 cells. These labels
describe returned pages; they do not reveal whether a remote decision depended on the IP address,
browser or request. The source benchmark also published a `blocked` column:

| | OK | gated | **blocked** |
|---|---|---|---|
| nodriver | 28 | 3 | **0** |
| CloakBrowser | 26 | 3 | 2 |
| curl_cffi | 26 | 3 | 2 |
| Patchright | 25 | 3 | 3 |
| Camoufox | 25 | 3 | 3 |
| Playwright (vanilla) | 24 | 2 | 5 |
| rebrowser-playwright | 24 | 2 | 5 |
| **Svipall** | **26** | 5 | **0** |

That table is a citation, not a measurement. The seven rows above Svipall are the figures
that benchmark published; this project did not run those tools and cannot vouch for them. Different
machine, different address, months apart — **the OK counts are not comparable cell for cell and are
not offered as if they were.** What *is* checkable here is the porting: the target list and the
four-way rule live in [`bench/src/targets.rs`](../bench/src/targets.rs), so you can read exactly what
Svipall's own row was scored under and re-run it yourself.

The `blocked` counts are subject to the same differences in machine, address, time and measurement
conditions as the success counts. They do not establish a ranking between these tools.

The aggregate counts do not establish which targets every tool passed. For Svipall, resolved by
tier across the three historical runs: `http` 44, `real`
29, `warm` 4 — **44 of 93 recorded target visits passed at HTTP**. Median cost: **115.4 s per
run of 31, or 3.7 s per page.**

<details>
<summary><b>The five cells that do not pass, and what each one actually is</b></summary>

| Consistently gated | What it is |
|---|---|
| `bot.incolumitas.com`, `browserscan.net` bot page | Detection panels that score a visitor and print a verdict. There is no article to come back with. Fetching a panel does not prove that it judged the visitor human |
| `sedarplus.ca` | A WAF response in the saved Svipall run; the later native/auto audit also records refusals |
| `medium.com`, `canadianinsider.com` | Historical rule/manual-inspection disagreement: saved responses had site titles and substantial bodies, while the rule matched `cdn-cgi/challenge-platform`. Neither status, title, size nor that script alone establishes useful content; the later native/auto audit separately reviews content |

That last row is the ported rule being over-broad, measured directly rather than argued about.
Svipall's own classifier is right and the imported one is wrong — and **the cells are still reported
as failures**, because moving a target or bending a scoring function to win two cells is how a
benchmark stops meaning anything. What is *not* done is escalating those pages to a browser to
satisfy the rule: opening a browser on a page already in hand would make the tool worse in exchange
for a number.

`indeed-jobs` used to be a sixth, and it is the single cell that took this list from 25 to 26. It is
a real Cloudflare managed challenge and it is also the flakiest target here — this benchmark has
watched it, `crunchbase` and `zillow` swap places across four separate rounds. What was different on
the run that moved it is **not the code but the address**, which had been left alone for two hours.
A number that moves when the address rests is a number about the address. It is reported as an
improvement only because the median left the previous range, which is this project's rule.

A Firefox arm was also measured once — `http_firefox = true`, one run, `25/31`, committed as
`bench/baseline/public31-firefox.*`. One run is not a median, so the default Chrome configuration
stays the headline.
</details>

### Anti-bot: `hard12`, our own list, chosen *because* it has walls

Twelve sites, scored by whether the expected text came back with no wall reported. Three runs,
2026-09-04. A 7/12 here and a 26/31 there are not the same kind of number, and quoting one
against the other — in either direction — is reading noise as signal. Both are published, each with
its list, so nobody has to.

| Site | Protection | Passed | Tier that answered | Time |
|---|---|---|---|---|
| example.com | none | 3/3 | `http` | 0.2 s |
| en.wikipedia.org | none | 3/3 | `http` | 0.3 s |
| news.ycombinator.com | none, JS-light | 3/3 | `browser` | 1.5–1.7 s |
| nowsecure.nl | Cloudflare Turnstile | 3/3 | `browser` | 1.5–2.1 s |
| amazon.com search | Amazon's own detection + JS-rendered listings | 3/3 | `browser` | 2.9–3.3 s |
| newegg.com listing | Akamai Bot Manager | 3/3 | `browser` | 6.9–11.6 s |
| stackoverflow.com | Cloudflare (403 on plain HTTP) | 3/3 | `warm` | 2.0–3.0 s |
| zillow.com | PerimeterX / HUMAN "Press & Hold" | **1/3** | `warm`, in the run that passed | 4.8 s pass; 57–61 s on the two failures |
| crunchbase.com | Cloudflare managed challenge | **0/3** | — | ~22 s to give up |
| indeed.com | Cloudflare managed challenge | **0/3** | — | 27–33 s |
| g2.com | DataDome (`captcha-delivery.com`) | **0/3** | — | 25–32 s |
| idealista.com | DataDome (`captcha-delivery.com`) | **0/3** | — | 25–32 s |

Resolved by tier across the three runs: `browser` 12, `http` 6, `warm` 4. Turnstile cleared on all
three runs of this list, in 1.5 s, 2.1 s and 1.6 s.

### Anti-bot: `vendors8`, four vendors named, two targets each

Two targets each behind **Kasada** (`twitch`, `hyatt`), **Akamai** (`newegg`, `homedepot`),
**DataDome** (`g2`, `idealista`) and **Cloudflare**'s managed challenge (`crunchbase`, `indeed`) —
the same ids the committed `bench/baseline/vendors8.json` uses. **Median 3/8, range 2..3.** It scores
worse than `hard12`, which is the point of publishing it. `hard12` and `public31` stay frozen, because a number only means something against
its own list.

Beyond the score:

- Kasada is passable: `twitch` clears at the `real` tier in all three runs —
  9.3 s, then 1.9 s, then 1.8 s. `hyatt`, behind the same vendor, fails in a way worth reading:
  28 s, then 63 s, then a timeout, across three runs minutes apart. The baseline reads that
  as the vendor's documented behaviour — the puzzle gets harder for an address it has seen
  repeatedly — and says so as a reading of the timings, not as something it measured inside the
  vendor.
- Akamai: the `homedepot` target answers `200` with its own error template — *"Oops!!
  Something went wrong. Please refresh page"*, 206 characters — after a day of benchmark runs
  against this address, and `403` with the same page over plain HTTP. That is a soft block wearing a
  success code. Svipall was returning those 206 characters *as the page*; it now treats a short
  "something went wrong, please refresh" as the stand-in it is.
- The two published records of the previous round disagree with each other, and rather than pick
  the flattering one, [`bench/baseline/README.md`](../bench/baseline/README.md) says so and declines to
  assert any movement at all: *"until that is resolved, 'the median left the previous range' cannot
  be asserted."*
- A change that fixed real detection is explicitly not credited with the score. Vendor signs on
  headers and cookies had been declared and never read, so a whole class of wall was reported as
  "the page did not render". Fixing it renames a block; it cannot make a page arrive — and the
  baseline says exactly that.

<a id="what-svipall-does-not-get-past-and-why"></a>

### Failures investigated in earlier runs

These historical failures were investigated on one connection. They are not permanent site
verdicts: the [2026-09-06 automatic snapshot](../bench/experiments/automatic-public-20260906/README.md)
also records G2 and Idealista deliveries. Remote history may contribute, but local experiments
cannot isolate all server-side signals or prove that an address change is necessary.

| Site | What happened in those runs | What the evidence supports |
|---|---|---|
| **g2.com**, **idealista.com** | The interstitial carries the verdict `'t':'bv'` — *blocked visitor* — in the top document; no slider is offered | Refusal persisted with a clean browser, fresh profile and rotated identity. Bare HTTP received a different challenge verdict on the same address. This is compatible with several combined signals and does not establish IP reputation as the sole cause |
| **crunchbase.com** | Passed at six seconds early in the day; refuses the same code, on a fresh profile wearing a different machine, after fifteen visits within the hour | The outcome changed with time and accumulated visits. That suggests history matters, but the run did not isolate its cause from other server or browser conditions |
| **indeed.com** | Not a fixed answer at all: 0 of 3 on `hard12` (2026-09-04), 2 of 3 on `public31` the next day, and 1 of 3 in an earlier round | Same shape as crunchbase. `indeed`, `crunchbase` and `zillow` have swapped places across four measurement rounds: this is the noise band of a list run from one address, not a code change, which is why it is listed here rather than counted as a pass |

> `web_route` can test an alternate exit supplied by the operator, without guaranteeing acceptance.
> Svipall does not bundle proxies or remote solving services. The local comparison uses neither.
>
> `evasion --exit URL` runs the same targets through an exit you supply and can help assess exit
> sensitivity. **The historical baseline records use no configured exit** — they read `"exit": null`.
> The new automatic-policy measurement likewise adds no proxy; its public-site results remain
> specific to the observed host, exit and history.

### Automation tells — 160 of 160, offline, and it fails the build

`fingerprint` asks public detectors what they see, which needs the network, which keeps it out of
the build. So there is a second harness that asks the same question of a page the benchmark serves
itself on loopback, across **five browser passes** — `browser`, `browser (reused)`, `stealth`,
`real` and `warm` — 32 probes each, and it **fails the build**:

```bash
cargo run -p svipall-bench --release -- tells --assert
```

It opened at 22 of 52 clean, fourteen probes across four tiers, before the harness grew to 32
probes and five passes. Every failure was a real contradiction that had been shipping. A sample of
what a harness catches that a person does not:

| Probe | What it caught |
|---|---|
| `residue` | `window.__svipall_console` — a ring buffer under a name that spelled out the product. Walking `window`'s own property names is the cheapest check a detector runs |
| `dom_rect` | `getBoundingClientRect` jittered `x`/`width`/`height` and not `left`/`right`/`top`/`bottom`, so every rectangle disagreed with its own arithmetic |
| `host_object_brands` | `navigator.connection` and `performance.memory` replaced by object literals: `[object Object]` where `[object NetworkInformation]` belongs |
| `languages_shape` | **`en;q=0.9` inside `navigator.languages`** — an `Accept-Language` header where a list of tags belongs. Nobody predicted this one |
| `screen_plausible` | Headless reports an 800×600 display while the flags size the window to 1366×768: a window wider than its screen |
| `worker_realm` | A worker reporting the host's real 32 cores beside a document reporting the identity's 8. One `postMessage` to catch |
| `cross_realm_tostring` | A same-origin `about:blank` iframe is a realm of its own, so the `toString` mask had never seen the top realm's accessors. One line returned the patch *and* the value it hid, at every tier |
| `navigator_webdriver` | `navigator.webdriver` was deleted outright. Every Chrome since 89 carries the property and answers `false` — the deletion was the only thing producing a navigator no real browser has |
| `runtime_domain_unobservable` | A watchdog, not a defect: it fires only if Chrome reopens the `Runtime.enable` console leak the CDP client's design rests on |

The recorded run passed all 32 probes at all five emulated passes. These are checks of known
automation tells, not a guarantee of undetectability or a test of native anonymity. Two of the fixes were structural rather than cosmetic: the
console ring is gone from the page entirely and comes from `Runtime.consoleAPICalled` on the protocol
side, and workers are handed the identity in the window between attaching paused and resuming.

### Identity coherence — asserted offline, in CI

A fingerprint is rarely caught by one odd value. It is caught by a combination no real device
produces: a macOS user agent with a Windows GPU, a desktop with no taskbar, a Firefox emitting
Chrome's client hints. Camoufox, the leading patched-Firefox project, names exactly this in its own
documentation as the thing it keeps getting wrong — not the spoofing technique, the *coherence
between spoofed values*. The quotation and what it implies are in
[`docs/firefox.md`](firefox.md).

```bash
cargo run -p svipall-bench --release -- fingerprint --engine chrome
```

checks all seven identities Svipall can wear (Chrome, Firefox and phone, across three operating
systems) plus a **sweep of 1,500 freshly drawn machines**, against themselves: engine ↔ user agent,
client hints ↔ engine, screen ↔ availHeight ↔ viewport, form factor ↔ platform, timezone ↔ language,
renderer ↔ engine, and the macOS OS-token spelling that differs between the two engines. No network,
no browser, and it **fails the build** on a contradiction. It runs in `qc` and in CI.

Run **without** the `--engine` flag, the same command adds a network half — the only part of it that
touches the wire — and asserts eight things against `tls.peet.ws`:

| What is asserted | Measured |
|---|---|
| Which engine actually ran | `wreq` — the emulating one, not the fallback |
| Negotiated protocol | `h2` |
| JA4 carries the h2 marker | `t13d1516h2_8daaf6152771_d8a2da3f94cd` |
| Cipher count is Chrome-shaped | **15**, where rustls sends 20 |
| Extension count is Chrome-shaped | **16**, where rustls sends 11 |
| GREASE values present | yes |
| User-Agent matches the emulation | Chrome 149 on Windows |
| Post-quantum key share | `X25519MLKEM768` offered **with a key share**, as Chrome 131+ does |

> An earlier version of this file called the post-quantum key share a known gap. **It was not:** the
> check was looking for it in `ja4_r`, which lists ciphers, extensions and signature algorithms and
> never supported groups. The engine had been offering it all along. That correction is in the
> baseline log too, because a benchmark that quietly deletes its own mistakes is a marketing page.

### CPU budgets — measured, not recalled

`cargo run -p svipall-bench --release -- micro` on a 195 KB generated news page. The fixture is
generated from a fixed seed rather than checked in, so two machines measure the same document.

**The `Measured` column is one run on one desktop and yours will differ; the `Budget` column is what
`--assert` actually enforces**, and it is the half that gates the build. Timing budgets carry
headroom for exactly that reason; the structural checks are exact and cannot flake on any machine.

| Check | Measured | Budget |
|---|---|---|
| `classify` a 200 KB page | 163 µs | 400 µs |
| `quality::assess` | 43.29 µs | 250 µs |
| `parse_page`, text + title | 2.2 ms | 14 ms |
| `parse_page`, everything | 5.92 ms | 20 ms |
| Markdown, voted | 5.22 ms | 8 ms |
| `template::strip` | 279 µs | 2 ms |
| `induce` a schema from a listing | 2.35 ms | 60 ms |
| `bm25_filter`, full page | 1.88 ms | 3 ms |
| `budget::take`, full page | 254 µs | 4 ms |
| `simhash`, full page | 700 µs | 5 ms |
| `cache::find_near` over 300 pages | 718 ns | 2 ms |
| **DOM parses** for text + title + markdown + links + metadata | **1** | exactly 1 |
| **Disk reads** across 10,000 domain-state lookups | **0** | exactly 0 |
| **Reputation-ledger writes** across 10,000 charges | **0** | at most 1 |
| Pruning kept the article, the code and the table; dropped the nav and the sidebar | pass | exact |

Measured column from `bench/experiments/cpu-budgets-20260907/micro.txt`, one Windows machine,
2026-09-07. CPU timings depend on the machine; the budget column does not, and
`cargo run -p svipall-bench --release -- micro --assert` is what CI enforces.

---
