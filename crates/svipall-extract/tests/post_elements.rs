//! A page whose posts and comments are custom elements is read from those elements, including
//! the comments its server streams inside a `<template>`.

use svipall_extract::{extract_markdown_opts, parse_page, ExtractOpts, ParseWants};

const THREAD: &str = include_str!("fixtures/post-elements-thread.html");
const FEED: &str = include_str!("fixtures/post-elements-feed.html");
const BASE: &str = "https://www.reddit.com/r/rustlearners/";

fn markdown(html: &str) -> String {
    extract_markdown_opts(
        html,
        &ExtractOpts {
            main_content_only: true,
            base_url: Some(BASE),
            ..Default::default()
        },
    )
}

#[test]
fn a_thread_gives_its_post_with_the_numbers_the_element_carries() {
    let md = markdown(THREAD);
    assert!(md.starts_with("# Borrowing across an await point"), "{md}");
    assert!(
        md.contains("r/rustlearners · u/first_poster · 42 points (91% upvoted) · 7 comments · 2026-03-04 09:15 UTC"),
        "{md}"
    );
    assert!(md.contains("held over an `.await`?"), "{md}");
    assert!(
        md.contains("[the async book](https://example.test/async-book)"),
        "{md}"
    );
}

#[test]
fn comments_streamed_in_a_template_are_read_with_author_score_and_depth() {
    let md = markdown(THREAD);
    assert!(md.contains("## Comments (4 of 7 loaded)"), "{md}");
    assert!(
        md.contains("**u/helper_one** · 30 points · 2026-03-04 09:20 UTC\nThe future may move between threads, so the borrow has to be **Send**."),
        "{md}"
    );
    assert!(
        md.contains("> **u/first_poster** · 5 points · 2026-03-04 09:31 UTC\n> That explains the error about the mutex guard."),
        "{md}"
    );
    assert!(
        md.contains("> > **u/helper_one** · 2 points · 2026-03-04 09:40 UTC\n> > Drop the guard before awaiting."),
        "{md}"
    );
    assert!(md.contains("**u/second_helper** · -3 points"), "{md}");
}

#[test]
fn a_comment_owns_only_its_own_text() {
    let md = markdown(THREAD);
    assert_eq!(
        md.matches("Drop the guard before awaiting.").count(),
        1,
        "{md}"
    );
    assert_eq!(md.matches("That explains the error").count(), 1, "{md}");
    // The metadata row inside the element is the header line already.
    assert!(!md.contains("4d ago"), "{md}");
}

#[test]
fn page_furniture_is_not_part_of_a_thread() {
    let md = markdown(THREAD);
    assert!(!md.contains("Popular"), "{md}");
    assert!(!md.contains("User Agreement"), "{md}");
}

#[test]
fn a_feed_lists_each_post_once_and_never_an_advertisement() {
    let md = markdown(FEED);
    assert!(md.starts_with("# Learning Rust together"), "{md}");
    assert!(
        md.contains("**[A tour of trait objects](https://www.reddit.com/r/rustlearners/comments/1aaa111/a_tour_of_trait_objects/)**\n276 points (97% upvoted) · 35 comments · u/link_poster · r/rustlearners · 2026-03-02 06:24 UTC\nLink: https://blog.example.test/trait-objects/"),
        "{md}"
    );
    assert!(md.contains("1 point · 1 comment · u/curious_one"), "{md}");
    assert!(
        md.contains("My FFI wrapper needs two unsafe blocks"),
        "{md}"
    );
    assert_eq!(md.matches("Is unsafe ever fine?").count(), 1, "{md}");
    assert!(!md.contains("keyboard"), "{md}");
    // A text post links to itself; saying so twice is noise.
    assert_eq!(md.matches("Link: ").count(), 1, "{md}");
}

#[test]
fn a_feed_names_its_next_page_without_the_session_it_was_served_to() {
    let md = markdown(FEED);
    assert!(
        md.contains("Next page: https://www.reddit.com/svc/shreddit/community-more-posts/top/?t=WEEK&after=dDNfMWFhYTIyMg%3D%3D&name=rustlearners"),
        "{md}"
    );
    assert!(!md.contains("navigationSessionId"), "{md}");
    assert!(!md.contains("ad_posts_served"), "{md}");
}

#[test]
fn the_text_a_page_is_judged_on_is_the_text_that_was_read() {
    let parts = parse_page(THREAD, &ParseWants::text());
    assert!(parts.declared);
    assert!(
        parts.text.contains("Drop the guard before awaiting."),
        "{}",
        parts.text
    );
    assert!(
        parts.text.contains("Borrowing across an await point"),
        "{}",
        parts.text
    );
    assert!(!parts.text.contains("**"), "{}", parts.text);
}

#[test]
fn the_title_is_the_one_the_page_declares_not_the_placeholder() {
    let parts = parse_page(
        THREAD,
        &ParseWants {
            title: true,
            ..ParseWants::text()
        },
    );
    assert_eq!(
        parts.title.as_deref(),
        Some("Borrowing across an await point : r/rustlearners")
    );
}

#[test]
fn a_selector_the_caller_wrote_still_wins() {
    let md = extract_markdown_opts(
        THREAD,
        &ExtractOpts {
            main_content_only: true,
            css_selector: Some("footer"),
            base_url: Some(BASE),
            ..Default::default()
        },
    );
    assert!(md.contains("User Agreement"), "{md}");
    assert!(!md.contains("## Comments"), "{md}");
}

#[test]
fn an_ordinary_page_is_untouched() {
    let html = "<html><body><main><h1>Plain</h1><p>Nothing here is a post element, only a paragraph of ordinary prose that an article would carry.</p></main></body></html>";
    let parts = parse_page(html, &ParseWants::text());
    assert!(!parts.declared);
    assert!(markdown(html).contains("# Plain"));
}

const SEARCH: &str = include_str!("fixtures/post-elements-search.html");

fn markdown_at(html: &str, url: &str) -> String {
    extract_markdown_opts(
        html,
        &ExtractOpts {
            main_content_only: true,
            base_url: Some(url),
            ..Default::default()
        },
    )
}

#[test]
fn search_results_read_like_a_feed() {
    let md = markdown_at(
        SEARCH,
        "https://www.reddit.com/r/rustlearners/search/?q=lifetimes",
    );
    assert!(md.starts_with("# lifetimes - Reddit Search!"), "{md}");
    assert!(
        md.contains("**[Lifetimes finally clicked \\[long\\]](https://www.reddit.com/r/rustlearners/comments/1bbb111/lifetimes_finally_clicked/)**\n730 points · 73 comments · r/rustlearners · 2026-02-13 20:11 UTC"),
        "{md}"
    );
    assert!(
        md.contains("1 point · 0 comments · r/rust · 2026-04-24 04:11 UTC"),
        "{md}"
    );
    assert_eq!(md.matches("A lifetime question").count(), 1, "{md}");
    assert!(!md.contains("Unable to load"), "{md}");
    assert!(
        md.contains("Next page: https://www.reddit.com/svc/shreddit/r/rustlearners/search/?q=lifetimes&sort=top&cursor=eyJjYW5kaWRhdGVz"),
        "{md}"
    );
}

#[test]
fn a_feed_names_its_rules_and_recites_them_only_when_asked() {
    let md = markdown(FEED);
    assert!(
        md.contains("Rules (https://www.reddit.com/r/rustlearners/#rules): 1. Be kind to beginners · 2. No homework dumps"),
        "{md}"
    );
    assert!(!md.contains("Every question was new"), "{md}");

    let rules = markdown_at(FEED, "https://www.reddit.com/r/rustlearners/#rules");
    assert!(
        rules.starts_with("# Learning Rust together\n\n## Rules"),
        "{rules}"
    );
    assert!(
        rules.contains("**1. Be kind to beginners**\nEvery question was new to somebody once."),
        "{rules}"
    );
    assert!(
        rules.contains("[Details](https://example.test/conduct)"),
        "{rules}"
    );
    assert!(
        rules.contains("**2. No homework dumps**\nShow what you tried first."),
        "{rules}"
    );
    assert!(!rules.contains("A tour of trait objects"), "{rules}");
}
