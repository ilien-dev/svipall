# Same-document extraction evidence

All three planned captures completed with status 200, native identity and warm mode, without a
blocked verdict. There were no replacement visits or retries. The original HTML, response
metadata and all replay outputs remain in ignored local state; `public-html-check.json` records
their hashes. The next public comparison must seed accounting from `state/public-html/limits`.

The captures ran during the candidate's corpus accuracy evaluation, after its timed default
CPU gate. Their times are not speed-comparison results. No current-tree build input was changed.

## Correcting the replay origin

The CLI's `raw:` input has no origin, so the initial replays could not resolve relative links.
Those outputs and counts are retained, but do not describe link extraction with the real page
origin. `replay-with-base.py` resolves only relative `href`, `src` and `data-src` attributes
against the captured response URL (respecting a document base when present). It preserves every
other HTML source byte. Both frozen extractors then receive the exact same derived document.
This additional offline check makes no network requests; see `public-html-resolved-replay.json`.

These are whole-output heading/link counts, not a completeness score or counts of valid records:

| Captured document | Main headings before / after | Main links before / after | Full outputs equal |
|---|---:|---:|---|
| Repository exploration | 25 / 26 | 13 / 14 | yes |
| Job listings | 9 / 9 | 2 / 2 | yes |
| Search results | 7 / 7 | 10 / 10 | yes |

The narrow heading-chain prototype restores one repository-page heading and link. It has no
effect on the saved job or search documents. This is insufficient evidence that it repairs
those public listing losses. Full-output equality is an unchanged-path control, not proof that
the full output contains every requested record or has ideal Markdown formatting.

## Further regressions grounded in the captured structure

The repository heading shares wrappers with an icon and a control. Job headings are wrapped
inside presentation-table cells next to employer/location metadata. Search headings sit inside
links that also contain a site label, with another control beside the link. These structures
do not satisfy the prototype's pure-div-chain and 200-character sibling-prose requirements.

Three synthetic fixtures reproduce those structural patterns with original text and `.test`
URLs, without copying public page prose. The frozen narrow prototype returns **0 of 5 titles and
destinations** in each main-content fixture, versus **5 of 5** in each full-content control.
The semantic navigation sentinel remains excluded in main mode. The expected-content assertion
fails with exit code 1; see `record-structures-red.txt`, its execution record and the fixture
hashes in `record-structures-red.json`.

A separate extractor workspace under ignored state investigates the correction without changing
the source snapshot of active QC. All three Rust integration tests failed before implementation.
The broader prototype then passed those three, the four existing heading/navigation/hidden-text
tests and all 123 extractor unit tests. It preserves neutral wrappers containing a single heading
block, including inline site labels, icons, controls and table-cell layout. Negative names,
navigation roles and semantic chrome stop propagation. Containers with multiple prose/list
blocks still face ordinary density pruning. It does not globally lower density thresholds.

The standalone prototype replays the original captured HTML with its original URL supplied
directly as `ExtractOpts::base_url`. All three full-content outputs are byte-identical to the
origin-resolved unchanged-extractor controls, validating that the replay-origin adjustment did
not account for the main-content gains. `broad-public-replay.json` records the following whole-output
counts (still not a record-completeness score):

| Captured document | Main headings, unchanged / broader | Main links, unchanged / broader |
|---|---:|---:|
| Repository exploration | 25 / 42 | 13 / 44 |
| Job listings | 9 / 54 | 2 / 20 |
| Search results | 7 / 7 | 10 / 19 |

Search titles nested inside a link do not appear as separate Markdown heading lines, so the
heading count alone misses their restoration. Existing block-link formatting remains a limitation.

The initial broader-prototype lint run rejected identical `if` branches. Combining their
conditions fixed the lint; all-target Clippy and the 130 extractor tests passed afterward.
Both the initial error and successful logs are retained. Its corpus check uses the unchanged
production evaluator in the isolated workspace, with identical dependency versions, run
serially after current-tree QC. It completed but regressed from 0.920/0.831 median/mean F1 to
0.919/0.830; the unrestricted prototype is rejected despite its local restoration gains.

A subsequent refinement requires independent record description and excludes headings whose
only links are adjacent navigation. It passes 131 local tests including a new regression that
failed on the broader prototype. Whole-output headings/links are 36/32 for the repository page,
47/19 for jobs and 7/19 for search; all full outputs remain identical. Compared with the broader
prototype it also loses six repository headings and several job headings, so these counts must
not be presented as complete record recovery. See `refined-public-replay.json`. Its complete
corpus diagnostic found a smaller regression: 50 pages improved and 123 worsened, with median
F1 0.919679 and mean 0.830703. This refinement is also rejected. Neither broader prototype was
integrated; the final public confirmation retains the narrow candidate.
