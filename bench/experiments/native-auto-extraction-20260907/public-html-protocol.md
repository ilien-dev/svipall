# Same-document public extraction check

Purpose: determine whether the local heading-preservation regression also applies to actual
listing markup. The saved paired responses contain extracted content, not the full source HTML.
Changes between separate website visits cannot establish an extractor effect.

After candidate-1 and the unchanged corpus measurement complete and the local regressions are
reviewed, capture one document each
from the existing `github-explore`, `indeed-jobs` and `google-search` targets, in that order.
Use the preserved pre-change CLI, explicit native warm mode, the same pinned browser, a fresh
unnamed profile and a 60-second fetch deadline. This is at most three calls, with no retries,
new exits, credentials, manual challenge assistance or budget resets. Copy the final shared
accounting from candidate-1 before starting, preserve it throughout, and seed the next public
comparison from the final capture accounting. These additional admissions are recorded separately
and do not enter the 918-call score.

Request HTML with an output file so the content budget does not truncate the document. Keep
captured pages, raw response metadata and their hashes in ignored local experiment state. A wall,
empty page or unavailable document remains a failed capture and must not be replaced silently.

Run both preserved CLI extractors against the exact same captured HTML through `fetch raw:
--stdin`, writing Markdown to files, with main-content pruning enabled and with `--full` as a
control. No network is used for these local replays. Verify the frozen before/after executable
hashes, compare the actual record headings and links, and publish losses and null results as well
as gains. The full-content outputs should remain identical because this candidate changes only
the pruning path. This check is about extraction fidelity, not network latency or a general
native-versus-auto winner.

Passing this check does not replace the corpus floors, local hidden-text/navigation regressions,
or uninterrupted final public comparison. A failed capture cannot establish an extractor result.

Pre-execution scheduling amendment: the captures may run during the candidate's corpus accuracy
evaluation, after the timed default CPU gate. The capture and replay executables are already
frozen and locally tested, so this does not depend on completion of the current-tree QC. Capture
latencies and corpus elapsed time are not used as speed comparisons. Record the overlap, and
keep the separate three-pair CPU comparison serial after QC. Full QC and corpus review remain
required before freezing or launching the final paired public comparison. This amendment was
made before any of the three captures, with no change to targets, count, accounting or scoring.
