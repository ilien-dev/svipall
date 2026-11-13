//! Streaming manifests, read for three things only: the caption renditions they list, the
//! thumbnails they publish for the seek bar, and whether the stream is encrypted.
//!
//! An encrypted stream is reported and left alone. Decrypting one would be breaking an access
//! control, not evading a wall, and nothing in this crate goes near it.

use super::{storyboard::Frame, CaptionKind, Track};
use quick_xml::events::Event;
use quick_xml::Reader;

#[derive(Debug, Default, PartialEq)]
pub struct Manifest {
    pub subtitles: Vec<Track>,
    /// Media segments, for a media playlist: a segmented caption rendition is one of these.
    pub segments: Vec<String>,
    /// What the stream says protects it, when anything does.
    pub drm: Option<String>,
    /// Seek-bar thumbnails a DASH manifest describes whole: tiled pictures, one per segment.
    pub thumbs: Vec<Frame>,
    /// Image playlists an HLS master lists: each is read with [`hls_tiles`].
    pub image_playlists: Vec<String>,
}

/// Sheets described by one manifest, at most: an hour of video in ten-second pictures is 360.
const MAX_SHEETS: usize = 2000;

/// `5x4` as columns and rows.
fn layout(s: &str) -> Option<(u32, u32)> {
    let (c, r) = s.trim().split_once(['x', 'X'])?;
    let (c, r) = (c.trim().parse::<u32>().ok()?, r.trim().parse::<u32>().ok()?);
    (c > 0 && r > 0 && c * r <= 10_000).then_some((c, r))
}

/// The cells of one tiled picture that starts at `start` and covers `span` seconds.
fn tiles(sheet: &str, start: f64, span: f64, cols: u32, rows: u32, out: &mut Vec<Frame>) {
    let n = cols * rows;
    for i in 0..n {
        out.push(Frame {
            t: start + span * i as f64 / n as f64,
            sheet: sheet.to_string(),
            x: (i % cols) as f32 / cols as f32,
            y: (i / cols) as f32 / rows as f32,
            w: 1.0 / cols as f32,
            h: 1.0 / rows as f32,
        });
    }
}

/// An HLS image playlist: every segment is a picture, tiled as its `#EXT-X-TILES` says, or one
/// whole picture when nothing says. The cells of a tiled picture share its duration evenly
/// unless the tag gives each cell's own.
pub fn hls_tiles(body: &str, base: &str) -> Vec<Frame> {
    let mut out = Vec::new();
    let (mut at, mut span) = (0.0f64, None::<f64>);
    let mut grid = (1u32, 1u32);
    let mut each = None::<f64>;
    let mut sheets = 0usize;
    for line in body.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            span = rest.split(',').next().and_then(|d| d.trim().parse().ok());
        } else if let Some(list) = line.strip_prefix("#EXT-X-TILES:") {
            let a = attributes(list);
            grid = get(&a, "LAYOUT").and_then(layout).unwrap_or((1, 1));
            each = get(&a, "DURATION").and_then(|d| d.parse().ok());
        } else if !line.is_empty() && !line.starts_with('#') {
            let Some(d) = span.take().filter(|d| *d > 0.0) else {
                continue;
            };
            if sheets >= MAX_SHEETS {
                break;
            }
            sheets += 1;
            let covered = each.map_or(d, |e| e * (grid.0 * grid.1) as f64);
            tiles(&join(base, line), at, covered, grid.0, grid.1, &mut out);
            // Cells past the end of the segment are padding, not pictures.
            out.retain(|f| f.t < at + d);
            at += d;
        }
    }
    out
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
        } else if let Some(list) = line.strip_prefix("#EXT-X-IMAGE-STREAM-INF:") {
            if let Some(uri) = get(&attributes(list), "URI") {
                m.image_playlists.push(join(base, uri));
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
    let mut total: Option<f64> = None;
    // The thumbnail pictures of the adaptation set we are inside, as far as they are described.
    #[derive(Default)]
    struct Pictures {
        image: bool,
        media: Option<String>,
        seg: Option<f64>,
        first: u64,
        grid: Option<(u32, u32)>,
        id: Option<String>,
        bandwidth: Option<String>,
    }
    let mut pics = Pictures::default();
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
                    b"MPD" => {
                        total = attr(b"mediaPresentationDuration")
                            .as_deref()
                            .and_then(super::iso_duration);
                    }
                    b"AdaptationSet" => {
                        let text = attr(b"contentType").as_deref() == Some("text")
                            || attr(b"mimeType").is_some_and(|t| t.starts_with("text/"));
                        set = Some((attr(b"lang"), text));
                        pics = Pictures {
                            image: attr(b"contentType").as_deref() == Some("image")
                                || attr(b"mimeType").is_some_and(|t| t.starts_with("image/")),
                            first: 1,
                            ..Pictures::default()
                        };
                    }
                    b"Representation" if set.is_some() => {
                        pics.image |= attr(b"mimeType").is_some_and(|t| t.starts_with("image/"));
                        if pics.id.is_none() {
                            pics.id = attr(b"id");
                            pics.bandwidth = attr(b"bandwidth");
                        }
                    }
                    b"SegmentTemplate" if set.is_some() => {
                        let scale = attr(b"timescale")
                            .and_then(|t| t.parse::<f64>().ok())
                            .filter(|t| *t > 0.0)
                            .unwrap_or(1.0);
                        pics.seg = attr(b"duration")
                            .and_then(|d| d.parse::<f64>().ok())
                            .map(|d| d / scale)
                            .filter(|d| *d > 0.0);
                        pics.media = attr(b"media");
                        pics.first = attr(b"startNumber")
                            .and_then(|n| n.parse().ok())
                            .unwrap_or(1);
                    }
                    b"EssentialProperty" | b"SupplementalProperty" if set.is_some() => {
                        if attr(b"schemeIdUri").is_some_and(|s| s.contains("thumbnail_tile")) {
                            pics.grid = attr(b"value").as_deref().and_then(layout);
                        }
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
                b"AdaptationSet" => {
                    set = None;
                    let p = std::mem::take(&mut pics);
                    if let (true, Some(media), Some(seg), Some(total), true) =
                        (p.image, p.media, p.seg, total, m.thumbs.is_empty())
                    {
                        let (cols, rows) = p.grid.unwrap_or((1, 1));
                        let count = ((total / seg).ceil() as usize).clamp(1, MAX_SHEETS);
                        for i in 0..count {
                            let number = p.first + i as u64;
                            let name = media
                                .replace("$RepresentationID$", p.id.as_deref().unwrap_or(""))
                                .replace("$Bandwidth$", p.bandwidth.as_deref().unwrap_or(""))
                                .replace("$Number$", &number.to_string())
                                .replace("$Number%05d$", &format!("{number:05}"));
                            let start = i as f64 * seg;
                            tiles(&join(base, &name), start, seg, cols, rows, &mut m.thumbs);
                        }
                        m.thumbs.retain(|f| f.t < total);
                    }
                }
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
    fn a_dash_manifest_describes_its_seek_bar_thumbnails_whole() {
        let mpd = MPD
            .replace("<MPD ", "<MPD mediaPresentationDuration=\"PT1M30S\" ")
            .replace(
                " </Period>",
                r#"  <AdaptationSet mimeType="image/jpeg" contentType="image">
   <SegmentTemplate media="thumbs/$RepresentationID$/tile_$Number$.jpg" duration="60" startNumber="1"/>
   <Representation id="th" bandwidth="1200" width="800" height="270">
    <EssentialProperty schemeIdUri="http://dashif.org/guidelines/thumbnail_tile" value="5x3"/>
   </Representation>
  </AdaptationSet>
 </Period>"#,
            );
        let m = dash(&mpd, "https://cdn.example/v/1/manifest.mpd");
        // Two pictures of fifteen cells, four seconds a cell; the second is cut at 1:30.
        assert_eq!(m.thumbs.len(), 15 + 8, "{:?}", m.thumbs.last());
        assert_eq!(
            m.thumbs[0].sheet,
            "https://cdn.example/v/1/thumbs/th/tile_1.jpg"
        );
        let f = &m.thumbs[16];
        assert_eq!(f.sheet, "https://cdn.example/v/1/thumbs/th/tile_2.jpg");
        assert_eq!(f.t, 64.0);
        assert!((f.x - 0.2).abs() < 1e-6 && f.y == 0.0 && (f.w - 0.2).abs() < 1e-6);
        assert_eq!(m.subtitles.len(), 1, "and the captions are still read");
        assert!(dash(MPD, "https://x/").thumbs.is_empty());
    }

    #[test]
    fn an_hls_master_lists_its_image_playlist_and_the_playlist_its_tiles() {
        let master = format!(
            "{MASTER}#EXT-X-IMAGE-STREAM-INF:BANDWIDTH=9000,RESOLUTION=320x180,CODECS=\"jpeg\",URI=\"img/320.m3u8\"\n"
        );
        let m = hls(&master, "https://cdn.example/v/1/master.m3u8");
        assert_eq!(
            m.image_playlists,
            vec!["https://cdn.example/v/1/img/320.m3u8"]
        );
        let playlist = "#EXTM3U\n#EXT-X-TARGETDURATION:24\n#EXT-X-TILES:RESOLUTION=320x180,LAYOUT=3x2,DURATION=4.0\n#EXTINF:24.0,\nt-0.jpg\n#EXT-X-TILES:RESOLUTION=320x180,LAYOUT=3x2,DURATION=4.0\n#EXTINF:10.0,\nt-1.jpg\n#EXT-X-ENDLIST\n";
        let f = hls_tiles(playlist, "https://cdn.example/v/1/img/320.m3u8");
        assert_eq!(
            f.len(),
            6 + 3,
            "the last picture's cells past its ten seconds are padding"
        );
        assert_eq!(
            (f[4].t, f[4].sheet.as_str()),
            (16.0, "https://cdn.example/v/1/img/t-0.jpg")
        );
        assert!((f[4].x - 1.0 / 3.0).abs() < 1e-6 && f[4].y == 0.5);
        assert_eq!(f[6].t, 24.0);
        // A playlist of whole pictures, with no tiles tag: one cell each.
        let whole = hls_tiles("#EXTINF:5,\na.jpg\n#EXTINF:5,\nb.jpg\n", "https://x/p.m3u8");
        assert_eq!(whole.len(), 2);
        assert_eq!((whole[1].t, whole[1].w, whole[1].h), (5.0, 1.0, 1.0));
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
