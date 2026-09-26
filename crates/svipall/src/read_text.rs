//! The text in a picture: an image `web_fetch` was handed, or a scanned page of a PDF.
//!
//! Two models the operator installs (`ocr_det.onnx`, `ocr_rec.onnx` and `ocr_rec.json` in
//! `~/.svipall/models/`, from the release's models archive or `tools/models/export_ocr.py`): a
//! detector that marks every pixel that belongs to a line of text, and a recogniser that reads one
//! line at a time. Nothing is downloaded at run time and nothing is sent anywhere.
//!
//! What is written here is what does not change with the models: turning the detector's map into
//! line boxes, cutting each line out, decoding the recogniser's output, and putting the lines back
//! in reading order. The lines are assumed straight, which scans and screenshots are; a page
//! photographed at an angle reads worse, and the result says nothing it did not read.
//!
//! This is not the captcha reader in `ocr`: that one reads a few distorted characters from a fixed
//! alphabet, this one reads a page.

use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::path::PathBuf;

pub fn det_path() -> PathBuf {
    crate::model_source::models_dir().join("ocr_det.onnx")
}

pub fn rec_path() -> PathBuf {
    crate::model_source::models_dir().join("ocr_rec.onnx")
}

pub fn config_path() -> PathBuf {
    crate::model_source::models_dir().join("ocr_rec.json")
}

/// True when this build can read text in pictures and the three files are where they belong.
pub fn available() -> bool {
    cfg!(feature = "onnx-read")
        && [det_path(), rec_path(), config_path()]
            .iter()
            .all(|p| p.is_file())
}

#[derive(Debug, Clone, Deserialize)]
pub struct DetConfig {
    /// The longer side is scaled down to at most this before detection.
    pub limit_side: u32,
    pub mean: [f32; 3],
    pub std: [f32; 3],
    /// A pixel at or above this probability is text.
    pub threshold: f32,
    /// A region whose mean probability is below this is dropped.
    pub box_threshold: f32,
    /// How far a region is grown back out, as a share of its area over its perimeter: the
    /// detector is trained on shrunk boxes.
    pub unclip: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecConfig {
    pub height: u32,
    pub max_width: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReadConfig {
    /// Class `i` of the recogniser's output; class 0 is the CTC blank.
    pub charset: Vec<String>,
    pub det: DetConfig,
    pub rec: RecConfig,
}

pub fn load_config() -> Result<ReadConfig> {
    let text = std::fs::read_to_string(config_path())
        .map_err(|e| anyhow!("no text reader at {}: {e}", config_path().display()))?;
    Ok(serde_json::from_str(&text)?)
}

/// A line of text found on the page, in the page's own pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Region {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    /// Mean text probability inside it.
    pub score: f32,
}

impl Region {
    fn height(&self) -> f32 {
        self.y1 - self.y0
    }
    fn center_y(&self) -> f32 {
        (self.y0 + self.y1) / 2.0
    }
}

/// The detector's map, `w` by `h`, turned into text regions: pixels over the threshold, joined by
/// 8-connectivity, kept when their mean probability clears `box_threshold`, then grown back out by
/// the unclip ratio. Coordinates are in the map's pixels.
pub fn regions(prob: &[f32], w: usize, h: usize, cfg: &DetConfig) -> Vec<Region> {
    let mut seen = vec![false; w * h];
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for start in 0..w * h {
        if seen[start] || prob[start] < cfg.threshold {
            continue;
        }
        seen[start] = true;
        stack.push(start);
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
        let (mut sum, mut n) = (0f32, 0usize);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
            sum += prob[i];
            n += 1;
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        continue;
                    }
                    let j = ny as usize * w + nx as usize;
                    if !seen[j] && prob[j] >= cfg.threshold {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        let score = sum / n as f32;
        // A speck is noise, not a line: the detector's smallest real box is a few pixels tall.
        if score < cfg.box_threshold || x1 - x0 < 2 || y1 - y0 < 2 {
            continue;
        }
        let (bw, bh) = ((x1 - x0 + 1) as f32, (y1 - y0 + 1) as f32);
        let grow = bw * bh * cfg.unclip / (2.0 * (bw + bh));
        out.push(Region {
            x0: (x0 as f32 - grow).max(0.0),
            y0: (y0 as f32 - grow).max(0.0),
            x1: (x1 as f32 + 1.0 + grow).min(w as f32),
            y1: (y1 as f32 + 1.0 + grow).min(h as f32),
            score,
        });
    }
    out
}

/// Regions in reading order, grouped into lines: top to bottom, and left to right within a line.
/// Two regions share a line when they overlap vertically by more than half the shorter one.
pub fn lines(mut regions: Vec<Region>) -> Vec<Vec<Region>> {
    regions.sort_by(|a, b| a.center_y().total_cmp(&b.center_y()));
    let mut lines: Vec<Vec<Region>> = Vec::new();
    for r in regions {
        let joins = lines.last().is_some_and(|line| {
            let last = line[line.len() - 1];
            let overlap = r.y1.min(last.y1) - r.y0.max(last.y0);
            overlap > 0.5 * r.height().min(last.height())
        });
        if joins {
            lines.last_mut().expect("checked").push(r);
        } else {
            lines.push(vec![r]);
        }
    }
    for line in &mut lines {
        line.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    }
    lines
}

/// Lines of text back into prose: words joined by spaces, lines by newlines, and a blank line
/// where the gap above a line is much taller than a line is, which is where a paragraph ends.
pub fn assemble(lines: &[(f32, f32, String)]) -> String {
    let mut heights: Vec<f32> = lines.iter().map(|(t, b, _)| b - t).collect();
    heights.sort_by(f32::total_cmp);
    let typical = heights.get(heights.len() / 2).copied().unwrap_or(0.0);
    let mut out = String::new();
    let mut prev_bottom: Option<f32> = None;
    for (top, bottom, text) in lines {
        if text.trim().is_empty() {
            continue;
        }
        if let Some(pb) = prev_bottom {
            out.push('\n');
            if typical > 0.0 && top - pb > typical * 1.2 {
                out.push('\n');
            }
        }
        out.push_str(text.trim());
        prev_bottom = Some(*bottom);
    }
    out
}

/// The text in an image file, or an error saying why there is none.
pub fn read(bytes: &[u8], deadline: std::time::Instant) -> Result<String> {
    let img = image::load_from_memory(bytes)
        .map_err(|e| anyhow!("not an image this build can open: {e}"))?;
    read_image(&img, deadline)
}

/// The text in a picture. Bounded by `deadline`: a page with hundreds of lines is read from the
/// top, and what was read comes back.
#[cfg(not(feature = "onnx-read"))]
pub fn read_image(_img: &image::DynamicImage, _deadline: std::time::Instant) -> Result<String> {
    Err(anyhow!(
        "this build cannot read text in pictures: it was compiled without the onnx-read feature"
    ))
}

#[cfg(feature = "onnx-read")]
pub fn read_image(img: &image::DynamicImage, deadline: std::time::Instant) -> Result<String> {
    imp::read(img, deadline)
}

/// What reading a scanned PDF found: its text with page markers, how many pages were read, and
/// the pages that could not be, with why.
pub struct PdfReading {
    pub text: String,
    pub pages_read: usize,
    pub skipped: Vec<String>,
}

/// Read each page's picture in a scanned PDF, up to `max_pages` and the deadline.
pub fn read_pdf(
    bytes: &[u8],
    max_pages: usize,
    deadline: std::time::Instant,
) -> Result<PdfReading> {
    use svipall_core::pdf_images::{page_images, PageImage};
    let mut out = PdfReading {
        text: String::new(),
        pages_read: 0,
        skipped: Vec::new(),
    };
    for (number, picture) in page_images(bytes, max_pages)? {
        if std::time::Instant::now() > deadline {
            out.skipped
                .push(format!("page {number} onwards: out of time"));
            break;
        }
        let img = match picture {
            PageImage::Encoded(b) => image::load_from_memory(&b).ok(),
            PageImage::Pixels {
                width,
                height,
                channels,
                data,
            } => {
                let n = width as usize * height as usize * channels as usize;
                match channels {
                    1 => image::GrayImage::from_raw(width, height, data[..n].to_vec())
                        .map(image::DynamicImage::ImageLuma8),
                    _ => image::RgbImage::from_raw(width, height, data[..n].to_vec())
                        .map(image::DynamicImage::ImageRgb8),
                }
            }
            PageImage::Unsupported(why) => {
                out.skipped.push(format!("page {number}: {why}"));
                continue;
            }
        };
        let Some(img) = img else {
            out.skipped
                .push(format!("page {number}: the image did not decode"));
            continue;
        };
        let text = read_image(&img, deadline)?;
        if !out.text.is_empty() {
            out.text.push_str("\n\n");
        }
        out.text.push_str(&format!("*page {number}*\n\n{text}"));
        out.pages_read += 1;
    }
    Ok(out)
}

#[cfg(feature = "onnx-read")]
mod imp {
    use super::*;
    use crate::model_source::{Located, SessionCache};

    static DET: SessionCache = SessionCache::new();
    static REC: SessionCache = SessionCache::new();

    fn located(name: &'static str, path: PathBuf) -> Result<Located> {
        let sidecar = std::fs::read_to_string(config_path())?;
        Located::disk(name, path.clone(), sidecar)
            .ok_or_else(|| anyhow!("no text reader at {}", path.display()))
    }

    /// The side lengths the detector is run at: the longer side at most `limit`, both rounded to
    /// a multiple of 32, which its down- and up-sampling needs.
    pub(super) fn det_size(w: u32, h: u32, limit: u32) -> (u32, u32) {
        let scale = (limit as f32 / w.max(h) as f32).min(1.0);
        let round = |v: u32| (((v as f32 * scale) / 32.0).round() as u32).max(1) * 32;
        (round(w), round(h))
    }

    fn detect(img: &image::DynamicImage, cfg: &DetConfig) -> Result<Vec<Region>> {
        let (w, h) = det_size(img.width(), img.height(), cfg.limit_side);
        let (shape, mut data) = crate::model_source::image_tensor(img, w, h, 3, true);
        let plane = (w * h) as usize;
        for c in 0..3 {
            for v in &mut data[c * plane..(c + 1) * plane] {
                *v = (*v - cfg.mean[c]) / cfg.std[c];
            }
        }
        let prob = DET.with(&located("ocr_det", det_path())?, |sess| {
            let name = sess.inputs()[0].name().to_string();
            let input = ort::value::Tensor::from_array((shape, data))?;
            let out = sess.run(ort::inputs![name.as_str() => input])?;
            let (_, v) = out[0].try_extract_tensor::<f32>()?;
            Ok(v.to_vec())
        })?;
        if prob.len() != plane {
            return Err(anyhow!(
                "the detector answered {} values for {plane} pixels",
                prob.len()
            ));
        }
        let (sx, sy) = (
            img.width() as f32 / w as f32,
            img.height() as f32 / h as f32,
        );
        Ok(regions(&prob, w as usize, h as usize, cfg)
            .into_iter()
            .map(|r| Region {
                x0: r.x0 * sx,
                y0: r.y0 * sy,
                x1: r.x1 * sx,
                y1: r.y1 * sy,
                ..r
            })
            .collect())
    }

    fn recognise(
        img: &image::DynamicImage,
        r: &Region,
        cfg: &RecConfig,
        charset: &[char],
    ) -> Result<String> {
        let (x, y) = (r.x0.floor() as u32, r.y0.floor() as u32);
        let cw = (r.x1.ceil() as u32)
            .min(img.width())
            .saturating_sub(x)
            .max(1);
        let ch = (r.y1.ceil() as u32)
            .min(img.height())
            .saturating_sub(y)
            .max(1);
        let crop = img.crop_imm(x, y, cw, ch);
        let width = ((cfg.height as f32 * cw as f32 / ch as f32).ceil() as u32)
            .clamp(cfg.height / 2, cfg.max_width);
        let (shape, mut data) =
            crate::model_source::image_tensor(&crop, width, cfg.height, 3, true);
        for v in &mut data {
            *v = (*v - 0.5) / 0.5;
        }
        REC.with(&located("ocr_rec", rec_path())?, |sess| {
            let name = sess.inputs()[0].name().to_string();
            let input = ort::value::Tensor::from_array((shape, data))?;
            let out = sess.run(ort::inputs![name.as_str() => input])?;
            let (s, v) = out[0].try_extract_tensor::<f32>()?;
            let classes = *s.last().ok_or_else(|| anyhow!("empty recogniser output"))? as usize;
            let best: Vec<usize> = v.chunks(classes).map(crate::model_source::argmax).collect();
            Ok(crate::ocr::ctc_decode(&best, charset))
        })
    }

    pub fn read(img: &image::DynamicImage, deadline: std::time::Instant) -> Result<String> {
        let cfg = load_config()?;
        let charset: Vec<char> = cfg
            .charset
            .iter()
            .map(|s| s.chars().next().unwrap_or('\0'))
            .collect();
        let found = detect(img, &cfg.det)?;
        let mut read_lines = Vec::new();
        for line in lines(found) {
            if std::time::Instant::now() > deadline {
                break;
            }
            let top = line.iter().map(|r| r.y0).fold(f32::MAX, f32::min);
            let bottom = line.iter().map(|r| r.y1).fold(0.0, f32::max);
            let words: Vec<String> = line
                .iter()
                .map(|r| recognise(img, r, &cfg.rec, &charset))
                .collect::<Result<_>>()?;
            read_lines.push((top, bottom, words.join(" ")));
        }
        Ok(assemble(&read_lines))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> DetConfig {
        DetConfig {
            limit_side: 960,
            mean: [0.485, 0.456, 0.406],
            std: [0.229, 0.224, 0.225],
            threshold: 0.3,
            box_threshold: 0.6,
            unclip: 1.5,
        }
    }

    /// A `w` by `h` map with the given rectangles filled at probability 0.9.
    fn map(w: usize, h: usize, rects: &[(usize, usize, usize, usize)]) -> Vec<f32> {
        let mut m = vec![0.0; w * h];
        for &(x0, y0, x1, y1) in rects {
            for y in y0..y1 {
                for x in x0..x1 {
                    m[y * w + x] = 0.9;
                }
            }
        }
        m
    }

    #[test]
    fn each_blob_of_text_becomes_one_region_grown_back_out() {
        let m = map(
            100,
            60,
            &[(10, 10, 50, 20), (60, 10, 90, 20), (10, 40, 70, 50)],
        );
        let r = regions(&m, 100, 60, &cfg());
        assert_eq!(r.len(), 3);
        let first = r.iter().find(|r| r.x0 < 10.0 && r.y0 < 10.0).unwrap();
        assert!(first.x1 > 50.0 && first.y1 > 20.0, "grown: {first:?}");
        assert!((first.score - 0.9).abs() < 1e-4, "{first:?}");
    }

    #[test]
    fn specks_and_faint_regions_are_not_text() {
        let mut m = map(50, 50, &[(5, 5, 6, 6)]);
        for v in m.iter_mut().skip(20 * 50).take(10 * 50) {
            *v = 0.4;
        }
        assert!(regions(&m, 50, 50, &cfg()).is_empty());
    }

    #[test]
    fn regions_are_read_top_to_bottom_and_left_to_right() {
        let r = |x0: f32, y0: f32, x1: f32, y1: f32| Region {
            x0,
            y0,
            x1,
            y1,
            score: 1.0,
        };
        let got = lines(vec![
            r(60.0, 10.0, 90.0, 22.0),
            r(10.0, 40.0, 70.0, 52.0),
            r(10.0, 11.0, 50.0, 21.0),
        ]);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0][0].x0, 10.0);
        assert_eq!(got[0][1].x0, 60.0);
        assert_eq!(got[1][0].y0, 40.0);
    }

    #[test]
    fn a_tall_gap_between_lines_is_a_paragraph_break() {
        let text = assemble(&[
            (0.0, 10.0, "First line".into()),
            (12.0, 22.0, "same paragraph".into()),
            (60.0, 70.0, "New paragraph".into()),
        ]);
        assert_eq!(text, "First line\nsame paragraph\n\nNew paragraph");
    }

    #[cfg(feature = "onnx-read")]
    #[test]
    fn the_detector_is_run_at_multiples_of_32_within_the_limit() {
        assert_eq!(imp::det_size(1000, 560, 960), (960, 544));
        assert_eq!(imp::det_size(100, 20, 960), (96, 32));
    }
}
