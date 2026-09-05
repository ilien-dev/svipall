//! Local OCR for image captchas. No network, no third-party API.
//!
//! When an ONNX model is embedded, or present at `~/.svipall/models/captcha.onnx` with a sidecar
//! `captcha.json` describing input size and charset, it is run with a CRNN + CTC decode. Without
//! one, OCR is unavailable and the caller routes the captcha to the human dashboard, which renders
//! the image for a person to read. Enable the `onnx-ocr` build feature to compile the inference
//! path in.

use anyhow::{anyhow, Result};
use std::path::PathBuf;

use crate::model_source::{self, Located};

pub fn models_dir() -> PathBuf {
    model_source::models_dir()
}

/// The OCR model, wherever it lives. On disk it keeps its historical name, `captcha.onnx`.
pub fn locate() -> Option<Located> {
    model_source::locate("ocr", "captcha", "onnx", svipall_models::ocr())
}

/// True when a usable OCR model is installed.
pub fn available() -> bool {
    cfg!(feature = "onnx-ocr") && locate().is_some()
}

/// Solve an image captcha from base64 (or a raw image byte string). Returns recognized text.
/// Errors when no model is installed or inference is not compiled in — the caller then falls
/// back to the human dashboard.
pub fn solve(image_b64: &str) -> Result<String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let bytes = STANDARD
        .decode(image_b64.trim())
        .unwrap_or_else(|_| image_b64.as_bytes().to_vec());
    solve_bytes(&bytes)
}

#[cfg(not(feature = "onnx-ocr"))]
pub fn solve_bytes(_bytes: &[u8]) -> Result<String> {
    Err(anyhow!(
        "OCR not compiled in (build with --features onnx-ocr)"
    ))
}

#[cfg(feature = "onnx-ocr")]
pub fn solve_bytes(bytes: &[u8]) -> Result<String> {
    imp::solve_bytes(bytes)
}

/// Model input + charset contract (sidecar `captcha.json`).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct OcrConfig {
    /// Input height the model expects.
    pub height: u32,
    /// Input width the model expects.
    pub width: u32,
    /// Characters the final axis maps to; index 0 is the CTC blank.
    pub charset: String,
    /// Grayscale (1 channel) or RGB (3). Default 1.
    #[serde(default = "one")]
    pub channels: u32,
    /// Normalize pixels to [0,1] (default) or keep [0,255].
    #[serde(default = "yes")]
    pub normalize: bool,
}

fn one() -> u32 {
    1
}
fn yes() -> bool {
    true
}

pub fn load_config() -> Result<OcrConfig> {
    locate()
        .ok_or_else(|| anyhow!("no OCR model installed or embedded"))?
        .config()
}

/// Greedy CTC decode: collapse repeats, drop the blank (index 0).
pub fn ctc_decode(best_per_step: &[usize], charset: &[char]) -> String {
    let mut out = String::new();
    let mut prev = usize::MAX;
    for &idx in best_per_step {
        if idx != prev && idx != 0 {
            if let Some(&c) = charset.get(idx) {
                out.push(c);
            }
        }
        prev = idx;
    }
    out
}

#[cfg(feature = "onnx-ocr")]
mod imp {
    use super::*;
    use crate::model_source::SessionCache;

    static SESSION: SessionCache = SessionCache::new();

    pub fn solve_bytes(bytes: &[u8]) -> Result<String> {
        let located = locate().ok_or_else(|| anyhow!("no OCR model installed or embedded"))?;
        let cfg: OcrConfig = located.config()?;
        let charset: Vec<char> = cfg.charset.chars().collect();
        let img = image::load_from_memory(bytes)?;
        let (shape, data) =
            model_source::image_tensor(&img, cfg.width, cfg.height, cfg.channels, cfg.normalize);
        SESSION.with(&located, |sess| {
            let name = sess.inputs()[0].name().to_string();
            let input = ort::value::Tensor::from_array((shape, data))?;
            let outputs = sess.run(ort::inputs![name.as_str() => input])?;
            let (out_shape, out) = outputs[0].try_extract_tensor::<f32>()?;

            // Expect [T, C] or [1, T, C]; classes are the last axis.
            let dims: Vec<usize> = out_shape.iter().map(|d| *d as usize).collect();
            let classes = *dims.last().ok_or_else(|| anyhow!("empty OCR output"))?;
            let steps = out.len() / classes.max(1);
            let best: Vec<usize> = (0..steps)
                .map(|t| model_source::argmax(&out[t * classes..(t + 1) * classes]))
                .collect();
            Ok(ctc_decode(&best, &charset))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctc_collapses_repeats_and_blanks() {
        let charset: Vec<char> = "-abc".chars().collect(); // index 0 '-' is blank
                                                           // a a blank a b b -> "aab"
        assert_eq!(ctc_decode(&[1, 1, 0, 1, 2, 2], &charset), "aab");
    }
}
