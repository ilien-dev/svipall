//! The conformance test that makes "a new player is a new row" true.
//!
//! Every row in `svipall_core::video::sources::VIDEO_SOURCES` has a fixture beside it: a player page
//! as it arrives, trimmed. Each is read from its own page, embeds of it are followed, and what comes
//! back is enough to fetch captions from.

use svipall_core::video::{self, sources::VIDEO_SOURCES, CaptionKind};
use svipall_core::{parse_page, ParseWants};

fn fixture_name(id: &str) -> String {
    id.replace(['.', '/'], "-") + ".html"
}

fn fixture(id: &str) -> Option<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/video")
        .join(fixture_name(id));
    std::fs::read_to_string(path).ok()
}

fn read(url: &str, html: &str) -> Option<video::VideoInfo> {
    let parts = parse_page(
        html,
        &ParseWants {
            metadata: true,
            metadata_base_url: Some(url.into()),
            media: Some(url.into()),
            ..Default::default()
        },
    );
    video::discover(url, html, parts.media.as_ref(), parts.metadata.as_ref())
}

#[test]
fn every_player_in_the_table_has_a_page_to_read() {
    let missing: Vec<&str> = VIDEO_SOURCES
        .iter()
        .filter(|s| fixture(s.id).is_none())
        .map(|s| s.id)
        .collect();
    assert!(missing.is_empty(), "no fixture for {missing:?}");
}

#[test]
fn every_player_is_read_from_its_own_page() {
    for s in VIDEO_SOURCES {
        let html = fixture(s.id).expect("fixture");
        let url = format!("https://{}/1", s.id);
        let info = read(&url, &html).unwrap_or_else(|| panic!("{} read nothing", s.id));
        assert_eq!(info.source, s.id);
        assert!(info.title.is_some(), "{}: no title", s.id);
        assert!(
            info.duration.is_some_and(|d| d > 0.0),
            "{}: no duration",
            s.id
        );
        assert!(!info.tracks.is_empty(), "{}: no caption tracks", s.id);
        assert!(
            info.tracks.iter().all(|t| t.url.starts_with("https://")),
            "{}: a track address the server cannot fetch",
            s.id
        );
    }
}

#[test]
fn every_player_is_followed_from_a_page_that_embeds_it() {
    for s in VIDEO_SOURCES {
        let frame = format!("https://{}123", s.embeds[0]);
        let html = format!("<html><body><iframe src=\"{frame}\"></iframe></body></html>");
        let parts = parse_page(
            &html,
            &ParseWants {
                media: Some("https://blog.example/post".into()),
                ..Default::default()
            },
        );
        let page = video::embedded_player(parts.media.as_ref(), None)
            .unwrap_or_else(|| panic!("{}: embed not recognised", s.id));
        let page = url::Url::parse(&page).unwrap();
        assert_eq!(
            video::sources::for_url(&page).map(|r| r.id),
            Some(s.id),
            "{}: the followed page is not this player's",
            s.id
        );
    }
}

#[test]
fn a_watch_page_says_which_tracks_are_machine_made_and_where_the_chapters_are() {
    let html = fixture("youtube.com/watch").unwrap();
    let info = read("https://www.youtube.com/watch?v=dQw4w9WgXcQ", &html).unwrap();
    assert!(info.tracks.iter().any(|t| t.kind == CaptionKind::Auto));
    assert!(info.tracks.iter().any(|t| t.kind == CaptionKind::Manual));
    assert_eq!(info.chapters.len(), 3, "from the description");
    assert!(!info.storyboard.is_empty());
    assert!(video::sources::spec("youtube.com/watch")
        .unwrap()
        .borrow
        .is_some());
}

#[test]
fn a_player_page_lists_its_stream() {
    let html = fixture("player.vimeo.com/video").unwrap();
    let info = read("https://player.vimeo.com/video/76979871", &html).unwrap();
    assert_eq!(info.streams[0].kind, video::StreamKind::Hls);
    assert_eq!(info.tracks[0].lang.as_deref(), Some("de"));
}
