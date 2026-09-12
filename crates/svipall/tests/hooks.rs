//! The Claude Code hook, which is the one piece of this that can annoy someone every day.
//!
//! It answers a `PreToolUse` event on `WebFetch` and `WebSearch` by declining and naming the
//! svipall tool that does the same job better. Two properties matter more than the wording: it is
//! silent unless the operator turned it on, and it never denies anything it was not asked about.

use serde_json::json;
use svipall::hooks;

fn event(tool: &str) -> serde_json::Value {
    json!({
        "hook_event_name": "PreToolUse",
        "tool_name": tool,
        "tool_input": { "url": "https://example.com/article" },
    })
}

#[test]
fn it_does_nothing_at_all_until_the_operator_turns_it_on() {
    // Installing the plugin registers this hook. Installing the plugin must not change how anyone's
    // WebFetch behaves — that is a decision, and a decision is a file the operator creates.
    let out = hooks::claude_web(&event("WebFetch"), false);
    assert_eq!(out, json!({}), "an unarmed hook has no opinion");
}

#[test]
fn armed_it_declines_webfetch_and_names_the_tool_that_replaces_it() {
    let out = hooks::claude_web(&event("WebFetch"), true);
    let d = &out["hookSpecificOutput"];
    assert_eq!(d["hookEventName"], json!("PreToolUse"));
    assert_eq!(d["permissionDecision"], json!("deny"));
    let reason = d["permissionDecisionReason"].as_str().unwrap_or_default();
    assert!(
        reason.contains("web_fetch"),
        "a denial that does not name the alternative is just an obstacle: {reason:?}"
    );
}

#[test]
fn armed_it_declines_websearch_and_names_the_search_tool() {
    let out = hooks::claude_web(&event("WebSearch"), true);
    let reason = out["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap_or_default();
    assert!(reason.contains("web_search"), "{reason:?}");
}

#[test]
fn the_url_the_caller_asked_for_survives_into_the_suggestion() {
    // The agent has to be able to act on the reason without going back to the user for the URL.
    let out = hooks::claude_web(&event("WebFetch"), true);
    let reason = out["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap_or_default();
    assert!(reason.contains("https://example.com/article"), "{reason:?}");
}

#[test]
fn the_denial_names_a_tool_that_exists_under_either_registration() {
    // The MCP prefix is not svipall's to choose. Registered by hand with
    // `claude mcp add -s user svipall` the tools arrive as `mcp__svipall__web_fetch`; installed as
    // the plugin — the path this repository ships and documents — the same server's tools arrive
    // as `mcp__plugin_svipall_svipall__web_fetch`. A hardcoded prefix is therefore wrong for one
    // of the two installs, and it is wrong at the worst moment: the harness has just refused the
    // call, and this sentence is the only thing the agent has to act on. The bare tool name
    // resolves under both.
    for tool in ["WebFetch", "WebSearch"] {
        let out = hooks::claude_web(&event(tool), true);
        let reason = out["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap_or_default();
        assert!(
            !reason.contains("mcp__"),
            "{tool}: the reason hardcodes an MCP prefix svipall does not control: {reason:?}"
        );
        let names = if tool == "WebFetch" {
            "web_fetch"
        } else {
            "web_search"
        };
        assert!(
            reason.contains(names),
            "{tool}: the reason never names {names}: {reason:?}"
        );
        assert!(
            reason.contains("svipall"),
            "{tool}: the reason never says whose tool that is: {reason:?}"
        );
    }
}

#[test]
fn it_never_denies_a_tool_it_was_not_asked_about() {
    // A hook matcher is a regular expression someone can widen by accident. Denying Read or Bash
    // because the matcher slipped would be indistinguishable from svipall being broken.
    for tool in [
        "Read",
        "Bash",
        "Edit",
        "Grep",
        "mcp__svipall__web_fetch",
        "Task",
    ] {
        assert_eq!(
            hooks::claude_web(&event(tool), true),
            json!({}),
            "{tool} was denied and should not have been"
        );
    }
}

#[test]
fn a_malformed_event_is_a_no_op_rather_than_a_denial() {
    // Whatever Claude Code sends next, the safe failure is to stay out of the way.
    for bad in [
        json!({}),
        json!({ "tool_name": 7 }),
        json!("nonsense"),
        json!(null),
    ] {
        assert_eq!(hooks::claude_web(&bad, true), json!({}), "{bad:?}");
    }
}
