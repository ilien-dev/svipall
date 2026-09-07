# Wrapped listing headings: local reproduction

Status: the narrow shared extraction correction is retained after regression tests, full QC,
corpus and cost checks. Both broader isolated expansions were rejected after corpus regressions.
The candidate-2 public comparison completed with the retained narrow correction; see its
[final findings](../native-auto-candidate2-20260907/final-findings.md).

The native/automatic content audit found job descriptions without titles or item links,
repository descriptions without repository links, and search snippets without source URLs.
This local experiment reproduces that shape without sending any network requests.

Five article records contain linked headings followed by substantive descriptions. The only
change between `fixtures/direct.html` and `fixtures/wrapped.html` is an extra neutral `div`
around each heading. `svipall fetch raw: --stdin --cache bypass` was run with both the default
main-content heuristic and `--full`. Results and the executable hash are in `local-before.json`.

| Shape | Main-content heuristic | Titles | Item links | Descriptions |
|---|---|---:|---:|---:|
| Direct headings | on | 5 | 5 | 5 |
| Wrapped headings | on | 0 | 0 | 5 |
| Direct headings | off | 5 | 5 | 5 |
| Wrapped headings | off | 5 | 5 | 5 |

These are content counts, not a latency benchmark. The four local invocations lasted about
two seconds in total while the candidate-1 public run remained active; they used a separate
local home and did not alter its profiles or network accounting.

The density pruner independently drops the small all-link wrapper while preserving its parent
article and adjacent paragraph. A direct heading is not a pruning candidate. A correction must
preserve record headings without restoring whole navigation rails, related-content blocks or
hidden text. Broadly loosening density thresholds already has documented regressions in the
extractor, so that alone is not evidence of an improvement.

The integration regression failed on the unchanged extractor: all five linked titles disappeared
at wrapper depth one, while the navigation and hidden-text controls passed. The candidate now
preserves neutral wrapper chains around a single heading when their parent contains substantial
non-link prose. It does not exempt named related-content containers, navigation roles, or their
ancestors. A further regression caught navigation-role wrappers in the first prototype; excluding
those roles fixed it. All 123 extractor unit tests and four integration tests now pass, including
equivalence at wrapper depths one, two and six. See `regression-red.txt`, `roles-red.txt`, and
`regression-green.txt`. These are correctness results, not a measured public improvement.

The rebuilt CLI also passes the original four fixture invocations. With main-content extraction,
the wrapped fixture now returns all five titles and item links, exactly matching the direct
fixture's content. The direct fixture and both `--full` controls are byte-for-byte unchanged.
`local-after.json` and `local-comparison.json` retain the output and executable hashes. These
four invocations took about two seconds in total and are not a latency benchmark.

The focused crate checks used one build job at idle process priority while the frozen public
comparison continued; the initial build took 40.25 seconds. This background work is recorded
because host load was not controlled by the public protocol. The public executable and its
source snapshot retain the unchanged extractor. Heavy corpus work remains queued after that run.

The completed local gold-standard corpus and extraction-cost checks are recorded below. The corpus
was found under `C:/svipall-corpora/extraction-corpus`, including all eight ground-truth JSONL
files. The earlier QC did not select it through `SVIPALL_CORPUS`; it was not proven unavailable.
The readiness check confirms all 3,985 HTML files are present, valid UTF-8 and actual content,
with matching saved output records for all three reference extractors. There are 3,975 gradable
pages and ten empty annotations; see `corpus-readiness.json`.

The unchanged extractor's corpus gate completed after verifying all 918 candidate-1 calls and
the frozen executable hash. It took 897.35 seconds and passed unchanged floors: median F1
**0.920**, mean **0.831**, IQR **0.773–0.976**, with **11.82%** of reachable gold words dropped.
The study's reference extractions scored median F1 0.963, 0.958 and 0.936 in the same evaluator.
The vote column scored 0.920 median and 0.847 mean in this run; these are fresh measurements,
not substitutions for historical recorded values. See `corpus-before.txt`, its stderr log,
`corpus-before-manifest.json` and `corpus-inputs.json` for execution and input hashes.

Current-tree QC scored the narrow heading candidate against these same inputs: median 0.920,
mean 0.831 and IQR 0.773–0.976 remain unchanged at reported precision. Reachable gold-word loss
fell by 216 words, from 623,328 to 623,112 of 5,271,338 (both round to 11.82%). The vote mean
moved from 0.847 to 0.846. These small changes are retained as measured, including the regression;
the result does not demonstrate a material corpus improvement.

The bounded three-document public check completed. Its narrow prototype restores one extra
repository-page heading/link and has no effect on the job or search captures. The raw CLI replay
initially lacked an origin for relative links; original outputs were retained and additional
offline replays corrected that limitation. See [same-document results](public-html-results.md).

Those captures support three more representative structural regressions. An isolated broader
prototype passed all 130 extractor tests and Clippy and restores titles/destinations in the
saved public HTML, with full-content outputs unchanged. It is not integrated into the product.
Its completed corpus check passed the absolute floors but regressed: median F1 0.919 and mean
0.830. A diagnostic using the unchanged primary scorer evaluated all 3,975 pages: 524 outputs
changed, 65 improved and 458 worsened. Exact median/mean moved from 0.919792/0.830820 to
0.919481/0.830188. The unrestricted prototype is rejected; restoring more text alone did not
justify letting unrelated news and previous-story links into article output. See
`broad-corpus-diff-summary.json` and `broad-corpus-execution.json`.

A further isolated refinement requires independent descriptive text in the enclosing record,
and prevents an unlinked navigation heading from protecting adjacent links. Its new navigation
regression failed before the change; all 131 extractor tests and Clippy pass afterward. Saved
public documents still gain titles and destinations, with full output unchanged. Its complete
primary-score diagnostic completed on all 3,975 pages with stable source hashes: 173 outputs
changed, 50 improved and 123 worsened. Median/mean F1 were 0.919679/0.830703 versus the narrow
candidate's 0.919792/0.830820. The regression is smaller but remains a regression, so this
refinement is also rejected. `refined-corpus-summary.json`, `refined-execution.json`, archived
source and frozen binaries preserve the result. No broader code or fixtures were copied into
the product. The retained narrow correction has the completed full QC recorded above.

This closes the tested heading-expansion hypotheses for this confirmation: they trade improved
record recovery on the saved listings for worse article precision across the corpus. It does
not establish an extractor-wide technical ceiling or complete listing extraction. The final
public comparison used the retained narrow candidate and records its remaining limitations.
