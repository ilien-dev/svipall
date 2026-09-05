//! Challenges that are arithmetic, not puzzles.
//!
//! A growing family of captchas asks the browser to burn a little CPU instead of asking a person to
//! identify a bus: the server sends a salt and a difficulty, and the client hunts for the number
//! that makes the hash come out with the required shape. They exist for privacy — nothing about the
//! visitor is measured — and that is exactly why they are the best possible target here.
//!
//! There is no model to install, no image to classify, no person to interrupt, and no ambiguity
//! about whether the answer is right: it either hashes to the target or it does not. Three of the
//! challenge types in circulation work this way, and all three come out at a hundred per cent.
//!
//! Both conventions in the wild are supported, because they are easy to confuse and confusing them
//! means never converging: some schemes ask for a number of leading zero *bits*, others for a
//! literal hex *prefix*.

use sha2::{Digest, Sha256};

/// What the server said it will accept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The digest must begin with this many zero bits.
    Bits(u32),
    /// The digest, in lowercase hex, must start with this string.
    HexPrefix(String),
}

/// A challenge to solve.
#[derive(Debug, Clone)]
pub struct Challenge {
    /// Hashed together with the candidate number. Usually a nonce the server issued.
    pub salt: String,
    pub target: Target,
    /// How far to count before giving up. A challenge that needs more than this was either
    /// misparsed or is not meant to be solved in a browser.
    pub max_iterations: u64,
}

impl Challenge {
    pub fn new(salt: impl Into<String>, target: Target) -> Self {
        Self {
            salt: salt.into(),
            target,
            // Twenty leading zero bits is about a million tries: a second or so, and already more
            // than any of these schemes asks for in practice.
            max_iterations: 4_000_000,
        }
    }
}

/// The answer, and what it cost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Solution {
    pub number: u64,
    pub digest: String,
    pub iterations: u64,
}

/// Count until the hash has the shape the server asked for.
///
/// `None` means the budget ran out, which is a real answer: it says the challenge was harder than
/// anything a browser is expected to do, and the caller should hand over rather than spin.
pub fn solve(c: &Challenge) -> Option<Solution> {
    for n in 0..c.max_iterations {
        let digest = hash(&c.salt, n);
        if matches(&digest, &c.target) {
            return Some(Solution {
                number: n,
                digest: hex(&digest),
                iterations: n + 1,
            });
        }
    }
    None
}

/// The hash these schemes agree on: the salt and the number, concatenated as text.
pub fn hash(salt: &str, number: u64) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(salt.as_bytes());
    h.update(number.to_string().as_bytes());
    h.finalize().into()
}

fn matches(digest: &[u8; 32], target: &Target) -> bool {
    match target {
        Target::Bits(bits) => leading_zero_bits(digest) >= *bits,
        Target::HexPrefix(prefix) => hex(digest).starts_with(&prefix.to_lowercase()),
    }
}

/// Leading zero bits, counted across byte boundaries.
///
/// Counting whole zero *bytes* instead is the classic mistake here, and it makes every odd
/// difficulty unsatisfiable while looking correct for the even ones.
pub fn leading_zero_bits(digest: &[u8]) -> u32 {
    let mut n = 0;
    for b in digest {
        if *b == 0 {
            n += 8;
        } else {
            n += b.leading_zeros();
            break;
        }
    }
    n
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Read a difficulty the way the schemes express it.
///
/// A bare number is bits. A hex string is a prefix. Getting this backwards means hunting for a
/// digest that will never come.
pub fn parse_target(raw: &str) -> Option<Target> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(bits) = raw.parse::<u32>() {
        // A plain integer is a bit count. Above 40 nothing finishes this side of an afternoon, so
        // treat it as a misparse rather than a challenge.
        return (bits <= 40).then_some(Target::Bits(bits));
    }
    let looks_hex = raw.chars().all(|c| c.is_ascii_hexdigit());
    (looks_hex && raw.len() <= 8).then(|| Target::HexPrefix(raw.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bit_target_is_solved_and_the_answer_verifies() {
        // Small difficulty so the test is instant; the mechanism is identical at any size.
        let c = Challenge::new("salt", Target::Bits(12));
        let s = solve(&c).expect("solvable");
        assert!(leading_zero_bits(&hash("salt", s.number)) >= 12);
        assert_eq!(hex(&hash("salt", s.number)), s.digest);
    }

    #[test]
    fn a_hex_prefix_target_is_solved_too() {
        // The other convention. Confusing the two is why this distinction exists at all.
        let c = Challenge::new("salt", Target::HexPrefix("00".into()));
        let s = solve(&c).expect("solvable");
        assert!(s.digest.starts_with("00"), "{}", s.digest);
    }

    #[test]
    fn leading_zeroes_are_counted_in_bits_not_bytes() {
        // Counting whole zero bytes looks right for even difficulties and makes every odd one
        // unsatisfiable.
        assert_eq!(leading_zero_bits(&[0x00, 0x00, 0xff]), 16);
        assert_eq!(leading_zero_bits(&[0x0f, 0xff]), 4);
        assert_eq!(leading_zero_bits(&[0x7f]), 1);
        assert_eq!(leading_zero_bits(&[0xff]), 0);
        assert_eq!(leading_zero_bits(&[0x01]), 7);
    }

    #[test]
    fn the_same_challenge_always_gives_the_same_answer() {
        // Deterministic: the search starts at zero and counts, so two runs agree. That matters for
        // replaying a captured challenge while debugging.
        let c = Challenge::new("abc", Target::Bits(10));
        assert_eq!(solve(&c), solve(&c));
    }

    #[test]
    fn a_different_salt_needs_a_different_answer() {
        let a = solve(&Challenge::new("one", Target::Bits(12))).unwrap();
        let b = solve(&Challenge::new("two", Target::Bits(12))).unwrap();
        assert_ne!(a.number, b.number);
    }

    #[test]
    fn an_impossible_budget_gives_up_instead_of_spinning() {
        // Saying so is a real answer: it means hand over rather than burn a core.
        let mut c = Challenge::new("salt", Target::Bits(40));
        c.max_iterations = 500;
        assert!(solve(&c).is_none());
    }

    #[test]
    fn a_difficulty_of_zero_is_satisfied_immediately() {
        let s = solve(&Challenge::new("salt", Target::Bits(0))).expect("trivial");
        assert_eq!(s.number, 0);
    }

    #[test]
    fn the_two_conventions_are_told_apart() {
        assert_eq!(parse_target("12"), Some(Target::Bits(12)));
        assert_eq!(parse_target(" 4 "), Some(Target::Bits(4)));
        assert_eq!(parse_target("00ab"), Some(Target::HexPrefix("00ab".into())));
        assert_eq!(
            parse_target("00AB"),
            Some(Target::HexPrefix("00ab".into())),
            "hex is case-insensitive"
        );
    }

    #[test]
    fn a_difficulty_nobody_could_mean_is_refused() {
        // Hunting for a digest that will never arrive is worse than admitting the parse failed.
        assert_eq!(
            parse_target("64"),
            None,
            "64 bits is not a browser challenge"
        );
        assert_eq!(parse_target(""), None);
        assert_eq!(parse_target("not-hex-at-all"), None);
        assert_eq!(
            parse_target("0123456789abcdef"),
            None,
            "too long to ever hit"
        );
    }
}
