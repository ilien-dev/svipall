//! `web_video` end to end against a local site: a page, its caption files, its manifests.
//!
//! Everything a player page does that needs a browser (a borrowed caption request) is covered by
//! the unit tests and by the ignored network test at the bottom; what runs here is the path every
//! plain video page takes, and it never leaves loopback.

mod support;

use serde_json::Value;
use support::{Reply, Site};
use svipall::server::SvipallServer;
use svipall::tools::WebVideoParams;

fn server() -> SvipallServer {
    support::isolate();
    SvipallServer::with_store(
        None,
        svipall_core::Config::default(),
        None,
        svipall_core::cache::Store::open_memory()
            .ok()
            .map(std::sync::Arc::new),
    )
}

fn typed(body: &str, content_type: &str) -> Reply {
    let mut r = Reply::plain(body);
    r.content_type = content_type.into();
    r
}

const VTT_EN: &str = "WEBVTT\n\n00:00:05.000 --> 00:00:08.000\nHello and welcome.\n\n00:00:40.000 --> 00:00:44.000\nNow the results.\n";

async fn video(url: String) -> Value {
    server()
        .video_json(WebVideoParams {
            url,
            ..Default::default()
        })
        .await
        .expect("web_video")
}

#[tokio::test]
async fn a_plain_page_with_a_track_becomes_a_timeline() {
    let site = Site::start(vec![
        (
            "/talk",
            Reply::html(
                "<html><head><title>Talk</title></head><body><h1>Talk</h1>\
                 <video controls><source src=\"/talk.mp4\" type=\"video/mp4\">\
                 <track kind=\"subtitles\" srclang=\"en\" label=\"English\" src=\"/talk-en.vtt\">\
                 <track kind=\"chapters\" src=\"/talk-ch.vtt\"></video></body></html>",
            ),
        ),
        ("/talk-en.vtt", typed(VTT_EN, "text/vtt")),
        (
            "/talk-ch.vtt",
            typed(
                "WEBVTT\n\n00:00.000 --> 00:30.000\nWelcome\n\n00:30.000 --> 01:00.000\nResults\n",
                "text/vtt",
            ),
        ),
    ])
    .await;
    let v = video(site.url("/talk")).await;
    let content = v["content"].as_str().unwrap_or_default();
    assert_eq!(v["source"], "html5", "{v}");
    assert_eq!(v["captions"]["kind"], "manual");
    assert_eq!(v["captions"]["lang"], "en");
    assert_eq!(v["captions"]["cues"], 2);
    assert!(
        content.contains("## Welcome\n\n[0:05] Hello and welcome."),
        "{content}"
    );
    assert!(
        content.contains("## Results\n\n[0:40] Now the results."),
        "{content}"
    );
    assert_eq!(v["chapters"].as_array().map(Vec::len), Some(2));
}

#[tokio::test]
async fn captions_found_only_in_the_stream_manifest_are_read_segment_by_segment() {
    let site = Site::start(vec![
        (
            "/v",
            Reply::html(
                "<html><head><title>Clip</title><script type=\"application/ld+json\">\
                 {\"@type\":\"VideoObject\",\"name\":\"Clip\",\"duration\":\"PT1M\",\
                 \"contentUrl\":\"/hls/master.m3u8\"}</script></head><body><p>A clip.</p></body></html>",
            ),
        ),
        (
            "/hls/master.m3u8",
            typed(
                "#EXTM3U\n#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"s\",LANGUAGE=\"es\",NAME=\"Español\",URI=\"subs/es.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=1,SUBTITLES=\"s\"\nv.m3u8\n",
                "application/vnd.apple.mpegurl",
            ),
        ),
        (
            "/hls/subs/es.m3u8",
            typed(
                "#EXTM3U\n#EXTINF:30,\nes-0.vtt\n#EXTINF:30,\nes-1.vtt\n#EXT-X-ENDLIST\n",
                "application/vnd.apple.mpegurl",
            ),
        ),
        (
            "/hls/subs/es-0.vtt",
            typed("WEBVTT\n\n00:00:01.000 --> 00:00:03.000\nHola.\n", "text/vtt"),
        ),
        (
            "/hls/subs/es-1.vtt",
            typed("WEBVTT\n\n00:00:31.000 --> 00:00:33.000\nAdiós.\n", "text/vtt"),
        ),
    ])
    .await;
    let v = video(site.url("/v")).await;
    let content = v["content"].as_str().unwrap_or_default();
    assert_eq!(v["source"], "json-ld", "{v}");
    assert_eq!(v["duration"], 60.0);
    assert_eq!(v["captions"]["lang"], "es");
    assert!(
        content.contains("Hola.") && content.contains("Adiós."),
        "{content}"
    );
    assert_eq!(site.hits("/hls/subs/es-1.vtt"), 1);
}

#[tokio::test]
async fn an_encrypted_stream_is_reported_and_its_media_never_requested() {
    let site = Site::start(vec![
        (
            "/locked",
            Reply::html(
                "<html><body><video src=\"/locked/master.m3u8\"></video><p>Members only film.</p></body></html>",
            ),
        ),
        (
            "/locked/master.m3u8",
            typed(
                "#EXTM3U\n#EXT-X-SESSION-KEY:METHOD=SAMPLE-AES,URI=\"skd://key\",KEYFORMAT=\"com.apple.streamingkeydelivery\"\n#EXT-X-STREAM-INF:BANDWIDTH=1\nv.m3u8\n",
                "application/vnd.apple.mpegurl",
            ),
        ),
    ])
    .await;
    let v = video(site.url("/locked")).await;
    assert_eq!(v["drm"], "SAMPLE-AES com.apple.streamingkeydelivery", "{v}");
    assert_eq!(
        site.hits("/locked/v.m3u8"),
        0,
        "the media playlist is never read"
    );
    let notes = v["notes"].to_string();
    assert!(notes.contains("encrypted"), "{notes}");
}

#[tokio::test]
async fn a_page_without_a_video_says_so_and_is_fetched_once() {
    let site = Site::start(vec![("/", Reply::page("Plain", &[]))]).await;
    let v = video(site.url("/")).await;
    assert!(v["video"].is_null() && v.get("video").is_some(), "{v}");
    assert!(v["note"].as_str().unwrap_or_default().contains("web_fetch"));
    assert_eq!(site.hits("/"), 1, "a delivered page is not climbed");
}

#[tokio::test]
async fn a_caption_file_that_is_not_captions_is_named_not_swallowed() {
    let site = Site::start(vec![
        (
            "/p",
            Reply::html(
                "<html><body><video><track src=\"/c.vtt\" srclang=\"en\"></video></body></html>",
            ),
        ),
        (
            "/c.vtt",
            Reply::html("<html><body>Sign in to see captions</body></html>"),
        ),
    ])
    .await;
    let v = video(site.url("/p")).await;
    let notes = v["notes"].to_string();
    assert!(notes.contains("not WebVTT"), "{v}");
    assert_eq!(v["captions"]["cues"], 0);
}

#[tokio::test]
async fn a_video_with_no_captions_names_the_speech_model_that_would_hear_it() {
    // The test home has no models, so the one thing this can say is what would change that.
    let site = Site::start(vec![(
        "/film",
        Reply::html("<html><body><h1>Film</h1><video src=\"/film.mp4\"></video></body></html>"),
    )])
    .await;
    let v = video(site.url("/film")).await;
    let notes = v["notes"].to_string();
    assert!(notes.contains("svipall models install"), "{v}");
    assert_eq!(
        site.hits("/film.mp4"),
        0,
        "nothing is downloaded that cannot be listened to"
    );
}

/// A real watch page: its captions are only answered for the player's own request, so this needs
/// a browser and the network. Run by hand.
#[tokio::test]
#[ignore = "network + browser"]
async fn a_real_watch_page_has_its_captions_borrowed() {
    let v = video("https://www.youtube.com/watch?v=dQw4w9WgXcQ".into()).await;
    assert_eq!(v["source"], "youtube.com/watch", "{v}");
    assert!(v["captions"]["cues"].as_u64().unwrap_or(0) > 10, "{v}");
    assert!(
        v["content"].as_str().unwrap_or_default().contains("[0:"),
        "{v}"
    );
}

/// A sprite of eight thumbnails, four across, each its own colour, as a player's seek bar uses.
fn sprite() -> Vec<u8> {
    let img = image::RgbImage::from_fn(640, 180, |x, y| {
        let cell = (x / 160) + 4 * (y / 90);
        image::Rgb([
            (cell * 30) as u8,
            200 - (cell * 20) as u8,
            ((x % 160) + (y % 90)) as u8,
        ])
    });
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .expect("png");
    out
}

/// A page whose player publishes its seek-bar thumbnails as a WebVTT metadata track, and captions.
async fn site_with_thumbnails() -> Site {
    let mut thumbs = String::from("WEBVTT\n\n");
    for i in 0..8 {
        thumbs.push_str(&format!(
            "00:{:02}:{:02}.000 --> 00:{:02}:{:02}.000\nsprite.png#xywh={},{},160,90\n\n",
            (i * 10) / 60,
            (i * 10) % 60,
            ((i + 1) * 10) / 60,
            ((i + 1) * 10) % 60,
            (i % 4) * 160,
            (i / 4) * 90
        ));
    }
    Site::start(vec![
        (
            "/talk",
            Reply::html(
                "<html><head><title>Seen talk</title><script type=\"application/ld+json\">\
                 {\"@type\":\"VideoObject\",\"name\":\"Seen talk\",\"duration\":\"PT80S\"}</script>\
                 </head><body><h1>Seen talk</h1><video controls>\
                 <source src=\"/talk.mp4\" type=\"video/mp4\">\
                 <track kind=\"subtitles\" srclang=\"en\" src=\"/talk-en.vtt\">\
                 <track kind=\"metadata\" src=\"/media/thumbs.vtt\"></video></body></html>",
            ),
        ),
        ("/talk-en.vtt", typed(VTT_EN, "text/vtt")),
        ("/media/thumbs.vtt", typed(&thumbs, "text/vtt")),
        ("/media/sprite.png", Reply::bytes(&sprite(), "image/png")),
    ])
    .await
}

#[tokio::test]
async fn a_plain_call_shows_the_whole_video_on_a_sheet_of_its_own_thumbnails() {
    let site = site_with_thumbnails().await;
    let v = video(site.url("/talk")).await;
    assert_eq!(v["seen"]["source"], "thumbnails", "{v}");
    assert_eq!(
        v["seen"]["moments"], 8,
        "every thumbnail there is, each once: {v}"
    );
    assert_eq!(
        (v["seen"]["from"].as_f64(), v["seen"]["to"].as_f64()),
        (Some(0.0), Some(80.0))
    );
    let sheets = v["sheets"].as_array().cloned().unwrap_or_default();
    assert_eq!(sheets.len(), 1, "{v}");
    let s = &sheets[0];
    assert_eq!(
        (s["cells"].as_u64(), s["grid"].as_str()),
        (Some(8), Some("3x3"))
    );
    let path = s["path"].as_str().unwrap();
    let img = image::open(path).expect("sheet written").to_rgb8();
    assert_eq!(img.dimensions(), (768, 432));
    assert_eq!(s["tokens"], 448, "what the sheet costs is said");
    // The second cell is the second thumbnail, with its time written over its corner.
    let p = img.get_pixel(256 + 200, 100).0;
    assert!(p[0] < 60 && p[1] > 150, "cell 1 is thumbnail 1: {p:?}");
    let corner = img.get_pixel(256 + 1, 1).0;
    assert!(
        corner.iter().all(|c| *c < 40),
        "the label's box: {corner:?}"
    );
    let content = v["content"].as_str().unwrap_or_default();
    assert!(
        content.contains("Pictures: 8 moments from 0:00 to 1:20"),
        "{content}"
    );
    assert!(
        content.contains(path),
        "the sheet is named where the model reads: {content}"
    );
    assert!(
        content.contains("[0:05] Hello and welcome."),
        "and the words are still there"
    );
    assert!(v.get("frames").is_none(), "nothing was captured: {v}");
}

#[tokio::test]
async fn frames_zero_turns_the_picture_off() {
    let site = site_with_thumbnails().await;
    let v = server()
        .video_json(WebVideoParams {
            url: site.url("/talk"),
            frames: Some(0),
            ..Default::default()
        })
        .await
        .expect("web_video");
    assert!(v.get("sheets").is_none() && v.get("seen").is_none(), "{v}");
    let content = v["content"].as_str().unwrap_or_default();
    assert!(
        !content.contains("Pictures:") && content.contains("[0:05]"),
        "{content}"
    );
}

#[tokio::test]
async fn a_search_finds_the_moment_something_is_said_and_returns_it_instead_of_the_timeline() {
    let site = site_with_thumbnails().await;
    let v = server()
        .video_json(WebVideoParams {
            url: site.url("/talk"),
            query: Some("the RESULTS".into()),
            ..Default::default()
        })
        .await
        .expect("web_video");
    let hits = v["matches"].as_array().cloned().unwrap_or_default();
    assert_eq!(hits.len(), 1, "{v}");
    assert_eq!(
        (hits[0]["by"].as_str(), hits[0]["t"].as_f64()),
        (Some("said"), Some(40.0))
    );
    let content = v["content"].as_str().unwrap_or_default();
    assert!(
        content.contains("[0:40] said: Now the results."),
        "{content}"
    );
    assert!(!content.contains("Hello and welcome"), "{content}");
    // Without the image-text model the answer says which searches it ran.
    if !svipall::zeroshot::available() {
        assert!(v["notes"].to_string().contains("image-text model"), "{v}");
    }
}

#[tokio::test]
async fn an_argument_that_cannot_be_read_stops_the_call_before_any_request() {
    // No site at all: nothing may be fetched for a call that cannot be understood.
    for (start, crop, want) in [
        (Some("later"), None, "start=\"later\""),
        (None, Some("10,10,200,200"), "fractions"),
    ] {
        let v = server()
            .video_json(WebVideoParams {
                url: "http://127.0.0.1:9/never".into(),
                start: start.map(str::to_string),
                crop: crop.map(str::to_string),
                ..Default::default()
            })
            .await
            .expect("web_video");
        assert!(
            v["error"].as_str().unwrap_or_default().contains(want),
            "{v}"
        );
    }
}

fn png(img: image::RgbImage) -> Vec<u8> {
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .expect("png");
    out
}

/// A slide shown for one second of a thirty-second talk falls between two evenly spread moments.
/// The thumbnails show it was held there, so it is on the sheet all the same.
#[tokio::test]
async fn a_slide_shown_between_two_spread_moments_is_on_the_sheet_too() {
    // Sixty thumbnails, one every half second, ten across: slide A, then B for one second, then C.
    let shade = |cell: u32| match cell {
        0..=17 => 20u8,
        18..=19 => 130,
        _ => 240,
    };
    let sprite = image::RgbImage::from_fn(640, 216, |x, y| {
        let v = shade((x / 64) + 10 * (y / 36));
        image::Rgb([v, v, v])
    });
    let mut thumbs = String::from("WEBVTT\n\n");
    for i in 0..60u32 {
        let (a, b) = (i * 500, (i + 1) * 500);
        thumbs.push_str(&format!(
            "00:00:{:02}.{:03} --> 00:00:{:02}.{:03}\nsprite.png#xywh={},{},64,36\n\n",
            a / 1000,
            a % 1000,
            b / 1000,
            b % 1000,
            (i % 10) * 64,
            (i / 10) * 36
        ));
    }
    let site = Site::start(vec![
        (
            "/deck",
            Reply::html(
                "<html><head><title>Deck</title><script type=\"application/ld+json\">\
                 {\"@type\":\"VideoObject\",\"name\":\"Deck\",\"duration\":\"PT30S\"}</script>\
                 </head><body><h1>Deck</h1><video controls>\
                 <source src=\"/deck.mp4\" type=\"video/mp4\">\
                 <track kind=\"subtitles\" srclang=\"en\" src=\"/deck-en.vtt\">\
                 <track kind=\"metadata\" src=\"/thumbs.vtt\"></video></body></html>",
            ),
        ),
        ("/deck-en.vtt", typed(VTT_EN, "text/vtt")),
        ("/thumbs.vtt", typed(&thumbs, "text/vtt")),
        ("/sprite.png", Reply::bytes(&png(sprite), "image/png")),
    ])
    .await;
    let v = video(site.url("/deck")).await;
    assert_eq!(v["seen"]["held"], 1, "{v}");
    assert_eq!(
        v["seen"]["moments"], 17,
        "sixteen spread, and the slide between two of them"
    );
    let content = v["content"].as_str().unwrap_or_default();
    assert!(
        content.contains("1 of them added where a picture was held between two of the others"),
        "{content}"
    );
    // The slide is a cell of the second sheet: sixteen cells on the first, it is the ninth moment.
    let sheets = v["sheets"].as_array().cloned().unwrap_or_default();
    let img = image::open(sheets[0]["path"].as_str().unwrap())
        .unwrap()
        .to_rgb8();
    let (cw, ch) = (img.width() / 4, img.height() / 4);
    let grey = (0..16u32)
        .filter(|i| {
            let p = img
                .get_pixel((i % 4) * cw + cw / 2, (i / 4) * ch + ch / 2)
                .0;
            (100..160).contains(&p[0])
        })
        .count();
    assert_eq!(
        grey, 1,
        "the one-second slide is one cell of the sheet: {v}"
    );
}

/// A stream whose DASH manifest publishes tiled thumbnails: the sheet is drawn from them, with no
/// page markup and no player naming them.
#[tokio::test]
async fn thumbnails_a_dash_manifest_publishes_are_a_source_of_the_sheet() {
    let tile = |seed: u8| {
        png(image::RgbImage::from_fn(320, 180, |x, y| {
            let cell = ((x / 160) + 2 * (y / 90)) as u8;
            image::Rgb([seed, cell * 60, ((x + y) % 200) as u8])
        }))
    };
    let mpd = r#"<?xml version="1.0"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" mediaPresentationDuration="PT1M">
 <Period>
  <AdaptationSet contentType="video" mimeType="video/mp4">
   <Representation id="v1"><BaseURL>video.mp4</BaseURL></Representation>
  </AdaptationSet>
  <AdaptationSet contentType="image" mimeType="image/png">
   <SegmentTemplate media="tiles/$RepresentationID$_$Number$.png" duration="30" startNumber="1"/>
   <Representation id="th" bandwidth="900" width="320" height="180">
    <EssentialProperty schemeIdUri="http://dashif.org/guidelines/thumbnail_tile" value="2x2"/>
   </Representation>
  </AdaptationSet>
 </Period>
</MPD>"#;
    let site = Site::start(vec![
        (
            "/v",
            Reply::html(
                "<html><head><title>Stream</title><script type=\"application/ld+json\">\
                 {\"@type\":\"VideoObject\",\"name\":\"Stream\",\"duration\":\"PT1M\",\
                 \"contentUrl\":\"/dash/manifest.mpd\"}</script></head>\
                 <body><p>A stream, and enough words to be a page.</p></body></html>",
            ),
        ),
        ("/dash/manifest.mpd", typed(mpd, "application/dash+xml")),
        ("/dash/tiles/th_1.png", Reply::bytes(&tile(10), "image/png")),
        (
            "/dash/tiles/th_2.png",
            Reply::bytes(&tile(200), "image/png"),
        ),
    ])
    .await;
    let v = video(site.url("/v")).await;
    assert_eq!(v["seen"]["source"], "thumbnails", "{v}");
    assert_eq!(v["seen"]["moments"], 8, "two pictures of four cells: {v}");
    let s = &v["sheets"][0];
    assert_eq!(
        (s["cells"].as_u64(), s["grid"].as_str()),
        (Some(8), Some("3x3"))
    );
    assert!(std::path::Path::new(s["path"].as_str().unwrap()).is_file());
}

/// Looking closer at a part of a video, from the player itself: frames spread over the part and
/// on a sheet, an exact moment, a part of the frame, and the second call reusing the first's
/// frames. Needs a browser. Run by hand.
#[tokio::test]
#[ignore = "browser"]
async fn a_part_of_the_video_is_captured_spread_and_not_captured_twice() {
    let site = Site::start(vec![
        (
            "/talk",
            Reply::html(
                r#"<!doctype html><html><head><title>Local talk</title>
<script type="application/ld+json">{"@context":"https://schema.org","@type":"VideoObject",
"name":"Local talk","duration":"PT20S","contentUrl":"/main.webm"}</script></head>
<body style="margin:0"><video src="/main.webm" muted autoplay playsinline
 style="width:640px;height:360px"></video>
<p>A talk about things, long enough to be a page and not a stub.</p></body></html>"#,
            ),
        ),
        (
            "/main.webm",
            Reply::bytes(include_bytes!("fixtures/video/main.webm"), "video/webm"),
        ),
    ])
    .await;
    let s = server();
    let ask = |p: WebVideoParams| {
        let s = &s;
        async move { s.video_json(p).await.expect("web_video") }
    };
    let started = std::time::Instant::now();
    let v = ask(WebVideoParams {
        url: site.url("/talk"),
        start: Some("4".into()),
        end: Some("0:16".into()),
        frames: Some(6),
        ..Default::default()
    })
    .await;
    eprintln!("first look: {:?}\n{}", started.elapsed(), v["content"]);
    assert_eq!(v["seen"]["source"], "player", "{v}");
    let frames = v["frames"].as_array().cloned().unwrap_or_default();
    assert!(frames.len() >= 4, "{v}");
    for f in &frames {
        let t = f["t"].as_f64().unwrap();
        assert!((4.0..16.0).contains(&t), "inside the part asked for: {t}");
        assert!(f["tokens"].as_u64().unwrap_or(0) > 0, "{v}");
        assert!(std::path::Path::new(f["path"].as_str().unwrap()).is_file());
    }
    assert!(
        v.get("sheets").is_none(),
        "frames this small cost less than a sheet of them would: {v}"
    );
    assert!(v["content"]
        .as_str()
        .unwrap()
        .contains("captured from the video"));

    // The same look again opens no browser: the frames are the first call's.
    let again = std::time::Instant::now();
    let v2 = ask(WebVideoParams {
        url: site.url("/talk"),
        start: Some("4".into()),
        end: Some("0:16".into()),
        frames: Some(6),
        ..Default::default()
    })
    .await;
    eprintln!("second look: {:?}", again.elapsed());
    assert_eq!(
        v2["seen"]["reused"].as_u64(),
        Some(frames.len() as u64),
        "{v2}"
    );
    assert_eq!(
        v2["frames"],
        v["frames"]
            .clone()
            .as_array()
            .map(|a| {
                serde_json::Value::Array(
                    a.iter()
                        .map(|f| {
                            let mut f = f.clone();
                            f.as_object_mut().unwrap().remove("cut");
                            f
                        })
                        .collect(),
                )
            })
            .unwrap(),
        "{v2}"
    );

    // One exact moment, and a quarter of its frame.
    let v3 = ask(WebVideoParams {
        url: site.url("/talk"),
        at: vec!["0:10".into()],
        crop: Some("0.5,0.5,0.5,0.5".into()),
        ..Default::default()
    })
    .await;
    let one = v3["frames"].as_array().cloned().unwrap_or_default();
    assert_eq!(one.len(), 1, "{v3}");
    assert!((one[0]["t"].as_f64().unwrap() - 10.0).abs() < 0.5, "{v3}");
    let part = image::open(one[0]["path"].as_str().unwrap()).unwrap();
    assert_eq!(
        (part.width(), part.height()),
        (80, 45),
        "a quarter of 160x90"
    );
    assert!(v3.get("sheets").is_none(), "one frame needs no sheet: {v3}");
}

/// A media file read directly: no page, no captions, frames captured from the browser's own
/// player where the picture changes. Run by hand.
#[tokio::test]
#[ignore = "network + browser"]
async fn a_media_file_gets_frames_where_the_picture_changes() {
    let v = server()
        .video_json(WebVideoParams {
            url: "https://interactive-examples.mdn.mozilla.net/media/cc0-videos/flower.webm".into(),
            frames: Some(3),
            ..Default::default()
        })
        .await
        .expect("web_video");
    assert_eq!(v["source"], "file", "{v}");
    let frames = v["frames"].as_array().cloned().unwrap_or_default();
    assert_eq!(frames.len(), 3, "{v}");
    for f in &frames {
        let path = f["path"].as_str().unwrap();
        let png = std::fs::read(path).expect("frame written");
        assert!(png.starts_with(b"\x89PNG"), "{path}");
    }
    assert!(
        v["content"]
            .as_str()
            .unwrap_or_default()
            .contains("(frame: "),
        "{v}"
    );
}

/// A player the way a big platform builds one, served from loopback: an advert on screen first,
/// the video preloaded above the page and parked there until something starts it, and controls
/// drawn over the picture. Frames come from the video, after the advert, without the controls.
/// Needs a browser. Run by hand.
#[tokio::test]
#[ignore = "browser"]
async fn frames_wait_out_the_advert_and_come_from_the_video_itself() {
    let main = include_bytes!("fixtures/video/main.webm");
    let ad = include_bytes!("fixtures/video/ad.webm");
    let page = r#"<!doctype html><html><head><title>Local talk</title>
<script type="application/ld+json">{"@context":"https://schema.org","@type":"VideoObject",
"name":"Local talk","duration":"PT20S","contentUrl":"/main.webm"}</script></head>
<body style="margin:0"><div style="position:relative;width:660px;height:371px">
<video id="ad" src="/ad.webm" muted autoplay playsinline
 style="position:absolute;left:0;top:0;width:640px;height:360px"></video>
<video id="main" src="/main.webm" preload="auto" muted playsinline
 style="position:absolute;left:0;top:-1050px;width:660px;height:371px"></video>
<div style="position:absolute;left:0;bottom:0;width:660px;height:60px;background:#f0f">controls</div>
</div><p>A talk about things, long enough to be a page and not a stub.</p>
<script>
const ad = document.getElementById('ad'), main = document.getElementById('main');
ad.addEventListener('ended', () => ad.remove());
main.addEventListener('play', () => { main.style.top = '0px'; });
</script></body></html>"#;
    let site = Site::start(vec![
        ("/talk", Reply::html(page)),
        ("/main.webm", Reply::bytes(main, "video/webm")),
        ("/ad.webm", Reply::bytes(ad, "video/webm")),
    ])
    .await;
    let v = server()
        .video_json(WebVideoParams {
            url: site.url("/talk"),
            frames: Some(4),
            ..Default::default()
        })
        .await
        .expect("web_video");
    let frames = v["frames"].as_array().cloned().unwrap_or_default();
    assert!(frames.len() >= 3, "{v}");
    let notes = v["notes"].to_string();
    assert!(notes.contains("advert played first"), "{v}");
    assert!(!notes.contains("blank"), "{v}");
    let mut seen = Vec::new();
    for f in &frames {
        let path = f["path"].as_str().unwrap();
        let img = image::open(path).expect("frame written").to_rgb8();
        assert_eq!(
            img.dimensions(),
            (160, 90),
            "the video's own picture, not a clip of the page with its controls: {path}"
        );
        assert!(!svipall::video_frames::blank(&img), "{path}");
        seen.push(svipall::video_frames::histogram(&img));
    }
    for (i, a) in seen.iter().enumerate() {
        for b in &seen[i + 1..] {
            assert!(svipall::video_frames::distance(a, b) > 0.002, "{v}");
        }
    }
}

/// A watch page read twice by one server: the second visit is the one that gets adverts, and it
/// used to come back with no captions and every frame blank. Network + browser; run by hand.
#[tokio::test]
#[ignore = "network + browser"]
async fn a_real_watch_page_gives_captions_and_frames_on_every_visit() {
    let s = server();
    for visit in 1..=2 {
        let v = s
            .video_json(WebVideoParams {
                url: "https://www.youtube.com/watch?v=ejjBbaq9RmY".into(),
                frames: Some(6),
                ..Default::default()
            })
            .await
            .expect("web_video");
        eprintln!("visit {visit}: {}", v["notes"]);
        assert!(v["captions"]["cues"].as_u64().unwrap_or(0) > 10, "{v}");
        let frames = v["frames"].as_array().cloned().unwrap_or_default();
        assert!(frames.len() >= 5, "{v}");
        for f in &frames {
            let path = f["path"].as_str().unwrap();
            let img = image::open(path).expect("frame written").to_rgb8();
            assert!(!svipall::video_frames::blank(&img), "{path}");
            assert!(img.width() >= 640, "{path}: {:?}", img.dimensions());
        }
    }
}

/// A video served from another origin without CORS: its picture cannot be read from the element,
/// so the frame is a screenshot of where it sits, and the answer says so. Needs a browser.
#[tokio::test]
#[ignore = "browser"]
async fn a_cross_origin_video_falls_back_to_a_screenshot_and_says_so() {
    let media = Site::start(vec![(
        "/main.webm",
        Reply::bytes(include_bytes!("fixtures/video/main.webm"), "video/webm"),
    )])
    .await;
    // Same loopback, another origin: `localhost` is not `127.0.0.1`.
    let src = format!("http://localhost:{}/main.webm", media.port);
    let page = format!(
        r#"<!doctype html><html><head><title>Elsewhere</title></head><body style="margin:0">
<video src="{src}" muted autoplay playsinline style="width:320px;height:180px"></video>
<p>A page whose video lives on another origin, and says enough to be a page.</p></body></html>"#
    );
    let site = Site::start(vec![("/p", Reply::html(&page))]).await;
    let v = server()
        .video_json(WebVideoParams {
            url: site.url("/p"),
            frames: Some(3),
            ..Default::default()
        })
        .await
        .expect("web_video");
    let frames = v["frames"].as_array().cloned().unwrap_or_default();
    assert!(!frames.is_empty(), "{v}");
    assert!(v["notes"].to_string().contains("screenshots"), "{v}");
    for f in &frames {
        let img = image::open(f["path"].as_str().unwrap()).unwrap().to_rgb8();
        assert!(!svipall::video_frames::blank(&img), "{v}");
    }
}
