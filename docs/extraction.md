# Extraction

How svipall turns a fetched page into the text a model reads, how good that is, and how you find
out for yourself.

## The number

ROUGE-LSum F1, median over the 3,975 gradable pages of the SIGIR-23 gold standard, scored by
`svipall-bench extract` against the study's own published extractions:

| | median | mean | IQR |
|---|---|---|---|
| **svipall** | **0.920** | 0.831 | 0.773 – 0.976 |
| svipall, with the vote | 0.919 | 0.846 | 0.804 – 0.976 |
| svipall, pruning off | 0.732 | 0.696 | 0.551 – 0.887 |
| svipall, plain-text walker | 0.767 | 0.723 | 0.592 – 0.903 |
| readability | 0.963 | 0.861 | 0.881 – 0.987 |
| trafilatura | 0.958 | 0.877 | 0.870 – 0.986 |
| resiliparse | 0.936 | 0.826 | 0.810 – 0.980 |

Boilerplate removal is worth **+0.19 F1** over the same markdown with it switched off. Both figures
are reported because the study's own §4.4 shows the per-page distribution is power-shaped, with the
mean falling barely inside the interquartile range — a single statistic here misleads either way.

On WCXB, which labels pages by type, svipall scores **0.806** on the development set and **0.870**
on the held-out set, which places it third of fourteen on that benchmark's published leaderboard.

On DAnIEL, five languages, ROUGE-LSum mean — with the share of pages the extractor essentially
failed on, which is the column the multilingual study leads with:

| | pages | F1 | share under 0.3 |
|---|---|---|---|
| English | 475 | 0.827 | 1% |
| Greek | 273 | 0.789 | 5% |
| Polish | 274 | 0.746 | 10% |
| Russian | 266 | 0.624 | 11% |
| Chinese | 401 | **0.608** | 9% |

Chinese is the floor and it is above the reference study's Trafilatura at 0.555 and below its
Readability at 0.672. A mean of 0.6 can be an extractor that is mediocre everywhere or one that is
excellent on two thirds of the pages and useless on the rest; 9% under 0.3 says it is nearer the
first, which is the better of the two.

### The number that matters more — required-snippet recall

▲ F1 answers "how much of the gold came back". It cannot see the failure that actually breaks an
answer: a page scoring 0.92 that dropped the one sentence carrying it. Cuconasu et al. (SIGIR 2024)
measured that the document which degrades a generated answer is the high-scoring, on-topic,
**answer-free** one — so the metric to move is whether the sentences a person marked as *required*
survived extraction.

WCXB ships them: `with[]` phrases a correct extraction must contain, `without[]` phrases from the
chrome it must not. Written by the corpus author, so they are a second opinion rather than a
restatement of our own scoring. Shipping extractor, measured:

| | required kept | boilerplate leaked | pages losing content |
|---|---|---|---|
| WCXB dev, 1,476 pages | **86.3%** | 13.1% | 355 |
| WCXB held-out, 505 pages | **93.3%** | 11.3% | 79 |

⚠ **This number moved 15.7 points and the extractor never changed.** It was first published as
72.7% from an audit nobody re-derived, measured here at 70.6%, and is 86.3% once a phrase is
compared correctly. The old comparison normalised whitespace and asked for a substring, which calls
a delivered phrase missing whenever markdown puts emphasis inside it (`the **eastern** quay`), the
page uses a typographic apostrophe where the corpus wrote a straight one, a `&shy;` splits a word,
or a footnote digit is glued to the last one (`daily2`). `wcxb::contains_snippet` now compares word
sequences and allows the edges to fall inside a word; a probe against the corpus asserts it never
calls lost anything the old one called kept.

### Where the remaining losses go, and why they stay

▲ 910 required phrases still do not survive on the development split. Each is attributed to the
stage that dropped it, by extracting the same page four ways:

| where it went | phrases | can it be fixed here |
|---|---|---|
| not in the HTML at all | 121 | no — the page builds it with JavaScript |
| inside `<script>`, `<noscript>`, `<head>`, `<style>`, `<svg>` | 210 | no — text a reader never sees |
| hidden by `style`, `hidden` or `aria-hidden` | 123 | **no — the rule forbids it** |
| the page truncates itself behind "read more" | part of 110 | no — needs JavaScript |
| lost in markdown rendering | 18 | swept and rejected; see below |
| a density threshold removed it | 60 | swept; see below |
| the region selector or an unreachable pruner clause | 381 | three experiments, all rejected |

The third row is the tool's own invariant, stated in `CLAUDE.md`: *hidden text never reaches the
model*. It exists because a paragraph parked at `left:-9999px` reads to an agent exactly like the
article does, and that is the whole prompt-injection surface. 123 phrases a human annotator marked
as content sit behind it. Extracting them would raise this number and break the promise; the
promise wins.

### What was tried against the last 441 phrases, and measured

Every lever with a number attached was swept, fitted on the development split and checked once
against the held-out one.

**A class name should not condemn a block that reads like prose.** The pruner drops a small
container called `related`, `promo`, `share` or `widget` without asking what is in it — the same
defect the forum detector was built for, generalised. Exempting blocks with commas, length and low
link density:

| commas / chars / link density | held-out recall | held-out leak | held-out F1 |
|---|---|---|---|
| **no exemption (ships)** | **93.3%** | **11.3%** | **0.870** |
| 4 / 300 / 0.25 | 93.3% | 11.4% | 0.865 |
| 3 / 200 / 0.35 | 93.6% | 11.8% | 0.862 |
| 2 / 120 / 0.50 | 93.8% | 12.6% | 0.859 |

Every setting brings back more boilerplate than content, and F1 falls monotonically as the rule
loosens. Development liked it (+1.0 recall); held-out did not.

**The main-region selector trusts a region holding a fifth of the page, first match wins.**

| region rule | held-out recall | held-out leak | held-out F1 |
|---|---|---|---|
| **first match, share ≥ 1/5 (ships)** | **93.3%** | **11.3%** | **0.870** |
| first match, share ≥ 1/2 | 93.1% | 11.4% | 0.869 |
| largest match, share ≥ 1/5 | 93.3% | 12.1% | 0.869 |

Neither alternative recovered a single required phrase.

**Markdown inserts tokens the plain-text walk does not.** A list marker between two sentences —
`…is as follows:` `1.` `All queries…` — or a drop-cap rendered `**F**ree` breaks a phrase that was
delivered whole. Stripping them back out before comparing:

| stripped as well | dev recall | held-out recall |
|---|---|---|
| **nothing (ships)** | **86.3%** | **93.3%** |
| list markers | 86.2% | 93.0% |
| list markers and `*`/backtick runs | 86.2% | 92.9% |

Both lose more than they recover, and the reason is in the pattern: `d+.` at the start of a line
eats a year or a price that begins a paragraph, and those are content.

**The density thresholds had never been fitted against anything.** They are now, and they are on
the frontier — no setting on the grid has both higher recall and no more leak:

| setting | kept | leak | F1 |
|---|---|---|---|
| **shipping (25 / 0.5 / 0.35)** | **86.3%** | **13.1%** | **0.806** |
| `min_text` 0 | 86.3% | 14.3% | 0.798 |
| `min_text` 60 | 85.6% | 12.0% | 0.805 |
| `max_link_density` 0.35 | 85.8% | 12.7% | 0.805 |
| `max_link_density` 0.65 | 86.4% | 13.5% | 0.806 |
| `min_score` 0.20 | 86.4% | 13.3% | 0.806 |
| `min_score` 0.50 | 85.7% | 12.1% | 0.805 |
| all three loosened | 86.6% | 15.1% | 0.797 |

▲ **That is the ceiling for this extractor as it is built.** What remains is not a setting: it is
JavaScript this path does not run, and text this tool has promised not to return.

## How it works

One DOM parse per response, asserted by tests and by a perf budget. Everything below reads that one
tree and returns node ids; nothing re-parses and nothing rewrites markup.

1. **Selection.** A CSS selector from the caller wins outright. Otherwise the semantic selectors in
   `MAIN_SELECTORS` are tried, and a match is trusted only if it carries at least 200 characters and
   a fifth of the page's text.
2. **Removal.** `extraction::prune` scores every container on link density, text density, commas and
   its class name, and marks what reads as furniture.
3. **Rendering.** `Md` walks the surviving tree once, streaming GFM: headings, lists, tables, code
   fences, links resolved against the page URL. Hidden text never reaches the output
   (`extraction::sanitize`).

### Forum detection -- the one thing that improved the shipping path

A discussion thread is the page type every article extractor destroys, because its posts live in
containers named `comment` and every article extractor is built to strip those. svipall had the
same defect in its own pruner, whose negative list carries `comments?`.

`content::forum` asks one question instead of the router's seven, and answers it from what the page
declares about itself. Measured per signal over all 2,008 WCXB pages, on both splits:

| signal | precision | recall (dev / test) |
|---|---|---|
| `DiscussionForumPosting` / `SocialMediaPosting` | **1.000** (50 fires, 50 right) | 0.384 / 0.137 |
| `itemtype=".../Comment"` alone | 1.000 dev, **0.792** test | 0.143 / 0.373 |
| structural: repeated siblings with an author and a date | 0.800 / 0.700 | 0.036 / 0.137 |
| `QAPage` / `Question` | 0.625 | -- refused |

Only the first drives the extraction. The others are reported as evidence and nothing more: letting
the bare `Comment` type through cost **0.016 F1** on the held-out forums, which is what a signal
that is perfect on one split and 0.792 on the other does.

What that buys, on the path a fetch actually runs:

| | before | after |
|---|---|---|
| forums, dev | 0.556 | **0.567** |
| forums, held-out test | 0.809 | **0.810** |
| every other page type | -- | unchanged |

Small, and it is the only change in this work that improved the shipping extractor at all. The
structural stage -- Harvest's test, reduced from "find the posts" to "do posts exist" -- is kept
because it is the only stage that works on a page which declares nothing — and by the corpus's own
count that is **47% of development forums and 49% of held-out ones**: the two declared signals
together reach recall 0.527 and 0.510, and the rest of the thread pages on the web say nothing
about themselves at all.

▲ Harvest's ancestor discount list was added to that stage and then removed. Measured on WCXB it
cost one real forum on the held-out split — structural precision 0.700 → 0.667 — and removed none
of the false positives it was added for. Measured, rejected, and written down in `content::forum`.

### Why the vote is off, and why the router is gone

Because the corpora say so. Scored on WCXB, mean word-level F1:

| | dev | held-out test |
|---|---|---|
| **shipping**, with the forum detector — what a fetch runs | **0.806** | **0.870** |
| the vote | 0.778 | 0.826 |
| the vote, with the forum detector | 0.781 | 0.828 |
| the vote, told the true page type (an oracle) | 0.787 | 0.835 |
| the vote, told the type by the router | 0.779 | 0.825 |

Three things follow, and none of them is what the design hoped for.

▲ **The vote loses to what ships**, by 0.027 on dev and 0.045 on test. It earns its place on the
SIGIR-23 hard tail — mean +0.016, first quartile +0.031 — and loses it on the modern multi-type
corpus. So it stays available, measured, and off.

▲ **Knowing the true page type is worth about +0.010**, and that is a *ceiling*, measured with the
corpus's own labels rather than a prediction. It is the same order as the +0.003/+0.007 that WCXB's
own hybrid pipeline reported for routing to a better extractor. Routing is not where the points are.

▲ **The router recovered almost none of it** — 0.779 against the vote's 0.778. It named the profile
right 72% of the time and the type right 52.8% against a 50.3% baseline of always answering
"article", which is not enough to capture a 0.010 gain. It has been retired; see below.

The one demonstrated win is the forum profile: told a thread is a thread, the vote scores 0.766 on
the held-out forums against 0.675 without, **+0.09**. The router misses most of it because it
cannot reliably tell a forum from an article -- which is why that gain was chased with a detector
instead, above.

### The vote — `ExtractOpts::vote`, off by default

Three heuristics read the same page and only what **all of them** condemn is removed:

- `content::candidates` — Readability's `grabArticle`, constants intact. Best median in the SIGIR-23
  comparison; worst of the thirteen on WCXB's non-article types.
- `content::blocks` — Kohlschütter's shallow-text decision tree (WSDM 2010). The only voter that
  reads no characters and no punctuation, which is why it carries the multilingual case.
- `extraction::prune` — the incumbent density pass, which knows that `<pre>` and data tables survive
  whatever their score.

Unanimity is the whole safety argument: a voter that misfires, a threshold that is wrong for this
page, or a page type nobody tuned for can each only cause boilerplate to be **kept**. None of them
can cause content to be dropped. `Rule::Majority` implements the two-thirds threshold the SIGIR-23
ensembles used and is not the default.

The disagreement rate falls out of the same sets for free and is the per-page confidence signal.

### What the router left behind

The seven-class model, its 22 structural ratios and its trainer are deleted. The **vocabulary** is
not: `PageType` and the profile table live in `content::profile`, because the forum detector
resolves to them and the corpora are labelled in them.

Deleting it also removed a cost nobody was paying for on purpose. `extraction::shape` was computed
on every fetch that had a model installed and read by nothing else, and `ParseWants` carried a
router closure through the single parse to feed it.

With the model gone the extractor runs the default profile, which is the article profile — the
shape that is both commonest and safest to be wrong about.

## What a local tool has that a library does not

Every extractor compared above sees one page and decides from that page alone. svipall has a cache:
the `page` table, indexed by domain, holding what this operator actually fetched, across sessions.
Alarte and Silva measured that templates are **40–50% of the data on the web**, and SIGIR-23 says
outright that no public benchmark can evaluate cross-page methods, because none of them ships the
sibling pages. That is a statement about benchmarks, not about crawlers.

### Cross-page template learning — `svipall_core::template`, off by default

One record per domain in `kv` under `template/<domain>`: how many of that domain's pages carried
each block. A block on most pages of a site is the site, not the page. It is learned from markdown
blocks and not from the DOM, because the cache stores the rendered page — which is exactly what
`dedup::Boilerplate` already consumed.

Two rules bound it. Nothing is stripped until sixteen pages of that domain have been seen (the
figure the multi-sequence-alignment literature reports as sufficient), and a strip that would leave
under a fifth of the page removes nothing at all — either the page *is* the site's frame, which
`MostlyBoilerplate` says, or a passage the rest of the site repeats is this page's substance.

▲ **And it is off, because TECO says so.** TECO is the only public corpus that ships each key page
*with its sibling pages*, which makes it the only one that can score this at all. Learned from
sixteen siblings and applied to the labelled key page, over its thirty forum sites:

| `MIN_BLOCK` | fired on | text saved there | **labelled content removed** |
|---|---|---|---|
| 40 characters | 4 of 11 sites | 7.6% | 12 words, on 3 sites |
| **120 characters** (ships) | 2 of 11 sites | 3.4% | **1 word, on 1 site** |

The bar for anything on by default is zero — no page may lose a word of human-labelled content the
extractor had reached — and at no threshold does this clear it. Raising the floor until one
particular corpus reports zero would be fitting to that corpus. So it ships the way the vote and
the router did: built, measured, and **off**, reachable by asking.

The gate that remains says the cost may not *grow*: `bench::teco::MAX_TEMPLATE_LOSS` is the exact
measured word, and a build that loses two fails. Zero is what it has to reach before this could be
turned on for everyone.

```json
{ "url": "…", "use_site_template": true }
```

A response it changed says so — `"template": {"learned_from": 16, "removed_blocks": 3}` — because a
result that differs between two sessions from something a tool learned in between, and does not say
so, is worse than one that never improved. The record is learned on every fetch regardless, so
turning it on works immediately rather than sixteen pages later.

WCXB was tried first and cannot answer: 108 of its 1,283 domains contribute more than one page,
none more than eight, and at every threshold the template removed zero blocks from zero pages —
its same-domain pages were sampled for variety and share nothing verbatim after pruning.

### Page-level extraction on TECO

The same corpus scores the ordinary extractor against per-node labels four engineers agreed on,
which is a different gold standard from either of the others:

| | P | R | F1 |
|---|---|---|---|
| shipping extractor vs `TECO_mainContent`, 12 forum sites | 0.727 | 0.747 | 0.676 |

Forums, and forums are the type svipall does worst on — 0.567 on WCXB development. The two corpora
agree about that, which is worth more than either number alone.

### Near-duplicate lookup across sessions — `Store::find_near`

`provenance::group` compares fingerprints inside one batch, so the same wire story fetched a week
apart read as two independent sources. `find_near` asks the whole cache instead, and Manku et al.
(WWW 2007) supply the index: split the 64-bit simhash into four 16-bit bands, and two hashes within
Hamming distance 3 differ in at most three bands, so **at least one band is equal**. Four equality
indexes return every true near-duplicate and a few false ones, which the exact distance discards. It
is lossless at three bits and stops being so at four, which is why `NEAR_DUPLICATE_BITS` and the
four bands are not independent numbers — a wider lookup is refused rather than answered partly.

Reported under `include_quality`, never acted on.

### Diversity ordering — `quality::diversity`

`fetch_many` reorders its results by Maximal Marginal Relevance (Carbonell & Goldstein, SIGIR 1998)
over the simhashes already computed. Nothing is dropped, and the caller's first choice never moves;
what changes is which result they read second. Cuconasu et al. (SIGIR 2024) is the reason: what
degrades an answer is the high-scoring, on-topic, **answer-free** page, and adding *distant*
documents raised accuracy. Four copies of one wire story at the top of a list is precisely that
shape.

Two constants, both measured rather than chosen:

- **λ = 0.5.** For an exact copy at rank 1 to lose to a novel result at the bottom of a list of *n*,
  λ must be under `n / (2(n−1))` — 0.625 at five results, tending to 0.5. At Carbonell's
  "favour relevance" setting of 0.7 the copies stay stacked at the top.
- **Redundancy is rebased on chance.** Two *unrelated* simhashes agree on about half their bits, so
  raw similarity charges every candidate a large constant penalty that cancels out of the
  comparison. Measured, on three copies of a story and one unrelated page: the unrelated page still
  finished last. Similarity of 0.5 and below is now zero redundancy.

A response whose order changed says `"reordered_for_diversity": true`.

### Provenance and calibration — `include_quality: true`

The full breakdown: the integrity verdict with its reasons, the optimisation level with its traits
and the structural signals behind them, the substance label, what the near-duplicate lookup found,
and observations about where the page came from — a byline, a publication date, outbound citations
counted by distinct host, and when this machine first saw the site (`MIN(fetched_at)`).

▲ **Observations, never a score.** The W3C Credible Web Community Group's own finding is why: acting
on signals of this kind produces "a bias towards larger, professional news organisations", because
an outlet with a masthead emits all of them and a specialist writing under a pseudonym emits none.

Each score also carries its percentile among the pages this machine has fetched, accumulated per
class in `kv` under `calib/<class>` — with the width of that claim (±9 points at thirty
observations, ±3 at two hundred), and an explicit refusal below thirty:

```json
"optimization_calibration": { "unavailable": "not enough observations yet: 7 of 30 needed" }
```

## Running the benchmark

The corpora are hundreds of megabytes of other people's web pages and are not in this repository.

```sh
scripts/fetch-extraction-corpus.sh  ~/corpora/sigir23   # 3,985 pages, 8 datasets, Apache-2.0
scripts/fetch-wcxb.sh               ~/corpora/wcxb      # 2,008 pages, 7 page types, CC-BY-4.0
scripts/fetch-daniel.sh             ~/corpora/daniel    # 1,689 pages, 5 languages
scripts/fetch-teco.sh               ~/corpora/teco forum # 30 forum sites WITH their siblings, BSD

cargo run -p svipall-bench --release -- extract \
  --corpus ~/corpora/sigir23 \
  --wcxb   ~/corpora/wcxb \
  --daniel ~/corpora/daniel/corpora/Corpus_daniel_v2.1 \
  --teco   ~/corpora/teco/forum
```

DAnIEL is fetched sparsely out of a repository holding several corpora, so the path to pass is the
subdirectory its own script prints rather than the clone root. TECO is fetched one category at a
time and that is not a convenience: its archives are stored uncompressed and `forum` alone is
5.0 GB, with all five categories north of 20 GB. `forum` is the default because it is the page type
svipall does worst on and the one the cross-page work was written for.

> On Windows, run this from PowerShell. Under Git Bash the process has been observed dying
> part-way through a long run with no message; the same binary completes with exit 0 from
> PowerShell, and with the scorer stubbed out every page extracts cleanly on a 2 MB stack, so it is
> an environment interaction rather than svipall.

Adding `--assert` turns the report into a gate against the floors in `bench::extraction::floors`.
`scripts/qc.{sh,ps1}` runs it when `SVIPALL_CORPUS` (and optionally `SVIPALL_WCXB`,
`SVIPALL_DANIEL`) point at the fetched corpora, and skips it with a notice when they do not.

## Attribution

- **SIGIR-23 corpus and baselines** — Bevendorff, Gupta, Kiesel and Stein, *An Empirical Comparison
  of Web Content Extraction Algorithms*, SIGIR 2023 (`10.1145/3539618.3591920`), Apache-2.0.
- **WCXB** — Foley, *WCXB: A Multi-Type Web Content Extraction Benchmark*, 2026
  (`10.5281/zenodo.19316874`), CC-BY-4.0.
- **DAnIEL** — Lejeune et al., 2012, as used by *Multilingual Benchmarking of Main Content
  Extractors*, SIGIR 2025.
- **Readability's algorithm** — `mozilla/readability`, Apache-2.0. Reimplemented from the published
  algorithm, not copied; its constants are kept identical so a disagreement is a bug here.
- **Boilerpipe's `NumWordsRulesClassifier`** — Kohlschütter, Fankhauser and Nejdl, *Boilerplate
  Detection using Shallow Text Features*, WSDM 2010; `boilerpipe`, Apache-2.0. Same treatment, with
  one deliberate departure: how a word is counted, so that a language written without spaces is not
  read as one word per sentence.
- **Trafilatura's selector vocabulary** — `adbar/trafilatura`, Apache-2.0.
- **TECO** — Alarte and Silva, *TeCo: A Template Extraction Corpus* (`arXiv:1409.6182`), BSD, and
  their HybEx/TemEx line of work on site-level template detection. Its condition of use is that
  results obtained with it are published, and the two tables above are those results — including
  the one that says the method does not hold its gate.
- **SimHash and its banded index** — Manku, Jain and Das Sarma, *Detecting Near-Duplicates for Web
  Crawling*, WWW 2007.
- **Maximal Marginal Relevance** — Carbonell and Goldstein, SIGIR 1998.

## What is not here, and why

- **No neural extractor.** SIGIR-23 found the deep models behind the heuristic ones, and WCXB found
  the same three years later on a modern corpus with a fine-tuned 0.6B model, at 36× the latency.
- **No `rs-trafilatura` or `libreadability` dependency.** Both take HTML and return HTML, which
  means a second parse of the document — roughly doubling the dominant cost — and neither can be
  told that a data table or a `<pre>` block must survive.
- **No truth or trust verdict.** See `docs/models.md` and the `quality` module.
