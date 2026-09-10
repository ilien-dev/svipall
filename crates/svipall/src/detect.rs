//! "Click on the …" and "draw a box around the …": one picture, and the answer is where things
//! are in it rather than which tiles hold them.
//!
//! Same arrangement as the grid classifier: inference is compiled in behind `onnx-detect`, the
//! model is the embedded one or the operator's `detect.onnx` with a `detect.json` beside it, and
//! without either the strategy declines and the job goes to a person. The model is a single-stage
//! detector of the common shape — one output tensor of boxes and class scores, laid out either
//! `[1, 4 + classes, N]` or `[1, N, 4 + classes]`; both are read.
//!
//! Everything this module hands back is a fraction of the picture it was given. Pixels appear
//! only inside `imp`, where the tensor is built, and never leave it.

use anyhow::{anyhow, Result};
use serde::Deserialize;

use crate::model_source::{self, Located};

/// The detector, wherever it lives.
pub fn locate() -> Option<Located> {
    model_source::locate("detect", "detect", "onnx", svipall_models::detect())
}

pub fn available() -> bool {
    cfg!(feature = "onnx-detect") && locate().is_some()
}

/// The sidecar that describes the model.
#[derive(Debug, Clone, Deserialize)]
pub struct DetectConfig {
    pub height: u32,
    pub width: u32,
    #[serde(default = "three")]
    pub channels: u32,
    #[serde(default = "yes")]
    pub normalize: bool,
    /// Class names, in output order. The prompt is matched against these.
    pub classes: Vec<String>,
    /// Minimum score for a box to count.
    #[serde(default = "point_four")]
    pub threshold: f32,
    /// Boxes overlapping a stronger one by more than this are the same object.
    #[serde(default = "point_five")]
    pub iou: f32,
}

fn three() -> u32 {
    3
}
fn yes() -> bool {
    true
}
fn point_four() -> f32 {
    0.4
}
fn point_five() -> f32 {
    0.5
}

pub fn load_config() -> Result<DetectConfig> {
    let located = locate().ok_or_else(|| anyhow!("no detect model installed or embedded"))?;
    let cfg: DetectConfig = located.config()?;
    if cfg.classes.is_empty() {
        return Err(anyhow!("{} lists no classes", located.describe()));
    }
    Ok(cfg)
}

/// One detection, as fractions of the picture: centre, size, confidence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Det {
    pub cx: f32,
    pub cy: f32,
    pub w: f32,
    pub h: f32,
    pub score: f32,
}

impl Det {
    pub fn left(&self) -> f32 {
        self.cx - self.w / 2.0
    }
    pub fn top(&self) -> f32 {
        self.cy - self.h / 2.0
    }
    pub fn right(&self) -> f32 {
        self.cx + self.w / 2.0
    }
    pub fn bottom(&self) -> f32 {
        self.cy + self.h / 2.0
    }
}

pub fn iou(a: &Det, b: &Det) -> f32 {
    let x1 = a.left().max(b.left());
    let y1 = a.top().max(b.top());
    let x2 = a.right().min(b.right());
    let y2 = a.bottom().min(b.bottom());
    let inter = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
    let union = a.w * a.h + b.w * b.h - inter;
    if union <= 0.0 {
        0.0
    } else {
        inter / union
    }
}

/// Keep the strongest box of each overlapping group. Sorted by score, ties broken by position,
/// so the same picture always yields the same boxes in the same order — a model that clicks in
/// a different order each time is not more human, only less debuggable.
pub fn nms(mut dets: Vec<Det>, iou_limit: f32) -> Vec<Det> {
    dets.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cx.partial_cmp(&b.cx).unwrap_or(std::cmp::Ordering::Equal))
            .then(a.cy.partial_cmp(&b.cy).unwrap_or(std::cmp::Ordering::Equal))
    });
    let mut kept: Vec<Det> = Vec::new();
    for d in dets {
        if kept.iter().all(|k| iou(k, &d) <= iou_limit) {
            kept.push(d);
        }
    }
    kept
}

/// Read one class out of a raw detector output.
///
/// `shape` is the output tensor's shape; the boxes axis is whichever one is not `4 + classes`.
/// Coordinates are centre-x, centre-y, width, height in input pixels, as the common export
/// produces, and come back as fractions of `(in_w, in_h)`.
pub fn decode(
    raw: &[f32],
    shape: &[i64],
    classes: usize,
    class: usize,
    threshold: f32,
    in_w: f32,
    in_h: f32,
) -> Vec<Det> {
    let attrs = 4 + classes;
    let dims: Vec<usize> = shape.iter().map(|&d| d.max(0) as usize).collect();
    let (n, attrs_first) = match dims.as_slice() {
        [_, a, n] if *a == attrs => (*n, true),
        [_, n, a] if *a == attrs => (*n, false),
        [a, n] if *a == attrs => (*n, true),
        [n, a] if *a == attrs => (*n, false),
        _ => return Vec::new(),
    };
    let at = |i: usize, k: usize| -> f32 {
        let idx = if attrs_first {
            k * n + i
        } else {
            i * attrs + k
        };
        raw.get(idx).copied().unwrap_or(0.0)
    };
    let mut out = Vec::new();
    for i in 0..n {
        let score = at(i, 4 + class);
        if score < threshold {
            continue;
        }
        let (cx, cy, w, h) = (at(i, 0), at(i, 1), at(i, 2), at(i, 3));
        if w <= 0.0 || h <= 0.0 {
            continue;
        }
        out.push(Det {
            cx: (cx / in_w).clamp(0.0, 1.0),
            cy: (cy / in_h).clamp(0.0, 1.0),
            w: (w / in_w).clamp(0.0, 1.0),
            h: (h / in_h).clamp(0.0, 1.0),
            score,
        });
    }
    out
}

/// Boxes of one class in a picture. Errors when there is no model, which is not a failure: the
/// caller falls through to a person.
#[cfg(not(feature = "onnx-detect"))]
pub fn detect(_png: &[u8], _class: usize) -> Result<Vec<Det>> {
    Err(anyhow!(
        "object detector not compiled in (build with --features onnx-detect)"
    ))
}

#[cfg(feature = "onnx-detect")]
pub fn detect(png: &[u8], class: usize) -> Result<Vec<Det>> {
    imp::detect(png, class)
}

/// The strongest evidence of `class` anywhere in the picture, 0..1 — what a tile classifier
/// would say about "contains one". Reads every box, threshold zero, no suppression: the answer
/// is a score, not a set of boxes.
#[cfg(not(feature = "onnx-detect"))]
pub fn strongest(_png: &[u8], _class: usize) -> Result<f32> {
    Err(anyhow!(
        "object detector not compiled in (build with --features onnx-detect)"
    ))
}

#[cfg(feature = "onnx-detect")]
pub fn strongest(png: &[u8], class: usize) -> Result<f32> {
    imp::strongest(png, class)
}

#[cfg(feature = "onnx-detect")]
mod imp {
    use super::*;
    use crate::model_source::SessionCache;

    static SESSION: SessionCache = SessionCache::new();

    /// Run the model once and decode one class at `threshold`, unsuppressed.
    fn raw(png: &[u8], class: usize, threshold: Option<f32>) -> Result<(Vec<Det>, DetectConfig)> {
        let located = locate().ok_or_else(|| anyhow!("no detect model installed or embedded"))?;
        let cfg: DetectConfig = located.config()?;
        if class >= cfg.classes.len() {
            return Err(anyhow!(
                "class {class} is outside the model's {} classes",
                cfg.classes.len()
            ));
        }
        let threshold = threshold.unwrap_or(cfg.threshold);
        let img = image::load_from_memory(png)?;
        let (shape, data) =
            model_source::image_tensor(&img, cfg.width, cfg.height, cfg.channels, cfg.normalize);
        let dets = SESSION.with(&located, |sess| {
            let name = sess.inputs()[0].name().to_string();
            let input = ort::value::Tensor::from_array((shape, data))?;
            let outputs = sess.run(ort::inputs![name.as_str() => input])?;
            let (out_shape, raw) = outputs[0].try_extract_tensor::<f32>()?;
            let dims: Vec<i64> = out_shape.iter().copied().collect();
            Ok(decode(
                raw,
                &dims,
                cfg.classes.len(),
                class,
                threshold,
                cfg.width as f32,
                cfg.height as f32,
            ))
        })?;
        Ok((dets, cfg))
    }

    pub fn detect(png: &[u8], class: usize) -> Result<Vec<Det>> {
        let (dets, cfg) = raw(png, class, None)?;
        Ok(nms(dets, cfg.iou))
    }

    pub fn strongest(png: &[u8], class: usize) -> Result<f32> {
        let (dets, _) = raw(png, class, Some(0.0))?;
        Ok(dets
            .iter()
            .map(|d| d.score)
            .fold(0.0f32, f32::max)
            .clamp(0.0, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(cx: f32, cy: f32, w: f32, h: f32, score: f32) -> Det {
        Det {
            cx,
            cy,
            w,
            h,
            score,
        }
    }

    #[test]
    fn without_a_model_detection_is_an_error_and_never_a_guess() {
        if svipall_models::detect().is_some() {
            // An embedded detector is a model; this contract is about having none.
            return;
        }
        std::env::set_var(
            "SVIPALL_HOME",
            std::env::temp_dir().join("svipall-detect-none"),
        );
        assert!(!available());
        assert!(detect(b"not a png", 0).is_err());
        std::env::remove_var("SVIPALL_HOME");
    }

    #[test]
    fn nms_keeps_the_strongest_box_and_is_deterministic() {
        let boxes = vec![
            d(0.5, 0.5, 0.2, 0.2, 0.6),
            d(0.52, 0.5, 0.2, 0.2, 0.9),
            d(0.1, 0.1, 0.1, 0.1, 0.7),
            d(0.9, 0.9, 0.1, 0.1, 0.7),
        ];
        let kept = nms(boxes.clone(), 0.5);
        assert_eq!(kept.len(), 3);
        assert_eq!(kept[0].score, 0.9);
        // Equal scores fall back to position, so the order never depends on input order.
        let mut shuffled = boxes;
        shuffled.reverse();
        assert_eq!(nms(shuffled, 0.5), kept);
        assert_eq!(kept[1].cx, 0.1, "leftmost of the tied pair first");
    }

    #[test]
    fn both_output_layouts_decode_to_the_same_fractions() {
        // Two boxes, two classes: attrs = 6.
        let rows: [[f32; 6]; 2] = [
            [320.0, 320.0, 64.0, 32.0, 0.1, 0.8],
            [64.0, 32.0, 32.0, 32.0, 0.9, 0.05],
        ];
        let boxes_last: Vec<f32> = rows.iter().flatten().copied().collect();
        let mut attrs_first = vec![0f32; 12];
        for (i, r) in rows.iter().enumerate() {
            for (k, v) in r.iter().enumerate() {
                attrs_first[k * 2 + i] = *v;
            }
        }
        let a = decode(&boxes_last, &[1, 2, 6], 2, 1, 0.4, 640.0, 640.0);
        let b = decode(&attrs_first, &[1, 6, 2], 2, 1, 0.4, 640.0, 640.0);
        assert_eq!(a, b);
        assert_eq!(a.len(), 1, "only the class asked for, above threshold");
        assert!((a[0].cx - 0.5).abs() < 1e-6 && (a[0].w - 0.1).abs() < 1e-6);
        let other = decode(&boxes_last, &[1, 2, 6], 2, 0, 0.4, 640.0, 640.0);
        assert_eq!(other.len(), 1);
        assert!((other[0].cx - 0.1).abs() < 1e-6);
    }

    #[test]
    fn boxes_are_fractions_of_the_picture_never_pixels() {
        let raw = [1200.0, 700.0, 100.0, 50.0, 0.99];
        let out = decode(&raw, &[1, 1, 5], 1, 0, 0.5, 640.0, 640.0);
        assert_eq!(out.len(), 1);
        assert!(
            out[0].cx <= 1.0 && out[0].cy <= 1.0,
            "clamped, never a pixel count"
        );
        assert!(
            decode(&raw, &[1, 1, 7], 1, 0, 0.5, 640.0, 640.0).is_empty(),
            "a shape that does not fit the class count is refused"
        );
    }
}
