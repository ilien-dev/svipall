//! `json_path`: the part of a JSON answer the caller asked for, and nothing else.
//!
//! 911 of 1 552 fetches in one month wrote the body to `out_file` so a script could pull three
//! fields out of it. This is that script. The grammar is the small common core of JSONPath and
//! JMESPath, chosen so that whatever an agent already writes mostly just works:
//!
//! - `$` (optional) is the document; `.name` or `["name"]` a field; `[3]` an element (`[-1]` the
//!   last); `[1:5]` a slice; `[*]` or `.*` every element or value.
//! - After a slice or a wildcard the rest of the path runs on each element, and a field asked of an
//!   array is asked of each element too: `items.name` is every item's name.
//! - `{a, b.c}` picks fields into a new object keyed by what was written: `items[:3].{id, owner.login}`.
//!
//! A path that names nothing gives `null`, never an error: the field is absent, and saying so is
//! the answer. A path that cannot be parsed is an error, because then nothing was asked.

use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq)]
enum Seg {
    Key(String),
    Index(i64),
    Slice(Option<i64>, Option<i64>),
    All,
    Pick(Vec<(String, Vec<Seg>)>),
}

/// Apply `path` to `doc`.
pub fn select(doc: &Value, path: &str) -> Result<Value, String> {
    let segs = parse(path)?;
    Ok(eval(doc, &segs))
}

fn parse(path: &str) -> Result<Vec<Seg>, String> {
    let chars: Vec<char> = path.trim().chars().collect();
    let mut i = 0;
    if chars.first() == Some(&'$') {
        i = 1;
    }
    let segs = parse_segs(&chars, &mut i, false)?;
    if i < chars.len() {
        return Err(format!(
            "json_path: unexpected {:?} at position {i} of {path:?}",
            chars[i]
        ));
    }
    Ok(segs)
}

/// Segments until the end, or until a `,` / `}` when inside a pick.
fn parse_segs(c: &[char], i: &mut usize, in_pick: bool) -> Result<Vec<Seg>, String> {
    let mut out = Vec::new();
    let mut first = true;
    while *i < c.len() {
        skip_ws(c, i);
        let Some(&ch) = c.get(*i) else { break };
        match ch {
            ',' | '}' if in_pick => break,
            '.' => {
                *i += 1;
                match c.get(*i) {
                    Some('*') => {
                        *i += 1;
                        out.push(Seg::All);
                    }
                    Some('{') => out.push(parse_pick(c, i)?),
                    Some('[') => {}
                    _ => out.push(Seg::Key(ident(c, i)?)),
                }
            }
            '[' => out.push(parse_bracket(c, i)?),
            '{' => out.push(parse_pick(c, i)?),
            '*' if first => {
                *i += 1;
                out.push(Seg::All);
            }
            _ if first => out.push(Seg::Key(ident(c, i)?)),
            other => return Err(format!("json_path: unexpected {other:?} at position {i}")),
        }
        first = false;
    }
    Ok(out)
}

fn skip_ws(c: &[char], i: &mut usize) {
    while c.get(*i).is_some_and(|ch| ch.is_whitespace()) {
        *i += 1;
    }
}

fn ident(c: &[char], i: &mut usize) -> Result<String, String> {
    let start = *i;
    while c
        .get(*i)
        .is_some_and(|ch| !matches!(ch, '.' | '[' | ']' | '{' | '}' | ',' | ' '))
    {
        *i += 1;
    }
    if *i == start {
        return Err(format!(
            "json_path: a field name was expected at position {start}"
        ));
    }
    Ok(c[start..*i].iter().collect())
}

fn parse_bracket(c: &[char], i: &mut usize) -> Result<Seg, String> {
    let start = *i;
    *i += 1;
    let close = c[*i..]
        .iter()
        .position(|&ch| ch == ']')
        .map(|p| *i + p)
        .ok_or_else(|| format!("json_path: `[` at position {start} is never closed"))?;
    let inner: String = c[*i..close].iter().collect::<String>().trim().to_string();
    *i = close + 1;
    if inner == "*" || inner.is_empty() {
        return Ok(Seg::All);
    }
    if let Some(q) = inner
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| inner.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
    {
        return Ok(Seg::Key(q.to_string()));
    }
    let num = |s: &str| -> Result<Option<i64>, String> {
        let s = s.trim();
        if s.is_empty() {
            return Ok(None);
        }
        s.parse()
            .map(Some)
            .map_err(|_| format!("json_path: {s:?} is not an index"))
    };
    if let Some((a, b)) = inner.split_once(':') {
        return Ok(Seg::Slice(num(a)?, num(b)?));
    }
    match inner.parse::<i64>() {
        Ok(n) => Ok(Seg::Index(n)),
        // `[name]` unquoted: a field, as most people mean it.
        Err(_) => Ok(Seg::Key(inner)),
    }
}

fn parse_pick(c: &[char], i: &mut usize) -> Result<Seg, String> {
    let start = *i;
    *i += 1;
    let mut items = Vec::new();
    loop {
        skip_ws(c, i);
        let from = *i;
        let segs = parse_segs(c, i, true)?;
        let label: String = c[from..*i].iter().collect::<String>().trim().to_string();
        if !segs.is_empty() {
            items.push((label, segs));
        }
        match c.get(*i) {
            Some(',') => *i += 1,
            Some('}') => {
                *i += 1;
                break;
            }
            _ => {
                return Err(format!(
                    "json_path: `{{` at position {start} is never closed"
                ))
            }
        }
    }
    if items.is_empty() {
        return Err("json_path: `{}` picks nothing; name the fields, e.g. {id, name}".into());
    }
    Ok(Seg::Pick(items))
}

fn eval(v: &Value, segs: &[Seg]) -> Value {
    let Some((seg, rest)) = segs.split_first() else {
        return v.clone();
    };
    match seg {
        Seg::Key(k) => match v {
            Value::Object(m) => m.get(k).map(|x| eval(x, rest)).unwrap_or(Value::Null),
            Value::Array(items) => Value::Array(items.iter().map(|x| eval(x, segs)).collect()),
            _ => Value::Null,
        },
        Seg::Index(n) => match v {
            Value::Array(items) => {
                let len = items.len() as i64;
                let at = if *n < 0 { len + n } else { *n };
                if (0..len).contains(&at) {
                    eval(&items[at as usize], rest)
                } else {
                    Value::Null
                }
            }
            _ => Value::Null,
        },
        Seg::Slice(a, b) => match v {
            Value::Array(items) => {
                let len = items.len() as i64;
                let norm = |x: i64| if x < 0 { (len + x).max(0) } else { x.min(len) };
                let from = a.map(norm).unwrap_or(0);
                let to = b.map(norm).unwrap_or(len);
                Value::Array(
                    items
                        .iter()
                        .skip(from as usize)
                        .take((to - from).max(0) as usize)
                        .map(|x| eval(x, rest))
                        .collect(),
                )
            }
            _ => Value::Null,
        },
        Seg::All => match v {
            Value::Array(items) => Value::Array(items.iter().map(|x| eval(x, rest)).collect()),
            Value::Object(m) => Value::Array(m.values().map(|x| eval(x, rest)).collect()),
            _ => Value::Null,
        },
        Seg::Pick(items) => match v {
            Value::Array(list) => Value::Array(list.iter().map(|x| eval(x, segs)).collect()),
            Value::Object(_) => {
                let mut out = Map::new();
                for (label, path) in items {
                    out.insert(label.clone(), eval(v, path));
                }
                eval(&Value::Object(out), rest)
            }
            _ => Value::Null,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc() -> Value {
        json!({
            "total": 3,
            "items": [
                {"id": 1, "name": "a", "owner": {"login": "x"}},
                {"id": 2, "name": "b", "owner": {"login": "y"}},
                {"id": 3, "name": "c", "owner": {"login": "z"}},
            ],
            "odd key": {"v": true},
        })
    }

    #[test]
    fn fields_indexes_and_slices() {
        assert_eq!(select(&doc(), "$.total").unwrap(), json!(3));
        assert_eq!(select(&doc(), "total").unwrap(), json!(3));
        assert_eq!(select(&doc(), "items[0].name").unwrap(), json!("a"));
        assert_eq!(select(&doc(), "items[-1].id").unwrap(), json!(3));
        assert_eq!(select(&doc(), "items[1:].id").unwrap(), json!([2, 3]));
        assert_eq!(
            select(&doc(), "items[:2].owner.login").unwrap(),
            json!(["x", "y"])
        );
        assert_eq!(select(&doc(), "items[*].id").unwrap(), json!([1, 2, 3]));
        assert_eq!(
            select(&doc(), "items.name").unwrap(),
            json!(["a", "b", "c"])
        );
        assert_eq!(select(&doc(), "$[\"odd key\"].v").unwrap(), json!(true));
    }

    #[test]
    fn a_pick_builds_objects_keyed_by_what_was_written() {
        assert_eq!(
            select(&doc(), "items[:2].{id, owner.login}").unwrap(),
            json!([{"id": 1, "owner.login": "x"}, {"id": 2, "owner.login": "y"}])
        );
        assert_eq!(
            select(&doc(), "{total, items[0].name}").unwrap(),
            json!({"total": 3, "items[0].name": "a"})
        );
    }

    #[test]
    fn a_path_that_names_nothing_is_null_and_a_broken_one_is_an_error() {
        assert_eq!(select(&doc(), "items[9].id").unwrap(), Value::Null);
        assert_eq!(select(&doc(), "nope").unwrap(), Value::Null);
        assert!(select(&doc(), "items[0").is_err());
        assert!(select(&doc(), "{id").is_err());
    }
}
