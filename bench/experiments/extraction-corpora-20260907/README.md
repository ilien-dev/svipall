# Extraction corpora: WCXB, DAnIEL and TECO

Status: run to completion on all three corpora. The one failed check is the absent SIGIR-23
gold standard, which is a separate corpus and is not fetched here; every figure below was
produced by this run.

Date: 2026-09-07. Commit `6fe972d`. Windows, release build.
`svipall-bench.exe` sha256 `b347ba7cf429341b2c12aaf09a3bdc85ecffd3feb2a30f34c26eeea66923a137`.

## Command

```bash
scripts/fetch-wcxb.sh ./wcxb-corpus
scripts/fetch-daniel.sh ./daniel-corpus
scripts/fetch-teco.sh ./teco-corpus

cargo run -p svipall-bench --release -- extract   --wcxb ./wcxb-corpus   --daniel ./daniel-corpus/corpora/Corpus_daniel_v2.1   --teco ./teco-corpus/forum
```

Neither corpus is carried in this repository. WCXB is 193 MB, DAnIEL 176 MB, and the TECO
forum archive unpacks to 13 GB, so all three are in `.gitignore` and are fetched on demand.

## What this run produced

| Figure | Value | Where it is stated |
|---|---|---|
| WCXB held-out, 505 pages, required-snippet recall | 93.3% | `extract.txt` |
| WCXB held-out, boilerplate leak | 11.3% | `extract.txt` |
| WCXB held-out, word-level F1 | 0.870 | `extract.txt` |
| WCXB development, 1,476 pages, word-level F1 | 0.806 | `extract.txt` |
| WCXB forum detector, precision | 1.000 dev, posting and comment | `extract.txt` |
| DAnIEL, worst language | 0.608, Chinese | `extract.txt` |
| DAnIEL, languages covered | 5: Greek, English, Polish, Russian, Chinese | `extract.txt` |
| TECO, page level against `TECO_mainContent` | P 0.727, R 0.747, F1 0.676 | `extract.txt` |
| TECO, cross-page template removal | fired on 2 of 11 armed sites, saved 3.4%, cost 1 word | `extract.txt` |

## What this run does not produce

`third of fourteen` is a placement against the benchmark's own published leaderboard. This
harness scores Svipall; it does not rank it against other people's submissions, so that
sentence is not backed by this log and is not backed by anything else in this repository.

## TECO's licence

TECO's condition of use is that results obtained with it are published. The harness says so
at the end of its own output. These figures belong in `docs/extraction.md`.

## Raw output

`extract.txt`, verbatim from the command above.
