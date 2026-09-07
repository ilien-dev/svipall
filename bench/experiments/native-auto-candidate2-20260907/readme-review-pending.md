# README reconciliation after validation

The items below were recorded before QC completed. They have now been reconciled in
`readme-reconciliation.json`, except for future broader-prototype acceptance/integration and
final public confirmation, which remain explicitly pending. The completed narrow validation
is preserved under `narrow-validation/`.

The root README is included in the 422-file QC input snapshot. Keep it stable while that run
is active, then record any documentation-only delta explicitly instead of claiming the final
documentation bytes were tested in the earlier snapshot.

- Replace pending managed-routing browser verification with the completed local result:
  eight automatic tests, one learning test and one native-timeout test passed. Four local
  browser tests also passed. The full QC and candidate corpus result are still pending.
- Update the latest validation paragraph and test-suite table from the completed current-tree
  QC log, preserving the dates and links for older runs.
- The unchanged extractor reconfirmed median 0.920, mean 0.831 and 11.82% reachable-word loss.
  Its current vote column is median 0.920 and mean 0.847; the older 0.919/0.846 figures must
  remain explicitly historical or be replaced with linked current measurements consistently.
  Wait for the candidate's table before changing current extractor claims.
- The competitor table's Svipall REST cell says "one endpoint per tool", while the API/FAQ
  correctly explain that nineteen of twenty-nine MCP tools are mapped. Make the cell explicit
  about the nineteen mapped tools; do not imply every MCP tool has a REST endpoint.
- The CLI FAQ still says every command prints one JSON object. Align it with the earlier
  qualified statement about completed data commands; `serve` is long-running and help is stderr.
- Replace prototype/public-confirmation status only after the corresponding work completes.
  Do not turn the interrupted candidate-1 result into uninterrupted evidence or reuse its
  score as the score for the heading/routing prototype.

No root README edits have been made during this QC run by this continuation.
