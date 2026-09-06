# Local before/after comparison

Fresh and returning visits are reported separately. Local state starts separately with identical identity seeds; server-side IP history is uncontrolled.

| Arm | Set | Visit | Runs | Delivered median | Range | Median page seconds | p95 seconds |
|---|---|---:|---:|---:|---|---:|---:|
| after | hard12 | 1 | 3 | 8.0/12 | 8..8 | 11.54 | 67.82 |
| after | hard12 | 2 | 3 | 8.0/12 | 8..8 | 1.13 | 90.00 |
| after | public31 | 1 | 3 | 27.0/31 | 27..27 | 0.98 | 27.87 |
| after | public31 | 2 | 3 | 27.0/31 | 27..27 | 0.81 | 62.38 |
| after | vendors8 | 1 | 3 | 3.0/8 | 3..3 | 31.32 | 59.56 |
| after | vendors8 | 2 | 3 | 3.0/8 | 3..3 | 0.92 | 62.93 |
| before | hard12 | 1 | 3 | 9.0/12 | 9..9 | 5.86 | 27.94 |
| before | hard12 | 2 | 3 | 8.0/12 | 8..8 | 0.98 | 62.65 |
| before | public31 | 1 | 3 | 26.0/31 | 26..26 | 1.35 | 30.76 |
| before | public31 | 2 | 3 | 27.0/31 | 27..27 | 0.72 | 62.27 |
| before | vendors8 | 1 | 3 | 3.0/8 | 3..3 | 25.51 | 29.07 |
| before | vendors8 | 2 | 3 | 3.0/8 | 3..3 | 0.97 | 62.68 |
| native | hard12 | 1 | 3 | 11.0/12 | 11..11 | 11.30 | 67.26 |
| native | hard12 | 2 | 3 | 11.0/12 | 11..11 | 1.87 | 90.01 |
| native | public31 | 1 | 3 | 27.0/31 | 27..28 | 0.87 | 29.03 |
| native | public31 | 2 | 3 | 27.0/31 | 27..28 | 0.51 | 62.65 |
| native | vendors8 | 1 | 3 | 7.0/8 | 7..7 | 14.62 | 58.82 |
| native | vendors8 | 2 | 3 | 7.0/8 | 7..7 | 2.86 | 5.23 |

The original public verdict, valid-status scoring, errors, reuse and renewals remain separate in summary.json. Missing responses and timeouts stay in the denominator.
