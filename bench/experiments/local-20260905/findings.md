# Local implementation results

The native browser configuration improved delivery on this machine. The changed default is a
mixed result: it recovers the first Glassdoor visit, loses the previously delivered empty Zillow
search page, and spends longer on the difficult sets. Neither configuration clears every wall.

## Delivery

Each entry is the median number of delivered pages across three rounds, written as **first visit /
returning visit**. The denominator is the number of targets in that set, for each visit separately.

| Frozen set | Original product | Changed default | Changed native mode |
|---|---:|---:|---:|
| public31 | 26 / 27 of 31 | 27 / 27 of 31 | 27 / 27 of 31 |
| hard12 | 9 / 8 of 12 | 8 / 8 of 12 | 11 / 11 of 12 |
| vendors8 | 3 / 3 of 8 | 3 / 3 of 8 | 7 / 7 of 8* |

All delivery counts were identical across the three rounds except native public31, which returned
28/28 in the first round and 27/27 in each subsequent round. Thus the native delivery medians
exceed the entire reference range for first visits in public31, both visits in hard12, and both
visits in vendors8. The default's public first-visit gain and hard12 first-visit regression also
fall outside their reference ranges. Other default delivery medians are unchanged.

*Home Depot passes the frozen brand/content rule with only 105–242 characters of JavaScript, a
title or help text. It does not yield an appliance catalogue. The score must not be read as seven
complete catalogues or seven fully defeated protection systems. See the [content audit](content-audit.md).

The repeatable substantive recoveries are G2 reviews, Idealista listings and Crunchbase company
profiles in native mode. Glassdoor changes from 0/3 to 3/3 successful first visits in both new
configurations. Canadian Insider succeeds in only one native round, so it is not a stable recovery.
Zillow's reference response is a 666-character search page reporting zero results; its new wall
is a delivery-score regression, not the loss of previously retrieved property listings. Hyatt
remains blocked. [targets.csv](targets.csv) compares every target and visit across all three arms.

## Time

These are median **cumulative fetch seconds for one complete set**, including both visits and
all failures. Brackets give the full range across the three rounds. Pauses between runs are excluded.

| Set | Original | Changed default | Native |
|---|---:|---:|---:|
| public31 | 406.30 [394.80–411.79] | 370.67 [349.84–381.71] | 363.90 [353.25–385.70] |
| hard12 | 247.37 [241.93–254.87] | 367.09 [362.21–367.45] | 264.08 [258.93–266.96] |
| vendors8 | 258.69 [252.91–279.66] | 326.78 [319.14–340.41] | 158.49 [154.12–170.43] |

Native mode uses about 39% less cumulative time in vendors8, but about 7% more in hard12 while
delivering more pages. The default uses about 48% more time in hard12 and 26% more in vendors8.
These are configuration comparisons; the experiment does not isolate the causal contribution of
each implementation change. Per-page medians and p95 values are in [comparison.md](comparison.md);
unrounded run totals are in [timing.json](timing.json).

## Implementation outcomes

- First-visit detection and stable-profile eligibility are corrected. A held document can fetch
  fresh same-origin HTML without navigation. Real loopback browser tests prove document survival
  and the new network response; **zero live comparison samples used that shortcut**, so no live
  improvement is credited to it.
- Adaptive waits permit six recorded renewals in the new configurations. They do not recover
  Hyatt; this is a measured null delivery result with additional waiting cost.
- Native browser identity is configurable through the tool, retaining native browser APIs and
  per-worker coherence. HTTP transport emulation remains a separate tier.
- CLI, MCP and REST settings persist and apply at request boundaries. Browser provisioning is
  automatic when needed; explicit installation/update selects the new executable for subsequent
  requests. The installer no longer asks a separate browser-download question.
- Default desktop builds include local detector/segmentation assets and inference paths. A build
  cache defect that silently omitted those assets was fixed and covered by regression tests.
  Both comparison arms contain identical model assets. Availability and actual local inference
  were verified; these runs do not establish a general CAPTCHA-solving success rate.
- Windows packaging includes app-local compiler runtime DLLs. The installer fixture verifies
  installation, browser opt-out persistence, removal and preservation of unrelated/modified files.
  A loaded-module check proves the MCP process uses its own bundled runtime.
- A final manual test exposed an isolated-profile cleanup defect on Windows. The isolated branch
  now retires its dedicated browser before bounded directory removal. This correction was made
  after measurement; the benchmark uses persistent profiles and never exercises that branch.

Use `svipall config preset native`, or configure `browser_identity: "native"` through `web_status`,
to select the measured native mode. Existing explicit sessions retain their previous policy;
new requests use the saved settings. [Configuration details](../../../docs/local-configuration.md).

## Scope and reproducibility

The completed comparison contains **27 runs and 918 samples**. All expected samples, paired target
orders, delivery verdicts and frozen executable/browser hashes pass [audit.json](audit.json).
There are also 64 preserved progress records from interrupted runs, plus any unobserved in-flight
request; these are separate from the complete paired trials and add uncontrolled remote history.

The sets overlap, and repeated visits are correlated. They are not 918 independent sites. The
first two rounds also share a target order because of the harness's documented seed normalization.
The same machine, browser and connection were used, with separate local state. Remote IP history,
site changes and machine-level browser modifications cannot be reset by those local state folders.
Captured public HTML in all three arms contains scripts referencing the machine's security
software; [browser-injection-audit.json](browser-injection-audit.json) records the observations.
Native mode here is the application's identity setting, not a claim of an otherwise untouched host.

Public-page delivery is not a JavaScript detector pass. A median outside the previous range is the
repository's descriptive criterion, not proof of statistical significance or universal success.
No remote solver, paid proxy, external automation framework or human challenge completion was used.

The original product is `b291e45c68e41604546542d753f0808e738b4a48`, compiled with the same measurement
harness as the candidate. [manifest.json](manifest.json) pins the binaries and browser;
[model-assets.json](model-assets.json) pins the identical model weights. Full responses remain in
`results/`; [samples.csv](samples.csv) and [timeline.csv](timeline.csv) support independent analysis.
The [protocol](README.md) documents pauses, interruptions and exclusions.

## Verification

[verification.json](verification.json) records the completed checks; [current-artifacts.json](current-artifacts.json)
records the final CLI/MCP hashes after the isolated-profile correction.

- The final full `scripts/qc.ps1` gate passed after the isolated-profile correction, with exit 0:
  feature lint arms, workspace/model tests, offline HTTP/3 checks, manifests, instruction guards,
  performance budgets, 160/160 automation probes and identity coherence. The external extraction
  corpus was not configured, so its four diagnostic tests remain unrun.
- All 13 manually enabled network/browser tests passed: eight browser checks, four HTTP
  fingerprint/redirect checks and one real HTTP/3 transfer. The original cleanup failure and its
  successful rerun remain in the logs; see [network-tests.json](results/network-tests.json).
- Native/emulated loopback regressions cover worker/main-realm agreement, explicit native
  locale/timezone settings, stable profile reuse, fresh document fetches and live configuration.
- The CLI reports both embedded models and local inference. MCP initialization exposes 29 tools
  and applies saved configuration on the next call. Its runtime DLLs load from its own folder.
- Windows installer validation uses an isolated local archive and prefix. The scripts parse,
  the release workflow YAML parses, and the install/uninstall fixture passes. A fresh Windows VM
  was not used. Release automation retains the portable `dist` profile; the experiment's locally
  tuned `release` executables are not portable distribution artifacts.
- The temporary measurement task completed successfully and was removed; its absence is verified
  in [controller-cleanup.json](controller-cleanup.json).
