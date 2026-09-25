//! Cues, chapters and frames put on one timeline.
//!
//! A cue is a line on screen, far too small a unit to read a video by. Segments are cues gathered
//! into passages: a new one starts at every chapter, at every captured frame (a scene change is
//! where the picture says something new), and otherwise once a passage has run for `window`
//! seconds. A frame with no speech over it still gets its segment, empty of text: a silent scene is
//! part of the video too.

use super::{chapters, Chapter, Cue};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FrameRef {
    pub t: f64,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapter: Option<String>,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<String>,
}

/// Seconds to `m:ss`, or `h:mm:ss` past the hour.
pub fn clock(t: f64) -> String {
    let s = t.max(0.0).floor() as u64;
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

pub fn timeline(
    cues: &[Cue],
    chapter_list: &[Chapter],
    frames: &[FrameRef],
    window: f64,
    range: (Option<f64>, Option<f64>),
) -> Vec<Segment> {
    let (lo, hi) = (range.0.unwrap_or(0.0), range.1.unwrap_or(f64::INFINITY));
    let inside = |t: f64| t >= lo && t < hi;
    let mut cuts: Vec<f64> = chapter_list
        .iter()
        .map(|c| c.start)
        .chain(frames.iter().map(|f| f.t))
        .filter(|t| inside(*t))
        .collect();
    cuts.sort_by(f64::total_cmp);

    let mut out: Vec<Segment> = Vec::new();
    let mut open: Option<Segment> = None;
    let mut next_cut = 0;
    for cue in cues.iter().filter(|c| c.end > lo && c.start < hi) {
        while next_cut < cuts.len() && cuts[next_cut] <= open.as_ref().map_or(f64::MIN, |s| s.start)
        {
            next_cut += 1;
        }
        let crosses = next_cut < cuts.len() && cue.start >= cuts[next_cut];
        let long = open.as_ref().is_some_and(|s| cue.start - s.start >= window);
        if crosses || long {
            out.extend(open.take());
        }
        let text = cue.text.split_whitespace().collect::<Vec<_>>().join(" ");
        match open.as_mut() {
            Some(s) => {
                s.end = s.end.max(cue.end);
                s.text.push(' ');
                s.text.push_str(&text);
            }
            None => {
                open = Some(Segment {
                    start: cue.start.max(lo),
                    end: cue.end,
                    chapter: None,
                    text,
                    frame: None,
                })
            }
        }
    }
    out.extend(open);

    for f in frames.iter().filter(|f| inside(f.t)) {
        match out
            .iter_mut()
            .find(|s| s.frame.is_none() && f.t >= s.start - 1e-6 && f.t < s.end)
        {
            Some(s) => s.frame = Some(f.path.clone()),
            None => out.push(Segment {
                start: f.t,
                end: f.t,
                chapter: None,
                text: String::new(),
                frame: Some(f.path.clone()),
            }),
        }
    }
    out.sort_by(|a, b| a.start.total_cmp(&b.start));
    for s in &mut out {
        s.chapter = chapters::at(chapter_list, s.start).map(|i| chapter_list[i].title.clone());
    }
    out
}

/// The timeline as the text a model reads: a heading per chapter, a timestamp per passage.
pub fn render(segments: &[Segment]) -> String {
    let mut out = String::new();
    let mut chapter: Option<&str> = None;
    for s in segments {
        if s.chapter.as_deref() != chapter {
            chapter = s.chapter.as_deref();
            if let Some(c) = chapter {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&format!("## {c}\n\n"));
            }
        }
        out.push_str(&format!("[{}]", clock(s.start)));
        if !s.text.is_empty() {
            out.push(' ');
            out.push_str(&s.text);
        }
        if let Some(f) = &s.frame {
            out.push_str(&format!(" (frame: {f})"));
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(start: f64, end: f64, text: &str) -> Cue {
        Cue {
            start,
            end,
            text: text.into(),
        }
    }

    fn cues() -> Vec<Cue> {
        (0..10)
            .map(|i| cue(i as f64 * 10.0, i as f64 * 10.0 + 9.0, &format!("line {i}")))
            .collect()
    }

    #[test]
    fn passages_close_after_the_window() {
        let s = timeline(&cues(), &[], &[], 30.0, (None, None));
        assert_eq!(s.len(), 4);
        assert_eq!(s[0].text, "line 0 line 1 line 2");
        assert_eq!((s[1].start, s[1].end), (30.0, 59.0));
    }

    #[test]
    fn a_chapter_starts_a_passage_and_names_it() {
        let ch = vec![
            Chapter {
                start: 0.0,
                title: "Intro".into(),
            },
            Chapter {
                start: 20.0,
                title: "Body".into(),
            },
        ];
        let s = timeline(&cues(), &ch, &[], 1000.0, (None, None));
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].text, "line 0 line 1");
        assert_eq!(s[0].chapter.as_deref(), Some("Intro"));
        assert_eq!(s[1].chapter.as_deref(), Some("Body"));
    }

    #[test]
    fn a_frame_cuts_and_lands_in_its_passage() {
        let f = vec![FrameRef {
            t: 42.0,
            path: "/f/42.png".into(),
        }];
        let s = timeline(&cues(), &[], &f, 1000.0, (None, None));
        assert_eq!(s.len(), 2);
        assert_eq!(s[1].start, 50.0, "cut at the first cue after the frame");
        assert_eq!(
            s[0].frame.as_deref(),
            Some("/f/42.png"),
            "42 s is inside 0..49"
        );
    }

    #[test]
    fn a_silent_scene_still_has_its_segment() {
        let f = vec![FrameRef {
            t: 5.0,
            path: "/f/5.png".into(),
        }];
        let s = timeline(&[], &[], &f, 30.0, (None, None));
        assert_eq!(s.len(), 1);
        assert!(s[0].text.is_empty());
        assert_eq!(s[0].frame.as_deref(), Some("/f/5.png"));
    }

    #[test]
    fn the_range_trims_the_timeline() {
        let s = timeline(&cues(), &[], &[], 1000.0, (Some(25.0), Some(45.0)));
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].text, "line 2 line 3 line 4");
        assert_eq!(s[0].start, 25.0);
    }

    #[test]
    fn rendered_with_headings_and_timestamps() {
        let s = vec![
            Segment {
                start: 0.0,
                end: 9.0,
                chapter: Some("Intro".into()),
                text: "hello".into(),
                frame: None,
            },
            Segment {
                start: 3725.0,
                end: 3730.0,
                chapter: Some("Q&A".into()),
                text: String::new(),
                frame: Some("/f.png".into()),
            },
        ];
        assert_eq!(
            render(&s),
            "## Intro\n\n[0:00] hello\n\n## Q&A\n\n[1:02:05] (frame: /f.png)\n"
        );
    }
}
