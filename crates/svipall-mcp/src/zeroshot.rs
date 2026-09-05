//! The grid asks for something the classifier was never taught. Before that becomes a person's
//! problem, ask a model that was taught everything at once.
//!
//! A contrastive image-text pair — an image encoder and a text encoder producing vectors in one
//! space — scores each tile against "a photo of a {subject}" versus "a photo of something else".
//! Both encoders are ONNX files the operator installs (`clip_image.onnx`, `clip_text.onnx`,
//! `clip.json`, and the tokenizer's `vocab.json` and `merges.txt`); nothing is downloaded.
//!
//! The tokenizer is implemented here rather than pulled in: it is byte-level BPE with a fixed
//! start and end token, ninety lines that do not change, and the alternative is a dependency
//! tree bigger than the rest of this crate.
//!
//! The answer is accepted only when the tiles disagree with each other: a grid where every tile
//! scores the same is a grid the model cannot read, and guessing spends one of two attempts and
//! tells the page exactly what we are. That case declines, as the classifier does.

use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

pub fn image_model_path() -> PathBuf {
    crate::ocr::models_dir().join("clip_image.onnx")
}

pub fn text_model_path() -> PathBuf {
    crate::ocr::models_dir().join("clip_text.onnx")
}

pub fn config_path() -> PathBuf {
    crate::ocr::models_dir().join("clip.json")
}

pub fn available() -> bool {
    cfg!(feature = "onnx-zeroshot")
        && image_model_path().is_file()
        && text_model_path().is_file()
        && config_path().is_file()
}

#[derive(Debug, Clone, Deserialize)]
pub struct ZeroShotConfig {
    pub height: u32,
    pub width: u32,
    /// Per-channel mean and standard deviation the image encoder was trained with.
    pub mean: [f32; 3],
    pub std: [f32; 3],
    /// Tokenizer files, relative to the models directory.
    pub vocab: String,
    pub merges: String,
    /// Token context length of the text encoder.
    #[serde(default = "seventy_seven")]
    pub context: usize,
    /// Minimum spread between the best and worst tile for the answer to count.
    #[serde(default = "point_fifteen")]
    pub margin: f32,
    /// Probability above which a tile is selected.
    #[serde(default = "half")]
    pub threshold: f32,
}

fn seventy_seven() -> usize {
    77
}
fn point_fifteen() -> f32 {
    0.15
}
fn half() -> f32 {
    0.5
}

pub fn load_config() -> Result<ZeroShotConfig> {
    let text = std::fs::read_to_string(config_path())
        .map_err(|e| anyhow!("no clip.json at {}: {e}", config_path().display()))?;
    Ok(serde_json::from_str(&text)?)
}

/// Byte-level BPE, as the contrastive models ship it: every byte maps to a printable character,
/// words are split by a fixed pattern, merges apply by rank, `</w>` marks a word's end.
pub struct Tokenizer {
    encoder: HashMap<String, u32>,
    ranks: HashMap<(String, String), usize>,
    byte_to_char: HashMap<u8, char>,
    start: u32,
    end: u32,
}

impl Tokenizer {
    pub fn load(vocab: &std::path::Path, merges: &std::path::Path) -> Result<Self> {
        let vocab_text = std::fs::read_to_string(vocab)?;
        let encoder: HashMap<String, u32> = serde_json::from_str(&vocab_text)?;
        let merges_text = std::fs::read_to_string(merges)?;
        Self::from_parts(encoder, &merges_text)
    }

    pub fn from_parts(encoder: HashMap<String, u32>, merges: &str) -> Result<Self> {
        let ranks = merges
            .lines()
            .filter(|l| !l.starts_with("#version") && !l.trim().is_empty())
            .enumerate()
            .filter_map(|(i, l)| {
                let mut it = l.split_whitespace();
                Some(((it.next()?.to_string(), it.next()?.to_string()), i))
            })
            .collect();
        let start = *encoder
            .get("<|startoftext|>")
            .ok_or_else(|| anyhow!("vocab has no start token"))?;
        let end = *encoder
            .get("<|endoftext|>")
            .ok_or_else(|| anyhow!("vocab has no end token"))?;
        Ok(Self {
            encoder,
            ranks,
            byte_to_char: bytes_to_unicode(),
            start,
            end,
        })
    }

    fn bpe(&self, word: &str) -> Vec<String> {
        let mut parts: Vec<String> = word.chars().map(|c| c.to_string()).collect();
        if let Some(last) = parts.last_mut() {
            last.push_str("</w>");
        }
        loop {
            let mut best: Option<(usize, usize)> = None;
            for i in 0..parts.len().saturating_sub(1) {
                if let Some(&r) = self.ranks.get(&(parts[i].clone(), parts[i + 1].clone())) {
                    if best.is_none_or(|(br, _)| r < br) {
                        best = Some((r, i));
                    }
                }
            }
            let Some((_, i)) = best else { break };
            let merged = format!("{}{}", parts[i], parts[i + 1]);
            parts.splice(i..i + 2, [merged]);
        }
        parts
    }

    /// Token ids for a prompt, padded with the end token to `context` and truncated to fit.
    pub fn encode(&self, text: &str, context: usize) -> Vec<i64> {
        let mut ids = vec![self.start as i64];
        for word in words(&text.to_lowercase()) {
            let mapped: String = word
                .bytes()
                .map(|b| self.byte_to_char.get(&b).copied().unwrap_or('?'))
                .collect();
            for piece in self.bpe(&mapped) {
                if let Some(id) = self.encoder.get(&piece) {
                    ids.push(*id as i64);
                }
            }
        }
        ids.truncate(context.saturating_sub(1).max(1));
        ids.push(self.end as i64);
        ids.resize(context, self.end as i64);
        ids
    }
}

/// The word pattern: contractions, words, numbers, and runs of anything else.
fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut kind = 0u8; // 0 none, 1 letters, 2 digits, 3 other
    for c in text.chars() {
        let k = if c.is_whitespace() {
            0
        } else if c.is_alphabetic() {
            1
        } else if c.is_numeric() {
            2
        } else {
            3
        };
        if k != kind || k == 2 {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            kind = k;
        }
        if k != 0 {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The byte-to-printable table the reference tokenizer uses: printable ASCII and two Latin
/// ranges keep themselves, everything else is shifted above U+0100.
fn bytes_to_unicode() -> HashMap<u8, char> {
    let mut bs: Vec<u32> = (b'!' as u32..=b'~' as u32)
        .chain(0xA1..=0xAC)
        .chain(0xAE..=0xFF)
        .collect();
    let mut cs = bs.clone();
    let mut n = 0u32;
    for b in 0..=255u32 {
        if !bs.contains(&b) {
            bs.push(b);
            cs.push(256 + n);
            n += 1;
        }
    }
    bs.into_iter()
        .zip(cs)
        .filter_map(|(b, c)| Some((b as u8, char::from_u32(c)?)))
        .collect()
}

/// Cosine similarity, which is what the two encoders are trained to make meaningful.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

/// Probability that each tile shows the subject rather than something else, from the two
/// similarities, with the temperature the reference models use.
pub fn probabilities(subject_sims: &[f32], other_sims: &[f32]) -> Vec<f32> {
    subject_sims
        .iter()
        .zip(other_sims)
        .map(|(s, o)| {
            let a = (100.0 * s).exp();
            let b = (100.0 * o).exp();
            a / (a + b)
        })
        .collect()
}

/// Tiles that show the subject, or `None` when the grid does not separate — the same spread rule
/// applies whichever model produced the probabilities.
pub fn select(probs: &[f32], threshold: f32, margin: f32) -> Option<Vec<usize>> {
    let best = probs.iter().copied().fold(f32::MIN, f32::max);
    let worst = probs.iter().copied().fold(f32::MAX, f32::min);
    if probs.is_empty() || best - worst < margin {
        return None;
    }
    let mut picked: Vec<(usize, f32)> = probs
        .iter()
        .enumerate()
        .filter(|(_, p)| **p >= threshold)
        .map(|(i, p)| (i, *p))
        .collect();
    picked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    Some(picked.into_iter().map(|(i, _)| i).collect())
}

/// Score every tile for a subject named in prose. `None` means the model could not tell the
/// tiles apart; `Err` means there is no model.
#[cfg(not(feature = "onnx-zeroshot"))]
pub fn pick_tiles(_tiles: &[Vec<u8>], _subject: &str) -> Result<Option<Vec<usize>>> {
    Err(anyhow!(
        "zero-shot model not compiled in (build with --features onnx-zeroshot and install ~/.svipall/models/clip_*.onnx)"
    ))
}

#[cfg(feature = "onnx-zeroshot")]
pub fn pick_tiles(tiles: &[Vec<u8>], subject: &str) -> Result<Option<Vec<usize>>> {
    imp::pick_tiles(tiles, subject)
}

#[cfg(feature = "onnx-zeroshot")]
mod imp {
    use super::*;
    use crate::model_source::{Located, SessionCache};
    use image::imageops::FilterType;
    use once_cell::sync::OnceCell;

    static IMAGE: SessionCache = SessionCache::new();
    static TEXT: SessionCache = SessionCache::new();
    static TOKENIZER: OnceCell<Tokenizer> = OnceCell::new();

    /// The pair shares one sidecar, which `model_source::locate` cannot express, so each half is
    /// located by hand; the cache still notices a swapped file.
    fn located(name: &'static str, path: PathBuf) -> Result<Located> {
        let sidecar = std::fs::read_to_string(config_path())?;
        Located::disk(name, path.clone(), sidecar)
            .ok_or_else(|| anyhow!("no zero-shot model at {}", path.display()))
    }

    fn embed_text(cfg: &ZeroShotConfig, prompt: &str) -> Result<Vec<f32>> {
        let tok = TOKENIZER.get_or_try_init(|| {
            let dir = crate::ocr::models_dir();
            Tokenizer::load(&dir.join(&cfg.vocab), &dir.join(&cfg.merges))
        })?;
        let ids = tok.encode(prompt, cfg.context);
        let shape = [1i64, cfg.context as i64];
        TEXT.with(&located("clip_text", text_model_path())?, |sess| {
            let name = sess.inputs()[0].name().to_string();
            let input = ort::value::Tensor::from_array((shape, ids))?;
            let outputs = sess.run(ort::inputs![name.as_str() => input])?;
            let (_, v) = outputs[0].try_extract_tensor::<f32>()?;
            Ok(v.to_vec())
        })
    }

    fn embed_image(cfg: &ZeroShotConfig, png: &[u8]) -> Result<Vec<f32>> {
        let img = image::load_from_memory(png)?;
        let resized = img.resize_exact(cfg.width, cfg.height, FilterType::Triangle);
        let plane = cfg.height as usize * cfg.width as usize;
        let mut data = vec![0f32; 3 * plane];
        for (i, p) in resized.to_rgb8().pixels().enumerate() {
            for c in 0..3 {
                data[c * plane + i] = (p.0[c] as f32 / 255.0 - cfg.mean[c]) / cfg.std[c];
            }
        }
        let shape = [1i64, 3, cfg.height as i64, cfg.width as i64];
        IMAGE.with(&located("clip_image", image_model_path())?, |sess| {
            let name = sess.inputs()[0].name().to_string();
            let input = ort::value::Tensor::from_array((shape, data))?;
            let outputs = sess.run(ort::inputs![name.as_str() => input])?;
            let (_, v) = outputs[0].try_extract_tensor::<f32>()?;
            Ok(v.to_vec())
        })
    }

    pub fn pick_tiles(tiles: &[Vec<u8>], subject: &str) -> Result<Option<Vec<usize>>> {
        if !available() {
            return Err(anyhow!("zero-shot models are not installed"));
        }
        let cfg = load_config()?;
        let yes = embed_text(&cfg, &format!("a photo of a {subject}"))?;
        let no = embed_text(&cfg, "a photo of something else")?;
        let mut subject_sims = Vec::with_capacity(tiles.len());
        let mut other_sims = Vec::with_capacity(tiles.len());
        for t in tiles {
            let e = embed_image(&cfg, t)?;
            subject_sims.push(cosine(&e, &yes));
            other_sims.push(cosine(&e, &no));
        }
        let probs = probabilities(&subject_sims, &other_sims);
        Ok(select(&probs, cfg.threshold, cfg.margin))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toy_tokenizer() -> Tokenizer {
        // A vocabulary just big enough for "a bus": single characters, one merge, and the two
        // special tokens.
        let mut vocab: HashMap<String, u32> = HashMap::new();
        for (i, s) in ["a</w>", "b", "u", "s</w>", "us</w>", "bus</w>"]
            .iter()
            .enumerate()
        {
            vocab.insert((*s).to_string(), i as u32);
        }
        vocab.insert("<|startoftext|>".into(), 100);
        vocab.insert("<|endoftext|>".into(), 101);
        Tokenizer::from_parts(vocab, "#version: 0.2\nu s</w>\nb us</w>\n").unwrap()
    }

    #[test]
    fn merges_apply_by_rank_and_the_prompt_is_framed_and_padded() {
        let t = toy_tokenizer();
        let ids = t.encode("A bus", 8);
        assert_eq!(ids, vec![100, 0, 5, 101, 101, 101, 101, 101]);
    }

    #[test]
    fn a_long_prompt_is_cut_to_the_context_and_still_ends_properly() {
        let t = toy_tokenizer();
        let ids = t.encode("a a a a a a a a a a", 4);
        assert_eq!(ids.len(), 4);
        assert_eq!(ids[0], 100);
        assert_eq!(*ids.last().unwrap(), 101);
    }

    #[test]
    fn words_split_the_way_the_reference_pattern_does() {
        assert_eq!(
            words("select all fire-hydrants 2x"),
            vec!["select", "all", "fire", "-", "hydrants", "2", "x"]
        );
    }

    #[test]
    fn every_byte_has_a_printable_stand_in() {
        let m = bytes_to_unicode();
        assert_eq!(m.len(), 256);
        assert_eq!(m[&b'a'], 'a');
        assert_eq!(m[&b' '], '\u{120}', "space is shifted above U+0100");
    }

    #[test]
    fn a_grid_that_does_not_separate_is_declined_not_guessed() {
        assert_eq!(select(&[0.51, 0.52, 0.50, 0.53], 0.5, 0.15), None);
        assert_eq!(select(&[], 0.5, 0.15), None);
        assert_eq!(select(&[0.9, 0.1, 0.8, 0.2], 0.5, 0.15), Some(vec![0, 2]));
    }

    #[test]
    fn probabilities_favour_the_closer_prompt() {
        let p = probabilities(&[0.30, 0.20], &[0.20, 0.30]);
        assert!(p[0] > 0.99 && p[1] < 0.01, "{p:?}");
    }

    #[test]
    fn without_a_model_zero_shot_is_an_error_and_never_a_guess() {
        std::env::set_var(
            "SVIPALL_HOME",
            std::env::temp_dir().join("svipall-zeroshot-none"),
        );
        assert!(!available());
        assert!(pick_tiles(&[b"x".to_vec()], "bus").is_err());
    }
}

#[cfg(test)]
mod cosine_tests {
    #[test]
    fn cosine_is_one_for_parallel_vectors_and_zero_for_orthogonal_ones() {
        assert!((super::cosine(&[1.0, 2.0], &[2.0, 4.0]) - 1.0).abs() < 1e-6);
        assert_eq!(super::cosine(&[1.0, 0.0], &[0.0, 1.0]), 0.0);
        assert_eq!(super::cosine(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
    }
}
