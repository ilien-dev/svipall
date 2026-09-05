//! Comparing a secret against what a caller sent.
//!
//! There are two of these in the tree now — the dashboard's `?t=` and the REST API's bearer key —
//! and there was very nearly a second copy of this function. It is security prose as much as it is
//! code: the whole point is the *absence* of an early return, which is exactly the kind of thing a
//! well-meaning simplification removes. A fix that lands in one of two copies is worse than either,
//! so it lives here and both callers import it.

/// Compare without an early return, so a caller cannot learn the token one byte at a time.
///
/// An empty `expected` matches an empty `got`, which is arithmetically correct and operationally a
/// trap: a server that forgot to configure a token would accept everybody. Callers must refuse an
/// empty secret before they get here; this function will not guess at their policy for them.
pub fn token_matches(expected: &str, got: Option<&String>) -> bool {
    let Some(got) = got else { return false };
    if got.len() != expected.len() {
        return false;
    }
    expected
        .bytes()
        .zip(got.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_that_matches_is_accepted() {
        assert!(token_matches("abc123", Some(&"abc123".to_string())));
    }

    #[test]
    fn no_token_at_all_is_not_a_match() {
        assert!(!token_matches("abc123", None));
    }

    #[test]
    fn a_token_of_the_wrong_length_is_refused_without_comparing_bytes() {
        // The length check is the one early return there is, and it leaks only the length, which
        // the attacker already chose.
        assert!(!token_matches("abc123", Some(&"abc12".to_string())));
        assert!(!token_matches("abc123", Some(&"abc1234".to_string())));
    }

    #[test]
    fn a_token_that_differs_only_in_its_last_byte_is_refused() {
        // The case an early-returning comparison gets right and gets right *slowly*: the whole
        // point of the fold is that this costs the same as a first-byte mismatch.
        assert!(!token_matches("abc123", Some(&"abc124".to_string())));
        assert!(!token_matches("abc123", Some(&"zbc123".to_string())));
    }

    #[test]
    fn an_empty_secret_matches_an_empty_offer_which_is_why_callers_must_refuse_one() {
        // Documented rather than defended here: the arithmetic is right and the policy belongs to
        // the caller. `svipall_mcp::rest::require_key` and the dashboard both refuse an empty
        // secret before reaching this function, and each has its own test for that.
        assert!(token_matches("", Some(&String::new())));
    }
}
