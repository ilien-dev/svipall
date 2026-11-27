//! A page that renders nothing without script but states what it holds as schema.org data is
//! read from that statement.

use svipall_extract::{extract_markdown_opts, parse_page, ExtractOpts, ParseWants};

const PROFILE: &str = include_str!("fixtures/linked-data-profile.html");
const THREAD: &str = include_str!("fixtures/linked-data-thread.html");

fn markdown(html: &str) -> String {
    extract_markdown_opts(
        html,
        &ExtractOpts {
            main_content_only: true,
            base_url: Some("https://skyline.test/profile/aurora.example"),
            ..Default::default()
        },
    )
}

fn shell(ld: &str) -> String {
    format!(
        "<html><head><title>Shell</title><script type=\"application/ld+json\">{ld}</script>\
         </head><body><div id=\"app\"></div><noscript>Enable JavaScript.</noscript></body></html>"
    )
}

#[test]
fn a_profile_gives_who_it_is_and_what_they_posted() {
    let md = markdown(PROFILE);
    assert!(md.starts_with("# Aurora Station"), "{md}");
    assert!(
        md.contains("@aurora.example · 12,400 followers · 318 posts · following 4 · since 2023-04-12 04:53 UTC"),
        "{md}"
    );
    assert!(md.contains("Questions: hello@aurora.example"), "{md}");
    assert!(md.contains("## Posts (3)"), "{md}");
    assert!(
        md.contains("**Aurora Station (@aurora.example)** · 2026-11-19 18:33 UTC · 1,204 likes · 1 share · 790 comments\nThe winter crew arrived today.\n\nFourteen people and one very confused dog.\nLink: https://skyline.test/profile/aurora.example/post/3n2opit"),
        "{md}"
    );
    assert!(
        md.contains("0 comments\nFirst light over the ridge"),
        "{md}"
    );
    // The page said that in a script, and the script's other JSON is configuration.
    assert!(!md.contains("apiHost"), "{md}");
    assert!(!md.contains("JavaScript Required"), "{md}");
}

#[test]
fn text_from_the_data_cannot_write_markdown() {
    let md = markdown(PROFILE);
    assert!(
        md.contains("the \\[magnetometer\\] reads \\*steady\\*."),
        "{md}"
    );
}

#[test]
fn a_thread_gives_the_post_and_the_comments_it_was_served_with() {
    let md = markdown(THREAD);
    assert!(
        md.starts_with("# We are opening the station logs to everyone."),
        "{md}"
    );
    assert!(
        md.contains("Aurora Station (@aurora.example) · 2024-10-17 07:06 UTC · 50,321 likes · 8,779 comments"),
        "{md}"
    );
    assert!(
        md.contains("Ask us anything about a winter at the pole."),
        "{md}"
    );
    assert!(md.contains("## Comments (4 of 8,779 loaded)"), "{md}");
    assert!(
        md.contains("**Polar Fan (@polar.fan)** · 2024-10-17 07:20 UTC · 41 upvotes\nHow cold does the dog get?"),
        "{md}"
    );
    assert!(
        md.contains("> **Aurora Station (@aurora.example)** · 2024-10-17 07:31 UTC\n> She has a better coat than any of us."),
        "{md}"
    );
    assert!(md.contains("**Skeptic (@skeptic.example)** · 2024-10-17 08:02 UTC · 1 upvote\nLogs or it did not happen."), "{md}");
    // A time in another zone is given as it was written.
    assert!(
        md.contains("**A plain name** · 2024-10-18T10:00:00+02:00\nFollowing."),
        "{md}"
    );
    assert_eq!(md.matches("How cold does the dog get?").count(), 1, "{md}");
}

#[test]
fn the_page_is_judged_on_the_text_that_was_read() {
    let parts = parse_page(
        THREAD,
        &ParseWants {
            title: true,
            ..ParseWants::text()
        },
    );
    assert!(parts.declared);
    assert!(
        parts.text.contains("She has a better coat"),
        "{}",
        parts.text
    );
    assert!(!parts.text.contains("**"), "{}", parts.text);
    assert_eq!(
        parts.title.as_deref(),
        Some("Aurora Station (@aurora.example) on Skyline")
    );
}

#[test]
fn questions_videos_and_listings_read_the_same_way() {
    let qa = markdown(&shell(
        r#"{"@context":"https://schema.org","@type":"QAPage","mainEntity":{"@type":"Question","name":"Why is a sorted array faster?","text":"Sorting the data first makes the loop six times faster. Why?","answerCount":2,"upvoteCount":27000,"dateCreated":"2012-06-27T13:51:36Z","author":{"@type":"Person","name":"asker"},"acceptedAnswer":{"@type":"Answer","text":"Branch prediction. With sorted data the branch goes the same way for long runs, and the processor guesses right.","upvoteCount":35000,"author":{"@type":"Person","name":"mystic"},"dateCreated":"2012-06-27T13:56:42Z"},"suggestedAnswer":[{"@type":"Answer","text":"Think of a railroad junction where the operator has to guess.","upvoteCount":4500,"author":{"@type":"Person","name":"second"}}]}}"#,
    ));
    assert!(qa.starts_with("# Why is a sorted array faster?"), "{qa}");
    assert!(
        qa.contains("asker · 2012-06-27 13:51 UTC · 27,000 upvotes · 2 answers"),
        "{qa}"
    );
    assert!(qa.contains("## Accepted answer"), "{qa}");
    assert!(
        qa.contains("**mystic** · 2012-06-27 13:56 UTC · 35,000 upvotes\nBranch prediction."),
        "{qa}"
    );
    assert!(qa.contains("## Answers (1)"), "{qa}");
    assert!(
        qa.contains("**second** · 4,500 upvotes\nThink of a railroad junction"),
        "{qa}"
    );

    let video = markdown(&shell(
        r#"{"@context":"https://schema.org","@type":"VideoObject","name":"A walk around the station","description":"Twelve minutes outside at minus forty, with commentary on what each building is for and why the flags matter.","uploadDate":"2005-04-23T20:31:52-07:00","duration":"PT12M4S","author":"Aurora Station","interactionStatistic":{"@type":"InteractionCounter","interactionType":{"@type":"WatchAction"},"userInteractionCount":"380000000"},"comment":[{"@type":"https://schema.org/Comment","dateCreated":"2020-02-17T18:58:15Z","text":"The flags are the best part of the whole walk and nobody can tell me otherwise.","author":{"@type":"Person","name":"viewer"},"upvoteCount":"4884863"}]}"#,
    ));
    assert!(video.starts_with("# A walk around the station"), "{video}");
    assert!(
        video.contains("Aurora Station · 2005-04-23T20:31:52-07:00 · 380,000,000 views · PT12M4S"),
        "{video}"
    );
    assert!(
        video.contains(
            "**viewer** · 2020-02-17 18:58 UTC · 4,884,863 upvotes\nThe flags are the best part"
        ),
        "{video}"
    );

    let list = markdown(&shell(
        r#"{"@context":"https://schema.org","@type":"ItemList","name":"Open roles","itemListElement":[{"@type":"ListItem","position":1,"item":{"@type":"JobPosting","title":"Station engineer","url":"https://jobs.test/1","datePosted":"2026-11-01","description":"Keep the generators running through the winter. Twelve month contract, all travel paid, and the station provides the cold weather gear.","hiringOrganization":{"@type":"Organization","name":"Aurora Station"},"jobLocation":{"@type":"Place","address":{"@type":"PostalAddress","addressLocality":"McMurdo","addressCountry":"AQ"}}}},{"@type":"ListItem","position":2,"url":"https://jobs.test/2","name":"Cook"},{"@type":"ListItem","position":3,"item":{"@type":"JobPosting","title":"Radio operator","url":"https://jobs.test/3","description":"Nights on the high frequency set, and the weather balloon at dawn."}}]}"#,
    ));
    assert!(list.starts_with("# Open roles"), "{list}");
    assert!(list.contains("**[Station engineer](https://jobs.test/1)**\nAurora Station · McMurdo, AQ · 2026-11-01\nKeep the generators running"), "{list}");
    assert!(list.contains("**[Cook](https://jobs.test/2)**"), "{list}");
}

#[test]
fn a_page_that_shows_its_content_is_read_from_what_it_shows() {
    let para =
        "The winter crew arrived today, fourteen people and one very confused dog. ".repeat(60);
    let html = format!(
        "<html><head><title>Article</title><script type=\"application/ld+json\">\
         {{\"@type\":\"NewsArticle\",\"headline\":\"From the data\",\"articleBody\":\"{}\"}}</script></head>\
         <body><main><h1>From the page</h1><p>{para}</p></main></body></html>",
        "Only in the data. ".repeat(400)
    );
    let parts = parse_page(&html, &ParseWants::text());
    assert!(!parts.declared);
    let md = markdown(&html);
    assert!(md.contains("# From the page"), "{md}");
    assert!(!md.contains("Only in the data"), "{md}");
}

#[test]
fn data_that_says_less_than_the_page_or_nothing_much_is_not_a_reading() {
    // A name and an address: what every page declares, and no content.
    let thin = shell(
        r#"{"@context":"https://schema.org","@type":"WebSite","name":"Skyline","url":"https://skyline.test"}"#,
    );
    assert!(!parse_page(&thin, &ParseWants::text()).declared);
    // Not JSON at all, and JSON that is not an object.
    for ld in ["{not json", "[1,2,3]", "\"text\"", ""] {
        assert!(
            !parse_page(&shell(ld), &ParseWants::text()).declared,
            "{ld}"
        );
    }
}

#[test]
fn a_selector_the_caller_wrote_still_wins() {
    let md = extract_markdown_opts(
        THREAD,
        &ExtractOpts {
            main_content_only: true,
            css_selector: Some("footer"),
            ..Default::default()
        },
    );
    assert!(md.contains("About Skyline"), "{md}");
    assert!(!md.contains("## Comments"), "{md}");
}

#[test]
fn nesting_without_end_is_cut_and_not_followed() {
    let mut ld = String::from(
        r#"{"@type":"Comment","text":"deepest words that are long enough to count as something said"}"#,
    );
    for i in 0..60 {
        ld = format!(
            r#"{{"@type":"Comment","author":"a{i}","text":"level {i} says something long enough to be kept as content here","comment":[{ld}]}}"#
        );
    }
    let ld = format!(
        r#"{{"@type":"DiscussionForumPosting","headline":"Deep","text":"A post with a very deep chain of replies beneath it, deeper than any reader follows.","comment":[{ld}]}}"#
    );
    let md = markdown(&shell(&ld));
    assert!(md.starts_with("# Deep"), "{md}");
    assert!(md.contains("level 59 says"), "{md}");
    assert!(!md.contains("deepest words"), "{md}");
}
