//! What a person hands back from the dashboard, in a shape the server can check.
//!
//! The old panel knew two answers: a string of text, or a token pasted from somewhere else. Eleven
//! modalities do not fit in two strings, and the failure mode of pretending they do is silent —
//! a click sent as "412,203" is accepted, stored, replayed against a widget that expected something
//! else, and reported as a solve that the site rejected.
//!
//! Two rules make the rest of the design work:
//!
//! **Every coordinate is normalised to 0..1** against the asset it was clicked on, never pixels.
//! A phone showing a 1280-wide challenge image on a 390-wide screen sends coordinates that are
//! wrong by a factor of three, and nothing anywhere would notice: the numbers are plausible, the
//! answer is simply wrong. Normalising is not a nicety for mobile, it is the only thing that makes
//! mobile answers correct at all.
//!
//! **The answer must match the question.** A submission is checked against the modality of the job
//! it answers before it is stored, so a mismatch is a rejection at the door with a reason, rather
//! than a wrong answer discovered a minute later by the site.

use crate::widget::Modality;
use serde::{Deserialize, Serialize};

/// A position on the asset, as a fraction of its width and height.
///
/// Never pixels. See the module note: the same picture is a different number of pixels wide on
/// every device that solves it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    fn in_range(&self) -> bool {
        (0.0..=1.0).contains(&self.x) && (0.0..=1.0).contains(&self.y)
    }
}

/// One answer to one challenge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Answer {
    /// An opaque string the widget produced.
    Token { value: String },
    /// Characters read from an image, or a written answer.
    Text { value: String },
    /// Which cells of the grid were chosen, counting across rows from zero.
    Tiles { indices: Vec<u32> },
    /// Where to click, in order.
    Points { points: Vec<Point> },
    /// A shape drawn around something.
    Polygon { points: Vec<Point> },
    /// How far along its track the handle goes, as a fraction of the track.
    Slide { fraction: f64 },
    /// How far to turn the image, in degrees clockwise.
    Rotate { degrees: f64 },
    /// A piece moved from one place to another.
    Drag { from: Point, to: Point },
    /// The number that satisfies an arithmetic challenge.
    Nonce { value: String },
    /// How long the button was held, in milliseconds.
    Hold { ms: u32 },
    /// How much of a page is actually in it, as a person read it.
    ///
    /// Four levels and not six. FineWeb-Edu distilled a 70B model's 0-5 judgements and published
    /// the result: recall 0.35 at the second-highest level and 0.01 at the highest. Asking anyone,
    /// person or model, for finer resolution than that collects noise and calls it a label.
    Rate {
        level: crate::quality::substance::Label,
    },
    /// "I cannot read this."
    ///
    /// A real answer, and the one that keeps the rest honest: a person guessing rather than
    /// declining teaches the ranking that a strategy works when it does not.
    Unknown,
}

/// Why a submission was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invalid {
    /// The answer is a fine answer to a different question.
    WrongModality { expected: Modality },
    /// A coordinate outside 0..1, which almost always means pixels were sent.
    NotNormalised,
    /// Nothing was actually chosen.
    Empty,
    /// A rotation outside a full turn.
    OutOfRange,
}

impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Invalid::WrongModality { expected } => {
                write!(f, "this challenge expects a {expected:?} answer")
            }
            Invalid::NotNormalised => write!(
                f,
                "coordinates must be fractions of the image between 0 and 1, not pixels"
            ),
            Invalid::Empty => write!(f, "nothing was selected"),
            Invalid::OutOfRange => write!(f, "value outside the range the challenge allows"),
        }
    }
}

impl Answer {
    /// The modality this answer belongs to. `Unknown` belongs to all of them.
    pub fn modality(&self) -> Option<Modality> {
        Some(match self {
            Answer::Token { .. } => Modality::Token,
            Answer::Text { .. } => Modality::Text,
            Answer::Tiles { .. } => Modality::Tiles,
            Answer::Points { .. } => Modality::Points,
            Answer::Polygon { .. } => Modality::Polygon,
            Answer::Slide { .. } => Modality::Slide,
            Answer::Rotate { .. } => Modality::Rotate,
            Answer::Drag { .. } => Modality::Drag,
            Answer::Nonce { .. } => Modality::Nonce,
            Answer::Hold { .. } => Modality::Hold,
            Answer::Rate { .. } => Modality::Rate,
            Answer::Unknown => return None,
        })
    }

    /// Is this a usable answer to a challenge of that modality?
    pub fn check(&self, expected: Modality) -> Result<(), Invalid> {
        if let Some(m) = self.modality() {
            if m != expected {
                return Err(Invalid::WrongModality { expected });
            }
        }
        let points_ok = |p: &[Point]| -> Result<(), Invalid> {
            if p.is_empty() {
                return Err(Invalid::Empty);
            }
            if !p.iter().all(Point::in_range) {
                return Err(Invalid::NotNormalised);
            }
            Ok(())
        };
        match self {
            Answer::Token { value } | Answer::Text { value } | Answer::Nonce { value } => {
                if value.trim().is_empty() {
                    return Err(Invalid::Empty);
                }
            }
            Answer::Tiles { indices } => {
                if indices.is_empty() {
                    return Err(Invalid::Empty);
                }
            }
            Answer::Points { points } => points_ok(points)?,
            // Three points is the smallest thing that encloses anything; two is a line.
            Answer::Polygon { points } => {
                points_ok(points)?;
                if points.len() < 3 {
                    return Err(Invalid::Empty);
                }
            }
            Answer::Slide { fraction } => {
                if !(0.0..=1.0).contains(fraction) {
                    return Err(Invalid::NotNormalised);
                }
            }
            Answer::Rotate { degrees } => {
                if !(0.0..360.0).contains(degrees) {
                    return Err(Invalid::OutOfRange);
                }
            }
            Answer::Drag { from, to } => {
                if !from.in_range() || !to.in_range() {
                    return Err(Invalid::NotNormalised);
                }
            }
            Answer::Hold { ms } => {
                // A hold is a gesture, not a click and not an afternoon.
                if !(200..=30_000).contains(ms) {
                    return Err(Invalid::OutOfRange);
                }
            }
            // Every level is a usable answer, including the lowest: "this page is junk" is a
            // label, and refusing it would leave the corpus with only the flattering half.
            Answer::Rate { .. } => {}
            Answer::Unknown => {}
        }
        Ok(())
    }

    /// Convert to a point on the asset, given its size in pixels.
    ///
    /// The one place pixels are allowed to exist: at the moment the gesture is played back into a
    /// browser that has the image at a known size.
    pub fn to_pixels(p: Point, width: f64, height: f64) -> (f64, f64) {
        (p.x * width, p.y * height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_is_a_fraction_of_the_picture_never_a_pixel() {
        // The whole reason this type exists. A phone sends 390-wide coordinates for a 1280-wide
        // image; nothing downstream can tell those numbers are wrong, so they are refused here.
        let pixels = Answer::Points {
            points: vec![Point::new(412.0, 203.0)],
        };
        assert_eq!(
            pixels.check(Modality::Points),
            Err(Invalid::NotNormalised),
            "pixel coordinates must never be accepted"
        );
        let normalised = Answer::Points {
            points: vec![Point::new(0.32, 0.61)],
        };
        assert_eq!(normalised.check(Modality::Points), Ok(()));
    }

    #[test]
    fn the_same_fraction_is_the_same_place_on_every_screen() {
        let p = Point::new(0.5, 0.25);
        assert_eq!(Answer::to_pixels(p, 1280.0, 800.0), (640.0, 200.0));
        assert_eq!(Answer::to_pixels(p, 390.0, 240.0), (195.0, 60.0));
    }

    #[test]
    fn an_answer_to_a_different_question_is_refused_at_the_door() {
        // Otherwise it is stored, replayed, rejected by the site, and recorded as a strategy that
        // failed — three wrong conclusions from one accepted submission.
        let text = Answer::Text {
            value: "abc".into(),
        };
        assert_eq!(
            text.check(Modality::Tiles),
            Err(Invalid::WrongModality {
                expected: Modality::Tiles
            })
        );
        assert_eq!(text.check(Modality::Text), Ok(()));
    }

    #[test]
    fn saying_you_cannot_read_it_is_valid_for_any_challenge() {
        // A person guessing teaches the ranking that something works when it does not.
        for m in [Modality::Tiles, Modality::Audio, Modality::Polygon] {
            assert_eq!(Answer::Unknown.check(m), Ok(()));
        }
    }

    #[test]
    fn an_empty_answer_is_not_an_answer() {
        assert_eq!(
            Answer::Text { value: "  ".into() }.check(Modality::Text),
            Err(Invalid::Empty)
        );
        assert_eq!(
            Answer::Tiles { indices: vec![] }.check(Modality::Tiles),
            Err(Invalid::Empty)
        );
        assert_eq!(
            Answer::Points { points: vec![] }.check(Modality::Points),
            Err(Invalid::Empty)
        );
    }

    #[test]
    fn a_polygon_needs_enough_corners_to_enclose_something() {
        let two = Answer::Polygon {
            points: vec![Point::new(0.1, 0.1), Point::new(0.2, 0.2)],
        };
        assert_eq!(two.check(Modality::Polygon), Err(Invalid::Empty));
        let three = Answer::Polygon {
            points: vec![
                Point::new(0.1, 0.1),
                Point::new(0.2, 0.2),
                Point::new(0.1, 0.3),
            ],
        };
        assert_eq!(three.check(Modality::Polygon), Ok(()));
    }

    #[test]
    fn a_slide_is_a_fraction_of_its_track_and_a_turn_is_degrees() {
        assert_eq!(
            Answer::Slide { fraction: 0.42 }.check(Modality::Slide),
            Ok(())
        );
        assert_eq!(
            Answer::Slide { fraction: 180.0 }.check(Modality::Slide),
            Err(Invalid::NotNormalised),
            "180 is a plausible pixel offset and a nonsense fraction"
        );
        assert_eq!(
            Answer::Rotate { degrees: 359.9 }.check(Modality::Rotate),
            Ok(())
        );
        assert_eq!(
            Answer::Rotate { degrees: 360.0 }.check(Modality::Rotate),
            Err(Invalid::OutOfRange),
            "a full turn is no turn; it means the sender did not wrap"
        );
    }

    #[test]
    fn a_hold_is_a_gesture_not_a_click_and_not_an_afternoon() {
        assert_eq!(Answer::Hold { ms: 3_000 }.check(Modality::Hold), Ok(()));
        assert_eq!(
            Answer::Hold { ms: 20 }.check(Modality::Hold),
            Err(Invalid::OutOfRange)
        );
        assert_eq!(
            Answer::Hold { ms: 600_000 }.check(Modality::Hold),
            Err(Invalid::OutOfRange)
        );
    }

    #[test]
    fn a_drag_needs_both_ends_on_the_picture() {
        let ok = Answer::Drag {
            from: Point::new(0.1, 0.5),
            to: Point::new(0.8, 0.5),
        };
        assert_eq!(ok.check(Modality::Drag), Ok(()));
        let off = Answer::Drag {
            from: Point::new(0.1, 0.5),
            to: Point::new(1.4, 0.5),
        };
        assert_eq!(off.check(Modality::Drag), Err(Invalid::NotNormalised));
    }

    #[test]
    fn the_wire_format_is_what_a_browser_can_send_without_a_library() {
        // The panel is plain JS with no bundler, so the JSON has to be writable by hand.
        let raw = r#"{"kind":"tiles","indices":[0,4,8]}"#;
        let a: Answer = serde_json::from_str(raw).expect("parses");
        assert_eq!(
            a,
            Answer::Tiles {
                indices: vec![0, 4, 8]
            }
        );
        assert_eq!(a.check(Modality::Tiles), Ok(()));

        let raw = r#"{"kind":"points","points":[{"x":0.1,"y":0.2}]}"#;
        let a: Answer = serde_json::from_str(raw).expect("parses");
        assert_eq!(a.check(Modality::Points), Ok(()));

        let raw = r#"{"kind":"unknown"}"#;
        assert_eq!(
            serde_json::from_str::<Answer>(raw).expect("parses"),
            Answer::Unknown
        );
    }

    #[test]
    fn every_modality_has_an_answer_that_fits_it() {
        // The conformance rule for this file: a modality with no way to answer it is a modality the
        // dashboard cannot show, and that is exactly how a widget silently becomes unsolvable.
        for m in Modality::ALL {
            let a = match m {
                Modality::Token => Answer::Token { value: "t".into() },
                Modality::Text => Answer::Text { value: "t".into() },
                Modality::Tiles => Answer::Tiles { indices: vec![0] },
                Modality::Points => Answer::Points {
                    points: vec![Point::new(0.5, 0.5)],
                },
                Modality::Polygon => Answer::Polygon {
                    points: vec![
                        Point::new(0.1, 0.1),
                        Point::new(0.9, 0.1),
                        Point::new(0.5, 0.9),
                    ],
                },
                Modality::Slide => Answer::Slide { fraction: 0.5 },
                Modality::Rotate => Answer::Rotate { degrees: 90.0 },
                Modality::Drag => Answer::Drag {
                    from: Point::new(0.1, 0.1),
                    to: Point::new(0.9, 0.9),
                },
                Modality::Audio => Answer::Text {
                    value: "7 3".into(),
                },
                Modality::Nonce => Answer::Nonce { value: "42".into() },
                Modality::Hold => Answer::Hold { ms: 3_000 },
                Modality::Rate => Answer::Rate {
                    level: crate::quality::substance::Label::Ordinary,
                },
            };
            // Audio is answered by typing what was heard, so its answer is text.
            let expected = if *m == Modality::Audio {
                Modality::Text
            } else {
                *m
            };
            assert_eq!(a.check(expected), Ok(()), "no answer fits {m:?}");
        }
    }
}
