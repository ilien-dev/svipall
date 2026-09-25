//! `web_video`: the parts that decide, kept apart from the parts that fetch.
//!
//! The server fetches the page, the caption file and (for a player whose caption addresses need
//! its own proof) a live page to borrow a request from. Which track to read, which request is the
//! player's own, and how the answer is laid out are decided here, where they can be tested without
//! a network.

use serde_json::{json, Value};
use svipall_core::video::{self, align, align::FrameRef, CaptionKind, Cue, Track, VideoInfo};

/// How long a passage runs before a new one starts, when nothing else cuts it.
pub const WINDOW_SECS: f64 = 30.0;

/// Caption segments read from one segmented rendition, at most. A two-hour stream in six-second
/// segments is 1 200 requests; past this the rest is reported, not fetched.
pub const MAX_SEGMENTS: usize = 400;

fn lang_matches(have: Option<&str>, want: &str) -> bool {
    let Some(have) = have else {
        return false;
    };
    let (have, want) = (have.to_ascii_lowercase(), want.to_ascii_lowercase());
    have == want || have.starts_with(&format!("{want}-")) || want.starts_with(&format!("{have}-"))
}

/// The track to read: the wanted language first, then a person's captions over a machine's, then
/// the order the page listed them in.
///
/// With no language asked for, the one spoken is wanted, and the platform's own recogniser says
/// which that is: its track is in the language it heard. Without that, a video with thirty
/// community translations read as the first of them (measured: Arabic, on an English lecture).
pub fn pick_track<'a>(tracks: &'a [Track], lang: Option<&str>) -> Option<&'a Track> {
    let spoken = tracks
        .iter()
        .find(|t| t.kind == CaptionKind::Auto)
        .and_then(|t| t.lang.as_deref())
        .map(|l| l.split('-').next().unwrap_or(l));
    let want = lang.or(spoken);
    tracks
        .iter()
        .enumerate()
        .min_by_key(|(i, t)| {
            let lang_miss = want.is_some_and(|l| !lang_matches(t.lang.as_deref(), l));
            (lang_miss, t.kind != CaptionKind::Manual, *i)
        })
        .map(|(_, t)| t)
}

/// The player's own caption request for this video, among every request the page made.
pub fn borrowed_request<'a>(
    seen: &'a [String],
    borrow: &video::sources::Borrow,
    id: Option<&str>,
) -> Option<&'a String> {
    seen.iter().find(|u| {
        let Ok(parsed) = url::Url::parse(u) else {
            return false;
        };
        parsed.path().contains(borrow.endpoint)
            && match id {
                Some(id) => parsed
                    .query_pairs()
                    .any(|(k, v)| k == borrow.id_param && v == id),
                None => true,
            }
    })
}

/// A same-session text fetch from inside the page: the one place a borrowed address is valid.
///
/// Self-contained and returning its value, like every script this crate evaluates: nothing is
/// left on `window` or in the DOM for the page to find.
pub fn in_page_text_js(url: &str) -> String {
    let target = serde_json::to_string(url).unwrap_or_else(|_| "\"\"".into());
    format!(
        r#"(async () => {{
            const abort = new AbortController();
            const timer = setTimeout(() => abort.abort(), 10000);
            try {{
                const r = await fetch({target}, {{credentials:'include', signal:abort.signal}});
                if (!r.ok) return null;
                const s = await r.text();
                return s.length > 8000000 ? s.slice(0, 8000000) : s;
            }} catch (_) {{ return null; }} finally {{ clearTimeout(timer); }}
        }})()"#
    )
}

/// Chapters from a WebVTT chapter track: each cue's text is a title.
pub fn chapters_from_cues(cues: &[Cue]) -> Vec<video::Chapter> {
    cues.iter()
        .map(|c| video::Chapter {
            start: c.start,
            title: c.text.replace('\n', " "),
        })
        .collect()
}

/// What a fetched page turned out to be.
pub enum Found {
    Video(Box<VideoInfo>),
    /// Only an embedded player: the page to read instead.
    Embed(String),
    Nothing,
}

/// What was read, and from where.
pub struct Reading {
    pub cues: Vec<Cue>,
    pub track: Option<Track>,
    /// Keyframes captured from the page, when they were asked for.
    pub frames: Vec<FrameRef>,
    pub notes: Vec<String>,
}

/// The timeline a model reads: a header saying what this is, then the passages.
pub fn content(info: &VideoInfo, reading: &Reading) -> String {
    let segments = align::timeline(
        &reading.cues,
        &info.chapters,
        &[],
        WINDOW_SECS,
        (None, None),
    );
    let mut out = String::new();
    if let Some(t) = &info.title {
        out.push_str(&format!("# {t}\n\n"));
    }
    if segments.is_empty() {
        match (&info.transcript, &info.description) {
            (Some(t), _) => out.push_str(&format!("Transcript published with the video:\n\n{t}\n")),
            (None, Some(d)) => out.push_str(&format!("No captions. Description:\n\n{d}\n")),
            (None, None) => out.push_str("No captions and no description.\n"),
        }
        if !info.chapters.is_empty() {
            out.push_str("\nChapters:\n");
            for c in &info.chapters {
                out.push_str(&format!("[{}] {}\n", align::clock(c.start), c.title));
            }
        }
        return out;
    }
    out.push_str(&align::render(&segments));
    out
}

/// The JSON around the timeline. The timeline itself is added by the server's budget, as
/// `content` or into `out_file`.
pub fn summary(info: &VideoInfo, reading: &Reading) -> Value {
    let tracks: Vec<Value> = info
        .tracks
        .iter()
        .map(|t| json!({"lang": t.lang, "label": t.label, "kind": t.kind}))
        .collect();
    let mut v = json!({
        "source": info.source,
        "id": info.id,
        "title": info.title,
        "duration": info.duration,
        "chapters": info.chapters,
        "tracks": tracks,
        "captions": reading.track.as_ref().map(|t| json!({
            "kind": t.kind, "lang": t.lang, "label": t.label, "cues": reading.cues.len(),
        })),
        "frames": reading.frames,
        "streams": info.streams.iter().map(|s| json!({"kind": s.kind, "mime": s.mime})).collect::<Vec<_>>(),
    });
    let obj = v.as_object_mut().expect("object");
    if let Some(u) = &info.unplayable {
        obj.insert("unplayable".into(), json!(u));
    }
    if !reading.notes.is_empty() {
        obj.insert("notes".into(), json!(reading.notes));
    }
    obj.retain(|k, v| !v.is_null() && !(k == "frames" && v.as_array().is_some_and(Vec::is_empty)));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(lang: &str, kind: CaptionKind) -> Track {
        Track {
            url: format!("https://x/{lang}"),
            lang: Some(lang.into()),
            label: None,
            kind,
        }
    }

    #[test]
    fn a_persons_captions_before_a_machines_and_the_wanted_language_before_both() {
        let t = vec![
            track("en", CaptionKind::Auto),
            track("de-DE", CaptionKind::Manual),
            track("es-419", CaptionKind::Manual),
        ];
        assert_eq!(pick_track(&t, Some("es")).unwrap().url, "https://x/es-419");
        assert_eq!(pick_track(&t, Some("en-US")).unwrap().url, "https://x/en");
        assert_eq!(
            pick_track(&t, Some("ja")).unwrap().url,
            "https://x/de-DE",
            "a language nobody wrote still gets something to read"
        );
        assert!(pick_track(&[], None).is_none());
    }

    #[test]
    fn with_no_language_asked_for_the_spoken_one_is_read() {
        let t = vec![
            track("ar", CaptionKind::Manual),
            track("en", CaptionKind::Manual),
            track("en", CaptionKind::Auto),
        ];
        assert_eq!(pick_track(&t, None).unwrap().lang.as_deref(), Some("en"));
        assert_eq!(pick_track(&t, None).unwrap().kind, CaptionKind::Manual);
        let only_translations = vec![
            track("de", CaptionKind::Manual),
            track("en-US", CaptionKind::Auto),
        ];
        assert_eq!(
            pick_track(&only_translations, None).unwrap().kind,
            CaptionKind::Auto
        );
        let no_machine = vec![
            track("fr", CaptionKind::Manual),
            track("en", CaptionKind::Manual),
        ];
        assert_eq!(
            pick_track(&no_machine, None).unwrap().lang.as_deref(),
            Some("fr")
        );
    }

    #[test]
    fn the_advert_caption_request_is_not_the_videos() {
        let b = video::sources::spec("youtube.com/watch")
            .unwrap()
            .borrow
            .as_ref()
            .unwrap();
        let seen = vec![
            "https://www.youtube.com/api/stats/qoe?v=ID".to_string(),
            "https://www.youtube.com/api/timedtext?v=ADVERT&pot=P1".to_string(),
            "https://www.youtube.com/api/timedtext?v=ID&pot=P2".to_string(),
        ];
        assert_eq!(
            borrowed_request(&seen, b, Some("ID")).map(String::as_str),
            Some("https://www.youtube.com/api/timedtext?v=ID&pot=P2")
        );
        assert!(borrowed_request(&seen[..2], b, Some("ID")).is_none());
    }

    #[test]
    fn the_in_page_fetch_leaves_nothing_behind() {
        let js = in_page_text_js("https://x/\"quoted\"");
        let low = js.to_ascii_lowercase();
        assert!(js.trim_start().starts_with("(async () =>"));
        assert!(!low.contains("svipall"));
        assert!(!low.contains("window.") && !low.contains("document."));
        assert!(!low.contains("setattribute") && !low.contains("dataset"));
        assert!(
            js.contains(r#""https://x/\"quoted\"""#),
            "the address is a JSON string"
        );
    }

    #[test]
    fn a_video_without_captions_still_says_what_it_is() {
        let info = VideoInfo {
            title: Some("Talk".into()),
            description: Some("About things.".into()),
            chapters: vec![video::Chapter {
                start: 0.0,
                title: "Intro".into(),
            }],
            ..VideoInfo::default()
        };
        let r = Reading {
            cues: vec![],
            track: None,
            frames: vec![],
            notes: vec![],
        };
        let c = content(&info, &r);
        assert!(c.contains("# Talk") && c.contains("About things.") && c.contains("[0:00] Intro"));
    }

    #[test]
    fn the_summary_names_the_track_read_without_its_address() {
        let t = track("es", CaptionKind::Auto);
        let info = VideoInfo {
            source: "html5",
            tracks: vec![t.clone()],
            ..VideoInfo::default()
        };
        let r = Reading {
            cues: vec![Cue {
                start: 0.0,
                end: 1.0,
                text: "hola".into(),
            }],
            track: Some(t),
            frames: vec![FrameRef {
                t: 0.5,
                path: "/f.png".into(),
            }],
            notes: vec!["n".into()],
        };
        let v = summary(&info, &r);
        assert_eq!(v["captions"]["kind"], "auto");
        assert_eq!(v["captions"]["cues"], 1);
        assert!(
            v["tracks"][0].get("url").is_none(),
            "addresses expire; they are noise"
        );
        assert_eq!(v["notes"][0], "n");
        assert!(v.get("title").is_none(), "no nulls");
        assert_eq!(v["frames"][0]["path"], "/f.png");
    }
}
