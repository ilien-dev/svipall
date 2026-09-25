//! Speech to timed text, on this machine, for a video that has no captions.
//!
//! An encoder-decoder speech model the operator installs (`asr_encoder.onnx`, `asr_decoder.onnx`,
//! `asr.json`, `asr_vocab.json`, all in `~/.svipall/models/`, from the release's models archive or
//! `tools/models/export_asr.py`). Nothing is downloaded at run time and nothing is sent anywhere.
//!
//! Everything about the model lives in `asr.json`: the front end (mel bins, FFT size, hop, rate),
//! the filterbank itself, the special tokens and the languages. What is written here is the
//! arithmetic that does not change with the model: the log-mel spectrogram, greedy decoding under
//! the timestamp rules, and turning byte-level BPE pieces back into text.
//!
//! The audio is decoded in pure Rust, as for audio captchas: a transcription never depends on a
//! media tool happening to be installed. What cannot be decoded that way (Opus in WebM, MPEG-TS
//! segments) is said, not guessed at.

use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use svipall_core::video::Cue;

pub fn models_dir() -> PathBuf {
    crate::model_source::models_dir()
}

pub fn encoder_path() -> PathBuf {
    models_dir().join("asr_encoder.onnx")
}

pub fn decoder_path() -> PathBuf {
    models_dir().join("asr_decoder.onnx")
}

pub fn config_path() -> PathBuf {
    models_dir().join("asr.json")
}

pub fn vocab_path() -> PathBuf {
    models_dir().join("asr_vocab.json")
}

/// True when this build can transcribe and the four files are where they belong.
pub fn available() -> bool {
    cfg!(feature = "onnx-asr")
        && [encoder_path(), decoder_path(), config_path(), vocab_path()]
            .iter()
            .all(|p| p.is_file())
}

#[derive(Debug, Clone, Deserialize)]
pub struct Tokens {
    pub eot: i64,
    pub sot: i64,
    pub transcribe: i64,
    pub translate: i64,
    pub no_timestamps: i64,
    pub no_speech: i64,
    pub timestamp_begin: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AsrConfig {
    pub n_mels: usize,
    pub n_fft: usize,
    pub hop: usize,
    pub sample_rate: u32,
    pub chunk_seconds: usize,
    pub frames: usize,
    pub time_precision: f64,
    pub max_initial_timestamp: f64,
    pub tokens: Tokens,
    #[serde(default)]
    pub languages: HashMap<String, i64>,
    /// `[n_mels][n_fft / 2 + 1]`, exactly as the model's own feature extractor built it.
    pub mel_filters: Vec<Vec<f32>>,
}

impl AsrConfig {
    pub fn chunk_samples(&self) -> usize {
        self.chunk_seconds * self.sample_rate as usize
    }
}

pub fn load_config() -> Result<AsrConfig> {
    Ok(serde_json::from_str(&std::fs::read_to_string(
        config_path(),
    )?)?)
}

// ---- front end ------------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct C(f64, f64);

impl C {
    fn mul(self, o: C) -> C {
        C(self.0 * o.0 - self.1 * o.1, self.0 * o.1 + self.1 * o.0)
    }
    fn add(self, o: C) -> C {
        C(self.0 + o.0, self.1 + o.1)
    }
}

/// A discrete Fourier transform of any length, by mixed-radix Cooley-Tukey: split on the smallest
/// factor, recurse, combine. The window here is 400 samples (2^4 x 5^2), which a radix-2
/// transform cannot take and padding to 512 would change the answer.
fn dft(x: &[C]) -> Vec<C> {
    let n = x.len();
    if n <= 1 {
        return x.to_vec();
    }
    let p = (2..=n).find(|p| n.is_multiple_of(*p)).unwrap_or(n);
    let tw = |k: usize| {
        let a = -2.0 * std::f64::consts::PI * k as f64 / n as f64;
        C(a.cos(), a.sin())
    };
    if p == n {
        return (0..n)
            .map(|k| {
                x.iter()
                    .enumerate()
                    .fold(C(0.0, 0.0), |acc, (j, v)| acc.add(v.mul(tw((j * k) % n))))
            })
            .collect();
    }
    let m = n / p;
    let subs: Vec<Vec<C>> = (0..p)
        .map(|r| dft(&(0..m).map(|k| x[r + p * k]).collect::<Vec<_>>()))
        .collect();
    (0..n)
        .map(|k| {
            (0..p).fold(C(0.0, 0.0), |acc, r| {
                acc.add(subs[r][k % m].mul(tw((r * k) % n)))
            })
        })
        .collect()
}

/// The log-mel spectrogram of one window, laid out `[n_mels][frames]`, the way the model was
/// trained on it: the samples padded (or cut) to the window length, a centred STFT with reflected
/// edges and a periodic Hann window, power, the model's filterbank, `log10` floored at `1e-10`,
/// the last frame dropped, everything clamped to 8 below the maximum, then `(x + 4) / 4`.
pub fn log_mel(samples: &[f32], cfg: &AsrConfig) -> Vec<f32> {
    let n = cfg.chunk_samples();
    let mut x: Vec<f64> = samples.iter().take(n).map(|v| *v as f64).collect();
    x.resize(n, 0.0);
    let half = cfg.n_fft / 2;
    // Reflect, excluding the edge sample, as `numpy.pad(mode="reflect")` does.
    let mut padded = Vec::with_capacity(n + 2 * half);
    padded.extend((1..=half).rev().map(|i| x[i]));
    padded.extend_from_slice(&x);
    padded.extend((0..half).map(|i| x[n - 2 - i]));
    let window: Vec<f64> = (0..cfg.n_fft)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / cfg.n_fft as f64).cos())
        .collect();
    let bins = cfg.n_fft / 2 + 1;
    let frames = cfg.frames;
    let mut mel = vec![0f64; cfg.n_mels * frames];
    let mut power = vec![0f64; bins];
    for f in 0..frames {
        let start = f * cfg.hop;
        let frame = &padded[start..start + cfg.n_fft];
        if frame.iter().all(|v| *v == 0.0) {
            // Silence (the padding of a short last window): the spectrum is zero, the answer the
            // floor. Skipping the transform is what keeps a mostly empty window cheap.
            power.iter_mut().for_each(|p| *p = 0.0);
        } else {
            let spec = dft(&frame
                .iter()
                .zip(&window)
                .map(|(v, w)| C(v * w, 0.0))
                .collect::<Vec<_>>());
            for (k, p) in power.iter_mut().enumerate() {
                *p = spec[k].0 * spec[k].0 + spec[k].1 * spec[k].1;
            }
        }
        for (m, filt) in cfg.mel_filters.iter().enumerate().take(cfg.n_mels) {
            let e: f64 = filt.iter().zip(&power).map(|(w, p)| *w as f64 * p).sum();
            mel[m * frames + f] = e.max(1e-10).log10();
        }
    }
    let top = mel.iter().cloned().fold(f64::MIN, f64::max);
    mel.iter()
        .map(|v| ((v.max(top - 8.0) + 4.0) / 4.0) as f32)
        .collect()
}

// ---- tokens ---------------------------------------------------------------------------------

/// Byte-level BPE writes every byte as a printable character; this is the table back.
fn byte_decoder() -> HashMap<char, u8> {
    let mut bs: Vec<u32> = (b'!' as u32..=b'~' as u32)
        .chain(0xA1..=0xAC)
        .chain(0xAE..=0xFF)
        .collect();
    let mut cs = bs.clone();
    let mut extra = 0;
    for b in 0..256u32 {
        if !bs.contains(&b) {
            bs.push(b);
            cs.push(256 + extra);
            extra += 1;
        }
    }
    bs.into_iter()
        .zip(cs)
        .filter_map(|(b, c)| Some((char::from_u32(c)?, b as u8)))
        .collect()
}

/// Text tokens back into text. Special tokens (anything from `eot` up) carry no text.
pub fn decode_text(ids: &[i64], vocab: &[String], eot: i64) -> String {
    let table = byte_decoder();
    let bytes: Vec<u8> = ids
        .iter()
        .filter(|&&id| id >= 0 && id < eot)
        .filter_map(|&id| vocab.get(id as usize))
        .flat_map(|piece| piece.chars().filter_map(|c| table.get(&c).copied()))
        .collect();
    String::from_utf8_lossy(&bytes).trim().to_string()
}

/// The rules that keep greedy decoding producing well-formed `<|t|> text <|t|>` segments:
/// nothing special but end-of-text and timestamps, timestamps in pairs and never going back, and a
/// timestamp chosen whenever timestamps together are likelier than any single word.
pub fn apply_rules(logits: &mut [f32], sampled: &[i64], t: &Tokens, max_initial: i64) {
    let tb = t.timestamp_begin as usize;
    let eot = t.eot as usize;
    let vocab = logits.len();
    let mask = |l: &mut [f32], r: std::ops::Range<usize>| {
        l[r.start.min(vocab)..r.end.min(vocab)]
            .iter_mut()
            .for_each(|v| *v = f32::NEG_INFINITY)
    };
    mask(logits, eot + 1..tb);
    let is_ts = |id: &i64| *id >= t.timestamp_begin;
    let last = sampled.last().map(is_ts).unwrap_or(false);
    let penultimate = sampled.len() < 2 || is_ts(&sampled[sampled.len() - 2]);
    if last {
        if penultimate {
            mask(logits, tb..vocab);
        } else {
            mask(logits, 0..eot);
        }
    }
    if let Some(prev) = sampled.iter().rev().find(|id| is_ts(id)) {
        let floor = if last && !penultimate {
            *prev
        } else {
            *prev + 1
        };
        mask(logits, tb..floor as usize);
    }
    if sampled.is_empty() {
        mask(logits, 0..tb);
        mask(logits, tb + max_initial as usize + 1..vocab);
    }
    let ts_mass = log_sum_exp(&logits[tb.min(vocab)..]);
    let text_best = logits[..tb.min(vocab)]
        .iter()
        .cloned()
        .fold(f32::NEG_INFINITY, f32::max);
    if ts_mass > text_best {
        mask(logits, 0..tb);
    }
}

fn log_sum_exp(v: &[f32]) -> f32 {
    let m = v.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    if m == f32::NEG_INFINITY {
        return m;
    }
    m + v.iter().map(|x| (x - m).exp()).sum::<f32>().ln()
}

/// Timed segments from one window's tokens, and how far into the window they account for: to the
/// last timestamp when the window ended mid-sentence, so the next window starts there, or the
/// whole window when it ended cleanly.
pub fn segments(
    sampled: &[i64],
    offset: f64,
    window: f64,
    vocab: &[String],
    t: &Tokens,
    precision: f64,
) -> (Vec<Cue>, f64) {
    let at = |id: i64| (id - t.timestamp_begin) as f64 * precision;
    let mut out = Vec::new();
    let mut start: Option<f64> = None;
    let mut text: Vec<i64> = Vec::new();
    for &id in sampled.iter().take_while(|id| **id != t.eot) {
        if id >= t.timestamp_begin {
            let ts = at(id);
            if let (Some(s), false) = (start, text.is_empty()) {
                let words = decode_text(&text, vocab, t.eot);
                if !words.is_empty() {
                    out.push(Cue {
                        start: offset + s,
                        end: offset + ts,
                        text: words,
                    });
                }
                text.clear();
            }
            start = Some(ts);
        } else {
            text.push(id);
        }
    }
    let ended_on_pair = sampled
        .iter()
        .rev()
        .filter(|id| **id != t.eot)
        .take(2)
        .all(|id| *id >= t.timestamp_begin);
    let words = decode_text(&text, vocab, t.eot);
    if !words.is_empty() {
        out.push(Cue {
            start: offset + start.unwrap_or(0.0),
            end: offset + window,
            text: words,
        });
    }
    let consumed = match (ended_on_pair, out.last()) {
        (false, Some(last)) if text.is_empty() && last.end - offset > 1.0 => last.end - offset,
        _ => window,
    };
    (out, consumed)
}

// ---- inference ------------------------------------------------------------------------------

/// What came back: the cues, the language the model heard, and how far it got.
pub struct Transcript {
    pub cues: Vec<Cue>,
    pub lang: Option<String>,
    /// Seconds of audio transcribed; less than the audio when the deadline came first.
    pub covered: f64,
    pub total: f64,
}

#[cfg(not(feature = "onnx-asr"))]
pub fn transcribe(
    _bytes: &[u8],
    _lang: Option<&str>,
    _deadline: std::time::Instant,
) -> Result<Transcript> {
    Err(anyhow!(
        "this build has no speech recognition: it was compiled without the onnx-asr feature"
    ))
}

#[cfg(feature = "onnx-asr")]
pub fn transcribe(
    bytes: &[u8],
    lang: Option<&str>,
    deadline: std::time::Instant,
) -> Result<Transcript> {
    imp::transcribe(bytes, lang, deadline)
}

#[cfg(feature = "onnx-asr")]
mod imp {
    use super::*;
    use crate::model_source::{Located, SessionCache};
    use once_cell::sync::OnceCell;

    static ENCODER: SessionCache = SessionCache::new();
    static DECODER: SessionCache = SessionCache::new();
    static VOCAB: OnceCell<Vec<String>> = OnceCell::new();

    /// Tokens a window may produce, at most: the model's own limit on half its context.
    const MAX_TOKENS: usize = 224;

    fn located(name: &'static str, path: PathBuf) -> Result<Located> {
        let sidecar = std::fs::read_to_string(config_path())?;
        Located::disk(name, path.clone(), sidecar)
            .ok_or_else(|| anyhow!("no speech model at {}", path.display()))
    }

    fn encode(cfg: &AsrConfig, mel: Vec<f32>) -> Result<(Vec<i64>, Vec<f32>)> {
        let shape = [1i64, cfg.n_mels as i64, cfg.frames as i64];
        ENCODER.with(&located("asr_encoder", encoder_path())?, |sess| {
            let name = sess.inputs()[0].name().to_string();
            let input = ort::value::Tensor::from_array((shape, mel))?;
            let out = sess.run(ort::inputs![name.as_str() => input])?;
            let (s, v) = out[0].try_extract_tensor::<f32>()?;
            Ok((s.iter().copied().collect(), v.to_vec()))
        })
    }

    /// The logits for the next token after `ids`.
    fn next_logits(ids: &[i64], hidden: &(Vec<i64>, Vec<f32>)) -> Result<Vec<f32>> {
        DECODER.with(&located("asr_decoder", decoder_path())?, |sess| {
            let names: Vec<String> = sess.inputs().iter().map(|i| i.name().to_string()).collect();
            let ids_name = names
                .iter()
                .find(|n| n.contains("input_ids"))
                .cloned()
                .unwrap_or_else(|| names[0].clone());
            let hid_name = names
                .iter()
                .find(|n| n.contains("encoder_hidden"))
                .cloned()
                .unwrap_or_else(|| names[1].clone());
            let ids_t = ort::value::Tensor::from_array(([1i64, ids.len() as i64], ids.to_vec()))?;
            let hid_t = ort::value::Tensor::from_array((hidden.0.clone(), hidden.1.clone()))?;
            let out =
                sess.run(ort::inputs![ids_name.as_str() => ids_t, hid_name.as_str() => hid_t])?;
            let (shape, v) = out[0].try_extract_tensor::<f32>()?;
            let vocab = *shape
                .last()
                .ok_or_else(|| anyhow!("empty decoder output"))? as usize;
            Ok(v[v.len() - vocab..].to_vec())
        })
    }

    fn argmax(v: &[f32]) -> i64 {
        crate::model_source::argmax(v) as i64
    }

    pub fn transcribe(
        bytes: &[u8],
        lang: Option<&str>,
        deadline: std::time::Instant,
    ) -> Result<Transcript> {
        let cfg = load_config()?;
        let vocab = VOCAB.get_or_try_init(|| -> Result<Vec<String>> {
            Ok(serde_json::from_str(&std::fs::read_to_string(
                vocab_path(),
            )?)?)
        })?;
        let (mono, rate) = crate::audio::decode(bytes)?;
        let audio = crate::audio::resample(&mono, rate, cfg.sample_rate);
        let total = audio.len() as f64 / cfg.sample_rate as f64;
        let t = &cfg.tokens;
        let max_initial = (cfg.max_initial_timestamp / cfg.time_precision).round() as i64;
        let window = cfg.chunk_seconds as f64;

        let mut cues = Vec::new();
        let mut heard: Option<String> = lang.map(|l| l.split('-').next().unwrap_or(l).to_string());
        let mut seek = 0.0f64;
        while seek < total - 0.5 {
            if std::time::Instant::now() > deadline {
                break;
            }
            let from = (seek * cfg.sample_rate as f64) as usize;
            let mel = log_mel(&audio[from.min(audio.len())..], &cfg);
            let hidden = encode(&cfg, mel)?;
            let lang_id = match heard.as_deref().and_then(|l| cfg.languages.get(l)) {
                Some(id) => *id,
                None => {
                    // The model's own guess: the likeliest language token after start-of-text.
                    let logits = next_logits(&[t.sot], &hidden)?;
                    let (code, id) = cfg
                        .languages
                        .iter()
                        .max_by(|a, b| logits[*a.1 as usize].total_cmp(&logits[*b.1 as usize]))
                        .ok_or_else(|| anyhow!("the model lists no languages"))?;
                    heard = Some(code.clone());
                    *id
                }
            };
            let prompt = [t.sot, lang_id, t.transcribe];
            let mut sampled: Vec<i64> = Vec::new();
            for _ in 0..MAX_TOKENS {
                let ids: Vec<i64> = prompt.iter().chain(&sampled).copied().collect();
                let mut logits = next_logits(&ids, &hidden)?;
                apply_rules(&mut logits, &sampled, t, max_initial);
                let next = argmax(&logits);
                if next == t.eot {
                    break;
                }
                sampled.push(next);
            }
            let (found, consumed) = segments(&sampled, seek, window, vocab, t, cfg.time_precision);
            cues.extend(found.into_iter().filter(|c| c.start < total));
            seek += consumed.max(1.0);
        }
        Ok(Transcript {
            cues,
            lang: heard,
            covered: seek.min(total),
            total,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens() -> Tokens {
        Tokens {
            eot: 10,
            sot: 11,
            transcribe: 12,
            translate: 13,
            no_timestamps: 14,
            no_speech: 15,
            timestamp_begin: 20,
        }
    }

    fn vocab() -> Vec<String> {
        // Byte-level pieces: `Ġ` is a leading space, `Ã±` is the two bytes of `ñ`.
        [
            "He", "llo", "Ġworld", ".", "Ġa", "Ã±o", "ĠYes", "!", "Ġok", "?",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    #[test]
    fn byte_level_pieces_become_text_again() {
        assert_eq!(decode_text(&[0, 1, 2, 3], &vocab(), 10), "Hello world.");
        assert_eq!(decode_text(&[4, 5], &vocab(), 10), "año");
        assert_eq!(
            decode_text(&[0, 11, 25, 1], &vocab(), 10),
            "Hello",
            "specials carry no text"
        );
    }

    #[test]
    fn segments_are_cut_at_timestamps_and_a_clean_end_consumes_the_window() {
        // <|0.00|> Hello world. <|1.00|><|1.00|> Yes! <|2.00|><|2.00|> eot
        let s = [20, 0, 1, 2, 3, 70, 70, 6, 7, 120, 120, 10];
        let (cues, consumed) = segments(&s, 30.0, 30.0, &vocab(), &tokens(), 0.02);
        assert_eq!(cues.len(), 2);
        assert_eq!((cues[0].start, cues[0].end), (30.0, 31.0));
        assert_eq!(cues[0].text, "Hello world.");
        assert_eq!(cues[1].text, "Yes!");
        assert_eq!(consumed, 30.0);
    }

    #[test]
    fn a_window_ending_mid_sentence_is_resumed_from_its_last_timestamp() {
        // <|0|> Hello <|1.00|> world (cut off) eot
        let s = [20, 0, 1, 70, 2, 10];
        let (cues, consumed) = segments(&s, 0.0, 30.0, &vocab(), &tokens(), 0.02);
        assert_eq!(cues[0].text, "Hello");
        assert_eq!(cues[1].text, "world");
        assert_eq!(
            cues[1].end, 30.0,
            "the unfinished line runs to the end of the window"
        );
        assert_eq!(
            consumed, 30.0,
            "a trailing line with no closing timestamp is kept whole"
        );
        let s = [20, 0, 1, 70, 70, 2, 3, 150, 10];
        let (_, consumed) = segments(&s, 0.0, 30.0, &vocab(), &tokens(), 0.02);
        assert!(
            (consumed - 2.6).abs() < 1e-9,
            "resume where the last line closed: {consumed}"
        );
    }

    fn logits() -> Vec<f32> {
        vec![0.0; 40]
    }

    #[test]
    fn a_window_starts_with_an_early_timestamp() {
        let mut l = logits();
        l[3] = 9.0;
        apply_rules(&mut l, &[], &tokens(), 5);
        assert!(
            l[..20].iter().all(|v| *v == f32::NEG_INFINITY),
            "no text first"
        );
        assert!(
            l[26..].iter().all(|v| *v == f32::NEG_INFINITY),
            "not later than the limit"
        );
        assert!(l[20..=25].iter().all(|v| v.is_finite()));
    }

    #[test]
    fn timestamps_come_in_pairs_and_never_go_back() {
        let t = tokens();
        // After text then a timestamp: another timestamp (the pair) or the end, not more text.
        let mut l = logits();
        l[10] = 5.0;
        apply_rules(&mut l, &[20, 1, 30], &t, 5);
        assert!(l[..10].iter().all(|v| *v == f32::NEG_INFINITY));
        assert!(
            l[20..30].iter().all(|v| *v == f32::NEG_INFINITY),
            "not before 30"
        );
        assert!(l[30].is_finite() && l[10].is_finite());
        // After a pair: text, not a third timestamp.
        let mut l = logits();
        l[2] = 5.0;
        apply_rules(&mut l, &[20, 1, 30, 30], &t, 5);
        assert!(l[20..].iter().all(|v| *v == f32::NEG_INFINITY));
        assert!(l[2].is_finite());
    }

    #[test]
    fn specials_other_than_the_end_are_never_produced() {
        let mut l = logits();
        l[12] = 50.0;
        apply_rules(&mut l, &[20, 1], &tokens(), 5);
        assert!(l[11..20].iter().all(|v| *v == f32::NEG_INFINITY));
    }

    #[test]
    fn any_length_transform_matches_the_definition() {
        let x: Vec<C> = (0..400).map(|i| C((i as f64 * 0.37).sin(), 0.0)).collect();
        let fast = dft(&x);
        for k in [0usize, 1, 7, 100, 199, 200] {
            let slow = x.iter().enumerate().fold(C(0.0, 0.0), |acc, (j, v)| {
                let a = -2.0 * std::f64::consts::PI * (j * k) as f64 / 400.0;
                acc.add(v.mul(C(a.cos(), a.sin())))
            });
            assert!(
                (fast[k].0 - slow.0).abs() < 1e-6 && (fast[k].1 - slow.1).abs() < 1e-6,
                "bin {k}"
            );
        }
    }

    /// The front end against the model's own feature extractor, on a signal both compute from
    /// scratch. The reference and the filterbank come from `tools/models/export_asr.py`.
    #[test]
    fn the_log_mel_matches_the_models_own_feature_extractor() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/asr");
        let (Ok(cfg), Ok(reference)) = (
            std::fs::read_to_string(dir.join("asr_frontend.json")),
            std::fs::read_to_string(dir.join("mel_reference.json")),
        ) else {
            panic!("fixtures missing: run tools/models/export_asr.py --fixtures");
        };
        let cfg: AsrConfig = serde_json::from_str(&cfg).unwrap();
        let reference: serde_json::Value = serde_json::from_str(&reference).unwrap();
        let rate = cfg.sample_rate as usize;
        let signal: Vec<f32> = (0..rate)
            .map(|i| {
                let t = i as f64 / rate as f64;
                let tau = 2.0 * std::f64::consts::PI;
                (0.4 * (tau * 440.0 * t).sin()
                    + 0.2 * (tau * 1000.0 * t).sin()
                    + 0.1 * (tau * (200.0 + 1800.0 * t) * t).sin()) as f32
            })
            .collect();
        let mel = log_mel(&signal, &cfg);
        let frames = reference["frame_index"].as_array().unwrap();
        for (i, f) in frames.iter().enumerate() {
            let f = f.as_u64().unwrap() as usize;
            let want = reference["frames"][i].as_array().unwrap();
            for (m, w) in want.iter().enumerate() {
                let got = mel[m * cfg.frames + f];
                let w = w.as_f64().unwrap() as f32;
                assert!((got - w).abs() < 2e-3, "frame {f} band {m}: {got} vs {w}");
            }
        }
    }
}
