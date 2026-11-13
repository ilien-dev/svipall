"""Fetch the image-text pair `web_video` searches pictures with, and grids fall back on.

CLIP ViT-B/32 (MIT), as int8 ONNX: an image encoder and a text encoder that put a picture and a
sentence in one space, so "a table of benchmark results" can be compared with a frame. No account,
no key, no service: the files are downloaded once from their public repository, at a pinned
revision, checked against the hashes pinned below, and everything else is local.

    python tools/models/export_clip.py [--out ~/.svipall/models]

What comes out, and the contract each keeps with `docs/models.md`:

  clip_image.onnx  input `pixel_values` [1, 3, 224, 224] (RGB, normalised with the mean and std
                   in clip.json) -> [1, 512]
  clip_text.onnx   input `input_ids` [1, 77] (int64, padded with the end token) -> [1, 512]
  vocab.json       the byte-pair vocabulary the text encoder was trained with
  merges.txt       its merge table
  clip.json        the numbers both encoders need, so nothing about the models is written in Rust

The text encoder reads English. A query in another language is best translated by whoever asks.
"""

import argparse
import hashlib
import json
import pathlib
import urllib.request

REVISION = "6b7c523703ae26e39be4b03774966f8571c7f523"
BASE = f"https://huggingface.co/Xenova/clip-vit-base-patch32/resolve/{REVISION}"
FILES = {
    "clip_image.onnx": (f"{BASE}/onnx/vision_model_quantized.onnx",
                        "583fd1110a514667812fee7d684952aaf82a99b959760c8d7dca7e0ab9839299"),
    "clip_text.onnx": (f"{BASE}/onnx/text_model_quantized.onnx",
                       "73baab855d406190da9faa498cfedf65f15cf309f4cc7385b7b032e6d08e5c3a"),
    "vocab.json": (f"{BASE}/vocab.json",
                   "5047b556ce86ccaf6aa22b3ffccfc52d391ea4accdab9c2f2407da5b742d4363"),
    "merges.txt": (f"{BASE}/merges.txt",
                   "9fd691f7c8039210e0fced15865466c65820d09b63988b0174bfe25de299051a"),
}


def fetch(url: str, sha256: str) -> bytes:
    with urllib.request.urlopen(url, timeout=300) as r:
        data = r.read()
    got = hashlib.sha256(data).hexdigest()
    if got != sha256:
        raise SystemExit(f"{url}: sha256 {got}, expected {sha256}")
    return data


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=str(pathlib.Path.home() / ".svipall" / "models"))
    args = ap.parse_args()
    out = pathlib.Path(args.out).expanduser()
    out.mkdir(parents=True, exist_ok=True)

    for name, (url, sha256) in FILES.items():
        (out / name).write_bytes(fetch(url, sha256))
    (out / "clip.json").write_text(json.dumps({
        "height": 224,
        "width": 224,
        "mean": [0.48145466, 0.4578275, 0.40821073],
        "std": [0.26862954, 0.26130258, 0.27577711],
        "vocab": "vocab.json",
        "merges": "merges.txt",
        "context": 77,
        "margin": 0.15,
        "threshold": 0.5,
    }, indent=1) + "\n")
    print(f"wrote {out}/clip_image.onnx, clip_text.onnx, vocab.json, merges.txt, clip.json")


if __name__ == "__main__":
    main()
