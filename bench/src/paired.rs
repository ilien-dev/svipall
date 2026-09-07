//! Matched automatic/native measurements. Every response, including failures, is retained.

use crate::targets::Set;
use serde_json::{json, Value};
use std::{sync::Arc, time::Instant};
use svipall_core::{cache::Store, Config};
use svipall_mcp::{server::SvipallServer, tools::WebFetchParams};

fn parameters(arm: &str, url: &str, timeout: u64) -> anyhow::Result<(Config, WebFetchParams)> {
    anyhow::ensure!(
        matches!(arm, "auto" | "native"),
        "arm must be auto or native"
    );
    let mut cfg = svipall_core::config::load();
    cfg.browser_identity = arm.into();
    cfg.auto_native_fallback = true;
    Ok((
        cfg,
        WebFetchParams {
            url: url.into(),
            mode: Some(if arm == "native" { "warm" } else { "auto" }.into()),
            extraction: Some("markdown".into()),
            cache: Some("write".into()),
            include_quality: Some(true),
            timeout: Some(timeout),
            ..Default::default()
        },
    ))
}

async fn measure(server: &SvipallServer, store: &Store, mut p: WebFetchParams) -> Value {
    let start = Instant::now();
    let first = server.fetch_json(p.clone()).await.value;
    let fetch_secs = start.elapsed().as_secs_f64();
    let mut response = first.clone();
    let mut content = first["content"].as_str().unwrap_or_default().to_string();
    let mut cursor = first["cursor"].as_str().map(str::to_owned);
    let mut chunks = vec![first];
    let mut seen = std::collections::HashSet::new();
    let mut stop = None;
    while let Some(next) = cursor.clone() {
        if chunks.len() >= 64 || start.elapsed().as_secs_f64() > fetch_secs + 15.0 {
            stop = Some("continuation_limit");
            break;
        }
        if !seen.insert(next.clone()) {
            stop = Some("repeated_cursor");
            break;
        }
        // Read mode can fetch on a miss. Check the private store before following a cursor.
        if !store.get(&p.url).is_some_and(|page| page.is_fresh()) {
            stop = Some("no_fresh_cached_continuation");
            break;
        }
        p.cursor = Some(next);
        p.cache = Some("read".into());
        let chunk = server.fetch_json(p.clone()).await.value;
        let cached = chunk["from_cache"] == true && chunk["stale_cursor"] != true;
        cursor = chunk["cursor"].as_str().map(str::to_owned);
        if cached {
            content.push_str(chunk["content"].as_str().unwrap_or_default());
        }
        chunks.push(chunk);
        if !cached {
            stop = Some("invalid_cached_continuation");
            break;
        }
    }
    let complete = cursor.is_none() && stop.is_none();
    response["content"] = json!(content);
    json!({"secs":start.elapsed().as_secs_f64(), "first_response_secs":fetch_secs,
        "response":response, "chunks":chunks, "content_complete":complete,
        "continuation_stop":stop})
}

pub async fn run(args: &[String]) -> anyhow::Result<usize> {
    let flag = |name| crate::flag(args, name);
    let set = Set::parse(&flag("--set").unwrap_or_default())
        .ok_or_else(|| anyhow::anyhow!("specify a known --set"))?;
    let target_name = flag("--target").unwrap_or_default();
    let target = set
        .targets()
        .iter()
        .find(|t| t.name == target_name)
        .ok_or_else(|| anyhow::anyhow!("specify a target in the chosen set"))?;
    let arm = flag("--arm").unwrap_or_default();
    let repeats: usize = flag("--repeat").unwrap_or_else(|| "3".into()).parse()?;
    anyhow::ensure!(repeats > 0, "repeat must be positive");
    let timeout: u64 = flag("--timeout")
        .unwrap_or_else(|| "60000".into())
        .parse()?;
    let (cfg, p) = parameters(&arm, target.url, timeout)?;
    let mut effective_config = serde_json::to_value(&cfg)?;
    effective_config["browser_path"] = json!("<managed-browser>");
    let store = Arc::new(Store::open_memory()?);
    let server = SvipallServer::with_store(None, cfg, None, Some(store.clone()));
    let started = svipall_core::automatic::now();
    let mut cells = Vec::new();
    for position in 1..=repeats {
        let spent =
            svipall_core::reputation::spent(&svipall_core::domain_from_url(target.url), None);
        let mut cell = measure(&server, &store, p.clone()).await;
        let response = &cell["response"];
        let content = response["content"].as_str().unwrap_or_default();
        let expected = target.expect.is_empty()
            || target
                .expect
                .iter()
                .any(|e| content.to_lowercase().contains(&e.to_lowercase()));
        let delivered = response["status"]
            .as_u64()
            .is_some_and(|s| (200..400).contains(&s))
            && response["blocked_reason"].is_null()
            && !content.trim().is_empty()
            && expected;
        cell["target"] = json!(target.name);
        cell["url"] = json!(target.url);
        cell["position"] = json!(position);
        cell["spent_before"] = json!(spent);
        cell["expected"] = json!(expected);
        cell["delivered"] = json!(delivered);
        eprintln!(
            "{} repeat {}: {} {:.2}s ({})",
            target.name,
            position,
            if delivered { "delivered" } else { "failed" },
            cell["secs"].as_f64().unwrap_or_default(),
            cell["response"]["tier_used"].as_str().unwrap_or("-")
        );
        cells.push(cell);
    }
    server.shutdown_configuration().await;
    svipall_core::reputation::flush();
    println!(
        "{}",
        json!({"schema":2, "label":flag("--label"), "set":set.name(),
        "arm":arm, "target":target.name, "repeat":repeats, "timeout_ms":timeout,
        "started_unix":started, "ended_unix":svipall_core::automatic::now(),
        "effective_config":effective_config, "cells":cells})
    );
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_control_starts_in_the_same_browser_tier_as_the_auto_fallback() {
        let (cfg, p) = parameters("native", "https://example.com", 60000).unwrap();
        assert_eq!(cfg.browser_identity, "native");
        assert_eq!(p.mode.as_deref(), Some("warm"));
        assert_eq!(p.cache.as_deref(), Some("write"));
        assert_eq!(p.extraction.as_deref(), Some("markdown"));
        assert_eq!(p.timeout, Some(60000));
        let (cfg, p) = parameters("auto", "https://example.com", 60000).unwrap();
        assert_eq!(cfg.browser_identity, "auto");
        assert!(cfg.auto_native_fallback);
        assert_eq!(p.mode.as_deref(), Some("auto"));
        assert!(parameters("typo", "https://example.com", 60000).is_err());
    }

    #[tokio::test]
    async fn continuations_recover_the_tail_without_another_transport() {
        let store = std::sync::Arc::new(svipall_core::cache::Store::open_memory().unwrap());
        let server = svipall_mcp::server::SvipallServer::with_store(
            None,
            svipall_core::Config::default(),
            None,
            Some(store.clone()),
        );
        let html = format!("<html><body><main>{}<p>UNIQUE_TAIL_MARKER</p></main></body></html>",
            (0..80).map(|i| format!("<p>Paragraph {i} contains enough distinct words to require another output page.</p>")).collect::<String>());
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/article", listener.local_addr().unwrap());
        let serving = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 8192];
            let _ = stream.read(&mut request).await.unwrap();
            let reply = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", html.len(), html);
            stream.write_all(reply.as_bytes()).await.unwrap();
        });
        let (_, mut p) = parameters("auto", &url, 60000).unwrap();
        p.max_tokens = Some(100);
        let result = measure(&server, &store, p).await;
        assert!(
            result["chunks"].as_array().unwrap().len() > 1,
            "stop: {}; complete: {}",
            result["continuation_stop"],
            result["content_complete"]
        );
        assert_eq!(result["content_complete"], true, "{result}");
        assert!(result["response"]["content"]
            .as_str()
            .unwrap()
            .contains("UNIQUE_TAIL_MARKER"));
        for chunk in result["chunks"].as_array().unwrap().iter().skip(1) {
            assert_eq!(chunk["from_cache"], true, "{chunk}");
            assert_ne!(chunk["stale_cursor"], true, "{chunk}");
        }
        server.shutdown_configuration().await;
        serving.await.unwrap();
    }
}
