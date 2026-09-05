"""Tiny ONNX graphs for the test suite — built by hand with `onnx.helper`, no weights, no download.

They exist so that CI runs a real ONNX Runtime session through the same code path a real model
takes, with an answer that can be asserted by construction:

  grid.onnx     [1,3,H,W] -> [1,2]        class 1 is "bright": the mean pixel value.
  segment.onnx  [1,3,H,W] -> [1,2,H,W]    plane 1 is "bright": each pixel's mean over channels.

Written to `crates/svipall-mcp/tests/fixtures/models/`. Run once; the files are committed.
"""

import json
import pathlib

import onnx
from onnx import TensorProto, helper


def grid(size):
    x = helper.make_tensor_value_info("input", TensorProto.FLOAT, [1, 3, size, size])
    y = helper.make_tensor_value_info("output", TensorProto.FLOAT, [1, 2])
    one = helper.make_tensor("one", TensorProto.FLOAT, [1, 1], [1.0])
    nodes = [
        helper.make_node("ReduceMean", ["input"], ["m"], axes=[1, 2, 3], keepdims=0),
        helper.make_node("Unsqueeze", ["m", "axes01"], ["m2"]),
        helper.make_node("Sub", ["one", "m2"], ["dark"]),
        helper.make_node("Concat", ["dark", "m2"], ["output"], axis=1),
    ]
    axes = helper.make_tensor("axes01", TensorProto.INT64, [1], [1])
    g = helper.make_graph(nodes, "grid_fixture", [x], [y], initializer=[one, axes])
    return helper.make_model(g, opset_imports=[helper.make_opsetid("", 13)])


def segment(size):
    x = helper.make_tensor_value_info("input", TensorProto.FLOAT, [1, 3, size, size])
    y = helper.make_tensor_value_info("output", TensorProto.FLOAT, [1, 2, size, size])
    one = helper.make_tensor("one", TensorProto.FLOAT, [1, 1, 1, 1], [1.0])
    nodes = [
        helper.make_node("ReduceMean", ["input"], ["m"], axes=[1], keepdims=1),
        helper.make_node("Sub", ["one", "m"], ["dark"]),
        helper.make_node("Concat", ["dark", "m"], ["output"], axis=1),
    ]
    g = helper.make_graph(nodes, "segment_fixture", [x], [y], initializer=[one])
    return helper.make_model(g, opset_imports=[helper.make_opsetid("", 13)])


def main():
    out = pathlib.Path("crates/svipall-mcp/tests/fixtures/models")
    out.mkdir(parents=True, exist_ok=True)
    size = 16
    m = grid(size)
    m.ir_version = 8
    onnx.checker.check_model(m)
    onnx.save(m, out / "grid.onnx")
    (out / "grid.json").write_text(json.dumps({
        "height": size, "width": size, "channels": 3, "normalize": True,
        "threshold": 0.5, "multilabel": True, "classes": ["dark", "bright"],
    }))
    m = segment(size)
    m.ir_version = 8
    onnx.checker.check_model(m)
    onnx.save(m, out / "segment.onnx")
    (out / "segment.json").write_text(json.dumps({
        "height": size, "width": size, "channels": 3, "normalize": True,
        "threshold": 0.5, "min_overlap": 0.08, "classes": ["dark", "bright"],
    }))
    for p in sorted(out.iterdir()):
        print(p, p.stat().st_size, "bytes")


if __name__ == "__main__":
    main()
