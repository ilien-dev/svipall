# The text reader on a fixed set, 2026-09-26

What `web_fetch` reads out of an image, or out of a scanned PDF page, once `svipall models
install` has put the text reader in place: PP-OCRv5 mobile, a detector plus a Latin recogniser,
fetched and checked by `tools/models/export_ocr.py`.

## The set

24 pictures in `set/`, written by `make_set.py`, with what each one says in `set/truth.json`. There
are four typefaces (Noto Sans, Noto Serif, IBM Plex Sans, IBM Plex Mono), three sizes (18, 26 and
36 px), Spanish and English, and three conditions that rotate across them: clean, JPEG at quality
35, and a 0.8 px Gaussian blur. Each picture holds three lines, 138 or 139 characters with accents,
ñ, digits and punctuation.

The set is rendered text, not paper through a scanner. There is no skew, no paper texture, no
bleed-through and no handwriting, so this is a floor under what a real scan costs, not an estimate
of it.

## How it was run

```
python tools/models/export_ocr.py --out /tmp/ocr
SVIPALL_TEST_OCR_MODELS=/tmp/ocr cargo test --release -p svipall --features onnx-read \
    --test read_text the_fixed_set -- --ignored --nocapture
```

The error rate is edit distance over characters against the truth, with blank lines dropped
(they are layout). Times are one `read_text::read` call per picture, model already loaded except
for the first, CPU only, on the development machine (AMD Ryzen 9 7950X3D, 32 threads).

## Result

One run, `results.txt`:

| | |
|---|---|
| Character error rate | **0.12%**, 4 edits in 3,324 characters |
| Pictures read without an error | 20 of 24 |
| Time per picture | median 74 ms, max 139 ms (the first, which loads the models) |

The four errors: `O'Neill` read as `0'Neill` in the monospace face three times, where the letter
and the digit are drawn alike, and a space inserted before a full stop after a closing parenthesis
once, in the same face under JPEG compression. No accent and no ñ was lost.

Through `web_fetch`, the one-page scanned PDF fixture (`crates/svipall-core/fixtures/pdf/scanned.pdf`)
and the same page as a PNG both read all four of their lines exactly (`read_text.rs`,
`an_image_of_text_and_a_scanned_pdf_are_read`).
