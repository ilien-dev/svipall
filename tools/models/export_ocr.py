"""Fetch the text reader `web_fetch` uses on images and scanned PDF pages.

PP-OCRv5 mobile (Apache-2.0), as ONNX: a detector that finds lines of text in a picture (any script)
and a recogniser for the Latin alphabet, accents and all, that reads one line at a time. No
account, no key, no service: both files are downloaded once from their public release, checked
against the hashes pinned below, and everything else is local.

    uv run --python 3.12 --with onnx tools/models/export_ocr.py [--out ~/.svipall/models]

What comes out, and the contract each keeps with `docs/models.md`:

  ocr_det.onnx   input `x` [1, 3, H, W] (H, W multiples of 32, ImageNet-normalised RGB)
                 -> [1, 1, H, W] probability that each pixel is text
  ocr_rec.onnx   input `x` [n, 3, 48, W] (one line, height 48, normalised to -1..1)
                 -> [n, W/8, classes] per-step class scores, decoded greedily as CTC
  ocr_rec.json   the recogniser's character set (class 0 is the CTC blank, the last class is a
                 space) and the numbers both stages need, so nothing about the models is written
                 in Rust
"""

import argparse
import hashlib
import json
import pathlib
import urllib.request

import onnx

BASE = "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.9.2/onnx/PP-OCRv5"
DET = (f"{BASE}/det/ch_PP-OCRv5_det_mobile.onnx",
       "4d97c44a20d30a81aad087d6a396b08f786c4635742afc391f6621f5c6ae78ae")
REC = (f"{BASE}/rec/latin_PP-OCRv5_rec_mobile.onnx",
       "b20bd37c168a570f583afbc8cd7925603890efbcdc000a59e22c269d160b5f5a")


def fetch(url: str, sha256: str) -> bytes:
    with urllib.request.urlopen(url, timeout=120) as r:
        data = r.read()
    got = hashlib.sha256(data).hexdigest()
    if got != sha256:
        raise SystemExit(f"{url}: sha256 {got}, expected {sha256}")
    return data


def charset(model: onnx.ModelProto) -> list[str]:
    """The characters the recogniser was trained on, from the model's own metadata."""
    meta = {p.key: p.value for p in model.metadata_props}
    raw = meta.get("character")
    if raw is None:
        raise SystemExit(f"no character list in the recogniser's metadata: {sorted(meta)}")
    return raw.splitlines()


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=str(pathlib.Path.home() / ".svipall" / "models"))
    args = ap.parse_args()
    out = pathlib.Path(args.out).expanduser()
    out.mkdir(parents=True, exist_ok=True)

    det = fetch(*DET)
    rec = fetch(*REC)
    chars = charset(onnx.load_from_string(rec))
    (out / "ocr_det.onnx").write_bytes(det)
    (out / "ocr_rec.onnx").write_bytes(rec)
    classes = onnx.load_from_string(rec).graph.output[0].type.tensor_type.shape.dim[2].dim_value
    # Blank first, the character list, then a space: how PaddleOCR builds its classes.
    table = [""] + chars + [" "]
    if classes and classes != len(table):
        raise SystemExit(f"{len(table)} characters for {classes} classes")
    (out / "ocr_rec.json").write_text(json.dumps({
        "charset": table,
        "det": {
            "limit_side": 960,
            "mean": [0.485, 0.456, 0.406],
            "std": [0.229, 0.224, 0.225],
            "threshold": 0.3,
            "box_threshold": 0.6,
            "unclip": 1.5,
        },
        "rec": {"height": 48, "max_width": 960},
    }, ensure_ascii=False, indent=1))
    print(f"wrote {out}/ocr_det.onnx, ocr_rec.onnx, ocr_rec.json ({len(table)} classes)")


if __name__ == "__main__":
    main()
