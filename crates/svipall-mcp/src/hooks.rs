//! Answers for an AI harness's tool-call hooks, in this binary rather than in a shell script.
//!
//! A hook is a command the harness runs before every matching tool call, so it is on the hot path
//! of somebody's day. Written as `sh` + `jq` it needs both installed and it does not run on
//! Windows; written here it is the binary that is already required, on every platform, with a test
//! around it.
//!
//! Everything here is a pure function of the event and one flag. Whether the flag is set is read
//! once, at the edge, so the behaviour can be asserted without touching this machine.

use serde_json::{json, Value};

/// The file whose existence arms the web hook.
///
/// A file rather than a config field, and rather than "the plugin is installed": installing a
/// plugin must never change how somebody's existing `WebFetch` behaves. Creating this is a
/// separate, reversible decision, and `rm` undoes it.
pub const STRICT_MARKER: &str = "claude_strict";

/// True when the operator has armed the web hook on this machine.
pub fn strict_armed() -> bool {
    svipall_core::config::home_dir()
        .join(STRICT_MARKER)
        .exists()
}

/// A `PreToolUse` answer for `WebFetch` and `WebSearch`.
///
/// Returns an empty object — no opinion, normal permission flow — for everything else, including
/// an event this build does not understand. The denial is worth having only because it names the
/// tool that does the same job and carries the URL across, so the agent can act on it in one step
/// instead of asking the user what it was fetching.
pub fn claude_web(event: &Value, strict: bool) -> Value {
    if !strict {
        return json!({});
    }
    let Some(tool) = event.get("tool_name").and_then(Value::as_str) else {
        return json!({});
    };
    let input = event.get("tool_input");
    let reason = match tool {
        "WebFetch" => {
            let url = input
                .and_then(|i| i.get("url"))
                .and_then(Value::as_str)
                .unwrap_or("the same url");
            format!(
                "This machine runs svipall, which reads pages the built-in fetch cannot: it climbs \
                 a tier ladder past anti-bot walls, answers captchas locally, and reports a block \
                 as a block instead of summarising the challenge page as the article. Call \
                 `mcp__svipall__web_fetch` with url {url} instead (mode defaults to auto — do not \
                 set a tier by hand). For several urls use `mcp__svipall__web_fetch_many`, for a \
                 whole site `mcp__svipall__web_crawl`."
            )
        }
        "WebSearch" => format!(
            "This machine runs svipall, which searches without an API key and can then read any \
             result past its anti-bot wall. Call `mcp__svipall__web_search` instead{}.",
            input
                .and_then(|i| i.get("query"))
                .and_then(Value::as_str)
                .map(|q| format!(", with query {q:?}"))
                .unwrap_or_default()
        ),
        _ => return json!({}),
    };
    json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }
    })
}

/// Run one hook event end to end: read the harness's JSON from stdin, write the answer to stdout.
///
/// Unknown event names answer with an empty object rather than an error. A hook that fails is a
/// hook that interrupts somebody's turn to tell them about itself.
pub fn run(event: &str) -> anyhow::Result<Value> {
    let mut raw = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut raw)?;
    let parsed: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
    Ok(match event {
        "claude-web" => claude_web(&parsed, strict_armed()),
        _ => json!({}),
    })
}
