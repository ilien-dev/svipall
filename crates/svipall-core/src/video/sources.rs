//! Players that say more in their own script than the page says in markup.
//!
//! Each row is one player, named by the endpoint it lives on (a protocol, not a product) exactly as
//! `widget::WIDGETS` names captcha widgets. A row finds the JSON the player was booted with, and a
//! reader turns that JSON into a [`VideoInfo`]. A new player is a new row, a reader, and a fixture
//! in `fixtures/video/`; nothing else changes.

use super::{storyboard, CaptionKind, Stream, StreamKind, Track, VideoInfo};
use serde_json::Value;

/// Caption addresses that only answer the player that minted them.
///
/// Some players attach a per-session proof to every caption request, and the address the page
/// hands out answers an empty body without it (measured: 200 with 0 bytes, from outside the page
/// and from inside it). The player's own request carries the proof, so the address is borrowed from
/// that request — seen in the network, never read from the page — and pointed at the wanted track.
#[derive(Debug)]
pub struct Borrow {
    /// Matches the player's own caption request.
    pub endpoint: &'static str,
    /// The query parameter holding the video id: the first caption request can be an advert's.
    pub id_param: &'static str,
    /// Taken from the wanted track's address, or removed when the track has none.
    pub copy: &'static [&'static str],
    /// Forced on the borrowed address.
    pub set: &'static [(&'static str, &'static str)],
}

pub struct VideoSourceSpec {
    /// Host and path of the player page. The stable identifier.
    pub id: &'static str,
    /// Hosts whose pages are this player, suffix-matched.
    pub hosts: &'static [&'static str],
    /// Text immediately before the boot JSON, in order of preference.
    pub markers: &'static [&'static str],
    pub read: fn(&Value) -> VideoInfo,
    /// Iframe addresses that embed this player.
    pub embeds: &'static [&'static str],
    /// The player page for any address of this player: an embed, a short link, the page itself.
    pub canonical: fn(&url::Url) -> Option<String>,
    pub borrow: Option<Borrow>,
}

pub const VIDEO_SOURCES: &[VideoSourceSpec] = &[
    VideoSourceSpec {
        id: "youtube.com/watch",
        hosts: &["youtube.com", "youtu.be", "youtube-nocookie.com"],
        markers: &["ytInitialPlayerResponse = ", "ytInitialPlayerResponse="],
        read: player_response,
        embeds: &["youtube.com/embed/", "youtube-nocookie.com/embed/"],
        canonical: watch_page,
        borrow: Some(Borrow {
            endpoint: "/api/timedtext",
            id_param: "v",
            copy: &["lang", "kind", "name", "tlang"],
            set: &[("fmt", "vtt")],
        }),
    },
    VideoSourceSpec {
        id: "player.vimeo.com/video",
        hosts: &["player.vimeo.com", "vimeo.com"],
        markers: &["window.playerConfig = ", "window.playerConfig="],
        read: player_config,
        embeds: &["player.vimeo.com/video/"],
        canonical: frame_page,
        borrow: None,
    },
];

pub fn spec(id: &str) -> Option<&'static VideoSourceSpec> {
    VIDEO_SOURCES.iter().find(|s| s.id == id)
}

fn host_is(url: &url::Url, hosts: &[&str]) -> bool {
    let Some(h) = url.host_str() else {
        return false;
    };
    hosts
        .iter()
        .any(|want| h == *want || h.ends_with(&format!(".{want}")))
}

/// The row whose player this address is.
pub fn for_url(url: &url::Url) -> Option<&'static VideoSourceSpec> {
    VIDEO_SOURCES.iter().find(|s| host_is(url, s.hosts))
}

/// The row whose player this iframe embeds, and the page to read instead.
pub fn embedded(frame: &str) -> Option<(&'static VideoSourceSpec, String)> {
    let low = frame.to_ascii_lowercase();
    let row = VIDEO_SOURCES
        .iter()
        .find(|s| s.embeds.iter().any(|e| low.contains(e)))?;
    let url = url::Url::parse(frame).ok()?;
    Some((row, (row.canonical)(&url)?))
}

/// The player page for an address on a player's own site that is not that page: a channel's
/// video page that loads the player in a frame, a short link. `None` when this is the page.
pub fn player_page(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    let page = (for_url(&u)?.canonical)(&u)?;
    (page != u.as_str()).then_some(page)
}

/// The JSON value that starts right after `marker`, parsed as one value and nothing more.
pub fn boot_json(html: &str, marker: &str) -> Option<Value> {
    let at = html.find(marker)? + marker.len();
    let rest = html[at..].trim_start();
    serde_json::Deserializer::from_str(rest)
        .into_iter::<Value>()
        .next()?
        .ok()
}

/// Point a borrowed caption request at another track, keeping its proof.
pub fn retarget(borrowed: &str, track: &str, b: &Borrow) -> Option<String> {
    let mut out = url::Url::parse(borrowed).ok()?;
    let track = url::Url::parse(track).ok()?;
    let wanted: Vec<(String, String)> = track.query_pairs().into_owned().collect();
    let kept: Vec<(String, String)> = out
        .query_pairs()
        .into_owned()
        .filter(|(k, _)| !b.copy.contains(&k.as_str()) && !b.set.iter().any(|(s, _)| s == k))
        .collect();
    let mut q = out.query_pairs_mut();
    q.clear();
    for (k, v) in &kept {
        q.append_pair(k, v);
    }
    for (k, v) in wanted.iter().filter(|(k, _)| b.copy.contains(&k.as_str())) {
        q.append_pair(k, v);
    }
    for (k, v) in b.set {
        q.append_pair(k, v);
    }
    drop(q);
    Some(out.to_string())
}

fn text(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string).or_else(|| {
        v.get("simpleText")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                let runs = v.get("runs")?.as_array()?;
                Some(
                    runs.iter()
                        .filter_map(|r| r.get("text").and_then(Value::as_str))
                        .collect(),
                )
            })
    })
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn player_response(v: &Value) -> VideoInfo {
    let d = &v["videoDetails"];
    let duration = num(&d["lengthSeconds"]);
    let tracks = v["captions"]["playerCaptionsTracklistRenderer"]["captionTracks"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    Some(Track {
                        url: t["baseUrl"].as_str()?.to_string(),
                        lang: t["languageCode"].as_str().map(str::to_string),
                        label: text(&t["name"]),
                        kind: if t["kind"].as_str() == Some("asr") {
                            CaptionKind::Auto
                        } else {
                            CaptionKind::Manual
                        },
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let status = v["playabilityStatus"]["status"].as_str().unwrap_or("OK");
    let streams = [
        ("hlsManifestUrl", StreamKind::Hls),
        ("dashManifestUrl", StreamKind::Dash),
    ]
    .into_iter()
    .filter_map(|(k, kind)| {
        Some(Stream {
            url: v["streamingData"][k].as_str()?.to_string(),
            kind,
            mime: None,
        })
    })
    .collect();
    VideoInfo {
        id: d["videoId"].as_str().map(str::to_string),
        title: d["title"].as_str().map(str::to_string),
        description: d["shortDescription"].as_str().map(str::to_string),
        duration,
        tracks,
        storyboard: v["storyboards"]["playerStoryboardSpecRenderer"]["spec"]
            .as_str()
            .map(|s| storyboard::pipe_spec(s, duration))
            .unwrap_or_default(),
        streams,
        unplayable: (status != "OK").then(|| {
            v["playabilityStatus"]["reason"]
                .as_str()
                .unwrap_or(status)
                .to_string()
        }),
        ..VideoInfo::default()
    }
}

fn watch_page(u: &url::Url) -> Option<String> {
    let id = if u.host_str()?.ends_with("youtu.be") {
        u.path_segments()?.next().map(str::to_string)
    } else if let Some(v) = u.query_pairs().find(|(k, _)| k == "v") {
        Some(v.1.into_owned())
    } else {
        let mut seg = u.path_segments()?;
        match seg.next()? {
            "embed" | "shorts" | "live" | "v" => seg.next().map(str::to_string),
            _ => None,
        }
    }?;
    let ok = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ok.then(|| format!("https://www.youtube.com/watch?v={id}"))
}

fn player_config(v: &Value) -> VideoInfo {
    let video = &v["video"];
    let req = &v["request"];
    let tracks = req["text_tracks"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    let kind = t["kind"].as_str().unwrap_or("subtitles");
                    if kind == "chapters" || kind == "metadata" {
                        return None;
                    }
                    let generated = t["provenance"]
                        .as_str()
                        .is_some_and(|p| p.contains("ai") || p.contains("auto"));
                    Some(Track {
                        url: t["url"].as_str()?.to_string(),
                        lang: t["lang"].as_str().map(str::to_string),
                        label: t["label"].as_str().map(str::to_string),
                        kind: if generated {
                            CaptionKind::Auto
                        } else {
                            CaptionKind::Manual
                        },
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let files = &req["files"];
    let cdn_url = |k: &str| -> Option<String> {
        let cdns = &files[k]["cdns"];
        let default = files[k]["default_cdn"].as_str();
        let pick = default
            .and_then(|d| cdns.get(d))
            .or_else(|| cdns.as_object()?.values().next())?;
        pick["url"]
            .as_str()
            .or_else(|| pick["avc_url"].as_str())
            .map(str::to_string)
    };
    let mut streams: Vec<Stream> = [("hls", StreamKind::Hls), ("dash", StreamKind::Dash)]
        .into_iter()
        .filter_map(|(k, kind)| {
            Some(Stream {
                url: cdn_url(k)?,
                kind,
                mime: None,
            })
        })
        .collect();
    if let Some(p) = files["progressive"].as_array() {
        streams.extend(p.iter().filter_map(|f| {
            Some(Stream {
                url: f["url"].as_str()?.to_string(),
                kind: StreamKind::Progressive,
                mime: f["mime"].as_str().map(str::to_string),
            })
        }));
    }
    VideoInfo {
        id: num(&video["id"]).map(|n| format!("{n:.0}")),
        title: video["title"].as_str().map(str::to_string),
        duration: num(&video["duration"]),
        tracks,
        streams,
        ..VideoInfo::default()
    }
}

fn frame_page(u: &url::Url) -> Option<String> {
    let host = u.host_str()?;
    if host.starts_with("player.") {
        return Some(u.to_string());
    }
    let id = u
        .path_segments()?
        .find(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))?;
    Some(format!("https://player.vimeo.com/video/{id}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> url::Url {
        url::Url::parse(s).unwrap()
    }

    #[test]
    fn every_row_is_complete() {
        for s in VIDEO_SOURCES {
            assert!(!s.hosts.is_empty(), "{}: no hosts", s.id);
            assert!(
                !s.markers.is_empty(),
                "{}: nothing to find its boot JSON by",
                s.id
            );
            assert!(
                !s.embeds.is_empty(),
                "{}: an embed would go unrecognised",
                s.id
            );
        }
    }

    #[test]
    fn every_address_of_a_watch_page_leads_to_it() {
        let want = Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string());
        for a in [
            "https://youtu.be/dQw4w9WgXcQ?t=3",
            "https://www.youtube.com/embed/dQw4w9WgXcQ?rel=0",
            "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ",
            "https://m.youtube.com/watch?v=dQw4w9WgXcQ&list=x",
            "https://www.youtube.com/shorts/dQw4w9WgXcQ",
        ] {
            assert_eq!(watch_page(&u(a)), want, "{a}");
        }
        assert_eq!(watch_page(&u("https://www.youtube.com/@channel")), None);
    }

    #[test]
    fn a_player_page_is_its_own_canonical_and_keeps_its_hash() {
        assert_eq!(
            frame_page(&u("https://vimeo.com/76979871")).as_deref(),
            Some("https://player.vimeo.com/video/76979871")
        );
        let unlisted = "https://player.vimeo.com/video/1?h=abc";
        assert_eq!(frame_page(&u(unlisted)).as_deref(), Some(unlisted));
    }

    #[test]
    fn an_address_on_a_players_site_leads_to_the_player_page_once() {
        assert_eq!(
            player_page("https://vimeo.com/76979871").as_deref(),
            Some("https://player.vimeo.com/video/76979871")
        );
        assert_eq!(player_page("https://player.vimeo.com/video/76979871"), None);
        assert_eq!(player_page("https://www.youtube.com/watch?v=abc"), None);
        assert_eq!(
            player_page("https://youtu.be/abc").as_deref(),
            Some("https://www.youtube.com/watch?v=abc")
        );
        assert_eq!(player_page("https://example.org/v/1"), None);
    }

    #[test]
    fn an_iframe_is_recognised_by_its_address() {
        let (row, page) = embedded("https://www.youtube.com/embed/abc_DEF-123").unwrap();
        assert_eq!(row.id, "youtube.com/watch");
        assert_eq!(page, "https://www.youtube.com/watch?v=abc_DEF-123");
        assert!(embedded("https://maps.example/embed/1").is_none());
    }

    #[test]
    fn the_boot_json_stops_at_the_end_of_its_value() {
        let html =
            r#"<script>var ytInitialPlayerResponse = {"a":{"b":"};"}};var meta = 1;</script>"#;
        let v = boot_json(html, "ytInitialPlayerResponse = ").unwrap();
        assert_eq!(v["a"]["b"], "};");
    }

    #[test]
    fn a_borrowed_request_is_pointed_at_the_wanted_track() {
        let b = spec("youtube.com/watch").unwrap().borrow.as_ref().unwrap();
        let seen = "https://www.youtube.com/api/timedtext?v=ID&ei=E&kind=asr&lang=en-US&variant=x&pot=PROOF&fmt=json3&c=WEB";
        let track = "https://www.youtube.com/api/timedtext?v=ID&ei=E&lang=es-419&name=Latino";
        let got = url::Url::parse(&retarget(seen, track, b).unwrap()).unwrap();
        let q: std::collections::HashMap<String, String> = got.query_pairs().into_owned().collect();
        assert_eq!(q["pot"], "PROOF", "the proof is what makes it answer");
        assert_eq!(q["lang"], "es-419");
        assert_eq!(q["name"], "Latino");
        assert_eq!(q["fmt"], "vtt");
        assert!(
            !q.contains_key("kind"),
            "the wanted track is not machine-made"
        );
        assert_eq!(q["c"], "WEB");
    }
}
