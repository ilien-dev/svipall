//! The page's own video elements: what `<video>`, `<source>`, `<track>` and an embedding `<iframe>`
//! say, resolved against the page, before any script has had a chance to replace them.
//!
//! This is the generic floor under every video page. A site with a player of its own says more in
//! its script, and `svipall_core::video` reads that; a plain page says all it has here.

use scraper::{ElementRef, Html, Selector};
use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Video {
    /// Every source the element offers: its own `src` first, then each `<source>`.
    pub sources: Vec<Source>,
    pub tracks: Vec<TextTrack>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poster: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Source {
    pub src: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TextTrack {
    pub src: String,
    /// `subtitles` when the page leaves it out, which is what the element itself assumes.
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub default: bool,
}

/// What the page's markup says about video.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PageMedia {
    pub videos: Vec<Video>,
    /// Every iframe `src`, resolved: an embedded player is found by its address, and which
    /// addresses are players is the caller's table, not this crate's.
    pub frames: Vec<String>,
}

fn resolve(base: Option<&url::Url>, raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.starts_with("blob:") || raw.starts_with("data:") {
        return None;
    }
    match base {
        Some(b) => b.join(raw).ok().map(|u| u.to_string()),
        None => Some(raw.to_string()),
    }
}

fn attr(el: &ElementRef<'_>, name: &str) -> Option<String> {
    el.value()
        .attr(name)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

pub(crate) fn media_from(doc: &Html, base_url: &str) -> PageMedia {
    let base = url::Url::parse(base_url).ok();
    let base = base.as_ref();
    let video = Selector::parse("video").expect("static selector");
    let source = Selector::parse("source").expect("static selector");
    let track = Selector::parse("track").expect("static selector");
    let iframe = Selector::parse("iframe[src]").expect("static selector");

    let videos = doc
        .select(&video)
        .map(|v| {
            let own = attr(&v, "src")
                .and_then(|s| resolve(base, &s))
                .map(|src| Source { src, mime: None });
            let listed = v.select(&source).filter_map(|s| {
                let src = resolve(base, &attr(&s, "src")?)?;
                Some(Source {
                    src,
                    mime: attr(&s, "type"),
                })
            });
            let tracks = v
                .select(&track)
                .filter_map(|t| {
                    Some(TextTrack {
                        src: resolve(base, &attr(&t, "src")?)?,
                        kind: attr(&t, "kind")
                            .map(|k| k.to_ascii_lowercase())
                            .unwrap_or_else(|| "subtitles".into()),
                        lang: attr(&t, "srclang"),
                        label: attr(&t, "label"),
                        default: t.value().attr("default").is_some(),
                    })
                })
                .collect();
            Video {
                sources: own.into_iter().chain(listed).collect(),
                tracks,
                poster: attr(&v, "poster").and_then(|p| resolve(base, &p)),
            }
        })
        .collect();
    let frames = doc
        .select(&iframe)
        .filter_map(|f| resolve(base, &attr(&f, "src")?))
        .collect();
    PageMedia { videos, frames }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(html: &str) -> PageMedia {
        media_from(
            &Html::parse_document(html),
            "https://example.org/talks/one.html",
        )
    }

    #[test]
    fn sources_and_tracks_are_resolved_against_the_page() {
        let m = parse(
            r#"<video id="v" controls poster="p.jpg">
                 <source src='v/clip.ogg' type='video/ogg'>
                 <source src='v/clip.mp4' type='video/mp4'>
                 <track label="English" kind="subtitles" srclang="en" src="subs-en.vtt" default>
                 <track kind="chapters" srclang="en" src="/chapters.vtt">
               </video>"#,
        );
        assert_eq!(m.videos.len(), 1);
        let v = &m.videos[0];
        assert_eq!(v.sources[1].src, "https://example.org/talks/v/clip.mp4");
        assert_eq!(v.sources[1].mime.as_deref(), Some("video/mp4"));
        assert_eq!(v.tracks[0].src, "https://example.org/talks/subs-en.vtt");
        assert_eq!(v.tracks[0].lang.as_deref(), Some("en"));
        assert!(v.tracks[0].default);
        assert_eq!(v.tracks[1].kind, "chapters");
        assert_eq!(v.tracks[1].src, "https://example.org/chapters.vtt");
        assert_eq!(v.poster.as_deref(), Some("https://example.org/talks/p.jpg"));
    }

    #[test]
    fn a_blob_source_is_not_an_address() {
        // A player built on media source extensions shows a blob URL that means nothing outside
        // the tab that minted it.
        let m = parse(r#"<video src="blob:https://example.org/1234"></video>"#);
        assert!(m.videos[0].sources.is_empty());
    }

    #[test]
    fn a_track_without_a_kind_is_subtitles() {
        let m = parse(r#"<video><track src="a.vtt"></video>"#);
        assert_eq!(m.videos[0].tracks[0].kind, "subtitles");
    }

    #[test]
    fn iframes_are_listed_for_the_caller_to_recognise() {
        let m = parse(r#"<iframe src="//player.example.net/video/42?h=1"></iframe>"#);
        assert_eq!(m.frames, vec!["https://player.example.net/video/42?h=1"]);
    }
}
