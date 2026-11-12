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

/// Eight multi-word queries, English and Spanish, to one engine that has answered such queries for
/// one of their words for stretches at a time. Prints, per query, how many of the top five share
/// any query word and how many share two; asserts only that an answer mostly off topic says why.
///
/// `cargo test -p svipall --test search_live bing -- --ignored --nocapture`
#[tokio::test]
#[ignore = "network"]
async fn eight_multi_word_queries_to_bing() {
    const MULTI: &[&str] = &[
        "rust async runtime",
        "how to configure nginx reverse proxy for websockets",
        "python dataclass default factory list",
        "postgres vacuum full vs analyze",
        "cómo hacer pan casero sin levadura",
        "mejores prácticas de seguridad para contraseñas",
        "receta tortilla de patatas con cebolla",
        "regulación europea inteligencia artificial",
    ];
    support::isolate();
    let s = SvipallServer::with_store(None, svipall_core::Config::default(), None, None);
    let (mut any, mut two, mut n) = (0, 0, 0);
    for q in MULTI {
        let out = s
            .search_json(WebSearchParams {
                query: q.to_string(),
                engine: Some("bing".into()),
                ..Default::default()
            })
            .await;
        let t = svipall::search::terms(q);
        let top: Vec<_> = out["results"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .take(5)
            .collect();
        let shared = |r: &serde_json::Value| {
            let hay = svipall::search::fold(&format!(
                "{} {} {}",
                r["title"].as_str().unwrap_or(""),
                r["snippet"].as_str().unwrap_or(""),
                r["url"].as_str().unwrap_or("")
            ));
            t.iter().filter(|w| hay.contains(w.as_str())).count()
        };
        let a = top.iter().filter(|r| shared(r) >= 1).count();
        let b = top.iter().filter(|r| shared(r) >= 2).count();
        let off = top.iter().filter(|r| r["off_topic"] == true).count();
        (any, two, n) = (any + a, two + b, n + top.len());
        let detail = out["attempts"][0]["detail"].as_str().unwrap_or("-");
        println!("| {q} | {a}/{} | {b}/{} | {detail} |", top.len(), top.len());
        if off * 2 > top.len() {
            assert!(
                out["note"].is_string(),
                "mostly off topic and unexplained: {out}"
            );
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
    println!("top five sharing a query word: {any}/{n}; sharing two: {two}/{n}");
}
