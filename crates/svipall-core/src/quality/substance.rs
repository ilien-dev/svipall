//! Is this page informative, or is it engineered to look like it?
//!
//! # Why this shape and not a transformer
//!
//! DCLM (Li et al., NeurIPS 2024 Datasets & Benchmarks) is the largest public comparison of quality
//! filters — 416 controlled experiments — and its Table 4 is unambiguous:
//!
//! | filter                                   | Core |
//! |------------------------------------------|------|
//! | **hashed bigram linear classifier**      | 30.2 |
//! | top-k average logits                     | 29.2 |
//! | perplexity filtering (CCNet)             | 29.0 |
//! | prompting a language model per document  | 28.6 |
//! | heuristic rules                          | 27.5 |
//! | linear classifier on sentence embeddings | 27.2 |
//! | semantic deduplication                   | 27.1 |
//! | link-graph ranking                       | 26.1 |
//!
//! A bag of hashed bigrams with one linear layer beat every embedding- and LLM-based method,
//! including a linear head on exactly the sort of sentence embedding one would reach for first.
//! Their other findings are built in here: bigrams beat unigrams, and a percentile is the
//! threshold rather than a raw score.
//!
//! It also happens to be the only architecture that fits this codebase without bending it. There
//! is no tokenizer dependency here on purpose (see the note in `zeroshot.rs`), and the ONNX session
//! wrapper binds one input by name and reads output zero — a two-input transformer would need both
//! changed. This is a hash, an average and a matrix multiply: a few hundred lines and a weights
//! file, running in microseconds on one core.
//!
//! # What it is allowed to do
//!
//! Nothing. It labels. It never removes a page, never reorders a result set, never stops the
//! ladder and never subtracts from an integrity verdict.
//!
//! That is not timidity, it is the finding. DCLM's Figure 9: the filter that agreed *best* with
//! human quality judgement (~82% ROC-AUC) performed *worst* as a filter, and agreement with human
//! labels explained less than 30% of the variance in downstream usefulness. "Quality" is not a
//! property of a document; it is a property of a document relative to an objective. So the score
//! travels with the page and the caller decides — and per Cuconasu et al. (SIGIR 2024), where
//! adding random documents to a prompt *improved* accuracy by up to 35% while plausible-but-
//! answerless ones hurt it, the caller is right to want the odd-looking page.
//!
//! # The model is a file
//!
//! Same contract as every other model here: nothing is downloaded, nothing is called, and a
//! machine with no model file simply gets no substance score rather than a guessed one.

use serde::{Deserialize, Serialize};

/// How the score is reported. Four levels, not six and not a continuous score.
///
/// The ceiling is known in advance. FineWeb-Edu distilled a 70B model's 0–5 judgements into a small
/// classifier and published the confusion matrix: macro-F1 0.50, recall 0.35 at the second-highest
/// level and **0.01 at the highest**. It separates junk from not-junk well and is useless at the
/// top of its own scale. Pretending to more resolution than that is inventing precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Label {
    Junk,
    Thin,
    Ordinary,
    Substantive,
}

impl Label {
    pub const ALL: &'static [Label] = &[
        Label::Junk,
        Label::Thin,
        Label::Ordinary,
        Label::Substantive,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Label::Junk => "junk",
            Label::Thin => "thin",
            Label::Ordinary => "ordinary",
            Label::Substantive => "substantive",
        }
    }

    fn from_index(i: usize) -> Option<Label> {
        Self::ALL.get(i).copied()
    }
}

/// What the model says about one page, with how sure it is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Substance {
    pub label: Label,
    /// Probability of the chosen label. Reported so a caller can tell a confident `junk` from a
    /// coin flip between two neighbouring levels, which the label alone hides.
    pub confidence: f32,
}

/// The sidecar, and the only description of the file beside it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Size of the hash space. Collisions are the point of the trick, not a flaw in it.
    pub buckets: usize,
    /// Width of the shared feature vectors.
    pub dim: usize,
    /// Longest n-gram hashed. DCLM measured 2 as better than 1.
    #[serde(default = "default_ngrams")]
    pub ngrams: usize,
}

fn default_ngrams() -> usize {
    2
}

impl Default for Config {
    fn default() -> Self {
        Self {
            // 65,536 × 8 floats is two megabytes: small enough to carry, wide enough that the
            // collisions are noise rather than structure.
            buckets: 1 << 16,
            dim: 8,
            ngrams: 2,
        }
    }
}

/// A hashed n-gram averaging classifier: lookup, mean, one linear layer.
#[derive(Debug, Clone)]
pub struct Model {
    cfg: Config,
    /// `buckets × dim`, row-major.
    embedding: Vec<f32>,
    /// `dim × classes`, row-major.
    output: Vec<f32>,
    bias: Vec<f32>,
}

/// The number of classes is fixed by `Label`, not by the file: a model that disagreed about how
/// many levels there are would be read as something it is not.
const CLASSES: usize = 4;

/// Magic and version, so a file from a future format is refused rather than misread as weights.
const MAGIC: &[u8; 8] = b"SVIPSUB1";

impl Model {
    /// Untrained, and still answering "I have no idea": the output layer and the bias start at
    /// zero, so every class is equally likely until something is learned.
    ///
    /// The feature vectors do **not** start at zero, and that is not a detail. With both layers at
    /// zero the hidden vector is zero, so the gradient reaching the feature vectors is zero, and
    /// they never move — the model trains its last layer against a constant and learns nothing.
    /// Small random vectors and a zero output layer is what fastText does, for this reason.
    ///
    /// The randomness is a fixed sequence, so two machines training on the same examples get the
    /// same weights.
    pub fn new(cfg: Config) -> Self {
        let n = cfg.buckets * cfg.dim;
        let spread = 1.0 / cfg.dim.max(1) as f32;
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        let embedding = (0..n)
            .map(|_| {
                // xorshift64*, so the sequence is the same everywhere without a dependency.
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let unit = (seed >> 11) as f32 / (1u64 << 53) as f32;
                (unit * 2.0 - 1.0) * spread
            })
            .collect();
        Self {
            embedding,
            output: vec![0.0; cfg.dim * CLASSES],
            bias: vec![0.0; CLASSES],
            cfg,
        }
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    /// Read a model file against its sidecar.
    pub fn load(bytes: &[u8], sidecar: &str) -> anyhow::Result<Self> {
        let cfg: Config = serde_json::from_str(sidecar)?;
        if bytes.len() < MAGIC.len() {
            anyhow::bail!("substance model is too short to be one");
        }
        if &bytes[..MAGIC.len()] != MAGIC {
            anyhow::bail!("substance model does not carry the expected format marker");
        }
        let floats = &bytes[MAGIC.len()..];
        if !floats.len().is_multiple_of(4) {
            anyhow::bail!("substance model is not a whole number of floats");
        }
        let want = cfg.buckets * cfg.dim + cfg.dim * CLASSES + CLASSES;
        let got = floats.len() / 4;
        // Refused, never reshaped — the same rule the image models are held to.
        if got != want {
            anyhow::bail!(
                "substance model holds {got} weights; its sidecar describes {want} \
                 ({} buckets x {} dim, {CLASSES} classes)",
                cfg.buckets,
                cfg.dim
            );
        }
        let mut w: Vec<f32> = Vec::with_capacity(got);
        for c in floats.as_chunks::<4>().0 {
            w.push(f32::from_le_bytes(*c));
        }
        let split = cfg.buckets * cfg.dim;
        let out_split = split + cfg.dim * CLASSES;
        Ok(Self {
            embedding: w[..split].to_vec(),
            output: w[split..out_split].to_vec(),
            bias: w[out_split..].to_vec(),
            cfg,
        })
    }

    /// The bytes to write beside the sidecar.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            MAGIC.len() + 4 * (self.embedding.len() + self.output.len() + self.bias.len()),
        );
        out.extend_from_slice(MAGIC);
        for v in self.embedding.iter().chain(&self.output).chain(&self.bias) {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }

    /// The sidecar that describes this model.
    pub fn sidecar(&self) -> String {
        serde_json::to_string(&self.cfg).unwrap_or_default()
    }

    /// What this page looks like.
    pub fn predict(&self, text: &str) -> Substance {
        let probs = self.probabilities(&self.hidden(text));
        let (best, p) = probs
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, p)| (i, *p))
            .unwrap_or((0, 0.0));
        Substance {
            label: Label::from_index(best).unwrap_or(Label::Junk),
            confidence: p,
        }
    }

    /// Mean of the feature vectors of every hashed n-gram in the text.
    fn hidden(&self, text: &str) -> Vec<f32> {
        let mut h = vec![0.0f32; self.cfg.dim];
        let mut n = 0usize;
        for bucket in features(text, self.cfg.ngrams, self.cfg.buckets) {
            let row = &self.embedding[bucket * self.cfg.dim..(bucket + 1) * self.cfg.dim];
            for (acc, v) in h.iter_mut().zip(row) {
                *acc += v;
            }
            n += 1;
        }
        if n > 0 {
            let inv = 1.0 / n as f32;
            for v in &mut h {
                *v *= inv;
            }
        }
        h
    }

    fn probabilities(&self, hidden: &[f32]) -> [f32; CLASSES] {
        let mut logits = [0.0f32; CLASSES];
        for (c, logit) in logits.iter_mut().enumerate() {
            let mut s = self.bias[c];
            for (d, h) in hidden.iter().enumerate() {
                s += h * self.output[d * CLASSES + c];
            }
            *logit = s;
        }
        softmax(logits)
    }

    /// One pass of stochastic gradient descent over the examples.
    ///
    /// Training lives here rather than in a script for the same reason everything else does: it is
    /// local. No framework, no export step, no Python — the thing that produces the weights and the
    /// thing that reads them are the same code, so they cannot drift apart.
    pub fn fit(&mut self, examples: &[(String, Label)], lr: f32) {
        for (text, want) in examples {
            let target = Label::ALL.iter().position(|l| l == want).unwrap_or(0);
            let buckets: Vec<usize> = features(text, self.cfg.ngrams, self.cfg.buckets).collect();
            if buckets.is_empty() {
                continue;
            }
            let hidden = self.hidden(text);
            let probs = self.probabilities(&hidden);

            // dL/dlogit for cross-entropy with a softmax is (p - y).
            let mut grad_hidden = vec![0.0f32; self.cfg.dim];
            for (c, p) in probs.iter().enumerate() {
                let g = p - if c == target { 1.0 } else { 0.0 };
                self.bias[c] -= lr * g;
                for (d, gh) in grad_hidden.iter_mut().enumerate() {
                    *gh += g * self.output[d * CLASSES + c];
                    self.output[d * CLASSES + c] -= lr * g * hidden[d];
                }
            }

            // The hidden vector is a mean, so each contributing row takes its share.
            let share = lr / buckets.len() as f32;
            for b in buckets {
                let row = &mut self.embedding[b * self.cfg.dim..(b + 1) * self.cfg.dim];
                for (v, g) in row.iter_mut().zip(&grad_hidden) {
                    *v -= share * g;
                }
            }
        }
    }
}

fn softmax(mut logits: [f32; CLASSES]) -> [f32; CLASSES] {
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut sum = 0.0;
    for v in &mut logits {
        *v = (*v - max).exp();
        sum += *v;
    }
    if sum > 0.0 {
        for v in &mut logits {
            *v /= sum;
        }
    }
    logits
}

/// The hashed unigrams and n-grams of a text.
///
/// Words are lowercased and stripped of surrounding punctuation, and nothing else is done to them.
/// There is no stemming and no stop-word list, which is deliberate: both are per-language, and a
/// per-language step is how a classifier quietly learns that a language is a quality.
fn features(text: &str, ngrams: usize, buckets: usize) -> impl Iterator<Item = usize> + '_ {
    let words: Vec<&str> = text
        .split_whitespace()
        .take(MAX_WORDS)
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|w| !w.is_empty())
        .collect();

    let mut out: Vec<usize> = Vec::with_capacity(words.len() * ngrams);
    for n in 1..=ngrams.max(1) {
        for w in words.windows(n) {
            out.push(hash_ngram(w) as usize % buckets.max(1));
        }
    }
    out.into_iter()
}

/// Words read from one document. A page is classified from its first few thousand words for the
/// same reason its shape is: the answer stops changing, and the cost should stop growing.
const MAX_WORDS: usize = 4_000;

/// FNV-1a over the lowercased words, with a separator so `["a b", "c"]` and `["a", "b c"]` differ.
fn hash_ngram(words: &[&str]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for (i, w) in words.iter().enumerate() {
        if i > 0 {
            h ^= 0x20;
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
        for b in w.as_bytes() {
            h ^= b.to_ascii_lowercase() as u64;
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny separable problem: two vocabularies that never overlap.
    fn training_set() -> Vec<(String, Label)> {
        let mut out = Vec::new();
        for i in 0..40 {
            out.push((
                format!(
                    "the council voted on tuesday to approve the harbour measure after debate {i} \
                     supporters argued the change was overdue and the money was already set aside"
                ),
                Label::Substantive,
            ));
            out.push((
                format!(
                    "best cheap anvils {i} buy now click here best price cheap anvils best deal \
                     limited offer buy cheap anvils now best cheap price"
                ),
                Label::Junk,
            ));
        }
        out
    }

    #[test]
    fn a_model_learns_to_tell_two_kinds_of_page_apart() {
        // The whole loop, end to end: an untrained model, a handful of passes, and a prediction on
        // text it has not seen. If this fails, nothing downstream of it means anything.
        let mut m = Model::new(Config::default());
        let data = training_set();
        for _ in 0..30 {
            m.fit(&data, 0.5);
        }

        let article = m.predict(
            "the council voted on wednesday to approve the eastern quay measure after a debate \
             supporters argued the money was already set aside for it",
        );
        let listicle = m.predict("best cheap anvils buy now click here for the best cheap price");
        assert_eq!(article.label, Label::Substantive, "{article:?}");
        assert_eq!(listicle.label, Label::Junk, "{listicle:?}");
        assert!(article.confidence > 0.5, "{article:?}");
    }

    #[test]
    fn an_untrained_model_is_undecided_rather_than_confident() {
        // A model with no weights must not answer as though it knew. Four classes, no evidence:
        // a quarter each.
        let m = Model::new(Config::default());
        let got = m.predict("anything at all");
        assert!((got.confidence - 0.25).abs() < 1e-5, "{got:?}");
    }

    #[test]
    fn a_model_survives_a_round_trip_through_its_file() {
        let mut m = Model::new(Config::default());
        m.fit(&training_set(), 0.5);
        let before = m.predict("best cheap anvils buy now");

        let back = Model::load(&m.to_bytes(), &m.sidecar()).expect("round trip");
        let after = back.predict("best cheap anvils buy now");
        assert_eq!(before.label, after.label);
        assert!((before.confidence - after.confidence).abs() < 1e-6);
    }

    #[test]
    fn a_file_that_does_not_match_its_sidecar_is_refused_rather_than_reshaped() {
        // The same rule the image models are held to: a shape that does not match the contract is
        // an error, because the alternative is reading noise as weights and answering from it.
        let m = Model::new(Config {
            buckets: 64,
            dim: 4,
            ngrams: 2,
        });
        let bytes = m.to_bytes();
        let lying = serde_json::to_string(&Config {
            buckets: 128,
            dim: 4,
            ngrams: 2,
        })
        .unwrap();
        let err = Model::load(&bytes, &lying).expect_err("must refuse");
        assert!(err.to_string().contains("weights"), "{err}");
    }

    #[test]
    fn something_that_is_not_a_model_is_not_read_as_one() {
        let cfg = serde_json::to_string(&Config::default()).unwrap();
        assert!(Model::load(b"not a model at all", &cfg).is_err());
        assert!(Model::load(b"", &cfg).is_err());
    }

    #[test]
    fn a_bigram_is_not_the_same_feature_as_its_two_words() {
        // Why DCLM found bigrams worth the extra features: "cheap anvils" has to be able to mean
        // something that "cheap" and "anvils" do not.
        let one: Vec<usize> = features("cheap anvils", 1, 1 << 16).collect();
        let two: Vec<usize> = features("cheap anvils", 2, 1 << 16).collect();
        assert_eq!(one.len(), 2);
        assert_eq!(two.len(), 3, "two words and the pair they make");
        assert!(!one.contains(&two[2]), "the pair is its own feature");
    }

    #[test]
    fn word_order_inside_an_ngram_matters() {
        assert_ne!(
            hash_ngram(&["cheap", "anvils"]),
            hash_ngram(&["anvils", "cheap"])
        );
    }
}
