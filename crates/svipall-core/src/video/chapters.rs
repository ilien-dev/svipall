//! Chapters written into a description by hand: `0:00 Intro`, `12:31 - Results`.
//!
//! The convention is shared by enough platforms that it is read the same way everywhere, with the
//! rule that stops a list of song lengths or a phone number from becoming chapters: the list starts
//! at zero, has at least two entries, and only moves forward.

use super::Chapter;
use regex::Regex;
use std::sync::LazyLock;

static CHAPTER_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*[\[(]?((?:\d{1,2}:)?\d{1,2}:\d{2})[\])]?\s*(?:[-–—:|]\s*)?(\S.*?)\s*$")
        .expect("static regex")
});

pub fn from_description(text: &str) -> Vec<Chapter> {
    let mut out: Vec<Chapter> = Vec::new();
    for line in text.lines() {
        let Some(c) = CHAPTER_LINE.captures(line) else {
            continue;
        };
        let Some(start) = super::cues::timestamp(&c[1]) else {
            continue;
        };
        if out.last().is_some_and(|l| start <= l.start) {
            // Out of order: not a chapter list, or a second list further down (a tracklist
            // under the chapters). The first run is the one that describes the video.
            break;
        }
        out.push(Chapter {
            start,
            title: c[2].to_string(),
        });
    }
    if out.len() >= 2 && out[0].start == 0.0 {
        out
    } else {
        Vec::new()
    }
}

/// The chapter a moment falls in, by index.
pub fn at(chapters: &[Chapter], t: f64) -> Option<usize> {
    chapters.iter().rposition(|c| c.start <= t + 1e-6)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_description_with_chapters() {
        let d = "Great talk.\n\n0:00 Intro\n1:05 - The problem\n(12:31) Results: a table\n1:02:03 Q&A\n\nFollow me!";
        let c = from_description(d);
        let got: Vec<(f64, &str)> = c.iter().map(|c| (c.start, c.title.as_str())).collect();
        assert_eq!(
            got,
            vec![
                (0.0, "Intro"),
                (65.0, "The problem"),
                (751.0, "Results: a table"),
                (3723.0, "Q&A")
            ]
        );
    }

    #[test]
    fn a_list_that_does_not_start_at_zero_is_not_chapters() {
        assert!(from_description("3:45 Song one\n4:10 Song two").is_empty());
    }

    #[test]
    fn one_timestamp_is_a_mention_not_a_chapter_list() {
        assert!(from_description("0:00 the start of it all").is_empty());
    }

    #[test]
    fn a_second_list_below_the_first_is_left_alone() {
        let c = from_description("0:00 A\n2:00 B\n\nTracklist\n0:30 x\n1:00 y");
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn a_moment_is_in_the_last_chapter_that_started_before_it() {
        let c = from_description("0:00 A\n2:00 B");
        assert_eq!(at(&c, 0.0), Some(0));
        assert_eq!(at(&c, 119.9), Some(0));
        assert_eq!(at(&c, 120.0), Some(1));
    }
}
