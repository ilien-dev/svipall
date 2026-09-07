# Measured automatic-mode results

Three rounds with persistent state; positions are consecutive calls within each target batch.
Delivery means the frozen check passed, not proven content completeness. Failures remain in every denominator.

| Set | Position | Passes per round | Median / targets | Range | Call median / p95 (s) | Successful-call median (s) | Fetch seconds / delivery |
|---|---:|---|---:|---|---:|---:|---:|
| public31 | 1 | 26, 26, 26 | 26/31 | 26..26 | 0.94/32.80 | 0.89 | 6.54 |
| public31 | 2 | 26, 26, 27 | 26/31 | 26..27 | 0.88/37.68 | 0.86 | 7.12 |
| public31 | 3 | 27, 27, 26 | 27/31 | 26..27 | 1.01/23.28 | 1.01 | 4.18 |
| hard12 | 1 | 10, 10, 7 | 10/12 | 7..10 | 0.64/49.33 | 2.51 | 13.66 |
| hard12 | 2 | 7, 9, 6 | 7/12 | 6..9 | 0.87/59.91 | 0.98 | 17.30 |
| hard12 | 3 | 8, 9, 6 | 8/12 | 6..9 | 1.00/49.83 | 1.02 | 10.01 |
| vendors8 | 1 | 5, 6, 4 | 5/8 | 4..6 | 2.08/40.76 | 5.62 | 12.71 |
| vendors8 | 2 | 4, 4, 4 | 4/8 | 4..4 | 1.11/39.33 | 3.22 | 17.36 |
| vendors8 | 3 | 4, 4, 4 | 4/8 | 4..4 | 0.89/9.18 | 3.07 | 5.67 |

The last column includes time spent on failures and local refusals, divided by delivered results.
Controller pauses and process startup/shutdown overhead are included in wall-clock duration, not fetch seconds.

| Set | Calls | Passes | Native recorded / delivered | Confirmed local deferrals | Empty attempt logs | Timeouts | Fetch seconds |
|---|---:|---:|---:|---:|---:|---:|---:|
| public31 | 279 | 237 | 10/1 | 16 | 16 | 3 | 1406.75 |
| hard12 | 108 | 72 | 14/11 | 23 | 23 | 6 | 979.72 |
| vendors8 | 72 | 39 | 4/3 | 27 | 27 | 0 | 467.03 |

The frozen summary field `refused_without_attempt` counts empty attempt logs. An outer timeout can also
produce an empty log, so the separate confirmed-deferral count requires an explicit local policy reason.
Native counts reflect recorded fallback flags; an outer timeout can lose internal attempt details.

Total recorded calls: 459. Unique requested URLs: 48.
Controller wall time: 63.95 minutes.

See `calls.csv`, `targets.csv`, `summary.json`, the content audit and compressed raw records for detail.
