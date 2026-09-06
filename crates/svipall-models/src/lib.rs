//! Models that ship inside the binary.
//!
//! Every function here answers the same question — "is this model compiled in?" — with the bytes
//! and the sidecar when it is, and `None` when it is not. The build script decides which is which
//! from what sits in `models/` at compile time, so a release built with the files in place carries
//! them and a developer build without them costs nothing. Nothing is downloaded, at build time or
//! after; a model file on disk under `~/.svipall/models/` still wins over the embedded one, so an
//! operator can fine-tune from their own corpus without rebuilding.
//!
//! The sidecar is the contract (input shape, classes, thresholds); the model is opaque bytes. The
//! two travel together or not at all.

/// One model as compiled in: the ONNX graph and the JSON that describes how to feed it.
#[derive(Debug, Clone, Copy)]
pub struct Embedded {
    pub model: &'static [u8],
    pub sidecar: &'static str,
}

macro_rules! embedded {
    ($fn_name:ident, $cfg:meta, $file:literal, $ext:literal) => {
        #[doc = concat!("The `", $file, "` model, when it was compiled in.")]
        pub fn $fn_name() -> Option<Embedded> {
            #[cfg($cfg)]
            {
                Some(Embedded {
                    model: include_bytes!(concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/models/",
                        $file,
                        ".",
                        $ext
                    )),
                    sidecar: include_str!(concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/models/",
                        $file,
                        ".json"
                    )),
                })
            }
            #[cfg(not($cfg))]
            {
                None
            }
        }
    };
}

embedded!(grid, embedded_grid, "grid", "onnx");
embedded!(detect, embedded_detect, "detect", "onnx");
embedded!(segment, embedded_segment, "segment", "onnx");
embedded!(ocr, embedded_ocr, "ocr", "onnx");
embedded!(audio, embedded_audio, "audio", "onnx");
embedded!(substance, embedded_substance, "substance", "bin");

/// Names of the models compiled into this binary, for `web_status`.
pub fn compiled_in() -> Vec<&'static str> {
    let mut out = Vec::new();
    if grid().is_some() {
        out.push("grid");
    }
    if detect().is_some() {
        out.push("detect");
    }
    if segment().is_some() {
        out.push("segment");
    }
    if ocr().is_some() {
        out.push("ocr");
    }
    if audio().is_some() {
        out.push("audio");
    }
    if substance().is_some() {
        out.push("substance");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_assets_present_in_the_source_tree_are_actually_embedded() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("models");
        for (name, ext, model) in [
            ("grid", "onnx", grid()),
            ("detect", "onnx", detect()),
            ("segment", "onnx", segment()),
            ("ocr", "onnx", ocr()),
            ("audio", "onnx", audio()),
            ("substance", "bin", substance()),
        ] {
            let present = dir.join(format!("{name}.{ext}")).is_file()
                && dir.join(format!("{name}.json")).is_file();
            assert_eq!(
                model.is_some(),
                present,
                "{name}: a cached build script must not use an old workspace path"
            );
        }
    }

    #[test]
    fn an_embedded_model_always_comes_with_its_contract() {
        for e in [grid(), detect(), segment(), ocr(), audio(), substance()]
            .into_iter()
            .flatten()
        {
            assert!(!e.model.is_empty(), "an embedded model has bytes");
            let json: serde_json_lite::Value = serde_json_lite::parse(e.sidecar);
            assert!(json.is_object(), "the sidecar is a JSON object");
        }
    }

    #[test]
    fn compiled_in_names_exactly_what_is_embedded() {
        let names = compiled_in();
        assert_eq!(names.contains(&"grid"), grid().is_some());
        assert_eq!(names.contains(&"detect"), detect().is_some());
        assert_eq!(names.contains(&"segment"), segment().is_some());
        assert_eq!(names.contains(&"ocr"), ocr().is_some());
        assert_eq!(names.contains(&"audio"), audio().is_some());
        assert_eq!(names.contains(&"substance"), substance().is_some());
    }

    /// Enough JSON to know a sidecar is an object, without adding a dependency to a crate whose
    /// whole point is to be a handful of `include_bytes!`.
    mod serde_json_lite {
        pub enum Value {
            Object,
            Other,
        }
        impl Value {
            pub fn is_object(&self) -> bool {
                matches!(self, Value::Object)
            }
        }
        pub fn parse(s: &str) -> Value {
            let t = s.trim();
            if t.starts_with('{') && t.ends_with('}') {
                Value::Object
            } else {
                Value::Other
            }
        }
    }
}
