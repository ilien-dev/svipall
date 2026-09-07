# README factual audit — 2026-09-07

This review updates the root README against current source, saved measurements and checked
publication metadata. It changes documentation, not product behavior. The machine interruption
did not require repeating the completed native/auto workload. Its previously recorded interruptions
and limitations remain in `final-findings.md`.

## Corrections and evidence

| Area | Correction / source checked |
|---|---|
| Automatic versus native | Preserved all three variants, useful versus mechanical delivery, blocked useful excerpts, shared-budget deferrals, interruptions and per-useful-result cost. Reconciled the table against `variant-comparison.json`. A near tie is neither proven equivalence nor a universal winner |
| Route behavior | Checked `core/src/automatic.rs` and MCP `server.rs`: native remains last and conditional; login/paywall/missing-page responses stop escalation; status-based backoff does not catch every textual quota |
| Extraction | Checked sanitization, document/PDF conversion, capture and growth paths. Replaced guarantees of whole-site completeness, all hidden-text removal, reusable APIs and redesign recovery with bounded, heuristic behavior. PDF text extraction is not OCR |
| Quality | Checked quality modules, calibration and corpus records. F1 is not token cost or answer completeness; percentiles describe local history. Removed unsupported causal extrapolations from other research to this implementation |
| Captchas | Checked widget fixtures, `solve_loop.rs`, `solver_engine.rs`, model feature declarations and export/build paths. Distinguished standalone OCR from the live loop, available models from successful answers, and recorded acceptance from independently verified correctness |
| Privacy | Checked action secret substitution, browser routing and config. Secret references are not response redaction; browser settings are not a network firewall. Documented native exposure and automatic downloads |
| Installation | Read shell, PowerShell and npm installers: checksum mismatch stops installation, but unavailable verification can warn and continue. Corrected npm MCP invocation using the `svipall` package's actual binary map. Separated development source from the published prerelease |
| Platform support | Read release workflow and Dockerfile. Distinguished artifact/build-matrix availability from host validation and headful display support. Removed the implication that a container provides every capability automatically |
| Interface | Matched all 29 MCP tool names and 19 REST tool routes; included CLI config, doctor and hook commands. REST can return other non-2xx statuses, including unknown jobs; persistent sessions are an API design exclusion, not an HTTP impossibility |
| Project comparison | Replaced unsupported absence claims and ecosystem rankings with concise documented project scopes and direct primary links; no new competitor benchmark was performed |
| Licence/name | Removed an overbroad AGPL paraphrase in favor of the actual licence terms. Replaced invented mythological detail with the cited stanza/translation |

## Publication checks

Read-only checks on 2026-09-07 found:

- [GitHub releases](https://github.com/ilien-dev/svipall/releases): prerelease `v1.0.0-rc`,
  published 2026-09-05, with five platform archives, checksums and Linux x64 deb/rpm assets.
- [npm metadata](https://registry.npmjs.org/svipall/1.0.0-rc): package `svipall` exposes both
  `svipall` and `svipall-mcp`. The separate `svipall-mcp` package lookup returned 404.
- GHCR manifest `ghcr.io/ilien-dev/svipall:1.0.0-rc`: Linux amd64 and arm64 entries exist.
  This was a manifest check, not a container launch or headful browser test.
- Official Firecrawl, Crawl4AI, Scrapling and Playwright MCP READMEs support the limited
  project descriptions linked in the root README. These were not installed or benchmarked.

These are dated observations, not promises that channels or upstream features remain unchanged.
Homebrew/Scoop repositories were reachable; winget/AUR publication was not established.

## Self-critique and validation

The second pass searched absolute claims and checked for contradictions across installation,
features, privacy, limits, FAQ and benchmark prose. It caught remaining claims about token widgets,
CPU/GPU detection, corpus retention, proxy leaks and the container's supposed full functionality.
Those claims were narrowed or removed; historical null results and failures were preserved.

Run the offline reconciliation from the repository root:

```powershell
python -X utf8 bench/experiments/native-auto-candidate2-20260907/check-readme-audit.py
git diff --check
```

`readme-factual-audit.json` records the current README hash, local target/anchor checks, matching
config defaults, tool/route coverage and measured-source reconciliation. Frozen product inputs
still match the measured snapshot; README, the two previously tested controller files and
`.gitattributes` differ among frozen inputs. The attributes were added during commit preparation
to preserve the new experiment files byte-for-byte in Git, including captured terminal whitespace.
The existing full QC record remains applicable to that product snapshot.

No new public benchmark, full QC run or installation was performed for these documentation edits.
Structural checks do not prove every sentence true. This audit does not validate every future
site, language, OS, model or detector, and linked historical guides were not comprehensively
rewritten. Earlier README hashes remain evidence of their earlier versions, not of this revision.
