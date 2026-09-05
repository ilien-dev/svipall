//! Image-grid challenges ("select all images with traffic lights"), solved locally.
//!
//! Two automatic attempts, then a person. That split is deliberate: a classifier that is right
//! about 90% of a tile at a time is wrong about a whole 3x3 grid often enough that retrying
//! forever would be slower *and* more suspicious than handing over, while never trying at all
//! wastes the cases it does get right. After the second failure the visible window opens, which is
//! what happened on the first failure before.
//!
//! Same arrangement as the text OCR next door: the inference path compiles under a feature flag,
//! the model is the embedded one or the operator's file (`model_source`), and with neither the
//! whole thing reports unavailable and the caller falls through to the human. Nothing here talks
//! to a network.
//!
//! `grid.onnx` — an image classifier, NCHW input, one score per class.
//! `grid.json` — the contract:
//!
//! ```json
//! { "height": 224, "width": 224, "channels": 3, "normalize": true, "threshold": 0.2,
//!   "classes": ["bicycle", "bus", "car", "crosswalk", "fire hydrant", "traffic light"] }
//! ```
//!
//! The default threshold is 0.2, not 0.5. That is the value the one peer-reviewed measurement of
//! this task settled on (Plesner et al., COMPSAC 2024): a grid asks "which of these *contain* a
//! bus", and a tile with a bus in one corner scores well below the point where a classifier would
//! call it "a picture of a bus". Half misses those tiles, and a missed tile fails the grid.

use anyhow::{anyhow, Result};

use crate::model_source::{self, Located};

/// How many times the grid is attempted automatically before a person is asked.
pub const MAX_ATTEMPTS: usize = 2;

/// The classifier, wherever it lives.
pub fn locate() -> Option<Located> {
    model_source::locate("grid", "grid", "onnx", svipall_models::grid())
}

/// True when a usable grid classifier is installed, or a detector can stand in for one.
pub fn available() -> bool {
    (cfg!(feature = "onnx-grid") && locate().is_some()) || crate::detect::available()
}

/// Model input contract and the classes it can name.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct GridConfig {
    pub height: u32,
    pub width: u32,
    #[serde(default = "three")]
    pub channels: u32,
    #[serde(default = "yes")]
    pub normalize: bool,
    /// Score above which a tile counts as a match.
    #[serde(default = "fifth")]
    pub threshold: f32,
    /// The output is one independent probability per class ("contains a bus", "contains a
    /// car") rather than one distribution over classes. Such rows do not sum to one, and must
    /// not be pushed through a softmax to make them.
    #[serde(default)]
    pub multilabel: bool,
    /// Class names, in the model's output order. The widget's wording is matched against these,
    /// so the vocabulary belongs to the model rather than to this file.
    pub classes: Vec<String>,
}

fn three() -> u32 {
    3
}
fn yes() -> bool {
    true
}
fn fifth() -> f32 {
    0.2
}

pub fn load_config() -> Result<GridConfig> {
    let cfg = match locate() {
        Some(located) => {
            let cfg: GridConfig = located.config()?;
            if cfg.classes.is_empty() {
                return Err(anyhow!("{} lists no classes", located.describe()));
            }
            cfg
        }
        // No classifier: the detector answers "does this tile contain one" instead, with the
        // vocabulary the detector has.
        None if crate::detect::available() => {
            let d = crate::detect::load_config()?;
            GridConfig {
                height: d.height,
                width: d.width,
                channels: d.channels,
                normalize: d.normalize,
                threshold: fifth(),
                multilabel: true,
                classes: d.classes,
            }
        }
        None => return Err(anyhow!("no grid model installed or embedded")),
    };
    Ok(cfg)
}

/// Reduce a word to something comparable: lowercase, no punctuation, no plural 's'.
///
/// The widget says "traffic lights" and a model's class is "traffic light". Matching them without
/// this is the difference between solving the challenge and not recognising the question.
fn stem(word: &str) -> String {
    let w: String = word
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    match w.strip_suffix("es") {
        // "buses" -> "bus", but not "lanes" -> "lan".
        Some(base) if base.ends_with('s') || base.ends_with("ch") || base.ends_with("sh") => {
            base.to_string()
        }
        _ => w.strip_suffix('s').unwrap_or(&w).to_string(),
    }
}

fn stems(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(stem)
        .collect()
}

/// Which class the widget is asking about, if it is one the model knows.
///
/// Longest match wins, so "fire hydrant" is not beaten by a class called "fire". An unknown
/// subject returns `None`, and the caller hands the challenge to a person rather than guessing —
/// clicking the wrong tiles is worse than not clicking, because it burns the attempt.
pub fn label_for(prompt: &str, classes: &[String]) -> Option<usize> {
    let words = stems(prompt);
    let mut best: Option<(usize, usize)> = None;
    for (i, class) in classes.iter().enumerate() {
        let wanted = stems(class);
        if wanted.is_empty() {
            continue;
        }
        // Every word of the class name has to appear, in order, somewhere in the prompt.
        let found = words
            .windows(wanted.len())
            .any(|w| w.iter().zip(&wanted).all(|(a, b)| a == b));
        if found && best.is_none_or(|(_, len)| wanted.len() > len) {
            best = Some((i, wanted.len()));
        }
    }
    best.map(|(i, _)| i)
}

/// Tiles whose score clears the threshold, highest first.
///
/// Ordered by confidence rather than by position because that is the order they get clicked in,
/// and a grid clicked strictly left-to-right every time is its own signal.
pub fn select(scores: &[f32], threshold: f32) -> Vec<usize> {
    let mut hits: Vec<(usize, f32)> = scores
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, s)| *s >= threshold)
        .collect();
    hits.sort_by(|a, b| b.1.total_cmp(&a.1));
    hits.into_iter().map(|(i, _)| i).collect()
}

/// Where each tile of an `rows` x `cols` grid sits, in the coordinates of whatever `x`/`y` are in.
///
/// Row-major, matching the order the widget lists its tiles, so a score index and a tile index
/// mean the same thing.
pub fn tile_boxes(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    rows: usize,
    cols: usize,
) -> Vec<(f64, f64, f64, f64)> {
    if rows == 0 || cols == 0 {
        return Vec::new();
    }
    let (tw, th) = (width / cols as f64, height / rows as f64);
    (0..rows)
        .flat_map(|r| (0..cols).map(move |c| (r, c)))
        .map(|(r, c)| (x + c as f64 * tw, y + r as f64 * th, tw, th))
        .collect()
}

/// Score every tile for one class. Errors when there is no model, which is not a failure: the
/// caller falls through to a person.
pub fn classify(tiles: &[Vec<u8>], class: usize) -> Result<Vec<f32>> {
    #[cfg(feature = "onnx-grid")]
    if locate().is_some() {
        return imp::classify(tiles, class);
    }
    // "Does this square contain a bus" is "does the detector see a bus in this square": the
    // strongest box of that class is the tile's score. The one peer-reviewed measurement of
    // these grids put its selection threshold at 0.2 for exactly this reading.
    if crate::detect::available() {
        return tiles
            .iter()
            .map(|png| crate::detect::strongest(png, class))
            .collect();
    }
    Err(anyhow!(
        "no grid classifier and no detector to stand in for one (build with --features onnx-grid or onnx-detect)"
    ))
}

#[cfg(feature = "onnx-grid")]
mod imp {
    use super::*;
    use crate::model_source::SessionCache;

    static SESSION: SessionCache = SessionCache::new();

    pub fn classify(tiles: &[Vec<u8>], class: usize) -> Result<Vec<f32>> {
        let located = locate().ok_or_else(|| anyhow!("no grid model installed or embedded"))?;
        let cfg: GridConfig = located.config()?;
        if class >= cfg.classes.len() {
            return Err(anyhow!(
                "class {class} is outside the model's {} classes",
                cfg.classes.len()
            ));
        }
        SESSION.with(&located, |sess| {
            let name = sess.inputs()[0].name().to_string();
            let mut out = Vec::with_capacity(tiles.len());
            for bytes in tiles {
                let img = image::load_from_memory(bytes)?;
                let (shape, data) = model_source::image_tensor(
                    &img,
                    cfg.width,
                    cfg.height,
                    cfg.channels,
                    cfg.normalize,
                );
                let input = ort::value::Tensor::from_array((shape, data))?;
                let outputs = sess.run(ort::inputs![name.as_str() => input])?;
                let (_, scores) = outputs[0].try_extract_tensor::<f32>()?;
                out.push(super::score_of(
                    scores,
                    class,
                    cfg.classes.len(),
                    cfg.multilabel,
                ));
            }
            Ok(out)
        })
    }
}

/// One class's probability from a raw output row.
///
/// Models are shipped both ways — already normalised, or as logits — and telling them apart by
/// eye is not something a caller should have to do. A row that already sums to about 1 is left
/// alone; anything else goes through a softmax first, so the configured threshold means the same
/// thing either way. A `multilabel` row is one probability per class and is never softmaxed.
pub fn score_of(row: &[f32], class: usize, classes: usize, multilabel: bool) -> f32 {
    let row = &row[..row.len().min(classes.max(1))];
    let Some(&raw) = row.get(class) else {
        return 0.0;
    };
    if multilabel {
        return raw.clamp(0.0, 1.0);
    }
    let sum: f32 = row.iter().sum();
    let looks_normalised = row.iter().all(|v| (0.0..=1.0).contains(v)) && (sum - 1.0).abs() < 0.05;
    if looks_normalised {
        return raw;
    }
    let max = row.iter().copied().fold(f32::MIN, f32::max);
    let exp: Vec<f32> = row.iter().map(|v| (v - max).exp()).collect();
    let total: f32 = exp.iter().sum();
    if total <= 0.0 {
        0.0
    } else {
        exp[class] / total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classes() -> Vec<String> {
        [
            "bicycle",
            "bus",
            "car",
            "crosswalk",
            "fire hydrant",
            "traffic light",
            "fire",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    #[test]
    fn the_plural_in_the_prompt_still_finds_the_singular_class() {
        let c = classes();
        let i = label_for("Select all images with traffic lights", &c).expect("matched");
        assert_eq!(c[i], "traffic light");
    }

    #[test]
    fn a_two_word_class_beats_a_one_word_class_that_is_a_prefix_of_it() {
        let c = classes();
        let i = label_for("Select all squares with fire hydrants", &c).expect("matched");
        assert_eq!(
            c[i], "fire hydrant",
            "matching 'fire' would click the wrong tiles and burn the attempt"
        );
    }

    #[test]
    fn an_unknown_subject_is_none_rather_than_a_guess() {
        assert!(label_for("Select all images with chimneys", &classes()).is_none());
    }

    #[test]
    fn the_words_have_to_be_adjacent() {
        // "fire" and "hydrant" both appear, but not as the phrase.
        let c = vec!["fire hydrant".to_string()];
        assert!(label_for("a hydrant is not on fire", &c).is_none());
    }

    #[test]
    fn plurals_ending_in_es_are_stemmed_without_eating_the_word() {
        assert_eq!(stem("buses"), "bus");
        assert_eq!(stem("crosswalks"), "crosswalk");
        assert_eq!(stem("lanes"), "lane");
        assert_eq!(stem("Lights."), "light");
    }

    #[test]
    fn only_tiles_over_the_threshold_are_selected_and_the_surest_goes_first() {
        let scores = [0.9, 0.1, 0.62, 0.5, 0.99];
        assert_eq!(select(&scores, 0.6), vec![4, 0, 2]);
    }

    #[test]
    fn nothing_over_the_threshold_selects_nothing() {
        assert!(select(&[0.1, 0.2], 0.6).is_empty());
    }

    #[test]
    fn tiles_are_laid_out_row_major_and_cover_the_whole_grid() {
        let boxes = tile_boxes(100.0, 200.0, 300.0, 300.0, 3, 3);
        assert_eq!(boxes.len(), 9);
        assert_eq!(boxes[0], (100.0, 200.0, 100.0, 100.0));
        // Index 1 is the tile to the right, not the one below: score index 1 has to mean the same
        // tile the widget calls 1.
        assert_eq!(boxes[1].0, 200.0);
        assert_eq!(boxes[1].1, 200.0);
        assert_eq!(boxes[3].0, 100.0);
        assert_eq!(boxes[3].1, 300.0);
        assert_eq!(boxes[8], (300.0, 400.0, 100.0, 100.0));
    }

    #[test]
    fn a_four_by_four_grid_is_handled_too() {
        assert_eq!(tile_boxes(0.0, 0.0, 400.0, 400.0, 4, 4).len(), 16);
    }

    #[test]
    fn an_empty_grid_is_empty_not_a_division_by_zero() {
        assert!(tile_boxes(0.0, 0.0, 100.0, 100.0, 0, 3).is_empty());
    }

    #[test]
    fn probabilities_are_passed_through_and_logits_are_softmaxed() {
        // Already a distribution: left alone.
        let p = score_of(&[0.1, 0.7, 0.2], 1, 3, false);
        assert!((p - 0.7).abs() < 1e-6, "{p}");

        // Logits: the largest becomes the largest probability, and they sum to 1.
        let a = score_of(&[2.0, 8.0, 1.0], 1, 3, false);
        let b = score_of(&[2.0, 8.0, 1.0], 0, 3, false);
        let c = score_of(&[2.0, 8.0, 1.0], 2, 3, false);
        assert!(a > 0.9, "{a}");
        assert!(((a + b + c) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn a_multilabel_row_is_read_as_is_because_a_softmax_would_flatten_it() {
        // Two classes both clearly present: a tile can hold a bus *and* a car. Softmaxing
        // [0.9, 0.8] gives about [0.52, 0.48], which reads as "barely either".
        assert!((score_of(&[0.9, 0.8], 0, 2, true) - 0.9).abs() < 1e-6);
        assert!((score_of(&[0.9, 0.8], 1, 2, true) - 0.8).abs() < 1e-6);
        assert!(score_of(&[0.9, 0.8], 0, 2, false) < 0.6);
    }

    #[test]
    fn a_class_index_past_the_end_scores_zero_rather_than_panicking() {
        assert_eq!(score_of(&[0.5, 0.5], 9, 2, false), 0.0);
    }

    #[test]
    fn without_a_model_classification_is_an_error_and_never_a_guess() {
        // The contract the fallback depends on: no model means "ask a person", not "click at
        // random", which would spend the attempt and teach the widget we are a bot.
        if !available() {
            assert!(classify(&[vec![0u8; 4]], 0).is_err());
        }
    }

    #[test]
    fn the_sidecar_can_say_its_rows_are_independent_probabilities() {
        let cfg: GridConfig =
            serde_json::from_str(r#"{"height":8,"width":8,"classes":["a"],"multilabel":true}"#)
                .unwrap();
        assert!(cfg.multilabel);
        let cfg: GridConfig =
            serde_json::from_str(r#"{"height":8,"width":8,"classes":["a"]}"#).unwrap();
        assert!(!cfg.multilabel, "a distribution is the default");
    }

    #[test]
    fn the_config_needs_classes_to_be_usable_and_defaults_to_the_measured_threshold() {
        let bad: Result<GridConfig, _> =
            serde_json::from_str(r#"{"height":224,"width":224,"classes":[]}"#);
        assert!(
            bad.is_ok(),
            "parsing succeeds; the emptiness check is in load_config"
        );
        let cfg = bad.unwrap();
        assert!(cfg.classes.is_empty());
        assert_eq!(cfg.channels, 3, "an image classifier defaults to RGB");
        // 0.2 is what the COMPSAC 2024 measurement of reCAPTCHAv2 grids used; 0.5 misses the
        // tiles where the subject is small or partial, and a missed tile fails the grid.
        assert!((cfg.threshold - 0.2).abs() < f32::EPSILON);
    }
}
