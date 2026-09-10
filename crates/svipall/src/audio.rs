//! Audio captchas, solved on this machine.
//!
//! Every token widget worth the name offers an audio alternative, and it is the easiest of the two
//! to answer: the vocabulary is closed — ten digits and a handful of letters, read slowly, over
//! noise that is there to defeat a general recogniser rather than a specific one.
//!
//! The arrangement is the one `ocr.rs` already uses, deliberately: the signal processing is
//! ordinary Rust that compiles and is tested always, the inference path is behind `onnx-audio`, and
//! the model itself is a file the operator installs at `~/.svipall/models/audio.onnx`. Nothing is
//! downloaded, nothing is sent anywhere, and a machine without the model says so rather than
//! guessing.
//!
//! Two rules that are not obvious and cost a day each if broken:
//!
//! The clip is fetched **from inside the page**, using the session that is already there. Pulling
//! it with a separate HTTP request means a second address asking for a challenge issued to the
//! first, which is a stronger signal than anything the audio itself could give away.
//!
//! Decoding is pure Rust. Shelling out to a media tool would make the answer depend on what happens
//! to be installed, and would put an external process in the middle of a captcha.

use anyhow::{anyhow, Result};
use std::f32::consts::PI;

use crate::model_source::{self, Located};

/// The audio model, wherever it lives.
pub fn locate() -> Option<Located> {
    model_source::locate("audio", "audio", "onnx", svipall_models::audio())
}

/// True when a usable audio model is installed and the inference path is compiled in.
pub fn available() -> bool {
    cfg!(feature = "onnx-audio") && locate().is_some()
}

/// How the model wants its input, and what its output axis means.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AudioConfig {
    /// Sample rate the model was trained at.
    pub sample_rate: u32,
    /// Window length in samples. Must be a power of two.
    pub n_fft: usize,
    /// Samples between the start of one window and the next.
    pub hop: usize,
    /// Number of mel bands.
    pub n_mels: usize,
    /// Characters the final axis maps to; index 0 is the CTC blank, as in `ocr.rs`.
    pub charset: String,
}

pub fn load_config() -> Result<AudioConfig> {
    locate()
        .ok_or_else(|| anyhow!("no audio model installed or embedded"))?
        .config()
}

/// Average the channels down to one.
///
/// Taking the left channel instead is the tempting shortcut and it is wrong: some of these clips
/// pan the spoken digits across the stereo field precisely so that half a recogniser hears half the
/// answer.
pub fn to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    let channels = channels.max(1);
    if channels == 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// Resample by linear interpolation.
///
/// Not the best resampler there is; it is the right one here. The model was trained on clips that
/// went through something equally ordinary, and the alternative is a dependency and a filter design
/// nobody will ever tune for speech at eight kilohertz.
pub fn resample(samples: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32> {
    if from_hz == to_hz || samples.is_empty() || from_hz == 0 || to_hz == 0 {
        return samples.to_vec();
    }
    let ratio = to_hz as f64 / from_hz as f64;
    let out_len = ((samples.len() as f64) * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let pos = i as f64 / ratio;
        let a = pos.floor() as usize;
        let frac = (pos - a as f64) as f32;
        let x0 = samples.get(a).copied().unwrap_or(0.0);
        let x1 = samples.get(a + 1).copied().unwrap_or(x0);
        out.push(x0 + (x1 - x0) * frac);
    }
    out
}

/// A periodic Hann window.
pub fn hann(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / n as f32).cos())
        .collect()
}

/// In-place radix-2 FFT. `re` and `im` must be the same length and a power of two.
///
/// Written out rather than pulled in: it is thirty lines, it has no configuration, and a captcha
/// solver that depends on a transform crate for this is carrying a dependency for thirty lines.
pub fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    debug_assert_eq!(n, im.len());
    debug_assert!(n.is_power_of_two(), "radix-2 needs a power of two");
    if n < 2 {
        return;
    }
    // Bit-reversal permutation.
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2usize;
    while len <= n {
        let angle = -2.0 * PI / len as f32;
        let (wr, wi) = (angle.cos(), angle.sin());
        let mut i = 0usize;
        while i < n {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (ur, ui) = (re[i + k], im[i + k]);
                let (vr, vi) = (
                    re[i + k + len / 2] * cr - im[i + k + len / 2] * ci,
                    re[i + k + len / 2] * ci + im[i + k + len / 2] * cr,
                );
                re[i + k] = ur + vr;
                im[i + k] = ui + vi;
                re[i + k + len / 2] = ur - vr;
                im[i + k + len / 2] = ui - vi;
                let next_cr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = next_cr;
            }
            i += len;
        }
        len <<= 1;
    }
}

fn hz_to_mel(hz: f32) -> f32 {
    2595.0 * (1.0 + hz / 700.0).log10()
}

fn mel_to_hz(mel: f32) -> f32 {
    700.0 * (10f32.powf(mel / 2595.0) - 1.0)
}

/// Triangular mel filters over the positive half of the spectrum.
///
/// Returns `n_mels` rows of `n_fft / 2 + 1` weights.
pub fn mel_filters(n_fft: usize, n_mels: usize, sample_rate: u32) -> Vec<Vec<f32>> {
    let bins = n_fft / 2 + 1;
    let nyquist = sample_rate as f32 / 2.0;
    let (lo, hi) = (hz_to_mel(0.0), hz_to_mel(nyquist));
    // n_mels + 2 edges give n_mels overlapping triangles.
    let points: Vec<f32> = (0..n_mels + 2)
        .map(|i| mel_to_hz(lo + (hi - lo) * i as f32 / (n_mels + 1) as f32))
        .map(|hz| hz * n_fft as f32 / sample_rate as f32)
        .collect();
    (0..n_mels)
        .map(|m| {
            let (left, centre, right) = (points[m], points[m + 1], points[m + 2]);
            (0..bins)
                .map(|b| {
                    let b = b as f32;
                    if b <= left || b >= right {
                        0.0
                    } else if b <= centre {
                        (b - left) / (centre - left).max(1e-6)
                    } else {
                        (right - b) / (right - centre).max(1e-6)
                    }
                })
                .collect()
        })
        .collect()
}

/// Log-mel spectrogram, laid out frame by frame: `frames * n_mels` values.
pub fn log_mel(samples: &[f32], cfg: &AudioConfig) -> Vec<f32> {
    let window = hann(cfg.n_fft);
    let filters = mel_filters(cfg.n_fft, cfg.n_mels, cfg.sample_rate);
    let bins = cfg.n_fft / 2 + 1;
    let mut out = Vec::new();
    let mut start = 0usize;
    while start + cfg.n_fft <= samples.len() {
        let mut re: Vec<f32> = (0..cfg.n_fft)
            .map(|i| samples[start + i] * window[i])
            .collect();
        let mut im = vec![0f32; cfg.n_fft];
        fft(&mut re, &mut im);
        let power: Vec<f32> = (0..bins).map(|b| re[b] * re[b] + im[b] * im[b]).collect();
        for f in &filters {
            let energy: f32 = f.iter().zip(&power).map(|(w, p)| w * p).sum();
            // Floored before the log: silence is common in these clips, and log(0) poisons every
            // downstream number with -inf.
            out.push((energy + 1e-10).ln());
        }
        start += cfg.hop.max(1);
    }
    out
}

/// The words these challenges actually use, mapped to what they mean.
///
/// A closed vocabulary is what makes this tractable at all: the model only has to tell ten digits
/// and a few letters apart, and anything it produces outside the list is a misrecognition rather
/// than a word worth keeping.
const SPOKEN: &[(&str, char)] = &[
    ("zero", '0'),
    ("oh", '0'),
    ("one", '1'),
    ("two", '2'),
    ("three", '3'),
    ("four", '4'),
    ("five", '5'),
    ("six", '6'),
    ("seven", '7'),
    ("eight", '8'),
    ("nine", '9'),
];

/// Turn recognised words into the string a form expects.
///
/// Digits already written as digits pass through, which is what a character-level model produces.
pub fn words_to_digits(text: &str) -> String {
    let mut out = String::new();
    for word in text.split(|c: char| !c.is_ascii_alphanumeric()) {
        if word.is_empty() {
            continue;
        }
        let lower = word.to_ascii_lowercase();
        match SPOKEN.iter().find(|(w, _)| *w == lower) {
            Some((_, d)) => out.push(*d),
            None => {
                for c in word.chars().filter(|c| c.is_ascii_alphanumeric()) {
                    out.push(c.to_ascii_lowercase());
                }
            }
        }
    }
    out
}

#[cfg(not(feature = "onnx-audio"))]
pub fn solve_bytes(_bytes: &[u8]) -> Result<String> {
    Err(anyhow!(
        "audio solving not compiled in (build with --features onnx-audio)"
    ))
}

#[cfg(feature = "onnx-audio")]
pub fn solve_bytes(bytes: &[u8]) -> Result<String> {
    imp::solve_bytes(bytes)
}

#[cfg(feature = "onnx-audio")]
mod imp {
    use super::*;
    use crate::model_source::SessionCache;
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    static SESSION: SessionCache = SessionCache::new();

    /// Decode whatever container the widget served into mono samples at its own rate.
    fn decode(bytes: &[u8]) -> Result<(Vec<f32>, u32)> {
        let source = std::io::Cursor::new(bytes.to_vec());
        let stream = MediaSourceStream::new(Box::new(source), Default::default());
        let probed = symphonia::default::get_probe().format(
            &Hint::new(),
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )?;
        let mut format = probed.format;
        let track = format
            .default_track()
            .ok_or_else(|| anyhow!("the clip has no audio track"))?;
        let track_id = track.id;
        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())?;
        let mut samples: Vec<f32> = Vec::new();
        let mut rate = 0u32;
        let mut channels = 1usize;
        while let Ok(packet) = format.next_packet() {
            if packet.track_id() != track_id {
                continue;
            }
            let decoded = match decoder.decode(&packet) {
                Ok(d) => d,
                // A truncated final packet is normal and not a reason to lose the clip.
                Err(_) => break,
            };
            let spec = *decoded.spec();
            rate = spec.rate;
            channels = spec.channels.count().max(1);
            let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
            buf.copy_interleaved_ref(decoded);
            samples.extend_from_slice(buf.samples());
        }
        if samples.is_empty() {
            return Err(anyhow!("nothing decoded from the clip"));
        }
        Ok((to_mono(&samples, channels), rate))
    }

    pub fn solve_bytes(bytes: &[u8]) -> Result<String> {
        let located = locate().ok_or_else(|| anyhow!("no audio model installed or embedded"))?;
        let cfg: AudioConfig = located.config()?;
        let charset: Vec<char> = cfg.charset.chars().collect();
        let (mono, rate) = decode(bytes)?;
        let samples = resample(&mono, rate, cfg.sample_rate);
        let frames_data = log_mel(&samples, &cfg);
        let frames = frames_data.len() / cfg.n_mels.max(1);
        if frames == 0 {
            return Err(anyhow!("the clip is shorter than one analysis window"));
        }
        let shape = [1i64, frames as i64, cfg.n_mels as i64];
        SESSION.with(&located, |sess| {
            let input = ort::value::Tensor::from_array((shape, frames_data))?;
            let name = sess.inputs()[0].name().to_string();
            let outputs = sess.run(ort::inputs![name.as_str() => input])?;
            let (out_shape, out) = outputs[0].try_extract_tensor::<f32>()?;
            let dims: Vec<usize> = out_shape.iter().map(|d| *d as usize).collect();
            let classes = *dims.last().ok_or_else(|| anyhow!("empty audio output"))?;
            let steps = out.len() / classes.max(1);
            let best: Vec<usize> = (0..steps)
                .map(|t| model_source::argmax(&out[t * classes..(t + 1) * classes]))
                .collect();
            // Same decode as the image path: one implementation, one set of edge cases.
            Ok(words_to_digits(&crate::ocr::ctc_decode(&best, &charset)))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> AudioConfig {
        AudioConfig {
            sample_rate: 8_000,
            n_fft: 256,
            hop: 128,
            n_mels: 40,
            charset: "-0123456789".into(),
        }
    }

    fn tone(hz: f32, seconds: f32, rate: u32) -> Vec<f32> {
        let n = (seconds * rate as f32) as usize;
        (0..n)
            .map(|i| (2.0 * PI * hz * i as f32 / rate as f32).sin())
            .collect()
    }

    #[test]
    fn both_channels_are_heard_not_just_the_left_one() {
        // Some clips pan the spoken digits across the stereo field precisely so that a recogniser
        // taking one channel hears half the answer.
        let stereo = [1.0, 0.0, 0.0, 1.0, 0.5, 0.5];
        assert_eq!(to_mono(&stereo, 2), vec![0.5, 0.5, 0.5]);
        assert_eq!(to_mono(&[1.0, 2.0], 1), vec![1.0, 2.0]);
    }

    #[test]
    fn resampling_changes_the_length_by_the_ratio_and_keeps_the_shape() {
        let s = tone(200.0, 0.1, 16_000);
        let down = resample(&s, 16_000, 8_000);
        assert_eq!(down.len(), s.len() / 2);
        // The first sample is the same instant in time either way.
        assert!((down[0] - s[0]).abs() < 1e-6);
        assert_eq!(resample(&s, 8_000, 8_000).len(), s.len());
        assert!(resample(&[], 16_000, 8_000).is_empty());
    }

    #[test]
    fn a_pure_tone_lands_in_the_bin_it_belongs_in() {
        // If the transform is wrong nothing downstream can be right, and every other test here
        // would still pass.
        let n = 256;
        let rate = 8_000u32;
        let hz = 1_000.0;
        let mut re: Vec<f32> = (0..n)
            .map(|i| (2.0 * PI * hz * i as f32 / rate as f32).sin())
            .collect();
        let mut im = vec![0f32; n];
        fft(&mut re, &mut im);
        let power: Vec<f32> = (0..n / 2).map(|b| re[b] * re[b] + im[b] * im[b]).collect();
        let peak = power
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(i, _)| i)
            .expect("a peak");
        let expected = (hz * n as f32 / rate as f32).round() as usize;
        assert_eq!(peak, expected, "1kHz at 8kHz over 256 points is bin 32");
    }

    #[test]
    fn the_transform_of_silence_is_silence() {
        let mut re = vec![0f32; 64];
        let mut im = vec![0f32; 64];
        fft(&mut re, &mut im);
        assert!(re.iter().chain(im.iter()).all(|v| v.abs() < 1e-6));
    }

    #[test]
    fn the_window_tapers_to_nothing_at_both_ends() {
        let w = hann(64);
        assert!(w[0].abs() < 1e-6);
        assert!(w[32] > 0.99, "{}", w[32]);
        // Periodic rather than symmetric: the last point is not zero, the point after it would be.
        assert!(w[63] < 0.01, "{}", w[63]);
    }

    #[test]
    fn the_mel_filters_cover_the_spectrum_without_gaps_or_overhang() {
        let filters = mel_filters(256, 40, 8_000);
        assert_eq!(filters.len(), 40);
        assert!(filters.iter().all(|f| f.len() == 129));
        for (i, f) in filters.iter().enumerate() {
            assert!(
                f.iter().any(|w| *w > 0.0),
                "filter {i} is empty, so a whole band of the clip is discarded"
            );
        }
        // Ordered: each triangle peaks further along than the one before it.
        let peak = |f: &Vec<f32>| {
            f.iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
                .map(|(i, _)| i)
                .unwrap()
        };
        let peaks: Vec<usize> = filters.iter().map(peak).collect();
        assert!(peaks.windows(2).all(|w| w[1] >= w[0]), "{peaks:?}");
    }

    #[test]
    fn a_spectrogram_has_one_row_of_bands_per_window_of_the_clip() {
        let c = cfg();
        let s = tone(440.0, 0.5, c.sample_rate);
        let m = log_mel(&s, &c);
        let expected_frames = (s.len() - c.n_fft) / c.hop + 1;
        assert_eq!(m.len(), expected_frames * c.n_mels);
        assert!(m.iter().all(|v| v.is_finite()), "silence must not be -inf");
    }

    #[test]
    fn a_clip_shorter_than_one_window_yields_nothing_rather_than_a_wrong_answer() {
        let c = cfg();
        assert!(log_mel(&[0.0; 10], &c).is_empty());
    }

    #[test]
    fn spoken_numbers_come_back_as_the_digits_a_form_expects() {
        assert_eq!(words_to_digits("seven three nine"), "739");
        assert_eq!(words_to_digits("SEVEN, Three  NINE"), "739");
        assert_eq!(words_to_digits("oh one two"), "012");
    }

    #[test]
    fn characters_already_recognised_as_characters_pass_straight_through() {
        // A character-level model produces "4b7c", not words; both paths end in the same string.
        assert_eq!(words_to_digits("4b7c"), "4b7c");
        assert_eq!(words_to_digits(""), "");
    }

    #[test]
    fn a_machine_without_the_model_says_so_instead_of_guessing() {
        // The whole contract of the optional features: absent means absent, never a wrong answer.
        if !available() {
            let err = solve_bytes(&[0, 1, 2]).expect_err("must refuse");
            assert!(
                err.to_string().contains("audio"),
                "the message has to say what is missing: {err}"
            );
        }
    }
}
