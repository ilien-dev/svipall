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
