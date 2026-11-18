//! `docs/reference/` is written from the code, never by hand.
//!
//! The tool list a model reads (`surface`) and the routes the REST API answers (`rest::ROUTES`)
//! are the sources of truth. These tests render the reference pages from them and fail when the
//! committed pages differ, so a change to a tool, an argument or a route cannot land without the
//! docs moving with it. To rewrite the pages after such a change:
//!
//! ```text
//! SVIPALL_BLESS_DOCS=1 cargo test -p svipall --test reference_docs
//! ```
mod support;

use serde_json::{Map, Value};
use std::path::PathBuf;
use svipall::server::SvipallServer;
use svipall::{rest, surface};

const BLESS: &str = "SVIPALL_BLESS_DOCS";

fn reference_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference")
}

fn server() -> SvipallServer {
    support::isolate();
    SvipallServer::new(None, svipall_core::Config::default(), None)
}

/// A table cell: one line, pipes escaped.
fn cell(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('|', "\\|")
}

fn type_of(p: &Value) -> String {
    match p.get("type") {
        Some(Value::String(t)) if t == "array" => {
            let inner = p
                .get("items")
                .and_then(|i| i.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("any");
            format!("array of {inner}")
        }
        Some(Value::String(t)) => t.clone(),
        Some(Value::Array(ts)) => ts
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" or "),
        _ => "any".into(),
    }
}

/// The parameter table of a JSON schema, in the order the schema lists them.
fn params_table(schema: &Map<String, Value>) -> String {
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let Some(Value::Object(props)) = schema.get("properties") else {
        return "No parameters.\n".into();
    };
    if props.is_empty() {
        return "No parameters.\n".into();
    }
    let mut out =
        String::from("| Parameter | Type | Required | What it does |\n|---|---|---|---|\n");
    for (name, p) in props {
        let doc = p.get("description").and_then(Value::as_str).unwrap_or("");
        let req = if required.contains(&name.as_str()) {
            "yes"
        } else {
            ""
        };
        out.push_str(&format!(
            "| `{name}` | {} | {req} | {} |\n",
            type_of(p),
            cell(doc)
        ));
    }
    out
}

fn render_tools() -> String {
    let tools = surface::tools();
    let mut out = String::from(
        "# MCP tools\n\n\
         The tools an MCP client lists, with every parameter, exactly as the server describes \
         them. This page is generated from the code by `crates/svipall/tests/reference_docs.rs`; \
         edit the tool definitions, not this file.\n\n",
    );
    out.push_str(&format!("There are {} tools.\n\n", tools.len()));
    for t in &tools {
        out.push_str(&format!("## `{}`\n\n", t.name));
        let ann = t.annotations.as_ref();
        let read_only = ann.and_then(|a| a.read_only_hint).unwrap_or(false);
        let open_world = ann.and_then(|a| a.open_world_hint).unwrap_or(false);
        out.push_str(&format!(
            "{}\n\n",
            t.description
                .as_deref()
                .unwrap_or("")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        ));
        out.push_str(&format!(
            "Read-only: {}. Reaches the network: {}.\n\n",
            if read_only { "yes" } else { "no" },
            if open_world { "yes" } else { "no" }
        ));
        out.push_str(&params_table(&t.input_schema));
        out.push('\n');
    }
    out
}

/// The operation a route runs: `/v1/fetch` is `web_fetch`, `/v1/solve_and_continue` is itself.
fn operation_for(path: &str, names: &[String]) -> String {
    let bare = path.trim_start_matches("/v1/");
    let prefixed = format!("web_{bare}");
    if names.contains(&prefixed) {
        prefixed
    } else {
        bare.to_string()
    }
}

fn render_rest() -> String {
    let server = server();
    let names = server.tool_names();
    let mut out = String::from(
        "# REST routes\n\n\
         Every route `svipall serve` answers, with the body it takes. This page is generated from \
         the code by `crates/svipall/tests/reference_docs.rs`; edit the routes, not this file. \
         How to start the server and authenticate is in [the REST API guide](../rest.md).\n\n",
    );
    out.push_str(&format!(
        "There are {} operation routes. Each takes `POST` with a JSON body.\n\n",
        rest::ROUTES.len()
    ));
    for path in rest::ROUTES {
        let op = operation_for(path, &names);
        out.push_str(&format!("## `POST {path}`\n\n"));
        let doc = server.operation_description(&op).unwrap_or_default();
        if !doc.is_empty() {
            out.push_str(&format!(
                "{}\n\n",
                doc.split_whitespace().collect::<Vec<_>>().join(" ")
            ));
        }
        out.push_str(&format!("Runs the `{op}` operation.\n\n"));
        match server.operation_schema(&op) {
            Some(schema) => out.push_str(&params_table(&schema)),
            None => out.push_str("No parameters.\n"),
        }
        out.push('\n');
    }
    out.push_str("## Job routes\n\nHow a caller follows a long operation without holding a connection open.\n\n");
    for path in rest::JOB_ROUTES {
        out.push_str(&format!("- `{path}`\n"));
    }
    out.push_str("\n## Operations that are not routes\n\n| Operation | Why |\n|---|---|\n");
    for (name, why) in rest::NOT_IN_REST {
        out.push_str(&format!("| `{name}` | {} |\n", cell(why)));
    }
    out
}

fn render_index() -> String {
    String::from(
        "# Reference\n\n\
         Generated from the code on every change, so it always matches the release it ships with.\n\n\
         ## Reference\n\n\
         - [MCP tools](tools.md): every tool an MCP client lists, with each parameter.\n\
         - [REST routes](rest.md): every route `svipall serve` answers, with the body it takes.\n",
    )
}

/// Compare a rendered page with the committed one, or rewrite it under `SVIPALL_BLESS_DOCS=1`.
fn check(file: &str, rendered: &str) {
    let path = reference_dir().join(file);
    if std::env::var_os(BLESS).is_some() {
        std::fs::create_dir_all(reference_dir()).expect("create docs/reference");
        std::fs::write(&path, rendered).expect("write reference page");
        return;
    }
    let committed = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| {
            panic!(
                "{} is missing; run {BLESS}=1 cargo test -p svipall --test reference_docs",
                path.display()
            )
        })
        .replace("\r\n", "\n");
    assert!(
        committed == rendered,
        "docs/reference/{file} is out of date with the code; run {BLESS}=1 cargo test -p svipall --test reference_docs"
    );
}

#[test]
fn the_tool_reference_matches_the_tool_list() {
    check("tools.md", &render_tools());
}

#[test]
fn the_rest_reference_matches_the_routes() {
    check("rest.md", &render_rest());
}

#[test]
fn the_reference_index_lists_every_page() {
    check("README.md", &render_index());
}

#[test]
fn every_listed_tool_appears_in_the_tool_reference() {
    let page = render_tools();
    for t in surface::tools() {
        assert!(
            page.contains(&format!("## `{}`", t.name)),
            "{} missing",
            t.name
        );
    }
}

#[test]
fn every_route_names_an_operation_the_server_has() {
    let server = server();
    let names = server.tool_names();
    for path in rest::ROUTES {
        let op = operation_for(path, &names);
        assert!(
            names.contains(&op),
            "{path} maps to `{op}`, which the server does not have"
        );
        assert!(
            server.operation_schema(&op).is_some(),
            "`{op}` has no schema to document"
        );
    }
}
