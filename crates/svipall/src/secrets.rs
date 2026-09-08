//! Credentials the model never sees.
//!
//! Logging in through `web_act` means putting a password in a tool call. That call is written to
//! the model's context, to the transcript, and to whatever logs sit around either — so a password
//! typed once through this server is a password stored in several places nobody thought about.
//!
//! Instead the operator writes them to `~/.svipall/secrets.env`, and the model refers to them by name:
//!
//! ```text
//! {"do": "type", "ref": "e4", "text": "${SHOP_PASSWORD}"}
//! ```
//!
//! The substitution happens here, on the way to the browser. The model asks for a name it can see
//! in `web_status`; the value never travels back the other way.
//!
//! This is not a secret store and does not pretend to be one. The file sits on disk in plain text,
//! readable by anything running as this user. What it buys is that the value stays out of the
//! model's context and out of the conversation, which is where it would otherwise leak.

use std::collections::BTreeMap;
use std::path::PathBuf;

pub fn secrets_path() -> PathBuf {
    svipall_core::config::home_dir().join("secrets.env")
}

/// Parse a dotenv-shaped file. Blank lines and `#` comments are skipped; quotes around a value are
/// stripped; anything else malformed is skipped rather than rejected, because one bad line should
/// not take a login down.
pub fn parse(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim();
        if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let v = v.trim();
        let v = v
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .or_else(|| v.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
            .unwrap_or(v);
        out.insert(k.to_string(), v.to_string());
    }
    out
}

/// Load whatever is on disk. A missing file is not an error: most installations have no secrets.
pub fn load() -> BTreeMap<String, String> {
    std::fs::read_to_string(secrets_path())
        .map(|t| parse(&t))
        .unwrap_or_default()
}

/// Replace every `${NAME}` with its value.
///
/// An unknown name is left exactly as written rather than blanked. Substituting an empty string
/// would submit a form with an empty password and report success, and the caller would be left
/// looking at a login page wondering why.
pub fn expand(text: &str, secrets: &BTreeMap<String, String>) -> String {
    if !text.contains("${") {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find('}') {
            Some(end) => {
                let name = &after[..end];
                match secrets.get(name) {
                    Some(v) => out.push_str(v),
                    None => {
                        out.push_str("${");
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                // Unclosed: the rest is literal text, not a reference.
                out.push_str("${");
                out.push_str(after);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// The names available, for `web_status`. Never the values.
pub fn names() -> Vec<String> {
    load().into_keys().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn a_reference_is_replaced_and_the_value_never_appears_in_the_input() {
        let s = map(&[("SHOP_PASSWORD", "hunter2")]);
        assert_eq!(expand("${SHOP_PASSWORD}", &s), "hunter2");
        assert_eq!(expand("pre ${SHOP_PASSWORD} post", &s), "pre hunter2 post");
    }

    #[test]
    fn an_unknown_name_is_left_alone_rather_than_blanked() {
        // Blanking it would submit an empty password and report success, and the caller would be
        // left looking at a login page with no idea why.
        let s = map(&[("A", "1")]);
        assert_eq!(expand("${MISSING}", &s), "${MISSING}");
        assert_eq!(expand("${A}-${MISSING}", &s), "1-${MISSING}");
    }

    #[test]
    fn ordinary_text_passes_through_untouched() {
        let s = map(&[("A", "1")]);
        assert_eq!(expand("no references here", &s), "no references here");
        assert_eq!(
            expand("a $ sign and { braces }", &s),
            "a $ sign and { braces }"
        );
    }

    #[test]
    fn an_unclosed_reference_is_literal_text() {
        let s = map(&[("A", "1")]);
        assert_eq!(expand("${A} and ${unclosed", &s), "1 and ${unclosed");
    }

    #[test]
    fn several_references_in_one_string_all_resolve() {
        let s = map(&[("U", "alice"), ("P", "s3cret")]);
        assert_eq!(expand("${U}:${P}", &s), "alice:s3cret");
    }

    #[test]
    fn the_file_format_is_the_one_people_already_have() {
        let text = "\
# a comment
SHOP_USER=alice
SHOP_PASSWORD=\"hunter2\"
export TOKEN='abc123'

BAD LINE WITHOUT EQUALS
not-a-valid-name=x
EMPTY=
";
        let s = parse(text);
        assert_eq!(s.get("SHOP_USER").map(String::as_str), Some("alice"));
        assert_eq!(s.get("SHOP_PASSWORD").map(String::as_str), Some("hunter2"));
        assert_eq!(s.get("TOKEN").map(String::as_str), Some("abc123"));
        assert_eq!(s.get("EMPTY").map(String::as_str), Some(""));
        assert!(
            !s.contains_key("not-a-valid-name"),
            "a hyphen is not a name"
        );
        assert_eq!(s.len(), 4, "{s:?}");
    }

    #[test]
    fn one_malformed_line_does_not_take_the_file_down() {
        // A login failing because of a stray line three entries above it is a bad afternoon.
        let s = parse("GOOD=1\n]]]garbage[[[\nALSO_GOOD=2");
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn a_value_containing_an_equals_sign_survives() {
        // Base64 and tokens routinely end in `=`.
        let s = parse("TOKEN=abc=def==");
        assert_eq!(s.get("TOKEN").map(String::as_str), Some("abc=def=="));
    }
}
