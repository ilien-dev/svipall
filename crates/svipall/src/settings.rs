//! Configuration through the tool itself; no hand-edited files are required.
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use svipall_core::config;

pub fn run(args: &[String]) -> Result<Value> {
    let home = config::home_dir();
    let action = args.first().map(String::as_str).unwrap_or("show");
    let cfg = match action {
        "show" => config::load_in(&home)?,
        "set" => {
            let mut patch = serde_json::Map::new();
            for pair in &args[1..] {
                let (key, value) = pair
                    .split_once('=')
                    .ok_or_else(|| anyhow!("use key=value"))?;
                patch.insert(
                    key.into(),
                    serde_json::from_str(value).unwrap_or_else(|_| json!(value)),
                );
            }
            anyhow::ensure!(!patch.is_empty(), "provide at least one key=value setting");
            config::update_in(&home, Value::Object(patch))?
        }
        "preset" => {
            let mode = args.get(1).map(String::as_str).unwrap_or("local");
            let identity = match mode {
                "local" | "auto" => "auto",
                "emulated" => "emulated",
                "native" => "native",
                _ => return Err(anyhow!("preset must be local, auto, emulated or native")),
            };
            config::update_in(
                &home,
                json!({"browser_identity":identity,
                "browser_auto_install":true,"warm_adaptive":true,"warm_wait_ms":20000,
                "warm_max_wait_ms":55000,"warm_keep_max":2,"warm_keep_secs":120,
                "browser_idle_secs":180,"block_ads":false}),
            )?
        }
        _ => return Err(anyhow!("config expects show, set or preset")),
    };
    let mut effective = serde_json::to_value(cfg)?;
    if effective["api_key"].as_str().is_some_and(|s| !s.is_empty()) {
        effective["api_key"] = json!("[redacted]");
    }
    Ok(json!({"config":effective,"saved":action != "show",
        "applies":"next command; running servers refresh browser policy on their next request","path":home.join("settings.toml")}))
}
