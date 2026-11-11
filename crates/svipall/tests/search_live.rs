//! Ten real searches against the real engines, through the real handler: pacing, exits and engine
//! health all apply. Run by hand; it prints one line per query and one per engine asked.
//!
//! `cargo test -p svipall --test search_live -- --ignored --nocapture`
mod support;

use svipall::server::SvipallServer;
use svipall::tools::WebSearchParams;

const QUERIES: &[&str] = &[
    "rust async runtime",
    "how to configure nginx reverse proxy for websockets",
    "cómo hacer pan casero sin levadura",
    "mejores prácticas de seguridad para contraseñas",
    "\"the quick brown fox jumps over the lazy dog\"",
    "tokio",
    "what is the difference between tcp and udp explained simply for complete beginners",
    "receta tradicional de tortilla de patatas española con cebolla y huevos camperos",
    "python dataclass default factory list",
    "\"inteligencia artificial\" regulación europea",
];

#[tokio::test]
#[ignore = "network"]
async fn ten_real_searches_answer_on_topic() {
    support::isolate();
    let s = SvipallServer::with_store(None, svipall_core::Config::default(), None, None);
    let mut good = 0;
    for q in QUERIES {
        let out = s
            .search_json(WebSearchParams {
                query: q.to_string(),
                ..Default::default()
            })
            .await;
        let results = out["results"].as_array().cloned().unwrap_or_default();
        let on = results.iter().filter(|r| r["off_topic"] != true).count();
        if on > 0 {
            good += 1;
        }
        println!(
            "| {q} | {} | {} | {on} | {} | {} |",
            out["engine"].as_str().unwrap_or("?"),
            results.len(),
            out["retried_with"].as_str().unwrap_or("-"),
            results
                .first()
                .and_then(|r| r["url"].as_str())
                .unwrap_or("-"),
        );
        for a in out["attempts"].as_array().cloned().unwrap_or_default() {
            println!(
                "|   | {} {} | {} | {} | {} |",
                a["engine"].as_str().unwrap_or("?"),
                a["outcome"].as_str().unwrap_or("?"),
                a["count"],
                a["ms"],
                a["detail"].as_str().unwrap_or(""),
            );
        }
        if let Some(n) = out["note"].as_str() {
            println!("|   | note: {n} |");
        }
    }
    println!("on-topic answers: {good} of {}", QUERIES.len());
    assert!(
        good >= 9,
        "{good} of {} queries got an on-topic answer",
        QUERIES.len()
    );
}
