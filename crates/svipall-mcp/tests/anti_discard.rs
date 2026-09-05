//! The one rule the whole quality system exists to serve: **nothing is ever withheld**.
//!
//! Every signal svipall attaches to a page — how much of it arrived, how engineered it is, what a
//! classifier makes of it, whether three results are really one — is a label. None of them removes
//! a result, reorders one, or stops the ladder. This file is what makes that true rather than
//! stated, and it is deliberately its own test binary: it installs a trained classifier into the
//! test home, and the point is to prove that even with the model saying "junk" at full confidence,
//! the page still comes back whole.
//!
//! Why it matters, in one measurement. Cuconasu et al. (SIGIR 2024) found that the documents which
//! actually degrade an answer are the *high-scoring, on-topic, answer-free* ones, while adding
//! plainly random documents **improved** accuracy by up to 35%. And Dodge et al. (EMNLP 2021)
//! measured what a reasonable-looking filter did to C4: African-American English removed at 42%
//! and Hispanic-aligned English at 32%, against 6.2% for the majority dialect. A scoring system
//! that is allowed to drop things gets both of those wrong quietly.

mod support;

use support::{Reply, Site};
use svipall_core::quality::substance::Label;
use svipall_mcp::quality_cli::Example;
use svipall_mcp::server::SvipallServer;
use svipall_mcp::tools::WebFetchParams;

/// The answer nothing else on the web says, so finding it in the response proves the page came
/// back rather than something like it.
const ANSWER: &str = "the eastern quay reopens on the fourteenth of November";

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

/// Fit a classifier and install it where svipall looks, so the test exercises the real loader and
/// the real inference path rather than a stand-in.
fn install_a_classifier(home: &std::path::Path) {
    let mut body = String::new();
    for i in 0..60 {
        for (slug, text, label) in [
            (
                format!("https://x.test/news/{i}"),
                format!(
                    "the council voted on tuesday to approve the harbour measure after debate {i} \
                     supporters argued the change was overdue and the money was already set aside"
                ),
                Label::Substantive,
            ),
            (
                format!("https://x.test/deal/{i}"),
                format!(
                    "best cheap anvils {i} buy now click here best price limited offer deal buy \
                     cheap anvils now best cheap anvil price click here for the best deal"
                ),
                Label::Junk,
            ),
        ] {
            // Distinct URLs: the trainer keeps one row per URL, so sixty rows claiming to be
            // the same page are one page.
            let e = Example {
                url: slug,
                text,
                label,
                source: "test".into(),
                why: None,
            };
            body.push_str(&serde_json::to_string(&e).expect("json"));
            body.push('\n');
        }
    }
    // ▲ A name of its own. Both tests in this binary share one isolated home — they have to, the
    // model must land where svipall looks — and they run in parallel, so a single `training.jsonl`
    // has one test reading it while the other is halfway through writing it. That race was always
    // here; it used to show up as a model fitted on half a file, which passes and means nothing.
    let set = home.join(format!(
        "training-{}.jsonl",
        std::thread::current()
            .name()
            .unwrap_or("t")
            .replace(":", "-")
    ));
    std::fs::write(&set, body).expect("write training set");
    svipall_mcp::quality_cli::train(&set, &home.join("models"), 40, 0.5).expect("train");
    assert!(
        svipall_mcp::substance::available(),
        "the model was written where svipall does not look for it"
    );
}

#[tokio::test]
async fn the_page_the_classifier_calls_junk_still_comes_back_whole() {
    let home = support::isolate();
    install_a_classifier(&home);

    // A page in exactly the register the classifier was taught to condemn — and the only page on
    // this site that holds the answer.
    let junk_with_the_answer = format!(
        "<!doctype html><html><head><title>Best cheap anvils</title></head><body><main>\
         <p>best cheap anvils buy now click here best price limited offer deal buy cheap anvils \
         now best cheap anvil price click here for the best deal on cheap anvils today. \
         By the way, {ANSWER}. \
         best cheap anvils buy now click here best price limited offer deal buy cheap anvils.</p>\
         </main></body></html>"
    );
    let site = Site::start(vec![("/anvils", Reply::html(&junk_with_the_answer))]).await;

    let out = server()
        .fetch_json(WebFetchParams {
            url: site.url("/anvils"),
            max_tier: Some("http".into()),
            ..Default::default()
        })
        .await;

    // The label is made, and it is the harshest one.
    assert_eq!(out.value["substance"], "junk", "{:?}", out.value);

    // And none of that took anything away.
    let content = out.value["content"].as_str().unwrap_or_default();
    assert!(
        content.contains("fourteenth of November"),
        "the answer was withheld from the caller: {:?}",
        out.value
    );
    assert!(
        out.value.get("blocked_reason").is_none(),
        "a low score is not a wall: {:?}",
        out.value
    );
    assert_eq!(out.value["status"], 200, "{:?}", out.value);
}

#[tokio::test]
async fn a_set_of_results_keeps_every_member_however_they_score() {
    let home = support::isolate();
    install_a_classifier(&home);

    let junk = format!(
        "<!doctype html><html><head><title>Deals</title></head><body><main><p>\
         best cheap anvils buy now click here best price limited offer deal buy cheap anvils now \
         best cheap anvil price. {ANSWER}. best cheap anvils buy now click here best price.\
         </p></main></body></html>"
    );
    let article = "<!doctype html><html><head><title>Harbour</title></head><body><main><p>\
        The council voted on Tuesday to approve the harbour measure after a debate that ran past \
        midnight. Supporters argued the change was overdue and that the money was already set \
        aside for it three budgets ago.</p></main></body></html>";
    let site = Site::start(vec![
        ("/deals", Reply::html(&junk)),
        ("/harbour", Reply::html(article)),
        // Two more copies of the article, so provenance has something to group as well.
        ("/wire-a", Reply::html(article)),
        ("/wire-b", Reply::html(article)),
    ])
    .await;

    let out = server()
        .fetch_many_json(
            serde_json::from_value(serde_json::json!({
                "urls": [
                    site.url("/deals"),
                    site.url("/harbour"),
                    site.url("/wire-a"),
                    site.url("/wire-b"),
                ],
                "max_tier": "http",
            }))
            .expect("params"),
        )
        .await;

    // Everything asked for, whatever any signal said about it. The order may change — three
    // copies of one document are reordered so the one that differs is read sooner — but the
    // caller's own first choice never moves, and nothing is dropped.
    assert_eq!(out["count"], 4, "{out:?}");
    let results = out["results"].as_array().expect("results");
    assert_eq!(results.len(), 4);
    assert!(
        results[0]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("fourteenth of November"),
        "the lowest-scoring result was the one with the answer, and it was withheld: {:?}",
        results[0]
    );
    // Three of the four are one document; saying so must not remove two of them.
    assert_eq!(out["corroboration"]["independent"], 2, "{out:?}");
    assert!(
        results
            .iter()
            .all(|r| !r["content"].as_str().unwrap_or_default().trim().is_empty()),
        "a duplicate is labelled, never emptied: {results:?}"
    );
}
