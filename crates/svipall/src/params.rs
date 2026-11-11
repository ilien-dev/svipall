//! What a caller sent, made to fit what a tool declared, before serde ever sees it.
//!
//! Two failures from the field, both silent. A client that sends `max_tokens: "6000"` (27 calls in
//! one month) got a deserialisation error for a value whose meaning was never in doubt. And a client
//! that sent `max_token`, or a parameter another tool has, got the call carried out *without* it:
//! the limit it believed it had set was never applied, and nothing in the answer said so.
//!
//! So every tool's arguments pass through [`normalize`] against the tool's own JSON schema, on every
//! transport (MCP in `call_tool`, REST in `rest::tool`): a number or a boolean spelled as a string is
//! read as what it says, and a key the schema does not declare is named back and the call refused.
//! Schema-driven rather than a serde attribute per field, so a parameter added tomorrow is covered
//! without anybody remembering to opt it in.

use serde_json::{Map, Value};

/// Coerce `args` in place to the types `schema` declares, and return the top-level keys the schema
/// does not declare.
///
/// Only top-level keys are judged unknown: below them sit free-form maps (`headers`, `configure`,
/// a `schema` spec) whose keys are the caller's own.
pub fn normalize(schema: &Map<String, Value>, args: &mut Map<String, Value>) -> Vec<String> {
    let props = schema.get("properties").and_then(Value::as_object);
    let mut unknown = Vec::new();
    for (key, value) in args.iter_mut() {
        match props.and_then(|p| p.get(key)) {
            Some(prop) => coerce(schema, prop, value, 0),
            None => unknown.push(key.clone()),
        }
    }
    unknown
}

/// The declared parameter names of a tool schema.
pub fn declared(schema: &Map<String, Value>) -> Vec<String> {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|p| p.keys().cloned().collect())
        .unwrap_or_default()
}

/// What a caller is told when it sent a key the tool does not have. Says that nothing ran, names
/// the closest real parameter when there is one, and lists the rest so the next call is right.
pub fn unknown_message(tool: &str, unknown: &[String], known: &[String]) -> String {
    let mut parts = Vec::new();
    for k in unknown {
        match closest(k, known) {
            Some(near) => parts.push(format!("`{k}` (did you mean `{near}`?)")),
            None => parts.push(format!("`{k}`")),
        }
    }
    format!(
        "{tool} has no parameter {}; nothing was run, so no limit you meant to set was applied. \
         Its parameters are: {}.",
        parts.join(", "),
        known.join(", ")
    )
}

/// The declared name nearest to `k`: a case or separator slip, a prefix, or one or two edits away.
fn closest<'a>(k: &str, known: &'a [String]) -> Option<&'a str> {
    let squash = |s: &str| s.to_ascii_lowercase().replace(['-', '_'], "");
    let want = squash(k);
    known
        .iter()
        .map(|n| {
            let have = squash(n);
            let d = if have == want {
                0
            } else if have.starts_with(&want) || want.starts_with(&have) {
                1
            } else {
                edits(&want, &have)
            };
            (d, n.as_str())
        })
        .filter(|(d, _)| *d <= 2)
        .min_by_key(|(d, _)| *d)
        .map(|(_, n)| n)
}

fn edits(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != *cb);
            cur.push(sub.min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// The types a property schema allows, following a `$ref` and the members of an `anyOf`/`oneOf`.
fn types_of<'a>(root: &'a Map<String, Value>, prop: &'a Value, out: &mut Vec<&'a str>, depth: u8) {
    if depth > 8 {
        return;
    }
    let Some(obj) = prop.as_object() else { return };
    if let Some(target) = resolve(root, obj) {
        return types_of(root, target, out, depth + 1);
    }
    match obj.get("type") {
        Some(Value::String(t)) => out.push(t),
        Some(Value::Array(ts)) => out.extend(ts.iter().filter_map(Value::as_str)),
        _ => {}
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(members) = obj.get(key).and_then(Value::as_array) {
            for m in members {
                types_of(root, m, out, depth + 1);
            }
        }
    }
}

/// `{"$ref": "#/definitions/X"}` or `#/$defs/X`, looked up in the root schema.
fn resolve<'a>(root: &'a Map<String, Value>, obj: &Map<String, Value>) -> Option<&'a Value> {
    let r = obj.get("$ref")?.as_str()?;
    let mut parts = r.strip_prefix("#/")?.splitn(2, '/');
    let (table, name) = (parts.next()?, parts.next()?);
    root.get(table)?.get(name)
}

/// Which part of a schema [`child`] looks for.
#[derive(Clone, Copy)]
enum Part<'k> {
    Items,
    Property(&'k str),
}

/// The schema of an array's items, or of an object's property, wherever it is declared.
fn child<'a>(
    root: &'a Map<String, Value>,
    prop: &'a Value,
    want: Part<'_>,
    depth: u8,
) -> Option<&'a Value> {
    if depth > 8 {
        return None;
    }
    let obj = prop.as_object()?;
    if let Some(target) = resolve(root, obj) {
        return child(root, target, want, depth + 1);
    }
    let found = match want {
        Part::Items => obj.get("items"),
        Part::Property(k) => obj.get("properties").and_then(|p| p.get(k)),
    };
    if found.is_some() {
        return found;
    }
    for key in ["anyOf", "oneOf"] {
        for m in obj.get(key).and_then(Value::as_array).into_iter().flatten() {
            if let Some(found) = child(root, m, want, depth + 1) {
                return Some(found);
            }
        }
    }
    None
}

fn coerce(root: &Map<String, Value>, prop: &Value, value: &mut Value, depth: u8) {
    if depth > 8 || value.is_null() {
        return;
    }
    let mut types = Vec::new();
    types_of(root, prop, &mut types, 0);
    if types.is_empty() {
        // `serde_json::Value` fields (`configure`, `clear_cache`) declare nothing: anything goes.
        return;
    }
    let allows = |t: &str| types.contains(&t);
    // Already the right shape: leave it, apart from what is inside it.
    let fits = match value {
        Value::Bool(_) => allows("boolean"),
        Value::Number(n) => allows("number") || (allows("integer") && is_integral(n)),
        Value::String(_) => allows("string"),
        Value::Array(_) => allows("array"),
        Value::Object(_) => allows("object"),
        Value::Null => true,
    };
    if !fits {
        if let Some(v) = convert(value, &types) {
            *value = v;
        }
    }
    match value {
        Value::Array(items) => {
            if let Some(item) = child(root, prop, Part::Items, 0) {
                for v in items {
                    coerce(root, item, v, depth + 1);
                }
            }
        }
        Value::Object(fields) => {
            for (k, v) in fields.iter_mut() {
                if let Some(p) = child(root, prop, Part::Property(k), 0) {
                    coerce(root, p, v, depth + 1);
                }
            }
        }
        _ => {}
    }
}

fn is_integral(n: &serde_json::Number) -> bool {
    n.is_i64() || n.is_u64()
}

/// The value read as one of `types`, when it can be without guessing.
fn convert(value: &Value, types: &[&str]) -> Option<Value> {
    let allows = |t: &str| types.contains(&t);
    match value {
        Value::String(s) => {
            let t = s.trim();
            if allows("integer") {
                if let Ok(n) = t.parse::<i64>() {
                    return Some(Value::from(n));
                }
                if let Ok(n) = t.parse::<u64>() {
                    return Some(Value::from(n));
                }
                if let Ok(f) = t.parse::<f64>() {
                    if f.fract() == 0.0 && f.is_finite() && f.abs() < 9.0e15 {
                        return Some(Value::from(f as i64));
                    }
                }
            }
            if allows("number") {
                if let Ok(f) = t.parse::<f64>() {
                    if f.is_finite() {
                        return serde_json::Number::from_f64(f).map(Value::Number);
                    }
                }
            }
            if allows("boolean") {
                match t.to_ascii_lowercase().as_str() {
                    "true" | "yes" | "1" | "on" => return Some(Value::Bool(true)),
                    "false" | "no" | "0" | "off" => return Some(Value::Bool(false)),
                    _ => {}
                }
            }
            // A list or an object sent as its JSON text, which some clients do for every
            // non-scalar argument.
            if (allows("array") && t.starts_with('[')) || (allows("object") && t.starts_with('{'))
            {
                if let Ok(v) = serde_json::from_str::<Value>(t) {
                    if (v.is_array() && allows("array")) || (v.is_object() && allows("object")) {
                        return Some(v);
                    }
                }
            }
            None
        }
        Value::Number(n) => {
            if allows("integer") {
                if let Some(f) = n.as_f64() {
                    if f.fract() == 0.0 && f.abs() < 9.0e15 {
                        return Some(Value::from(f as i64));
                    }
                }
            }
            if allows("boolean") {
                match n.as_f64() {
                    Some(x) if x == 1.0 => return Some(Value::Bool(true)),
                    Some(x) if x == 0.0 => return Some(Value::Bool(false)),
                    _ => {}
                }
            }
            // `scroll: 5` where the parameter is the string "5".
            allows("string").then(|| Value::String(n.to_string()))
        }
        Value::Bool(b) => allows("string").then(|| Value::String(b.to_string())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> Map<String, Value> {
        json!({
            "type": "object",
            "properties": {
                "url": {"type": "string"},
                "max_tokens": {"type": ["integer", "null"]},
                "timeout": {"type": "integer", "nullable": true},
                "ratio": {"type": "number"},
                "tables": {"type": ["boolean", "null"]},
                "scroll": {"type": ["string", "null"]},
                "urls": {"type": "array", "items": {"type": "string"}},
                "schema": {"anyOf": [{"type": "string"}, {"type": "object"}]},
                "configure": true,
                "actions": {"type": "array", "items": {"$ref": "#/definitions/Action"}},
            },
            "definitions": {
                "Action": {"type": "object", "properties": {"ms": {"type": ["integer", "null"]}}}
            }
        })
        .as_object()
        .cloned()
        .unwrap()
    }

    fn run(args: Value) -> (Value, Vec<String>) {
        let mut a = args.as_object().cloned().unwrap();
        let unknown = normalize(&schema(), &mut a);
        (Value::Object(a), unknown)
    }

    #[test]
    fn numbers_and_booleans_sent_as_strings_are_read_as_what_they_say() {
        let (v, unknown) = run(json!({
            "max_tokens": "6000", "timeout": " 30000 ", "ratio": "0.5", "tables": "true",
            "scroll": 5, "actions": [{"ms": "250"}], "urls": "[\"a\",\"b\"]",
        }));
        assert!(unknown.is_empty());
        assert_eq!(v["max_tokens"], 6000);
        assert_eq!(v["timeout"], 30000);
        assert_eq!(v["ratio"], 0.5);
        assert_eq!(v["tables"], true);
        assert_eq!(v["scroll"], "5");
        assert_eq!(v["actions"][0]["ms"], 250);
        assert_eq!(v["urls"], json!(["a", "b"]));
    }

    #[test]
    fn what_cannot_be_read_without_guessing_is_left_for_serde_to_refuse() {
        let (v, _) = run(json!({"max_tokens": "lots", "tables": "maybe", "schema": "auto"}));
        assert_eq!(v["max_tokens"], "lots");
        assert_eq!(v["tables"], "maybe");
        assert_eq!(v["schema"], "auto");
    }

    #[test]
    fn free_form_values_are_never_touched() {
        let (v, unknown) = run(json!({"configure": {"anything": "1"}, "schema": {"x": "2"}}));
        assert!(unknown.is_empty());
        assert_eq!(v["configure"]["anything"], "1");
        assert_eq!(v["schema"]["x"], "2");
    }

    #[test]
    fn an_undeclared_key_is_named_with_the_nearest_real_one() {
        let (_, mut unknown) = run(json!({"url": "x", "max_token": 5, "frobnicate": 1}));
        unknown.sort();
        assert_eq!(unknown, vec!["frobnicate".to_string(), "max_token".to_string()]);
        let known = declared(&schema());
        let msg = unknown_message("web_fetch", &unknown, &known);
        assert!(msg.contains("`max_token` (did you mean `max_tokens`?)"), "{msg}");
        assert!(msg.contains("`frobnicate`") && !msg.contains("`frobnicate` (did"), "{msg}");
        assert!(msg.contains("nothing was run"), "{msg}");
    }
}
