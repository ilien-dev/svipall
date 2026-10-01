//! What `web_fetch` does with a file that is not a page: audio and video are read for what they
//! say, an image is said to be one, and none of them comes back as bytes decoded as text.
//!
//! The speech model is never installed in a test's home, so what is checked here is the routing
//! and what the reader says when it cannot listen; the transcription itself is `asr`'s own tests.
mod support;
use support::{Reply, Site};
use svipall::{
    server::SvipallServer,
    tools::{WebFetchParams, WebVideoParams},
};

fn server() -> SvipallServer {
    support::isolate();
    SvipallServer::new(
        None,
        svipall_core::Config {
            request_min_interval_ms: 1,
            max_tier: "stealth".into(),
            ..Default::default()
        },
        None,
    )
}

fn params(url: String) -> WebFetchParams {
    WebFetchParams {
        url,
        cache: Some("bypass".into()),
        robots: Some("ignore".into()),
        ..Default::default()
    }
}

/// Bytes that are nothing like text: an MP3 frame header and a run of binary noise.
fn binary() -> Vec<u8> {
    let mut b = b"ID3\x04\x00\x00\x00\x00\x00\x00\xff\xfb\x90\x64".to_vec();
    b.extend((0..4000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8));
    b
}

fn tiers_tried(out: &serde_json::Value) -> usize {
    out["attempts"].as_array().map(|a| a.len()).unwrap_or(0)
}

#[tokio::test]
async fn an_audio_file_with_no_extension_is_read_as_audio_not_as_text() {
    let site = Site::start(vec![("/episode", Reply::bytes(&binary(), "audio/mpeg"))]).await;
    let out = server()
        .fetch_json(params(site.url("/episode?id=3")))
        .await
        .value;
    let content = out["content"].as_str().unwrap_or_default();
    assert!(
        !content.contains('\u{FFFD}') && !content.contains("ID3"),
        "no bytes decoded as text: {out}"
    );
    assert!(content.contains("svipall models install"), "{out}");
    assert_eq!(out["media"]["kind"], "audio", "{out}");
    assert_eq!(out["tier_used"], "http", "{out}");
    assert_eq!(tiers_tried(&out), 1, "a file is not climbed: {out}");
}

#[tokio::test]
async fn a_video_file_is_read_for_its_sound() {
    let site = Site::start(vec![("/clip", Reply::bytes(&binary(), "video/mp4"))]).await;
    let out = server().fetch_json(params(site.url("/clip"))).await.value;
    assert_eq!(out["media"]["kind"], "video", "{out}");
    assert!(out["content"]
        .as_str()
        .unwrap_or_default()
        .contains("svipall models install"));
}

#[tokio::test]
async fn an_image_says_it_is_an_image() {
    let site = Site::start(vec![("/scan", Reply::bytes(&binary(), "image/png"))]).await;
    let out = server().fetch_json(params(site.url("/scan"))).await.value;
    let content = out["content"].as_str().unwrap_or_default();
    assert_eq!(out["media"]["kind"], "image", "{out}");
    assert!(!content.contains('\u{FFFD}'), "{out}");
    assert!(content.contains("image"), "{out}");
    assert_eq!(tiers_tried(&out), 1, "{out}");
}

#[tokio::test]
async fn web_video_reads_an_audio_address_known_only_by_its_type() {
    let site = Site::start(vec![("/episode", Reply::bytes(&binary(), "audio/mpeg"))]).await;
    let out = server()
        .video_json(WebVideoParams {
            url: site.url("/episode?id=3"),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(
        out["content"]
            .as_str()
            .unwrap_or_default()
            .contains("svipall models install"),
        "{out}"
    );
}

#[tokio::test]
async fn a_local_audio_file_is_read_the_same_way() {
    let home = support::isolate();
    let dir = home.join("in");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("memo.mp3");
    std::fs::write(&path, binary()).unwrap();
    let url = url::Url::from_file_path(&path).unwrap().to_string();
    let out = server().fetch_json(params(url)).await.value;
    assert_eq!(out["media"]["kind"], "audio", "{out}");
    assert_eq!(out["media"]["title"], "memo.mp3", "{out}");
}

const PODCAST: &str = r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd" xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel><title>A show</title>
<item><title>Episode 12</title><link>https://pod.example/12</link>
<enclosure url="https://cdn.pod.example/12.mp3" length="39000000" type="audio/mpeg"/>
<itunes:duration>41:02</itunes:duration>
<podcast:transcript url="https://pod.example/12.vtt" type="text/vtt"/></item>
</channel></rss>"#;

#[tokio::test]
async fn a_mapped_podcast_feed_keeps_each_episodes_recording_and_transcript() {
    let mut reply = Reply::plain(PODCAST);
    reply.content_type = "application/rss+xml".into();
    let site = Site::start(vec![("/feed.xml", reply)]).await;
    let out = server()
        .map_json(svipall::tools::WebMapParams {
            url: site.url("/"),
            sources: Some(vec!["feeds".into()]),
            limit: None,
            include: None,
        })
        .await
        .unwrap();
    let row = out["urls"]
        .as_array()
        .and_then(|u| u.iter().find(|r| r["url"] == "https://pod.example/12"))
        .unwrap_or_else(|| panic!("the episode is mapped: {out}"))
        .clone();
    assert_eq!(
        row["enclosure"]["url"], "https://cdn.pod.example/12.mp3",
        "{row}"
    );
    assert_eq!(row["enclosure"]["duration"], "41:02", "{row}");
    assert_eq!(row["transcript"], "https://pod.example/12.vtt", "{row}");
}

#[tokio::test]
async fn a_published_transcript_is_read_before_any_recognition_is_tried() {
    let page = r#"<html><head><title>Ep</title><script type="application/ld+json">
        {"@context":"https://schema.org","@type":"PodcastEpisode","name":"Episode 12",
         "transcript":"Welcome to episode twelve, where we talk about queues.",
         "associatedMedia":{"@type":"MediaObject","contentUrl":"/ep12.mp3"}}</script>
        </head><body><h1>Episode 12</h1></body></html>"#;
    let site = Site::start(vec![
        ("/e/12", Reply::html(page)),
        ("/ep12.mp3", Reply::bytes(&binary(), "audio/mpeg")),
    ])
    .await;
    let out = server()
        .video_json(WebVideoParams {
            url: site.url("/e/12"),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(
        out["content"]
            .as_str()
            .unwrap_or_default()
            .contains("episode twelve"),
        "{out}"
    );
    assert!(
        !out.to_string().contains("speech model"),
        "no recognition was needed: {out}"
    );
    assert_eq!(
        site.hits("/ep12.mp3"),
        0,
        "the recording was not downloaded"
    );
}

const SCANNED_PDF: &[u8] = include_bytes!("../../svipall-core/fixtures/pdf/scanned.pdf");

#[tokio::test]
async fn a_scanned_pdf_is_labelled_as_one_not_passed_off_as_empty() {
    let site = Site::start(vec![(
        "/invoice.pdf",
        Reply::bytes(SCANNED_PDF, "application/pdf"),
    )])
    .await;
    let out = server()
        .fetch_json(params(site.url("/invoice.pdf")))
        .await
        .value;
    assert_eq!(out["document"]["scanned"], true, "{out}");
    assert_eq!(out["document"]["pages"], 1, "{out}");
    assert!(
        out["content"]
            .as_str()
            .unwrap_or_default()
            .contains("no text layer"),
        "{out}"
    );
    assert_eq!(
        tiers_tried(&out),
        1,
        "a browser would not read it either: {out}"
    );
}
