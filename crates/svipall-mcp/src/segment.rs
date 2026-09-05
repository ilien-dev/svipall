//! The 4x4 grid that is really one picture.
//!
//! reCAPTCHA's third kind of image challenge shows a single photograph cut into sixteen squares
//! and asks for "all squares with a fire hydrant". A tile classifier is the wrong tool for it — a
//! square that holds the bottom third of a hydrant is not a picture *of* a hydrant, and a
//! classifier trained on whole tiles scores it accordingly. The one peer-reviewed measurement of
//! this task (Plesner et al., COMPSAC 2024) segments the whole picture instead and marks every
//! cell the mask touches, and that is what this module does.
//!
//! Same arrangement as `detect`: inference behind `onnx-segment`, the model embedded or installed
//! as `segment.onnx` + `segment.json`, and without it the strategy declines. The model is a
//! semantic segmenter with one score plane per class, `[1, classes, H, W]` (logits or
//! probabilities), or a class-id plane `[1, H, W]`; both are read.
//!
//! Every cell index this module hands back is row-major in the widget's own order, and every
//! threshold is a fraction of a cell, never a pixel count.

use anyhow::{anyhow, Result};
use serde::Deserialize;

use crate::model_source::{self, Located};

/// The segmenter, wherever it lives.
pub fn locate() -> Option<Located> {
    model_source::locate("segment", "segment", "onnx", svipall_models::segment())
}

pub fn available() -> bool {
    cfg!(feature = "onnx-segment") && locate().is_some()
}

/// The sidecar that describes the model.
#[derive(Debug, Clone, Deserialize)]
pub struct SegmentConfig {
    pub height: u32,
    pub width: u32,
    #[serde(default = "three")]
    pub channels: u32,
    #[serde(default = "yes")]
    pub normalize: bool,
    /// Class names, in output-plane order. Plane 0 is usually "background".
    pub classes: Vec<String>,
    /// A pixel belongs to the class when its score clears this (score-plane models only).
    #[serde(default = "half")]
    pub threshold: f32,
    /// A cell is selected when at least this fraction of it is under the mask. Low on purpose:
    /// the question is "does this square contain any of it", and a hydrant's cap in one corner
    /// is a yes.
    #[serde(default = "min_overlap_default")]
    pub min_overlap: f32,
}

fn three() -> u32 {
    3
}
fn yes() -> bool {
    true
}
fn half() -> f32 {
    0.5
}
fn min_overlap_default() -> f32 {
    0.08
}

pub fn load_config() -> Result<SegmentConfig> {
    let located = locate().ok_or_else(|| anyhow!("no segment model installed or embedded"))?;
    let cfg: SegmentConfig = located.config()?;
    if cfg.classes.is_empty() {
        return Err(anyhow!("{} lists no classes", located.describe()));
    }
    Ok(cfg)
}

/// A boolean mask over the model's output plane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mask {
    pub width: usize,
    pub height: usize,
    pub on: Vec<bool>,
}

impl Mask {
    fn at(&self, x: usize, y: usize) -> bool {
        self.on.get(y * self.width + x).copied().unwrap_or(false)
    }

    /// How many pixels are on — a mask with none is a picture with none of the subject in it.
    pub fn coverage(&self) -> usize {
        self.on.iter().filter(|b| **b).count()
    }
}

/// Read one class's mask out of a raw segmentation output.
///
/// `[1, C, H, W]` (or `[C, H, W]`) is one score plane per class: the pixel is the class when its
/// plane clears `threshold` *and* is the largest of all planes — a plane that clears a low
/// threshold while another plane is far higher is not that class. `[1, H, W]` (or `[H, W]`) is a
/// plane of class ids. Anything else is refused, not reshaped.
pub fn decode(
    raw: &[f32],
    shape: &[i64],
    classes: usize,
    class: usize,
    threshold: f32,
) -> Option<Mask> {
    let dims: Vec<usize> = shape.iter().map(|&d| d.max(0) as usize).collect();
    match dims.as_slice() {
        [1, c, h, w] | [c, h, w] if *c == classes && *c > 1 => {
            let (c, h, w) = (*c, *h, *w);
            let plane = h * w;
            if raw.len() < c * plane || class >= c {
                return None;
            }
            let mut on = vec![false; plane];
            for i in 0..plane {
                let mine = raw[class * plane + i];
                if mine < threshold {
                    continue;
                }
                let best = (0..c).all(|k| k == class || raw[k * plane + i] <= mine);
                on[i] = best;
            }
            Some(Mask {
                width: w,
                height: h,
                on,
            })
        }
        [1, h, w] | [h, w] => {
            let (h, w) = (*h, *w);
            if raw.len() < h * w {
                return None;
            }
            let on = raw[..h * w]
                .iter()
                .map(|v| v.round() as i64 == class as i64)
                .collect();
            Some(Mask {
                width: w,
                height: h,
                on,
            })
        }
        _ => None,
    }
}

/// Which cells of a `rows` x `cols` grid the mask touches, row-major, most-covered first.
///
/// A cell counts when at least `min_overlap` of its area is on. Ordering by coverage is what the
/// clicks follow, for the same reason the tile classifier orders by score: a grid always clicked
/// top-left to bottom-right is its own signal.
pub fn cells_touched(mask: &Mask, rows: usize, cols: usize, min_overlap: f32) -> Vec<usize> {
    if rows == 0 || cols == 0 || mask.width == 0 || mask.height == 0 {
        return Vec::new();
    }
    let mut hits: Vec<(usize, f32)> = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            let x0 = c * mask.width / cols;
            let x1 = ((c + 1) * mask.width / cols).max(x0 + 1);
            let y0 = r * mask.height / rows;
            let y1 = ((r + 1) * mask.height / rows).max(y0 + 1);
            let mut on = 0usize;
            let mut total = 0usize;
            for y in y0..y1.min(mask.height) {
                for x in x0..x1.min(mask.width) {
                    total += 1;
                    if mask.at(x, y) {
                        on += 1;
                    }
                }
            }
            if total == 0 {
                continue;
            }
            let frac = on as f32 / total as f32;
            if frac >= min_overlap && on > 0 {
                hits.push((r * cols + c, frac));
            }
        }
    }
    hits.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    hits.into_iter().map(|(i, _)| i).collect()
}

/// The cells of a `rows` x `cols` grid drawn over `png` that hold `class`. Errors when there is
/// no model; the caller declines.
#[cfg(not(feature = "onnx-segment"))]
pub fn cells(_png: &[u8], _class: usize, _rows: usize, _cols: usize) -> Result<Vec<usize>> {
    Err(anyhow!(
        "segmenter not compiled in (build with --features onnx-segment)"
    ))
}

#[cfg(feature = "onnx-segment")]
pub fn cells(png: &[u8], class: usize, rows: usize, cols: usize) -> Result<Vec<usize>> {
    imp::cells(png, class, rows, cols)
}

#[cfg(feature = "onnx-segment")]
mod imp {
    use super::*;
    use crate::model_source::SessionCache;

    static SESSION: SessionCache = SessionCache::new();

    pub fn cells(png: &[u8], class: usize, rows: usize, cols: usize) -> Result<Vec<usize>> {
        let located = locate().ok_or_else(|| anyhow!("no segment model installed or embedded"))?;
        let cfg: SegmentConfig = located.config()?;
        if class >= cfg.classes.len() {
            return Err(anyhow!(
                "class {class} is outside the model's {} classes",
                cfg.classes.len()
            ));
        }
        let img = image::load_from_memory(png)?;
        let (shape, data) =
            model_source::image_tensor(&img, cfg.width, cfg.height, cfg.channels, cfg.normalize);
        SESSION.with(&located, |sess| {
            let name = sess.inputs()[0].name().to_string();
            let input = ort::value::Tensor::from_array((shape, data))?;
            let outputs = sess.run(ort::inputs![name.as_str() => input])?;
            let (out_shape, raw) = outputs[0].try_extract_tensor::<f32>()?;
            let dims: Vec<i64> = out_shape.iter().copied().collect();
            let mask =
                decode(raw, &dims, cfg.classes.len(), class, cfg.threshold).ok_or_else(|| {
                    anyhow!(
                        "segment output {dims:?} does not fit {} classes",
                        cfg.classes.len()
                    )
                })?;
            Ok(cells_touched(&mask, rows, cols, cfg.min_overlap))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mask with one rectangle on, in plane coordinates.
    fn rect_mask(w: usize, h: usize, x0: usize, y0: usize, x1: usize, y1: usize) -> Mask {
        let mut on = vec![false; w * h];
        for y in y0..y1 {
            for x in x0..x1 {
                on[y * w + x] = true;
            }
        }
        Mask {
            width: w,
            height: h,
            on,
        }
    }

    #[test]
    fn a_shape_in_the_top_left_quarter_touches_exactly_those_cells() {
        // 16x16 plane, 4x4 grid, so each cell is 4x4 pixels. A blob over pixels 0..8 x 0..8
        // covers cells (0,0) (0,1) (1,0) (1,1) completely and nothing else.
        let m = rect_mask(16, 16, 0, 0, 8, 8);
        let mut cells = cells_touched(&m, 4, 4, 0.08);
        cells.sort_unstable();
        assert_eq!(cells, vec![0, 1, 4, 5]);
    }

    #[test]
    fn a_sliver_in_a_cell_still_counts_because_the_question_is_contains_not_is() {
        // One pixel of a 4x4 cell is 1/16 = 0.0625 of it — below 0.08, so not selected — but two
        // pixels (0.125) are. The threshold is a fraction of the cell, never a pixel count.
        let one = rect_mask(16, 16, 5, 5, 6, 6);
        assert!(cells_touched(&one, 4, 4, 0.08).is_empty());
        let two = rect_mask(16, 16, 5, 5, 7, 6);
        assert_eq!(cells_touched(&two, 4, 4, 0.08), vec![5]);
    }

    #[test]
    fn the_most_covered_cell_comes_first_and_ties_are_stable() {
        // Cell 5 fully covered, cell 6 half covered.
        let mut m = rect_mask(16, 16, 4, 4, 8, 8);
        for y in 4..8 {
            for x in 8..10 {
                m.on[y * 16 + x] = true;
            }
        }
        assert_eq!(cells_touched(&m, 4, 4, 0.08), vec![5, 6]);
    }

    #[test]
    fn an_empty_mask_or_grid_selects_nothing() {
        let m = rect_mask(16, 16, 0, 0, 0, 0);
        assert!(cells_touched(&m, 4, 4, 0.08).is_empty());
        assert!(cells_touched(&rect_mask(16, 16, 0, 0, 16, 16), 0, 4, 0.08).is_empty());
        assert_eq!(m.coverage(), 0);
    }

    #[test]
    fn score_planes_pick_the_class_only_where_it_wins() {
        // 3 classes, 2x2 plane. Pixel 0: class 1 clears the threshold and wins. Pixel 1: class 1
        // clears it but class 2 is higher. Pixel 2: below threshold. Pixel 3: class 1 wins.
        let raw = [
            // class 0 plane
            0.1, 0.1, 0.9, 0.1, // class 1 plane
            0.8, 0.6, 0.05, 0.7, // class 2 plane
            0.1, 0.9, 0.05, 0.2,
        ];
        let m = decode(&raw, &[1, 3, 2, 2], 3, 1, 0.5).expect("decoded");
        assert_eq!(m.on, vec![true, false, false, true]);
        assert_eq!(m.coverage(), 2);
    }

    #[test]
    fn a_class_id_plane_is_read_directly() {
        let raw = [0.0, 2.0, 1.0, 2.0];
        let m = decode(&raw, &[1, 2, 2], 3, 2, 0.5).expect("decoded");
        assert_eq!(m.on, vec![false, true, false, true]);
    }

    #[test]
    fn a_shape_that_does_not_fit_the_class_count_is_refused_not_reshaped() {
        let raw = vec![0.0; 4 * 4];
        assert!(decode(&raw, &[1, 4, 2, 2], 3, 1, 0.5).is_none());
        assert!(decode(&raw, &[2, 2, 2, 2], 3, 1, 0.5).is_none());
        // Too few values for the declared shape.
        assert!(decode(&raw[..3], &[1, 2, 2], 3, 1, 0.5).is_none());
    }

    #[test]
    fn without_a_model_segmentation_is_an_error_and_never_a_guess() {
        if !available() {
            assert!(cells(b"not a png", 0, 4, 4).is_err());
        }
    }

    #[test]
    fn the_config_defaults_ask_for_containment_not_a_portrait() {
        let cfg: SegmentConfig =
            serde_json::from_str(r#"{"height":320,"width":320,"classes":["bg","bus"]}"#).unwrap();
        assert_eq!(cfg.channels, 3);
        assert!(
            cfg.min_overlap < 0.25,
            "a corner of a hydrant is still a hydrant"
        );
        assert!((cfg.threshold - 0.5).abs() < f32::EPSILON);
    }
}
