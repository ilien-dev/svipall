//! Video pages, read for what the video says rather than skipped as a box with nothing in it.
//!
//! Everything here is pure: markup and bodies in, a description of the video out. Fetching the
//! page, the caption files and the frames is the server's job; this is what it asks what to fetch
//! and how to read what came back.
//!
//! What a page knows is layered, and every layer is read because none is reliable alone:
//! - a known player's boot JSON (`sources::VIDEO_SOURCES`), the richest when it is there;
//! - schema.org `VideoObject` in the page's JSON-LD;
//! - the page's own `<video>`, `<source>` and `<track>`;
//! - OpenGraph `og:video`.

pub mod align;
pub mod chapters;
pub mod cues;
pub mod manifest;
pub mod sources;
pub mod storyboard;

use serde::Serialize;
use serde_json::Value;
use svipall_extract::{Metadata, PageMedia};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// Who wrote a caption track: a person, the platform's recogniser, or ours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptionKind {
    Manual,
    Auto,
    Asr,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Track {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub kind: CaptionKind,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Chapter {
    pub start: f64,
    pub title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamKind {
    Progressive,
    Hls,
    Dash,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Stream {
    pub url: String,
    pub kind: StreamKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct VideoInfo {
    /// Which layer described the video first: a row id, `json-ld`, `html5` or `og`.
    pub source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    pub tracks: Vec<Track>,
    /// WebVTT chapter tracks: cue text is the chapter title.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub chapter_tracks: Vec<String>,
    pub chapters: Vec<Chapter>,
    #[serde(skip)]
    pub storyboard: Vec<storyboard::Frame>,
    pub streams: Vec<Stream>,
    /// A transcript published beside the video, as schema.org allows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript: Option<String>,
    /// Why the player will not play it (private, removed, age check), in its own words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unplayable: Option<String>,
}

impl VideoInfo {
    fn is_empty(&self) -> bool {
        self.tracks.is_empty()
            && self.streams.is_empty()
            && self.duration.is_none()
            && self.transcript.is_none()
            && self.storyboard.is_empty()
    }

    /// Fill what this layer left empty from a lower one. Never overwrites: the higher layer is the
    /// one that knows the player.
    fn fill_from(&mut self, low: VideoInfo) {
        if self.source.is_empty() {
            self.source = low.source;
        }
        self.id = self.id.take().or(low.id);
        self.title = self.title.take().or(low.title);
        self.description = self.description.take().or(low.description);
        self.duration = self.duration.or(low.duration);
        self.transcript = self.transcript.take().or(low.transcript);
        for t in low.tracks {
            if !self.tracks.iter().any(|m| m.url == t.url) {
                self.tracks.push(t);
            }
        }
        for s in low.streams {
            if !self.streams.iter().any(|m| m.url == s.url) {
                self.streams.push(s);
            }
        }
        self.chapter_tracks.extend(low.chapter_tracks);
        if self.chapters.is_empty() {
            self.chapters = low.chapters;
        }
    }
}

fn stream_kind(url: &str, mime: Option<&str>) -> StreamKind {
    let m = mime.unwrap_or("").to_ascii_lowercase();
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or(url)
        .to_ascii_lowercase();
    if m.contains("mpegurl") || path.ends_with(".m3u8") {
        StreamKind::Hls
    } else if m.contains("dash") || path.ends_with(".mpd") {
        StreamKind::Dash
    } else {
        StreamKind::Progressive
    }
}

/// ISO 8601 durations as schema.org writes them: `PT1H2M3.5S`, `P0DT0H3M33S`.
pub fn iso_duration(s: &str) -> Option<f64> {
    let s = s.trim().strip_prefix('P')?;
    let (days, time) = match s.split_once('T') {
        Some((d, t)) => (d, t),
        None => (s, ""),
    };
    let mut total = 0.0;
    let mut read = |part: &str, units: &[(char, f64)]| -> Option<()> {
        let mut num = String::new();
        for c in part.chars() {
            if c.is_ascii_digit() || c == '.' {
                num.push(c);
            } else {
                let (_, mult) = units.iter().find(|(u, _)| *u == c)?;
                total += num.parse::<f64>().ok()? * mult;
                num.clear();
            }
        }
        num.is_empty().then_some(())
    };
    read(days, &[('D', 86400.0)])?;
    read(time, &[('H', 3600.0), ('M', 60.0), ('S', 1.0)])?;
    Some(total)
}

fn from_json_ld(meta: &Metadata) -> Option<VideoInfo> {
    let is_video = |v: &&Value| match &v["@type"] {
        Value::String(t) => t == "VideoObject",
        Value::Array(a) => a.iter().any(|t| t == "VideoObject"),
        _ => false,
    };
    let v = meta.json_ld.iter().find(is_video)?;
    let s = |k: &str| v[k].as_str().map(str::to_string);
    let chapters = v["hasPart"]
        .as_array()
        .map(|parts| {
            parts
                .iter()
                .filter_map(|p| {
                    Some(Chapter {
                        start: p["startOffset"]
                            .as_f64()
                            .or_else(|| p["startOffset"].as_str()?.parse().ok())?,
                        title: p["name"].as_str()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let streams = s("contentUrl")
        .map(|u| Stream {
            kind: stream_kind(&u, s("encodingFormat").as_deref()),
            mime: s("encodingFormat"),
            url: u,
        })
        .into_iter()
        .collect();
    Some(VideoInfo {
        source: "json-ld",
        title: s("name"),
        description: s("description"),
        duration: s("duration").as_deref().and_then(iso_duration),
        transcript: s("transcript"),
        chapters,
        streams,
        ..VideoInfo::default()
    })
}

fn from_html5(media: &PageMedia) -> Option<VideoInfo> {
    let mut info = VideoInfo {
        source: "html5",
        ..VideoInfo::default()
    };
    for v in &media.videos {
        for s in &v.sources {
            info.streams.push(Stream {
                kind: stream_kind(&s.src, s.mime.as_deref()),
                url: s.src.clone(),
                mime: s.mime.clone(),
            });
        }
        for t in &v.tracks {
            match t.kind.as_str() {
                "subtitles" | "captions" => info.tracks.push(Track {
                    url: t.src.clone(),
                    lang: t.lang.clone(),
                    label: t.label.clone(),
                    kind: CaptionKind::Manual,
                }),
                "chapters" => info.chapter_tracks.push(t.src.clone()),
                _ => {}
            }
        }
    }
    (!media.videos.is_empty()).then_some(info)
}

fn from_open_graph(meta: &Metadata) -> Option<VideoInfo> {
    let og = &meta.open_graph;
    let url = ["og:video:secure_url", "og:video:url", "og:video"]
        .iter()
        .find_map(|k| og.get(*k))?;
    let mime = og.get("og:video:type").cloned();
    Some(VideoInfo {
        source: "og",
        title: og.get("og:title").cloned(),
        description: og.get("og:description").cloned(),
        streams: vec![Stream {
            kind: stream_kind(url, mime.as_deref()),
            url: url.clone(),
            mime,
        }],
        ..VideoInfo::default()
    })
}

/// `%2C` back to `,`: a file name is shown as the person who named it wrote it.
fn percent_decoded(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        match (
            b[i],
            b.get(i + 1).and_then(|c| hex(*c)),
            b.get(i + 2).and_then(|c| hex(*c)),
        ) {
            (b'%', Some(h), Some(l)) => {
                out.push((h * 16 + l) as u8);
                i += 3;
            }
            (c, _, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// An address that is a media file or a stream manifest rather than a page: read as the stream.
pub fn direct(url: &str) -> Option<VideoInfo> {
    let u = url::Url::parse(url).ok()?;
    let path = u.path().to_ascii_lowercase();
    // Sound alone counts: a recording with no picture is read the same way, by its words.
    const FILES: &[&str] = &[
        ".mp4", ".m4v", ".webm", ".mov", ".ogv", ".mkv", ".m3u8", ".mpd", ".mp3", ".m4a", ".wav",
        ".ogg", ".oga", ".flac",
    ];
    let name = u
        .path_segments()
        .and_then(|mut s| s.next_back())
        .map(percent_decoded);
    FILES.iter().any(|e| path.ends_with(e)).then(|| VideoInfo {
        source: "file",
        title: name,
        streams: vec![Stream {
            kind: stream_kind(url, None),
            url: url.to_string(),
            mime: None,
        }],
        ..VideoInfo::default()
    })
}

/// Everything the page says about its video. `None` when it says nothing: the page is not a video
/// page, and the caller should look at [`embedded_player`] before concluding so.
pub fn discover(
    url: &str,
    html: &str,
    media: Option<&PageMedia>,
    meta: Option<&Metadata>,
) -> Option<VideoInfo> {
    let page = url::Url::parse(url).ok();
    let row = page.as_ref().and_then(sources::for_url);
    let mut info = row
        .and_then(|r| {
            let v = r.markers.iter().find_map(|m| sources::boot_json(html, m))?;
            let mut i = (r.read)(&v);
            i.source = r.id;
            Some(i)
        })
        .unwrap_or_default();
    for low in [
        meta.and_then(from_json_ld),
        media.and_then(from_html5),
        meta.and_then(from_open_graph),
    ]
    .into_iter()
    .flatten()
    {
        info.fill_from(low);
    }
    if info.chapters.is_empty() {
        if let Some(d) = &info.description {
            info.chapters = chapters::from_description(d);
        }
    }
    // JSON-LD and OpenGraph are written by hand as often as not, and a relative address is common.
    if let Some(base) = &page {
        let abs = |u: &mut String| {
            if let Ok(j) = base.join(u) {
                *u = j.to_string();
            }
        };
        info.tracks.iter_mut().for_each(|t| abs(&mut t.url));
        info.streams.iter_mut().for_each(|s| abs(&mut s.url));
        info.chapter_tracks.iter_mut().for_each(abs);
    }
    (!info.is_empty()).then_some(info)
}

/// The page to read instead, when this one only embeds a known player.
pub fn embedded_player(media: Option<&PageMedia>, meta: Option<&Metadata>) -> Option<String> {
    let json_ld_embed = meta.and_then(|m| {
        m.json_ld
            .iter()
            .find_map(|v| v["embedUrl"].as_str().map(str::to_string))
    });
    media
        .into_iter()
        .flat_map(|m| m.frames.iter().cloned())
        .chain(json_ld_embed)
        .find_map(|f| sources::embedded(&f).map(|(_, page)| page))
}

#[cfg(test)]
mod tests {
    use super::*;
    use svipall_extract::{parse_page, ParseWants};

    fn read(url: &str, html: &str) -> (Option<VideoInfo>, Option<String>) {
        let parts = parse_page(
            html,
            &ParseWants {
                metadata: true,
                metadata_base_url: Some(url.into()),
                media: Some(url.into()),
                ..Default::default()
            },
        );
        (
            discover(url, html, parts.media.as_ref(), parts.metadata.as_ref()),
            embedded_player(parts.media.as_ref(), parts.metadata.as_ref()),
        )
    }

    #[test]
    fn iso_durations() {
        assert_eq!(iso_duration("PT3M33S"), Some(213.0));
        assert_eq!(iso_duration("PT1H0M2.5S"), Some(3602.5));
        assert_eq!(iso_duration("P0DT0H1M"), Some(60.0));
        assert_eq!(iso_duration("3:33"), None);
        assert_eq!(iso_duration("PT3X"), None);
    }

    #[test]
    fn a_plain_page_with_a_video_element_and_a_track() {
        let html = r#"<html><body><video controls><source src="v/a.mp4" type="video/mp4">
            <track kind="subtitles" srclang="en" src="a-en.vtt" default>
            <track kind="chapters" src="a-ch.vtt"></video></body></html>"#;
        let (info, follow) = read("https://example.org/talk/", html);
        let info = info.unwrap();
        assert_eq!(info.source, "html5");
        assert_eq!(info.tracks[0].url, "https://example.org/talk/a-en.vtt");
        assert_eq!(
            info.chapter_tracks,
            vec!["https://example.org/talk/a-ch.vtt"]
        );
        assert_eq!(info.streams[0].kind, StreamKind::Progressive);
        assert_eq!(follow, None);
    }

    #[test]
    fn json_ld_names_the_video_and_its_clips() {
        let html = r#"<html><head><script type="application/ld+json">
            {"@context":"https://schema.org","@type":"VideoObject","name":"Talk","duration":"PT10M",
             "contentUrl":"https://cdn.example/talk/master.m3u8","transcript":"Hello all.",
             "hasPart":[{"@type":"Clip","name":"Start","startOffset":0},{"@type":"Clip","name":"End","startOffset":300}]}
            </script></head><body></body></html>"#;
        let info = read("https://example.org/talk", html).0.unwrap();
        assert_eq!(info.source, "json-ld");
        assert_eq!(info.duration, Some(600.0));
        assert_eq!(info.streams[0].kind, StreamKind::Hls);
        assert_eq!(info.transcript.as_deref(), Some("Hello all."));
        assert_eq!(info.chapters[1].title, "End");
    }

    #[test]
    fn a_relative_address_is_resolved_against_the_page() {
        let html = r#"<html><head><script type="application/ld+json">
            {"@type":"VideoObject","name":"Clip","contentUrl":"/hls/master.m3u8"}
            </script></head><body></body></html>"#;
        let info = read("https://example.org/talks/clip", html).0.unwrap();
        assert_eq!(info.streams[0].url, "https://example.org/hls/master.m3u8");
    }

    #[test]
    fn a_page_that_only_embeds_a_player_points_at_it() {
        let html = r#"<html><body><p>Our talk:</p>
            <iframe src="https://www.youtube-nocookie.com/embed/abcDEF12345?start=4"></iframe></body></html>"#;
        let (info, follow) = read("https://blog.example/post", html);
        assert_eq!(info, None);
        assert_eq!(
            follow.as_deref(),
            Some("https://www.youtube.com/watch?v=abcDEF12345")
        );
    }

    #[test]
    fn a_media_address_is_its_own_stream() {
        let f = direct("https://cdn.example/media/flower.webm?x=1").unwrap();
        assert_eq!(f.source, "file");
        assert_eq!(f.streams[0].kind, StreamKind::Progressive);
        assert_eq!(
            direct("https://cdn.example/live/index.m3u8")
                .unwrap()
                .streams[0]
                .kind,
            StreamKind::Hls
        );
        assert!(direct("https://example.org/watch").is_none());
        assert_eq!(
            direct("https://cdn.example/talk.WAV").unwrap().streams[0].kind,
            StreamKind::Progressive
        );
        assert_eq!(
            direct("https://cdn.example/Caracas%2C_Intro.wav")
                .unwrap()
                .title
                .as_deref(),
            Some("Caracas,_Intro.wav")
        );
        assert!(direct("https://example.org/video.mp4.html").is_none());
    }

    #[test]
    fn a_page_with_nothing_to_play_is_not_a_video() {
        let (info, follow) = read(
            "https://example.org/",
            "<html><body><p>Hi</p></body></html>",
        );
        assert_eq!((info, follow), (None, None));
    }
}
