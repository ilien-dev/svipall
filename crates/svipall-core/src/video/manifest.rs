//! Streaming manifests, read for two things only: the caption renditions they list, and whether
//! the stream is encrypted.
//!
//! An encrypted stream is reported and left alone. Decrypting one would be breaking an access
//! control, not evading a wall, and nothing in this crate goes near it.

use super::{CaptionKind, Track};
use quick_xml::events::Event;
use quick_xml::Reader;

#[derive(Debug, Default, PartialEq)]
pub struct Manifest {
    pub subtitles: Vec<Track>,
    /// Media segments, for a media playlist: a segmented caption rendition is one of these.
    pub segments: Vec<String>,
    /// What the stream says protects it, when anything does.
    pub drm: Option<String>,
}

fn join(base: &str, rel: &str) -> String {
    url::Url::parse(base)
        .and_then(|b| b.join(rel.trim()))
        .map(|u| u.to_string())
        .unwrap_or_else(|_| rel.trim().to_string())
}

/// `KEY="value",KEY=value` as HLS writes attribute lists; quoted values may hold commas.
fn attributes(list: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = list;
    while let Some(eq) = rest.find('=') {
        let key = rest[..eq].trim().trim_start_matches(',').trim().to_string();
        rest = &rest[eq + 1..];
        let value = if let Some(stripped) = rest.strip_prefix('"') {
            let end = stripped.find('"').unwrap_or(stripped.len());
            let v = stripped[..end].to_string();
            rest = stripped.get(end + 1..).unwrap_or("");
            v
        } else {
            let end = rest.find(',').unwrap_or(rest.len());
            let v = rest[..end].to_string();
            rest = &rest[end..];
            v
        };
        out.push((key, value));
    }
    out
}

fn get<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.as_str())
}

/// An HLS playlist, master or media.
pub fn hls(body: &str, base: &str) -> Manifest {
    let mut m = Manifest::default();
    let mut expect_uri = false;
    for line in body.lines().map(str::trim) {
        if let Some(list) = line.strip_prefix("#EXT-X-MEDIA:") {
            let a = attributes(list);
            if get(&a, "TYPE") == Some("SUBTITLES") {
                if let Some(uri) = get(&a, "URI") {
                    m.subtitles.push(Track {
                        url: join(base, uri),
                        lang: get(&a, "LANGUAGE").map(str::to_string),
                        label: get(&a, "NAME").map(str::to_string),
                        kind: CaptionKind::Manual,
                    });
                }
            }
        } else if let Some(list) = line
            .strip_prefix("#EXT-X-KEY:")
            .or_else(|| line.strip_prefix("#EXT-X-SESSION-KEY:"))
        {
            let a = attributes(list);
            let method = get(&a, "METHOD").unwrap_or("NONE");
            if !method.eq_ignore_ascii_case("NONE") && m.drm.is_none() {
                m.drm = Some(match get(&a, "KEYFORMAT") {
                    Some(f) => format!("{method} {f}"),
                    None => method.to_string(),
                });
            }
        } else if line.starts_with("#EXTINF") {
            expect_uri = true;
        } else if expect_uri && !line.is_empty() && !line.starts_with('#') {
            m.segments.push(join(base, line));
            expect_uri = false;
        }
    }
    m
}

/// A DASH manifest.
pub fn dash(body: &str, base: &str) -> Manifest {
    let mut m = Manifest::default();
    let mut r = Reader::from_str(body);
    let mut buf = Vec::new();
    // (lang, is_text) of the adaptation set we are inside.
    let mut set: Option<(Option<String>, bool)> = None;
    let mut in_base = false;
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let attr = |name: &[u8]| {
                    e.attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == name)
                        .map(|a| String::from_utf8_lossy(&a.value).into_owned())
                };
                match e.local_name().as_ref() {
                    b"AdaptationSet" => {
                        let text = attr(b"contentType").as_deref() == Some("text")
                            || attr(b"mimeType").is_some_and(|t| t.starts_with("text/"));
                        set = Some((attr(b"lang"), text));
                    }
                    b"ContentProtection" if m.drm.is_none() => {
                        m.drm = Some(attr(b"schemeIdUri").unwrap_or_else(|| "protected".into()));
                    }
                    b"BaseURL" => in_base = true,
                    _ => {}
                }
            }
            Ok(Event::Text(t)) if in_base => {
                if let Some((lang, true)) = &set {
                    let rel = t.unescape().unwrap_or_default().trim().to_string();
                    if !rel.is_empty() {
                        m.subtitles.push(Track {
                            url: join(base, &rel),
                            lang: lang.clone(),
                            label: None,
                            kind: CaptionKind::Manual,
                        });
                    }
                }
            }
            Ok(Event::End(e)) => match e.local_name().as_ref() {
                b"AdaptationSet" => set = None,
                b"BaseURL" => in_base = false,
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    const MASTER: &str = r#"#EXTM3U
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="a",NAME="en",URI="audio/en.m3u8"
#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID="subs",LANGUAGE="es",NAME="Español, latino",AUTOSELECT=YES,URI="subs/es.m3u8"
#EXT-X-STREAM-INF:BANDWIDTH=800000,CODECS="avc1.4d401f,mp4a.40.2",SUBTITLES="subs"
video/720.m3u8
"#;

    #[test]
    fn subtitle_renditions_of_a_master_playlist() {
        let m = hls(MASTER, "https://cdn.example/v/1/master.m3u8");
        assert_eq!(m.subtitles.len(), 1, "audio renditions are not captions");
        let t = &m.subtitles[0];
        assert_eq!(t.url, "https://cdn.example/v/1/subs/es.m3u8");
        assert_eq!(t.lang.as_deref(), Some("es"));
        assert_eq!(
            t.label.as_deref(),
            Some("Español, latino"),
            "a quoted comma"
        );
        assert_eq!(m.drm, None);
    }

    #[test]
    fn segments_of_a_media_playlist() {
        let p = "#EXTM3U\n#EXT-X-TARGETDURATION:60\n#EXTINF:60.0,\nes-0.vtt\n#EXTINF:12.5,\nes-1.vtt\n#EXT-X-ENDLIST\n";
        let m = hls(p, "https://cdn.example/v/1/subs/es.m3u8");
        assert_eq!(
            m.segments,
            vec![
                "https://cdn.example/v/1/subs/es-0.vtt",
                "https://cdn.example/v/1/subs/es-1.vtt"
            ]
        );
    }

    #[test]
    fn a_key_is_reported_and_none_is_not_a_key() {
        let plain = "#EXTM3U\n#EXT-X-KEY:METHOD=NONE\n";
        assert_eq!(hls(plain, "https://x/").drm, None);
        let locked = "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://k\",KEYFORMAT=\"com.apple.streamingkeydelivery\"\n";
        assert_eq!(
            hls(locked, "https://x/").drm.as_deref(),
            Some("SAMPLE-AES com.apple.streamingkeydelivery")
        );
    }

    const MPD: &str = r#"<?xml version="1.0"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011">
 <Period>
  <AdaptationSet contentType="video" mimeType="video/mp4">
   <Representation id="v1"><BaseURL>video.mp4</BaseURL></Representation>
  </AdaptationSet>
  <AdaptationSet contentType="text" mimeType="text/vtt" lang="en">
   <Representation id="t1"><BaseURL>subs/en.vtt</BaseURL></Representation>
  </AdaptationSet>
 </Period>
</MPD>"#;

    #[test]
    fn text_adaptation_sets_of_a_dash_manifest() {
        let m = dash(MPD, "https://cdn.example/v/1/manifest.mpd");
        assert_eq!(m.subtitles.len(), 1);
        assert_eq!(m.subtitles[0].url, "https://cdn.example/v/1/subs/en.vtt");
        assert_eq!(m.subtitles[0].lang.as_deref(), Some("en"));
        assert_eq!(m.drm, None);
    }

    #[test]
    fn content_protection_is_drm() {
        let locked = MPD.replace(
            "<AdaptationSet contentType=\"video\" mimeType=\"video/mp4\">",
            "<AdaptationSet contentType=\"video\" mimeType=\"video/mp4\"><ContentProtection schemeIdUri=\"urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed\"/>",
        );
        assert_eq!(
            dash(&locked, "https://x/").drm.as_deref(),
            Some("urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed")
        );
    }
}
