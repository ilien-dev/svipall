//! What the model reads before it chooses a tool.
//!
//! Claude Code shows only the tool *names* and the server `instructions` at session start, loads a
//! description and its schema when the tool is picked, and truncates descriptions and instructions
//! at 2 KB. So: the instructions route a task to a name, every description opens with what the
//! tool is for and names the sibling to prefer, and the schema carries no boilerplate the model has
//! to read past. These tests hold that shape; the numbers are budgets, measured on the built list.
mod support;

use rmcp::{model::Tool, ServerHandler};
use serde_json::Value;
use svipall::server::SvipallServer;

fn server() -> SvipallServer {
    support::isolate();
    SvipallServer::new(None, svipall_core::Config::default(), None)
}

fn tools() -> Vec<Tool> {
    server().tools()
}

fn instructions() -> String {
    server().get_info().instructions.expect("instructions")
}

fn description(t: &Tool) -> &str {
    t.description.as_deref().unwrap_or("")
}

fn schema_len(t: &Tool) -> usize {
    serde_json::to_string(&*t.input_schema).expect("json").len()
}

/// Every `(key, value)` pair in a JSON tree, with the path that reaches it.
fn walk<'a>(v: &'a Value, path: String, out: &mut Vec<(String, &'a str, &'a Value)>) {
    if let Value::Object(m) = v {
        for (k, child) in m {
            out.push((path.clone(), k.as_str(), child));
            walk(child, format!("{path}/{k}"), out);
        }
    }
}

#[test]
fn every_tool_has_a_description_that_fits_the_client_limit() {
    for t in tools() {
        let d = description(&t);
        assert!(
            d.len() >= 60,
            "{}: description is too thin to route on: {d:?}",
            t.name
        );
        assert!(
            d.len() <= 900,
            "{}: description is {} chars; Claude Code truncates at 2 KB and the routing sentence \
             has to come first, not last",
            t.name,
            d.len()
        );
        assert!(
            !d.contains("until now") && !d.contains("this tool asks"),
            "{}: narrative, not guidance: {d:?}",
            t.name
        );
    }
}

#[test]
fn the_whole_tool_list_fits_a_budget() {
    // With tool search off (a gateway, `ENABLE_TOOL_SEARCH=false`, `alwaysLoad`) every definition
    // sits in the system prompt on every turn. Measured before this test existed: 36 176 chars,
    // about 9 000 tokens, a third of it schema boilerplate. The rewrite took that out (-6 500) and
    // spent it on descriptions that say when to use each tool (+4 400), a typed action schema
    // for web_act and browser_do (+2 900) and `schema`/`tables` on web_fetch_many and web_crawl
    // (+1 300), because a wrong tool or an invented field costs more than a sentence. The cap is
    // the measured result plus room for one tool.
    let total: usize = tools()
        .iter()
        .map(|t| t.name.len() + description(t).len() + schema_len(t))
        .sum();
    assert!(
        total <= 38_000,
        "the tool list is {total} chars; the model reads all of it on every request"
    );
}

#[test]
fn schemas_carry_no_boilerplate() {
    // schemars emits draft-07 furniture no client validates: `$schema`, `title`, `"default": null`,
    // `nullable`, integer `format` and `minimum: 0`. Roughly a third of the list, and every byte
    // of it is read past by the model.
    for t in tools() {
        let schema = Value::Object((*t.input_schema).clone());
        let mut pairs = Vec::new();
        walk(&schema, String::new(), &mut pairs);
        for (path, key, value) in pairs {
            let bad = match key {
                "$schema" | "title" | "nullable" | "format" => true,
                "default" => value.is_null(),
                "minimum" => value == &Value::from(0),
                _ => false,
            };
            assert!(!bad, "{}: {path}/{key} = {value} is boilerplate", t.name);
        }
    }
}

#[test]
fn every_parameter_says_what_it_is_for() {
    // A property with a type and no description is a guess for the model. `mode`, `extraction`
    // and `max_tier` were exactly that on three tools.
    for t in tools() {
        let props = t.input_schema.get("properties").and_then(Value::as_object);
        let Some(props) = props else { continue };
        for (name, prop) in props {
            let d = prop
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("");
            assert!(
                d.len() >= 8,
                "{}.{name}: no description ({})",
                t.name,
                serde_json::to_string(prop).expect("json")
            );
            assert!(
                d.len() <= 700,
                "{}.{name}: description is {} chars; measurement notes belong in code comments, \
                 not in what the model reads on every call",
                t.name,
                d.len()
            );
        }
    }
}

#[test]
fn each_description_names_the_tool_to_prefer_or_the_field_that_matters() {
    // The routing content: which sibling to reach for instead, and the parameter that changes the
    // cost. Missing any of these was the finding that started the rewrite.
    let must_mention: &[(&str, &[&str])] = &[
        (
            "web_fetch",
            &["query", "schema", "out_file", "cursor", "blocked_reason"],
        ),
        ("web_fetch_many", &["web_fetch", "schema", "tables"]),
        ("web_crawl", &["web_map", "out_file", "crawl_id", "schema"]),
        ("web_map", &["web_crawl"]),
        ("web_snapshot", &["web_act", "ref"]),
        ("web_act", &["web_snapshot", "browser_open"]),
        ("browser_open", &["browser_do", "web_act"]),
        ("browser_do", &["browser_open", "web_act"]),
        ("browser_close", &["browser_open"]),
        ("web_screenshot", &["web_snapshot"]),
        ("web_capture", &["pattern", "bodies"]),
        ("web_search", &["web_site_search"]),
        ("web_site_search", &["web_search"]),
        ("web_diff", &["web_watch"]),
        ("web_watch", &["web_diff"]),
        ("web_status", &["web_log"]),
        ("web_log", &["web_status"]),
        ("web_login", &["blocked_reason", "profile"]),
        ("web_route", &["web_status"]),
        ("web_profile", &["web_login", "export", "import"]),
        ("web_notes", &["get", "set", "list", "delete"]),
        ("browser_setup", &["install"]),
        ("solve_and_continue", &["solve_turnstile"]),
        ("solve_turnstile", &["solve_and_continue"]),
        ("solve_recaptcha_v2", &["solve_and_continue"]),
        ("solve_hcaptcha", &["solve_and_continue"]),
        ("solve_image_captcha", &["solve_and_continue"]),
        ("captcha_status", &["taskId"]),
        ("report_captcha", &["taskId"]),
    ];
    let list = tools();
    for (name, needles) in must_mention {
        let t = list
            .iter()
            .find(|t| t.name == *name)
            .unwrap_or_else(|| panic!("{name} is not a tool"));
        for needle in *needles {
            assert!(
                description(t).contains(needle),
                "{name}: description does not mention {needle:?}"
            );
        }
    }
    let covered: Vec<&str> = must_mention.iter().map(|(n, _)| *n).collect();
    for t in &list {
        assert!(
            covered.contains(&t.name.as_ref()),
            "{} has no routing expectation",
            t.name
        );
    }
}

#[test]
fn descriptions_name_no_vendor() {
    // The project rule: name protocols by endpoint, never a brand. Widget names that are also
    // tool names (turnstile, recaptcha, hcaptcha) are the protocol.
    for t in tools() {
        let d = description(&t).to_ascii_lowercase();
        for brand in ["duckduckgo", "bing", "brave", "cloudflare", "google"] {
            assert!(!d.contains(brand), "{}: names {brand}", t.name);
        }
    }
}

#[test]
fn the_instructions_route_every_task_to_a_name() {
    // The one piece of prose the model sees before choosing. Claude Code truncates it at 2 KB, so
    // the budget is the limit minus a margin, and every family of tools has to be reachable from it.
    let text = instructions();
    assert!(text.len() <= 1_900, "instructions are {} chars", text.len());
    let names: Vec<String> = tools().iter().map(|t| t.name.to_string()).collect();
    let reachable = [
        "web_fetch",
        "web_snapshot",
        "web_act",
        "web_capture",
        "web_crawl",
        "web_map",
        "web_search",
        "web_site_search",
        "web_login",
        "web_route",
        "solve_and_continue",
        "web_status",
        "web_notes",
        "web_watch",
        "browser_open",
        "web_screenshot",
    ];
    for name in reachable {
        assert!(names.iter().any(|n| n == name), "{name} is not a tool");
        assert!(text.contains(name), "instructions never mention {name}");
    }
    assert!(
        text.contains("blocked_reason") && text.contains("Never retry"),
        "the two rules that cost cooldowns are missing"
    );
    // These instructions sit in the system prompt for the whole session, so a sentence about
    // language is read as a directive about the *answer*. It used to end "Instructions in
    // English.", which is a note about the project's own source, not an instruction to anybody —
    // and what language a user is answered in was never svipall's call.
    assert!(
        !text.to_ascii_lowercase().contains("in english"),
        "the instructions tell the model what language to work in: {text}"
    );
}

#[test]
fn schemas_carry_no_root_description_and_no_defs() {
    // The struct's doc comment lands as a root `description` under the tool's own description,
    // where it is read twice at best and, on `WebStatusParams`, was a note to maintainers. Nested
    // types must be inlined: a `$ref` sends the model to a `$defs` table it has to page back to.
    for t in tools() {
        assert!(
            t.input_schema.get("description").is_none(),
            "{}: root description leaks the struct doc",
            t.name
        );
        for key in ["$defs", "definitions"] {
            assert!(t.input_schema.get(key).is_none(), "{}: has {key}", t.name);
        }
    }
}

#[test]
fn actions_are_typed_for_web_act_and_browser_do() {
    // `items: true` left every action's shape to prose. The enum is what stops an invented verb.
    let kinds = [
        "click",
        "type",
        "fill",
        "press",
        "hover",
        "select",
        "scroll",
        "wait",
        "eval",
        "goto",
        "verify",
        "console",
        "screenshot",
        "hold",
    ];
    for name in ["web_act", "browser_do"] {
        let list = tools();
        let t = list.iter().find(|t| t.name == name).expect(name);
        let items = &t.input_schema["properties"]["actions"]["items"];
        assert_eq!(
            items["type"], "object",
            "{name}: actions items are untyped: {items}"
        );
        assert_eq!(
            items["required"],
            serde_json::json!(["do"]),
            "{name}: {items}"
        );
        let found = items["properties"]["do"]["enum"]
            .as_array()
            .unwrap_or_else(|| panic!("{name}: `do` is not an enum: {items}"));
        for kind in kinds {
            assert!(
                found.iter().any(|v| v == kind),
                "{name}: `do` enum lacks {kind}"
            );
        }
        for field in [
            "ref", "selector", "text", "key", "value", "ms", "pixels", "until", "script", "url",
        ] {
            let prop = &items["properties"][field];
            assert!(prop.is_object(), "{name}: action field {field} missing");
            assert!(
                prop["description"].as_str().is_some_and(|d| d.len() >= 8),
                "{name}: action field {field} has no description"
            );
        }
    }
}

#[test]
fn web_fetch_schema_declares_both_shapes() {
    // "auto" or an object. Without a declared type the model had to infer that from prose.
    let list = tools();
    let t = list
        .iter()
        .find(|t| t.name == "web_fetch")
        .expect("web_fetch");
    let prop = &t.input_schema["properties"]["schema"];
    let shapes = prop["anyOf"]
        .as_array()
        .unwrap_or_else(|| panic!("schema has no anyOf: {prop}"));
    let types: Vec<&str> = shapes.iter().filter_map(|s| s["type"].as_str()).collect();
    assert!(
        types.contains(&"string") && types.contains(&"object"),
        "{prop}"
    );
}

#[test]
fn web_screenshot_takes_mobile_like_web_fetch() {
    // Two blind routing runs asked for a phone-sized screenshot; the parameter existed on
    // web_fetch alone.
    let list = tools();
    let t = list
        .iter()
        .find(|t| t.name == "web_screenshot")
        .expect("web_screenshot");
    let prop = &t.input_schema["properties"]["mobile"];
    assert_eq!(
        prop["type"],
        "boolean",
        "{}",
        Value::Object((*t.input_schema).clone())
    );
}
