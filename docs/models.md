# Local models: what svipall runs, where they come from, and how to bring your own

Every model svipall runs is a file: either one compiled into the binary, or one you put in
`~/.svipall/models/`. **Your file wins.** Nothing is downloaded at run time, nothing is called,
and a model that is in neither place means that kind of challenge goes to the human dashboard
instead. Each model is described by a JSON sidecar; the sidecar is the contract.

Build with the matching features: `cargo build --release --features onnx-ocr,onnx-grid,onnx-audio,onnx-detect,onnx-segment,onnx-zeroshot`.
The release binaries are built that way.

## What the binary carries

| Feature | Embedded | Answers | Where it came from |
|---|---|---|---|
| `onnx-detect` | **yes** — `detect.onnx`, 13.8 MB | "click on the …", "draw a box around the …", and tile grids when there is no classifier | torchvision SSDLite320-MobileNetV3, COCO weights (BSD-3); twelve classes image challenges ask for |
| `onnx-segment` | **yes** — `segment.onnx`, 44.1 MB | the 4×4 single-picture grid: one segmentation, every square the mask touches | torchvision DeepLabV3-MobileNetV3, COCO/VOC weights (BSD-3); 21 classes |
| `onnx-grid` | no | "select all images with …", one tile at a time | yours — the detector stands in until you train one from your corpus |
| `onnx-ocr` | no | text captchas (image → string) | yours |
| `onnx-audio` | no | audio captchas (clip → digits or words) | yours |
| `onnx-zeroshot` | no | grid subjects no classifier was taught | a CLIP-style pair you install (`clip_image.onnx`, `clip_text.onnx`, `clip.json`, `vocab.json`, `merges.txt`) — too large to embed |
| — (no feature) | no | how much is actually in a page: `junk`/`thin`/`ordinary`/`substantive` | yours — `svipall quality train` fits it from your own history and your own ratings |

`tools/models/export.py` reproduces the embedded ones from torchvision on any machine with
Python; the release workflow runs it before building. No GPU is needed for any of this: every
model runs on the CPU execution provider, and on a 320 px picture the detector answers in about
10 ms and the segmenter in about 30 ms (`cargo run -p svipall-bench --release --features onnx -- micro`
measures them, with a budget that fails the build if they regress).

### Why a detector can stand in for a classifier

A grid asks "which of these squares *contain* a bus". That is not "which of these is a picture of
a bus": a square holding a wheel and a mirror contains a bus. The strongest box of the class
anywhere in the tile is exactly that question, so `grid` scores tiles with the detector when
there is no `grid.onnx`, and the selection threshold defaults to **0.2** — the value the one
peer-reviewed measurement of this task settled on (Plesner et al., *Breaking reCAPTCHAv2*,
COMPSAC 2024), where 0.5 misses the partial tiles and a missed tile fails the grid. The same paper
is why the 4×4 kind is segmented rather than classified.

## Sidecars

```json
// captcha.json — CRNN/CTC; charset[0] is the CTC blank
{"height": 32, "width": 128, "channels": 1, "normalize": true, "charset": "-0123456789abcdefghijklmnopqrstuvwxyz"}

// grid.json — image classifier; one probability (or logit) per class. `multilabel: true` says the
// row is one independent probability per class and must not be softmaxed.
{"height": 224, "width": 224, "channels": 3, "normalize": true, "threshold": 0.2, "multilabel": false,
 "classes": ["bicycle", "bus", "car", "crosswalk", "fire hydrant", "motorcycle", "traffic light"]}

// audio.json — mel spectrogram in, CTC out
{"sample_rate": 8000, "n_fft": 256, "hop": 128, "n_mels": 40, "charset": "-0123456789"}

// detect.json — single-stage detector; output [1, 4+classes, N] or [1, N, 4+classes],
// boxes as centre-x, centre-y, width, height in input pixels
{"height": 320, "width": 320, "channels": 3, "normalize": true, "threshold": 0.4, "iou": 0.5,
 "classes": ["bicycle", "car", "motorcycle", "bus", "train", "truck", "boat", "traffic light",
             "fire hydrant", "stop sign", "parking meter", "bench"]}

// segment.json — semantic segmenter; output [1, classes, H, W] of probabilities (or logits, with a
// threshold that then means what you make it), or a class-id plane [1, H, W]. A cell is selected
// when at least `min_overlap` of it is under the mask.
{"height": 320, "width": 320, "channels": 3, "normalize": true, "threshold": 0.5, "min_overlap": 0.08,
 "classes": ["background", "airplane", "bicycle", "bird", "boat", "bottle", "bus", "car", "…"]}

// clip.json — contrastive image/text pair; the text encoder takes [1, context] token ids
{"height": 224, "width": 224, "mean": [0.4815, 0.4578, 0.4082], "std": [0.2686, 0.2613, 0.2758],
 "vocab": "vocab.json", "merges": "merges.txt", "context": 77, "margin": 0.15, "threshold": 0.5}

// substance.json — hashed n-gram linear classifier over page text. Not ONNX: `substance.bin` is
// a magic marker and f32 weights, and the arithmetic is a hundred lines of Rust. Four classes,
// fixed by the code rather than the file — a model that disagreed about how many levels there
// are would be read as something it is not.
{"buckets": 65536, "dim": 8, "ngrams": 2}
```

Every image model takes pixels in `0..1` (`normalize: true`) with any mean/std the network wants
folded into the graph, so svipall builds one kind of tensor for all of them.

Rules the code enforces, so a model cannot be misread:

- A subject the model's `classes` do not name is never guessed at. With `onnx-zeroshot`
  installed it is scored by the contrastive pair, and accepted only when the best and worst tile
  differ by at least `margin`; otherwise the job goes to a person. A wrong grid spends one of two
  attempts and confirms what we are.
- A detector output whose class axis does not equal `4 + classes.len()` is refused, not reshaped.
  A segmenter output whose plane count does not equal `classes.len()` likewise.
- Everything a model returns to the solver is a fraction of the picture, never a pixel.
- A model file on disk without its sidecar is ignored, and the embedded copy is used. Half a
  model is no model.

## Swapping a model while svipall runs

Drop the new `.onnx` (and its `.json`) into `~/.svipall/models/`. The session is rebuilt on the
next solve: the loader compares the file's modification time and length with the one it has, and
reloads when they differ. No restart, no registration. `web_status` reports which copy — file or
embedded — answered.

## The corpus: training data from what this machine has seen

Every challenge svipall answers — by a model, by zero-shot, or by a person at the dashboard — is
kept for `corpus_keep_days` (default 30, `0` disables) in `~/.svipall/jobs.db`: the tiles or
picture, the prompt, the answer as given, who gave it, whether the page accepted it, and now which
strategy answered and how long it took.

```bash
svipall solver export-corpus --out ./corpus [--since 30] [--modality tiles|points|polygon|text|audio|rotate|drag] [--source model|zeroshot|human]
```

writes `corpus/manifest.jsonl` and one image per asset under `corpus/<modality>/`. A manifest line:

```json
{"task_id": "3f9c1a2b", "widget": "google.com/recaptcha", "modality": "tiles",
 "prompt": {"prompt": "Select all images with buses", "rows": 3, "cols": 3},
 "answer": {"kind": "tiles", "indices": [0, 4, 5]}, "source": "human", "ok": true,
 "at": "2026-09-03T01:02:03+00:00",
 "files": [{"file": "tiles/3f9c1a2b-tile-0.png", "kind": "tile", "idx": 0}, …]}
```

Rows with `"source": "human", "ok": true` are labelled by a person and verified by the page — the
best training data a captcha model can have. Rows where the model answered and `ok` is `false`
are the ones to look at next.

Training is yours: any framework that exports ONNX will do. Keep the sidecar contract — input
size, channels, class order — and drop the result into `~/.svipall/models/`. It is used on the
next solve, and it wins over the embedded copy. The measured recipe for a tile classifier: a
small ImageNet-pretrained backbone fine-tuned on labelled tiles reached 82 % top-1 over thirteen
classes on about twelve thousand images (the COMPSAC 2024 paper above); that is the bar the
embedded detector is standing in for.

## What a person sees

When every strategy has spent its budget on a challenge that is still on the page, the page is
parked, what it shows is posted to the dashboard with its pictures, and the answer — tiles,
points, a slider position, a hold, a turn, a drag, a transcription — is replayed on the page by
the same behaviour layer the models use. The page decides, and the verdict joins the corpus. Set
`SVIPALL_HUMAN_ASSIST=0` to skip this and report the wall at once.
