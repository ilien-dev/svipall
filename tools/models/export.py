"""Export the models svipall embeds, from torchvision weights, to `crates/svipall-models/models/`.

No account, no key, no service: torchvision downloads its published weights once (BSD-3) and
everything else is local. Run once, `cargo build`, and the binary carries the models.

    py tools/models/export.py [--out crates/svipall-models/models] [--size 320]

What comes out, and the contract each keeps with `docs/models.md`:

  detect.onnx + detect.json
      SSDLite320-MobileNetV3 (COCO). One tensor `[1, N, 4 + classes]`: centre-x, centre-y,
      width, height in input pixels, then one probability per class. Non-maximum suppression
      is left to svipall (`detect::nms`), so the export is the model and nothing else.
      Classes are the COCO subset image challenges actually ask for.

  segment.onnx + segment.json
      DeepLabV3-MobileNetV3 (COCO, VOC labels). One tensor `[1, classes, H, W]` of
      probabilities, so the sidecar's `threshold` means the same thing as everywhere else.

Both take pixels in 0..1 (`normalize: true`) with the network's own mean/std folded into the
graph, so the Rust side builds one kind of tensor for every model.

There is no grid.onnx here on purpose: `grid` falls back to the detector — "does this tile
contain a bus" is "does the detector see a bus in it" — and a classifier fine-tuned on a real
corpus (`svipall solver export-corpus`) belongs on disk, where it wins over the embedded copy.
"""

import argparse
import json
import pathlib

import torch
import torch.nn as nn
import torchvision
from torchvision.models.detection import (
    SSDLite320_MobileNet_V3_Large_Weights,
    ssdlite320_mobilenet_v3_large,
)
from torchvision.models.detection.image_list import ImageList
from torchvision.models.segmentation import (
    DeepLabV3_MobileNet_V3_Large_Weights,
    deeplabv3_mobilenet_v3_large,
)

# What image challenges ask for, spelled the way they ask. Order is the model's output order.
DETECT_CLASSES = [
    "bicycle", "car", "motorcycle", "bus", "train", "truck", "boat",
    "traffic light", "fire hydrant", "stop sign", "parking meter", "bench",
]

# VOC names, renamed to the words a challenge uses.
SEGMENT_RENAME = {"aeroplane": "airplane", "motorbike": "motorcycle", "diningtable": "dining table",
                  "pottedplant": "potted plant", "tvmonitor": "tv"}


class Detect(nn.Module):
    """SSDLite's raw head, decoded to boxes, as one tensor svipall can read without the model's
    own post-processing (which is dynamic and hard to export faithfully)."""

    def __init__(self, model, keep):
        super().__init__()
        self.m = model.eval()
        self.keep = torch.tensor(keep, dtype=torch.long)
        t = model.transform
        self.register_buffer("mean", torch.tensor(t.image_mean).view(1, 3, 1, 1))
        self.register_buffer("std", torch.tensor(t.image_std).view(1, 3, 1, 1))

    def forward(self, x):
        x = (x - self.mean) / self.std
        feats = list(self.m.backbone(x).values())
        head = self.m.head(feats)
        anchors = self.m.anchor_generator(ImageList(x, [(x.shape[2], x.shape[3])]), feats)[0]
        boxes = self.m.box_coder.decode_single(head["bbox_regression"][0], anchors)
        scores = torch.softmax(head["cls_logits"][0], dim=-1)[:, self.keep]
        x1, y1, x2, y2 = boxes.unbind(-1)
        cxcywh = torch.stack([(x1 + x2) / 2, (y1 + y2) / 2, x2 - x1, y2 - y1], dim=-1)
        return torch.cat([cxcywh, scores], dim=-1).unsqueeze(0)


class Segment(nn.Module):
    def __init__(self, model, weights):
        super().__init__()
        self.m = model.eval()
        tf = weights.transforms()
        self.register_buffer("mean", torch.tensor(tf.mean).view(1, 3, 1, 1))
        self.register_buffer("std", torch.tensor(tf.std).view(1, 3, 1, 1))

    def forward(self, x):
        x = (x - self.mean) / self.std
        return torch.softmax(self.m(x)["out"], dim=1)


def export(module, example, path, opset=17):
    torch.onnx.export(
        module, example, str(path), opset_version=opset, dynamo=False,
        input_names=["input"], output_names=["output"], do_constant_folding=True,
    )
    import onnx
    onnx.checker.check_model(str(path))
    return path.stat().st_size


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="crates/svipall-models/models")
    ap.add_argument("--size", type=int, default=320)
    a = ap.parse_args()
    out = pathlib.Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    print("torchvision", torchvision.__version__)

    # --- detector -------------------------------------------------------------------------
    w = SSDLite320_MobileNet_V3_Large_Weights.DEFAULT
    coco = w.meta["categories"]
    keep = [coco.index(c) for c in DETECT_CLASSES]
    model = ssdlite320_mobilenet_v3_large(weights=w)
    det = Detect(model, keep)
    example = torch.rand(1, 3, a.size, a.size)
    with torch.no_grad():
        n_boxes = det(example).shape[1]
    size = export(det, example, out / "detect.onnx")
    (out / "detect.json").write_text(json.dumps({
        "height": a.size, "width": a.size, "channels": 3, "normalize": True,
        "threshold": 0.4, "iou": 0.5, "classes": DETECT_CLASSES,
        "source": "torchvision ssdlite320_mobilenet_v3_large, COCO weights (BSD-3)",
    }, indent=1))
    print(f"detect.onnx  {size/1e6:6.1f} MB  {n_boxes} boxes x (4 + {len(DETECT_CLASSES)})")

    # --- segmenter ------------------------------------------------------------------------
    ws = DeepLabV3_MobileNet_V3_Large_Weights.DEFAULT
    classes = [SEGMENT_RENAME.get(c, c) for c in ws.meta["categories"]]
    classes[0] = "background"
    seg = Segment(deeplabv3_mobilenet_v3_large(weights=ws), ws)
    size = export(seg, example, out / "segment.onnx")
    (out / "segment.json").write_text(json.dumps({
        "height": a.size, "width": a.size, "channels": 3, "normalize": True,
        "threshold": 0.5, "min_overlap": 0.08, "classes": classes,
        "source": "torchvision deeplabv3_mobilenet_v3_large, COCO/VOC weights (BSD-3)",
    }, indent=1))
    print(f"segment.onnx {size/1e6:6.1f} MB  {len(classes)} classes")


if __name__ == "__main__":
    main()
