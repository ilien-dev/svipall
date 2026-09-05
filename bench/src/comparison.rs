//! Identical measurement code compiled into both product variants.
use crate::targets::{historical_verdict, Set};
use serde_json::json;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use svipall_mcp::{server::SvipallServer, tools::WebFetchParams};

pub async fn run(args: &[String]) -> anyhow::Result<usize> {
    let flag = |name| crate::flag(args, name);
    let set = Set::parse(&flag("--set").unwrap_or_else(|| "hard12".into()))
        .ok_or_else(|| anyhow::anyhow!("unknown target set"))?;
    let seed: u64 = flag("--seed").unwrap_or_else(|| "20260905".into()).parse()?;
    let repeats: usize = flag("--repeat").unwrap_or_else(|| "2".into()).parse()?;
    anyhow::ensure!(repeats > 0, "repeat must be positive");
    let timeout: u64 = flag("--timeout").unwrap_or_else(|| "90000".into()).parse()?;
    let mut targets: Vec<_> = set.targets().iter().collect();
    let mut random = seed | 1;
    for i in (1..targets.len()).rev() {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        targets.swap(i, random as usize % (i + 1));
    }
    let cfg = svipall_core::config::load();
    let config = std::fs::read_to_string(svipall_core::config::home_dir().join("config.toml"))
        .unwrap_or_default();
    let store = std::sync::Arc::new(svipall_core::cache::Store::open_memory()?);
    let server = SvipallServer::with_store(None, cfg, None, Some(store));
    let started = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let mut cells = Vec::new();
    for target in targets {
        let domain = svipall_core::domain_from_url(target.url);
        for position in 1..=repeats {
            let spent = svipall_core::reputation::spent(&domain, None);
            let before = Instant::now();
            let result = server.fetch_json(WebFetchParams {
                url: target.url.into(), timeout: Some(timeout), cache: Some("bypass".into()),
                extraction: (set == Set::Public31).then(|| "html".into()),
                ..Default::default()
            }).await.value;
            let elapsed = before.elapsed().as_secs_f64();
            let status = result["status"].as_u64().and_then(|v| u16::try_from(v).ok());
            let content = result["content"].as_str().unwrap_or_default();
            let title = result["title"].as_str().unwrap_or_default();
            let historical = historical_verdict(status, title, content).name();
            let valid_status = status.is_some_and(|s| (100..=599).contains(&s));
            let expected = target.expect.is_empty() || target.expect.iter()
                .any(|s| content.to_lowercase().contains(&s.to_lowercase()));
            let delivered = status.is_some_and(|s| (200..400).contains(&s))
                && result["blocked_reason"].is_null() && !content.trim().is_empty() && expected;
            let outcome = if !valid_status { "error" } else if delivered { "delivered" }
                else if !result["blocked_reason"].is_null() { "wall" } else { "missing_content" };
            eprintln!("{} repeat {}: {} {:.2}s ({})", target.name, position, outcome, elapsed,
                result["tier_used"].as_str().unwrap_or("-"));
            cells.push(json!({"target":target.name,"url":target.url,"position":position,
                "secs":elapsed,"spent_before":spent,"historical_verdict":historical,
                "valid_status":valid_status,"expected":expected,"delivered":delivered,
                "outcome":outcome,"response":result}));
        }
    }
    svipall_core::reputation::flush();
    println!("{}", json!({"schema":1,"label":flag("--label"),"set":set.name(),
        "seed":seed,"repeat":repeats,"timeout_ms":timeout,"started_unix":started,
        "ended_unix":SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        "config_toml":config,"browser":server.pool().executable(),"cells":cells}));
    Ok(0)
}
