//! `web_video`: the parts that decide, kept apart from the parts that fetch.
//!
//! The server fetches the page, the caption file and (for a player whose caption addresses need
//! its own proof) a live page to borrow a request from. Which track to read, which request is the
//! player's own, and how the answer is laid out are decided here, where they can be tested without
//! a network.

use serde::Serialize;
use serde_json::{json, Value};
use svipall_core::video::{
    self, align, align::FrameRef, look, look::Window, CaptionKind, Cue, Track, VideoInfo,
};

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

/// What a call asked to be shown of the video, beyond what it says.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Look {
    pub window: Window,
    /// Exact moments, in seconds.
    pub at: Vec<f64>,
    /// Moments wanted over the window; `None` when the caller named no number.
    pub frames: Option<usize>,
    /// The part of each frame to return, as fractions of it.
    pub crop: Option<[f32; 4]>,
    pub query: Option<String>,
    /// Moments added by the server, never by the caller: where the player's thumbnails show a
    /// picture held between two spread moments.
    pub also: Vec<f64>,
}

impl Look {
    /// Read a call's arguments. `Err` is a sentence for the caller: nothing ran.
    pub fn parse(
        start: Option<&str>,
        end: Option<&str>,
        at: &[String],
        frames: Option<u32>,
        crop: Option<&str>,
        query: Option<&str>,
    ) -> Result<Self, String> {
        let time = |name: &str, v: Option<&str>| match v.map(str::trim).filter(|v| !v.is_empty()) {
            Some(v) => look::parse_time(v).map(Some).ok_or(format!(
                "{name}=\"{v}\" is not a time: write seconds (90) or m:ss (1:30)"
            )),
            None => Ok(None),
        };
        let window = Window {
            start: time("start", start)?,
            end: time("end", end)?,
        };
        if let (Some(a), Some(b)) = (window.start, window.end) {
            if b <= a {
                return Err("end must come after start".into());
            }
        }
        let mut moments = Vec::new();
        for v in at.iter().flat_map(|v| v.split(',')) {
            let v = v.trim();
            if v.is_empty() {
                continue;
            }
            moments.push(look::parse_time(v).ok_or(format!(
                "at=\"{v}\" is not a time: write seconds (90) or m:ss (1:30)"
            ))?);
        }
        moments.sort_by(f64::total_cmp);
        moments.dedup();
        if moments.len() > look::MAX_MOMENTS {
            return Err(format!("at most {} moments a call", look::MAX_MOMENTS));
        }
        let crop = match crop.map(str::trim).filter(|c| !c.is_empty()) {
            Some(c) => Some(crate::video_sheet::parse_crop(c).ok_or(format!(
                "crop=\"{c}\" is not a box: write x,y,w,h as fractions of the frame, e.g. 0.5,0,0.5,0.5"
            ))?),
            None => None,
        };
        Ok(Self {
            window,
            at: moments,
            frames: frames.map(|n| (n as usize).min(look::MAX_MOMENTS)),
            crop,
            query: query
                .map(str::trim)
                .filter(|q| !q.is_empty())
                .map(str::to_string),
            also: Vec::new(),
        })
    }

    /// Exact moments and nothing spread around them: the answer is about those moments, and
    /// the timeline that comes back is the passages they fall in rather than the whole video.
    pub fn only_exact(&self) -> bool {
        !self.at.is_empty() && self.frames.is_none() && self.window.is_whole()
    }

    /// The caller asked for the picture off: `frames=0` and nothing else to show.
    pub fn off(&self) -> bool {
        self.frames == Some(0) && self.at.is_empty() && self.crop.is_none()
    }

    /// Whether this needs the video itself playing: a number of frames, exact moments, a part of
    /// the video, or a part of the frame. A plain call, and a search, are answered from the
    /// player's thumbnails when it has them.
    pub fn live(&self) -> bool {
        !self.off()
            && (self.frames.is_some()
                || !self.at.is_empty()
                || !self.window.is_whole()
                || self.crop.is_some())
    }
}

/// The moments a look comes down to, once the video's length is known.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Moments {
    pub from: f64,
    pub to: f64,
    /// The exact ones asked for, cut to the video.
    pub at: Vec<f64>,
    /// The ones spread over the window, in time order.
    pub spread: Vec<f64>,
    /// Pictures held between two spread moments, which the spread alone would have stepped over.
    pub held: Vec<f64>,
}

impl Moments {
    /// Every moment in the order to capture it: the exact ones first, then the spread ones
    /// coarse to fine, so the clock running out leaves the whole window seen thinly.
    pub fn order(&self) -> Vec<f64> {
        self.at
            .iter()
            .copied()
            .chain(
                look::coarse_first(self.spread.len())
                    .into_iter()
                    .map(|i| self.spread[i]),
            )
            .chain(self.held.iter().copied())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.at.len() + self.spread.len() + self.held.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Seconds between two spread moments.
    pub fn every(&self) -> Option<f64> {
        (self.spread.len() > 1).then(|| (self.to - self.from) / self.spread.len() as f64)
    }
}

/// Which moments to show of a video `duration` seconds long. `spoken` is whether there are words
/// to read beside the picture; without them the picture gets twice the moments.
///
/// Exact moments alone are just those. A window, a number, or nothing at all spreads moments
/// over the window. `Err` when the window lies outside the video.
pub fn moments(want: &Look, duration: f64, spoken: bool) -> Result<Moments, String> {
    let Some((from, to)) = want.window.within(duration) else {
        return Err(format!(
            "that part is outside the video, which is {} long",
            align::clock(duration)
        ));
    };
    let at: Vec<f64> = want
        .at
        .iter()
        .map(|t| t.min((duration - 0.05).max(0.0)))
        .collect();
    let only_exact = want.only_exact();
    if only_exact {
        // Nothing is spread: what was looked at is those moments, not the video.
        return Ok(Moments {
            from: at.first().copied().unwrap_or(from),
            to: at.last().copied().unwrap_or(to),
            at,
            spread: Vec::new(),
            held: Vec::new(),
        });
    }
    let n = {
        let densest = ((to - from) / look::DENSEST).floor().max(1.0) as usize;
        want.frames
            .unwrap_or_else(|| look::budget(to - from, spoken))
            .min(densest)
    };
    Ok(Moments {
        from,
        to,
        at,
        spread: look::spread(from, to, n),
        held: want
            .also
            .iter()
            .copied()
            .filter(|t| *t >= from && *t < to)
            .collect(),
    })
}

/// A contact sheet written to disk.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SheetRef {
    pub path: String,
    pub from: f64,
    pub to: f64,
    pub cells: usize,
    /// Columns by rows, e.g. `4x4`.
    pub grid: String,
    /// What reading it costs a model, in patch tokens.
    pub tokens: u32,
}

/// What was shown and what was not: the part of the answer that keeps a model from taking a
/// thin look for a whole one.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Seen {
    pub from: f64,
    pub to: f64,
    /// Moments shown, on sheets or as frames.
    pub moments: usize,
    /// Seconds between two of them, when they are spread.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub every: Option<f64>,
    /// `thumbnails` (the player's own, small) or `player` (the video itself, captured).
    pub source: &'static str,
    /// Captures that came back with nothing in them.
    #[serde(skip_serializing_if = "is_zero")]
    pub blank: usize,
    /// Captures that showed the same picture as the one before, and were left out.
    #[serde(skip_serializing_if = "is_zero")]
    pub repeated: usize,
    /// Moments the clock ran out before reaching.
    #[serde(skip_serializing_if = "is_zero")]
    pub unreached: usize,
    /// Frames already captured by an earlier call, and not captured again.
    #[serde(skip_serializing_if = "is_zero")]
    pub reused: usize,
    /// Moments added because the thumbnails showed a picture held between two spread moments:
    /// a slide the spread alone would have stepped over.
    #[serde(skip_serializing_if = "is_zero")]
    pub held: usize,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// The lines that tell a model what it can look at, and how to look closer.
pub fn pictures(seen: &Seen, sheets: &[SheetRef], frames: &[FrameRef]) -> String {
    let mut out = format!(
        "Pictures: {} moment{} from {} to {}",
        seen.moments,
        if seen.moments == 1 { "" } else { "s" },
        align::clock(seen.from),
        align::clock(seen.to)
    );
    if let Some(e) = seen.every {
        out.push_str(&match e {
            e if e >= 1.5 => format!(", one every {} s", e.round()),
            e => format!(", {:.0} a second", (1.0 / e).round().max(1.0)),
        });
    }
    out.push_str(match seen.source {
        "thumbnails" => {
            ", from the player's own thumbnails (small: enough to find a moment, not to read one)"
        }
        _ => ", captured from the video",
    });
    out.push('.');
    if seen.held > 0 {
        out.push_str(&format!(
            " {} of them added where a picture was held between two of the others.",
            seen.held
        ));
    }
    for (n, why) in [
        (seen.unreached, "not reached before the time ran out"),
        (seen.repeated, "left out for repeating the picture before"),
        (seen.blank, "left out for being blank"),
    ] {
        if n > 0 {
            out.push_str(&format!(" {n} {why}."));
        }
    }
    out.push('\n');
    if !sheets.is_empty() {
        out.push_str(
            "Sheets (each cell has its time in its corner; read left to right, top to bottom):\n",
        );
        for s in sheets {
            out.push_str(&format!(
                "- {}  {}-{}  {} cells {}  ~{} tokens\n",
                s.path,
                align::clock(s.from),
                align::clock(s.to),
                s.cells,
                s.grid,
                s.tokens
            ));
        }
    }
    if let Some(t) = frames.iter().filter_map(|f| f.tokens).max() {
        out.push_str(&format!(
            "Frames are in the timeline below, up to ~{t} tokens each.\n"
        ));
    }
    out.push_str(
        "Closer: start/end for frames of a part, at for exact moments, crop to read small text, \
         query to find a moment.\n",
    );
    out
}

/// What a search found, as the lines a model reads.
pub fn found_lines(query: &str, hits: &[crate::video_index::Hit]) -> String {
    if hits.is_empty() {
        return format!("Nothing found for \"{query}\".\n");
    }
    let mut out = format!("Moments for \"{query}\":\n");
    for h in hits {
        out.push_str(&format!("[{}] {}", align::clock(h.t), h.by));
        match &h.text {
            Some(t) => out.push_str(&format!(": {t}")),
            None => out.push_str(&format!(" (likeness {:.3})", h.score)),
        }
        if let Some(p) = &h.path {
            out.push_str(&format!(" (frame: {p})"));
        }
        out.push('\n');
    }
    out
}

/// What a fetched page turned out to be.
pub enum Found {
    Video(Box<VideoInfo>),
    /// Only an embedded player: the page to read instead.
    Embed(String),
    Nothing,
}

/// What was read, and from where.
#[derive(Default)]
pub struct Reading {
    pub cues: Vec<Cue>,
    pub track: Option<Track>,
    /// Frames captured from the page, when the call looked at the video itself.
    pub frames: Vec<FrameRef>,
    pub sheets: Vec<SheetRef>,
    /// What was shown, when anything was.
    pub seen: Option<Seen>,
    /// The part of the video the call asked about.
    pub window: Window,
    /// The call asked about exact moments: only the passages with a frame in them are returned.
    pub moments_only: bool,
    /// A search and what it found: returned in place of the timeline.
    pub found: Option<(String, Vec<crate::video_index::Hit>)>,
    pub notes: Vec<String>,
}

/// The timeline a model reads: a header saying what this is, then the passages.
pub fn content(info: &VideoInfo, reading: &Reading) -> String {
    let mut segments = align::timeline(
        &reading.cues,
        &info.chapters,
        &reading.frames,
        WINDOW_SECS,
        (reading.window.start, reading.window.end),
    );
    if reading.moments_only && !reading.frames.is_empty() {
        segments.retain(|s| s.frame.is_some());
    }
    let mut out = String::new();
    if let Some(t) = &info.title {
        out.push_str(&format!("# {t}\n\n"));
    }
    if let Some(seen) = &reading.seen {
        out.push_str(&pictures(seen, &reading.sheets, &reading.frames));
        out.push('\n');
    }
    if let Some((query, hits)) = &reading.found {
        out.push_str(&found_lines(query, hits));
        // A search that captured nothing still has the pictures it found, on a sheet of their own.
        if reading.seen.is_none() {
            for s in &reading.sheets {
                out.push_str(&format!(
                    "The pictures found, each with its time: {}  ~{} tokens\n",
                    s.path, s.tokens
                ));
            }
        }
        return out;
    }
    if reading.cues.is_empty() {
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
        if !reading.frames.is_empty() {
            out.push_str("\nFrames:\n");
            out.push_str(&align::render(&segments));
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
        "sheets": reading.sheets,
        "seen": reading.seen,
        "matches": reading.found.as_ref().map(|(_, hits)| hits),
        "streams": info.streams.iter().map(|s| json!({"kind": s.kind, "mime": s.mime})).collect::<Vec<_>>(),
    });
    let obj = v.as_object_mut().expect("object");
    if let Some(u) = &info.unplayable {
        obj.insert("unplayable".into(), json!(u));
    }
    if !reading.notes.is_empty() {
        obj.insert("notes".into(), json!(reading.notes));
    }
    obj.retain(|k, v| {
        !v.is_null()
            && !(matches!(k.as_str(), "frames" | "sheets")
                && v.as_array().is_some_and(Vec::is_empty))
    });
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
            ..Reading::default()
        };
        let c = content(&info, &r);
        assert!(c.contains("# Talk") && c.contains("About things.") && c.contains("[0:00] Intro"));
    }

    #[test]
    fn frames_land_in_the_timeline_with_captions_or_without() {
        let info = VideoInfo {
            title: Some("Talk".into()),
            description: Some("About things.".into()),
            ..VideoInfo::default()
        };
        let frame = FrameRef {
            t: 6.0,
            path: "/f/00-00-06.png".into(),
            ..Default::default()
        };
        let mut r = Reading {
            cues: vec![],
            track: None,
            frames: vec![frame],
            ..Reading::default()
        };
        let silent = content(&info, &r);
        assert!(silent.contains("About things."), "{silent}");
        assert!(
            silent.contains("[0:06] (frame: /f/00-00-06.png)"),
            "{silent}"
        );
        r.cues = vec![Cue {
            start: 5.0,
            end: 8.0,
            text: "hello".into(),
        }];
        let spoken = content(&info, &r);
        assert!(
            spoken.contains("[0:05] hello (frame: /f/00-00-06.png)"),
            "{spoken}"
        );
    }

    fn look(start: Option<&str>, end: Option<&str>, at: &[&str], frames: Option<u32>) -> Look {
        let at: Vec<String> = at.iter().map(|s| s.to_string()).collect();
        Look::parse(start, end, &at, frames, None, None).unwrap()
    }

    #[test]
    fn a_plain_call_plays_nothing_and_a_part_or_a_number_does() {
        assert!(!look(None, None, &[], None).live());
        assert!(look(Some("1:00"), None, &[], None).live());
        assert!(look(None, None, &["0:30"], None).live());
        assert!(look(None, None, &[], Some(8)).live());
        let off = look(None, None, &[], Some(0));
        assert!(off.off() && !off.live());
        let search = Look::parse(None, None, &[], None, None, Some(" a chart ")).unwrap();
        assert!(!search.live(), "a search reads the thumbnails first");
        assert_eq!(search.query.as_deref(), Some("a chart"));
    }

    #[test]
    fn an_argument_that_cannot_be_read_is_said_and_nothing_runs() {
        let bad = |s: Option<&str>, e: Option<&str>, at: &[&str], crop: Option<&str>| {
            let at: Vec<String> = at.iter().map(|s| s.to_string()).collect();
            Look::parse(s, e, &at, None, crop, None).unwrap_err()
        };
        assert!(bad(Some("soon"), None, &[], None).contains("start=\"soon\""));
        assert!(bad(Some("2:00"), Some("1:00"), &[], None).contains("after start"));
        assert!(bad(None, None, &["1:00", "x"], None).contains("at=\"x\""));
        assert!(bad(None, None, &[], Some("10,20,30,40")).contains("fractions"));
        // Several moments in one string are several moments.
        assert_eq!(
            look(None, None, &["90, 0:30", "90"], None).at,
            vec![30.0, 90.0]
        );
    }

    #[test]
    fn moments_follow_the_length_unless_a_number_is_named() {
        let m = moments(&look(None, None, &[], None), 1200.0, true).unwrap();
        assert_eq!((m.from, m.to, m.spread.len()), (0.0, 1200.0, 64));
        assert_eq!(m.every(), Some(18.75));
        let silent = moments(&look(None, None, &[], None), 1200.0, false).unwrap();
        assert_eq!(silent.spread.len(), 120, "nothing said: twice the picture");
        let part = moments(&look(Some("1:00"), Some("1:10"), &[], None), 1200.0, true).unwrap();
        assert_eq!((part.from, part.to, part.spread.len()), (60.0, 70.0, 16));
        let named = moments(&look(None, None, &[], Some(5)), 1200.0, true).unwrap();
        assert_eq!(named.spread.len(), 5);
        // A second of video is at most four moments, whatever was asked.
        let burst = moments(&look(Some("10"), Some("11"), &[], Some(50)), 1200.0, true).unwrap();
        assert_eq!(burst.spread.len(), 4);
    }

    #[test]
    fn exact_moments_are_only_those_and_are_captured_first() {
        let m = moments(&look(None, None, &["0:30", "99:00"], None), 100.0, true).unwrap();
        assert!(m.spread.is_empty());
        assert_eq!(
            m.at,
            vec![30.0, 99.95],
            "a moment past the end is the last frame"
        );
        assert_eq!(
            (m.from, m.to),
            (30.0, 99.95),
            "what was looked at is those moments"
        );
        let both = moments(
            &look(Some("0"), Some("40"), &["0:30"], Some(4)),
            100.0,
            true,
        )
        .unwrap();
        assert_eq!(both.len(), 5);
        let order = both.order();
        assert_eq!(order[0], 30.0, "the moment asked for by name comes first");
        assert_eq!(
            order[1], 25.0,
            "then the middle of the part, then its halves"
        );
        let mut held = look(Some("0"), Some("40"), &[], Some(4));
        held.also = vec![12.0, 90.0];
        let m = moments(&held, 100.0, true).unwrap();
        assert_eq!(m.held, vec![12.0], "only the held pictures inside the part");
        assert_eq!(
            m.order().last(),
            Some(&12.0),
            "after everything that was asked for"
        );
        assert_eq!(m.len(), 5);
        assert!(moments(&look(Some("5:00"), None, &[], None), 100.0, true)
            .unwrap_err()
            .contains("1:40 long"));
    }

    #[test]
    fn the_answer_says_what_was_shown_what_was_not_and_how_to_look_closer() {
        let seen = Seen {
            from: 0.0,
            to: 1200.0,
            moments: 20,
            every: Some(60.0),
            source: "thumbnails",
            ..Seen::default()
        };
        let sheets = vec![SheetRef {
            path: "/v/sheet-thumbs-00-00-30-000.jpg".into(),
            from: 30.0,
            to: 930.0,
            cells: 16,
            grid: "4x4".into(),
            tokens: 1196,
        }];
        let text = pictures(&seen, &sheets, &[]);
        assert!(text.starts_with("Pictures: 20 moments from 0:00 to 20:00, one every 60 s"));
        assert!(text.contains("not to read one"), "{text}");
        assert!(text.contains(
            "- /v/sheet-thumbs-00-00-30-000.jpg  0:30-15:30  16 cells 4x4  ~1196 tokens"
        ));
        assert!(text.contains("Closer: start/end"));

        let live = Seen {
            from: 60.0,
            to: 62.0,
            moments: 6,
            every: Some(0.25),
            source: "player",
            unreached: 2,
            repeated: 1,
            ..Seen::default()
        };
        let frame = FrameRef {
            t: 60.1,
            path: "/v/f.png".into(),
            tokens: Some(1196),
            ..Default::default()
        };
        let text = pictures(&live, &[], &[frame]);
        assert!(
            text.contains("4 a second, captured from the video."),
            "{text}"
        );
        assert!(text.contains("2 not reached before the time ran out."));
        let with_held = Seen {
            held: 3,
            ..live.clone()
        };
        assert!(pictures(&with_held, &[], &[])
            .contains("3 of them added where a picture was held between two of the others."));
        assert!(text.contains("1 left out for repeating"));
        assert!(text.contains("up to ~1196 tokens each"));
        assert!(!text.contains("Sheets"));
    }

    #[test]
    fn a_search_answers_with_moments_instead_of_the_timeline() {
        let info = VideoInfo {
            title: Some("Talk".into()),
            ..VideoInfo::default()
        };
        let hit = |t: f64, by: &'static str, text: Option<&str>| crate::video_index::Hit {
            t,
            by,
            text: text.map(str::to_string),
            score: 0.25,
            path: (by == "shown").then(|| "/v/f.png".to_string()),
        };
        let mut r = Reading {
            cues: vec![Cue {
                start: 5.0,
                end: 8.0,
                text: "the whole transcript".into(),
            }],
            found: Some((
                "chart".into(),
                vec![
                    hit(65.0, "said", Some("look at this chart")),
                    hit(70.0, "shown", Some("Chart 1: revenue")),
                    hit(300.0, "seen", None),
                ],
            )),
            ..Reading::default()
        };
        let c = content(&info, &r);
        assert!(c.contains("[1:05] said: look at this chart\n"), "{c}");
        assert!(c.contains("[1:10] shown: Chart 1: revenue (frame: /v/f.png)\n"));
        assert!(c.contains("[5:00] seen (likeness 0.250)\n"));
        assert!(!c.contains("the whole transcript"));
        assert_eq!(summary(&info, &r)["matches"][2]["by"], "seen");
        r.sheets = vec![SheetRef {
            path: "/v/sheet-found.jpg".into(),
            from: 65.0,
            to: 300.0,
            cells: 2,
            grid: "2x1".into(),
            tokens: 300,
        }];
        assert!(content(&info, &r)
            .contains("The pictures found, each with its time: /v/sheet-found.jpg  ~300 tokens"));
        r.found = Some(("chart".into(), vec![]));
        assert!(content(&info, &r).contains("Nothing found for \"chart\"."));
    }

    #[test]
    fn exact_moments_bring_back_their_passages_and_not_the_whole_transcript() {
        let info = VideoInfo::default();
        let r = Reading {
            cues: (0..6)
                .map(|i| Cue {
                    start: i as f64 * 40.0,
                    end: i as f64 * 40.0 + 5.0,
                    text: format!("line {i}"),
                })
                .collect(),
            frames: vec![FrameRef {
                t: 81.0,
                path: "/v/f.png".into(),
                ..Default::default()
            }],
            moments_only: true,
            ..Reading::default()
        };
        let c = content(&info, &r);
        assert!(c.contains("line 2 (frame: /v/f.png)"), "{c}");
        assert!(!c.contains("line 0") && !c.contains("line 4"), "{c}");
    }

    #[test]
    fn a_part_of_the_video_is_a_part_of_the_timeline_too() {
        let info = VideoInfo::default();
        let r = Reading {
            cues: (0..6)
                .map(|i| Cue {
                    start: i as f64 * 20.0,
                    end: i as f64 * 20.0 + 5.0,
                    text: format!("line {i}"),
                })
                .collect(),
            window: Window {
                start: Some(40.0),
                end: Some(80.0),
            },
            ..Reading::default()
        };
        let c = content(&info, &r);
        assert!(c.contains("line 2") && c.contains("line 3"), "{c}");
        assert!(!c.contains("line 1") && !c.contains("line 4"), "{c}");
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
                ..Default::default()
            }],
            notes: vec!["n".into()],
            ..Reading::default()
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
