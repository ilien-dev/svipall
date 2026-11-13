//! What has been seen of a video, kept between calls, and finding a moment in it.
//!
//! Looking at a video is several calls: the whole of it, then a part, then a moment. Each one
//! captures pictures, reads the text on them and, when the image-text model is installed, turns
//! them into vectors. None of that is done twice: it is written beside the frames, one file per
//! video, and the next call starts from it.
//!
//! Finding a moment asks three things of that record, and says which one answered: what was
//! `said` (the captions), what was `shown` (text read off the frames) and what was `seen` (the
//! picture itself, compared with the words by the image-text model).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const VERSION: u32 = 1;

/// One moment that was looked at.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Seen {
    pub t: f64,
    /// The frame written to disk; `None` for a storyboard cell, which is only ever a vector.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The frame's size in pixels, for what it costs to read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<(u32, u32)>,
    /// The text on screen, once read. `Some("")` is a frame read and found to show none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shown: Option<String>,
    /// The picture as the image-text model sees it: little-endian `f32`s, base64.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vec: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Index {
    #[serde(default)]
    version: u32,
    /// Captured frames, in time order.
    #[serde(default)]
    pub frames: Vec<Seen>,
    /// Storyboard cells, in time order: vectors only.
    #[serde(default)]
    pub cells: Vec<Seen>,
    /// Moments captured and found to show the same picture as a frame already kept: the moment,
    /// and that frame's path. They were looked at, so they are not looked at again.
    #[serde(default)]
    pub repeats: Vec<(f64, String)>,
}

/// Where a video's record is kept: beside its frames.
pub fn file(dir: &Path) -> PathBuf {
    dir.join("index.json")
}

impl Index {
    /// The record in `dir`, or an empty one: a missing, unreadable or older file is a video not
    /// looked at yet, never an error.
    pub fn load(dir: &Path) -> Self {
        std::fs::read_to_string(file(dir))
            .ok()
            .and_then(|s| serde_json::from_str::<Index>(&s).ok())
            .filter(|i| i.version == VERSION)
            .unwrap_or_default()
    }

    pub fn save(&mut self, dir: &Path) -> anyhow::Result<()> {
        self.version = VERSION;
        self.frames.sort_by(|a, b| a.t.total_cmp(&b.t));
        self.cells.sort_by(|a, b| a.t.total_cmp(&b.t));
        std::fs::create_dir_all(dir)?;
        std::fs::write(file(dir), serde_json::to_vec(self)?)?;
        Ok(())
    }

    /// The frame already captured within `within` seconds of `t` whose file is still there: one
    /// captured at that moment, or the one a capture at that moment was found to repeat.
    pub fn frame_near(&self, t: f64, within: f64) -> Option<&Seen> {
        let own = self.frames.iter().map(|f| (f.t, f));
        let repeated = self.repeats.iter().filter_map(|(at, path)| {
            let f = self
                .frames
                .iter()
                .find(|f| f.path.as_deref() == Some(path))?;
            Some((*at, f))
        });
        own.chain(repeated)
            .filter(|(at, _)| (at - t).abs() <= within)
            .filter(|(_, f)| f.path.as_deref().is_some_and(|p| Path::new(p).is_file()))
            .min_by(|a, b| (a.0 - t).abs().total_cmp(&(b.0 - t).abs()))
            .map(|(_, f)| f)
    }

    /// Record a frame, replacing one at the same path.
    pub fn put_frame(&mut self, seen: Seen) {
        match self.frames.iter_mut().find(|f| f.path == seen.path) {
            Some(f) => *f = seen,
            None => self.frames.push(seen),
        }
    }
}

pub fn pack(v: &[f32]) -> String {
    let bytes: Vec<u8> = v.iter().flat_map(|f| f.to_le_bytes()).collect();
    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes)
}

pub fn unpack(s: &str) -> Option<Vec<f32>> {
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, s).ok()?;
    (bytes.len() % 4 == 0).then(|| {
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect()
    })
}

/// A moment found.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hit {
    pub t: f64,
    /// `said`, `shown` or `seen`.
    pub by: &'static str,
    /// The words around the match, for `said` and `shown`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// How well it matched: the share of the words found, or the picture's likeness to them.
    pub score: f32,
    /// The frame, when the moment is one that was captured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// Lower case, accents off: a search for `funcion` finds `función`, and the other way round.
fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            c => c,
        })
        .collect()
}

/// Words that say nothing about a moment, in the two languages queries come in most.
const EMPTY_WORDS: &[&str] = &[
    "the", "of", "and", "or", "in", "on", "at", "to", "is", "are", "was", "it", "an", "for",
    "with", "by", "as", "that", "this", "where", "when", "what", "el", "la", "los", "las", "un",
    "una", "de", "del", "en", "que", "se", "es", "al", "lo", "con", "por", "para", "como", "donde",
    "cuando", "su",
];

/// The words of a query worth matching: folded, nothing shorter than two letters, and none of
/// the words every sentence has. A query made only of those keeps them: it is all there is.
pub fn words(query: &str) -> Vec<String> {
    let folded = fold(query);
    let all: Vec<String> = folded
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 2)
        .map(str::to_string)
        .collect();
    let mut out: Vec<String> = all
        .iter()
        .filter(|w| !EMPTY_WORDS.contains(&w.as_str()))
        .cloned()
        .collect();
    if out.is_empty() {
        out = all;
    }
    out.sort();
    out.dedup();
    out
}

/// Whether a word of the text is the word looked for: the same, or one of them the other with an
/// ending (`result` and `results` find each other). Never a word that merely contains it:
/// `table` is not in `comfortable`.
fn is_word(have: &str, want: &str) -> bool {
    let long = |w: &str| w.chars().count() >= 4;
    have == want || (long(have) && long(want) && (have.starts_with(want) || want.starts_with(have)))
}

/// The passages that hold the query's words, best first. A passage counts when it holds at least
/// half of them; one word of five is a coincidence, not a match.
pub fn text_hits<'a>(
    query: &str,
    passages: impl Iterator<Item = (f64, &'a str, Option<&'a str>)>,
    by: &'static str,
) -> Vec<Hit> {
    let want = words(query);
    if want.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<Hit> = passages
        .filter_map(|(t, text, path)| {
            // Folding keeps the letter count, so a position in the folded text is the same
            // position, counted in characters, in the original.
            let folded: Vec<char> = fold(text).chars().collect();
            let mut have: Vec<(usize, String)> = Vec::new();
            let mut start = None;
            for (i, c) in folded.iter().enumerate() {
                match (c.is_alphanumeric(), start) {
                    (true, None) => start = Some(i),
                    (false, Some(s)) => {
                        have.push((s, folded[s..i].iter().collect()));
                        start = None;
                    }
                    _ => {}
                }
            }
            if let Some(s) = start {
                have.push((s, folded[s..].iter().collect()));
            }
            let first = |w: &String| have.iter().find(|(_, h)| is_word(h, w)).map(|(at, _)| *at);
            let found: Vec<usize> = want.iter().filter_map(first).collect();
            let score = found.len() as f32 / want.len() as f32;
            if score < 0.5 {
                return None;
            }
            let at = found.iter().copied().min().unwrap_or(0);
            let chars: Vec<char> = text.chars().collect();
            let (lo, hi) = (at.saturating_sub(60), (at + 140).min(chars.len()));
            let mut snippet: String = chars[lo.min(hi)..hi].iter().collect();
            snippet = snippet.split_whitespace().collect::<Vec<_>>().join(" ");
            if lo > 0 {
                snippet.insert(0, '…');
            }
            if hi < chars.len() {
                snippet.push('…');
            }
            Some(Hit {
                t,
                by,
                text: Some(snippet),
                score,
                path: path.map(str::to_string),
            })
        })
        .collect();
    hits.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.t.total_cmp(&b.t)));
    hits
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let (na, nb) = (
        a.iter().map(|x| x * x).sum::<f32>().sqrt(),
        b.iter().map(|x| x * x).sum::<f32>().sqrt(),
    );
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

/// How far the best picture must stand above the middle one for the model to be saying anything.
/// Measured in likeness, where every picture of one video already scores within a few hundredths
/// of every other.
pub const STANDS_OUT: f32 = 0.015;

/// The `k` moments whose picture is most like the words, best first, no two within `apart`
/// seconds: neighbouring frames of one scene are one answer, not five.
///
/// The second value is false when no picture stands out from the rest: the model then cannot
/// tell them apart for this query, and the moments are its order and nothing more.
pub fn visual_hits(
    query: &[f32],
    seen: &[(f64, Vec<f32>, Option<String>)],
    k: usize,
    apart: f64,
) -> (Vec<Hit>, bool) {
    let mut scored: Vec<(usize, f32)> = seen
        .iter()
        .enumerate()
        .map(|(i, (_, v, _))| (i, cosine(query, v)))
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    let stands_out = match (scored.first(), scored.get(scored.len() / 2)) {
        (Some(best), Some(mid)) if scored.len() >= 4 => best.1 - mid.1 >= STANDS_OUT,
        _ => !scored.is_empty(),
    };
    let mut out: Vec<Hit> = Vec::new();
    for (i, score) in scored {
        if out.len() >= k {
            break;
        }
        let (t, _, path) = &seen[i];
        if out.iter().all(|h| (h.t - t).abs() >= apart) {
            out.push(Hit {
                t: *t,
                by: "seen",
                text: None,
                score: (score * 1000.0).round() / 1000.0,
                path: path.clone(),
            });
        }
    }
    (out, stands_out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_survives_a_round_trip_and_an_unreadable_one_is_empty() {
        let dir = std::env::temp_dir().join(format!("svipall-index-{}", uuid::Uuid::new_v4()));
        assert_eq!(Index::load(&dir), Index::default());
        let frame = dir.join("00-00-05.png");
        let mut i = Index::default();
        i.frames.push(Seen {
            t: 9.0,
            path: Some("/gone.png".into()),
            ..Default::default()
        });
        i.put_frame(Seen {
            t: 5.0,
            path: Some(frame.to_string_lossy().into_owned()),
            size: Some((640, 360)),
            shown: Some("hola".into()),
            vec: Some(pack(&[0.5, -1.25])),
        });
        i.save(&dir).unwrap();
        std::fs::write(&frame, b"x").unwrap();
        let back = Index::load(&dir);
        assert_eq!(back.frames.len(), 2);
        assert_eq!(back.frames[0].t, 5.0, "kept in time order");
        assert_eq!(
            unpack(back.frames[0].vec.as_deref().unwrap()),
            Some(vec![0.5, -1.25])
        );
        assert_eq!(back.frame_near(5.2, 0.5).map(|f| f.t), Some(5.0));
        assert!(back.frame_near(6.0, 0.5).is_none());
        // A moment that showed the same picture as that frame was looked at too.
        let mut again = back.clone();
        again
            .repeats
            .push((6.0, frame.to_string_lossy().into_owned()));
        assert_eq!(again.frame_near(6.1, 0.5).map(|f| f.t), Some(5.0));
        assert!(
            back.frame_near(9.0, 0.5).is_none(),
            "a frame whose file is gone is not a frame"
        );
        std::fs::write(file(&dir), b"{not json").unwrap();
        assert_eq!(Index::load(&dir), Index::default());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_frame_captured_again_replaces_itself() {
        let mut i = Index::default();
        for shown in ["a", "b"] {
            i.put_frame(Seen {
                t: 1.0,
                path: Some("/f.png".into()),
                shown: Some(shown.into()),
                ..Default::default()
            });
        }
        assert_eq!(i.frames.len(), 1);
        assert_eq!(i.frames[0].shown.as_deref(), Some("b"));
    }

    #[test]
    fn words_are_found_whatever_their_accents_and_case() {
        let passages = [
            (
                10.0,
                "Ahora definimos la FUNCIÓN principal del módulo",
                None,
            ),
            (40.0, "y aquí no hay nada de eso", None),
            (70.0, "la funcion se llama dos veces", Some("/f/70.png")),
        ];
        let hits = text_hits("función principal", passages.iter().copied(), "said");
        assert_eq!(hits.len(), 2, "{hits:?}");
        assert_eq!((hits[0].t, hits[0].score), (10.0, 1.0));
        assert_eq!((hits[1].t, hits[1].score), (70.0, 0.5));
        assert_eq!(hits[1].path.as_deref(), Some("/f/70.png"));
        assert!(hits[0]
            .text
            .as_deref()
            .unwrap()
            .contains("FUNCIÓN principal"));
        assert!(text_hits("a", passages.iter().copied(), "said").is_empty());
    }

    #[test]
    fn a_word_is_matched_as_a_word_and_empty_words_do_not_count() {
        let passages = [
            (
                0.0,
                "a comfortable chair, of all the results you could get",
                None,
            ),
            (30.0, "here is the table with every result", None),
            (60.0, "nothing of the sort", None),
        ];
        // `of`, `a` and `the` are in every passage and decide nothing; `table` is not in
        // `comfortable`; `result` and `results` are one word.
        let hits = text_hits("a table of the results", passages.iter().copied(), "said");
        let found: Vec<(f64, f32)> = hits.iter().map(|h| (h.t, h.score)).collect();
        assert_eq!(found, vec![(30.0, 1.0), (0.0, 0.5)]);
        assert_eq!(words("the of"), vec!["of", "the"], "all there is, is kept");
    }

    #[test]
    fn a_long_passage_comes_back_as_the_words_around_the_match() {
        let long = format!("{} aguja {}", "paja ".repeat(100), "heno ".repeat(100));
        let hits = text_hits("aguja", [(0.0, long.as_str(), None)].into_iter(), "shown");
        let text = hits[0].text.as_deref().unwrap();
        assert!(text.contains("aguja") && text.starts_with('…') && text.ends_with('…'));
        assert!(text.chars().count() < 220, "{}", text.chars().count());
    }

    #[test]
    fn the_pictures_most_like_the_words_come_first_and_not_five_of_one_scene() {
        let q = vec![1.0, 0.0];
        let seen: Vec<(f64, Vec<f32>, Option<String>)> = vec![
            (0.0, vec![0.1, 1.0], None),
            (10.0, vec![1.0, 0.1], None),
            (11.0, vec![1.0, 0.12], None),
            (12.0, vec![1.0, 0.14], None),
            (60.0, vec![0.7, 0.7], Some("/f/60.png".into())),
            (90.0, vec![0.0, 1.0], None),
        ];
        let (hits, clear) = visual_hits(&q, &seen, 3, 5.0);
        assert!(clear);
        let times: Vec<f64> = hits.iter().map(|h| h.t).collect();
        assert_eq!(times, vec![10.0, 60.0, 0.0]);
        assert_eq!(hits[1].path.as_deref(), Some("/f/60.png"));
        assert!(hits.iter().all(|h| h.by == "seen"));
    }

    #[test]
    fn pictures_that_all_score_alike_are_said_to() {
        let q = vec![1.0, 0.0];
        let seen: Vec<(f64, Vec<f32>, Option<String>)> = (0..8)
            .map(|i| (i as f64 * 10.0, vec![1.0, 0.5 + i as f32 * 0.001], None))
            .collect();
        let (hits, clear) = visual_hits(&q, &seen, 3, 5.0);
        assert_eq!(hits.len(), 3);
        assert!(!clear, "the model cannot tell these apart");
        assert_eq!(visual_hits(&q, &[], 3, 5.0), (vec![], false));
    }
}
