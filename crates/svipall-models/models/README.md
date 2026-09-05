# Embedded models

Drop `<name>.onnx` **and** `<name>.json` here and `cargo build` compiles them into the binary.
Names: `grid`, `detect`, `segment`, `ocr`, `audio`. Sidecar contracts are in `docs/models.md`.
A file under `~/.svipall/models/` still wins over the embedded copy at run time.

Producing them: `tools/models/export.py` (torchvision weights, CPU, no account, no key).
