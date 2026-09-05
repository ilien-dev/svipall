//! Turns whatever model files are in `models/` into `cfg(embedded_<name>)` flags.
//!
//! A model is embedded only when both halves are on disk — the weights and the `.json` sidecar
//! that is its contract. Half a model is no model, and silently compiling one in without the
//! other would make the inference path fail at run time on every machine instead of at build
//! time on this one.
//!
//! The extension travels with the name because a model here is a *file*, not a graph: most are
//! ONNX, and the classifier that reads page text is a weights blob a hundred lines of Rust knows
//! how to multiply.

use std::path::Path;

const MODELS: &[(&str, &str)] = &[
    ("grid", "onnx"),
    ("detect", "onnx"),
    ("segment", "onnx"),
    ("ocr", "onnx"),
    ("audio", "onnx"),
    ("substance", "bin"),
];

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("models");
    println!("cargo:rerun-if-changed={}", dir.display());
    for (name, ext) in MODELS {
        let weights = dir.join(format!("{name}.{ext}"));
        let json = dir.join(format!("{name}.json"));
        // Only for files that are actually there. `rerun-if-changed` on a path that does not exist
        // makes cargo re-run this script on every single invocation — and because that relinks
        // every binary downstream, `cargo run` had to replace `svipall-bench.exe` each time, which
        // on Windows fails outright while the image of the run before it is still locked. Adding a
        // model is covered by the watch on the directory above.
        for path in [&weights, &json] {
            if path.is_file() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
        match (weights.is_file(), json.is_file()) {
            (true, true) => println!("cargo:rustc-cfg=embedded_{name}"),
            (true, false) | (false, true) => println!(
                "cargo:warning=models/{name}: only one of .{ext}/.json is present; not embedded"
            ),
            (false, false) => {}
        }
    }
}
