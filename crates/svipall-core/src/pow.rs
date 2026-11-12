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

/// The digest a signed challenge names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hash {
    Sha256,
    Sha384,
    Sha512,
}

impl Hash {
    /// `SHA-256`, `sha512`, or the tail of `PBKDF2/SHA-384`.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_uppercase().replace('-', "").as_str() {
            "SHA256" => Some(Self::Sha256),
            "SHA384" => Some(Self::Sha384),
            "SHA512" => Some(Self::Sha512),
            _ => None,
        }
    }

    /// The digest of the parts, concatenated.
    pub fn digest(self, parts: &[&[u8]]) -> Vec<u8> {
        fn run<D: Digest>(parts: &[&[u8]]) -> Vec<u8> {
            let mut h = D::new();
            for p in parts {
                h.update(p);
            }
            h.finalize().to_vec()
        }
        match self {
            Self::Sha256 => run::<Sha256>(parts),
            Self::Sha384 => run::<sha2::Sha384>(parts),
            Self::Sha512 => run::<sha2::Sha512>(parts),
        }
    }

    /// PBKDF2 with this digest as its HMAC, filling `out`.
    pub fn derive(self, password: &[u8], salt: &[u8], rounds: u32, out: &mut [u8]) {
        match self {
            Self::Sha256 => pbkdf2::pbkdf2_hmac::<Sha256>(password, salt, rounds, out),
            Self::Sha384 => pbkdf2::pbkdf2_hmac::<sha2::Sha384>(password, salt, rounds, out),
            Self::Sha512 => pbkdf2::pbkdf2_hmac::<sha2::Sha512>(password, salt, rounds, out),
        }
    }
}

/// A challenge the server signed, as the widget served from `altcha.org` receives it.
///
/// Two wire formats are in circulation and one page can meet either. Both ask for a counter; they
/// differ in what is done with it:
///
/// * the **digest** form, `{algorithm, challenge, maxnumber, salt, signature}`, wants the number
///   whose `hash(salt ++ number)` is exactly `challenge`;
/// * the **derived** form, `{parameters: {algorithm: "PBKDF2/SHA-256", cost, keyLength, keyPrefix,
///   nonce, salt, ..}, signature}`, wants the counter whose key, derived from
///   `nonce ++ counter as a big-endian u32`, begins with `keyPrefix`.
///
/// Nothing but the counter is produced here. The page that fetched the challenge still holds the
/// server's own text of it, and the payload has to echo that text exactly as it was signed, so it
/// is assembled there rather than rebuilt from a parsed copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signed {
    Digest {
        hash: Hash,
        /// Lowercase hex of the digest to reproduce.
        challenge: String,
        salt: String,
        /// The server's own bound on the search. The answer is at or below it by construction.
        maxnumber: u64,
    },
    Derived {
        hash: Hash,
        cost: u32,
        key_length: usize,
        key_prefix: Vec<u8>,
        nonce: Vec<u8>,
        salt: Vec<u8>,
    },
}

/// What a solved [`Signed`] challenge hands back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counter {
    pub number: u64,
    /// Lowercase hex of the derived key. Empty for the digest form, whose payload carries none.
    pub derived_key: String,
    pub iterations: u64,
}

/// When to stop looking.
///
/// The hunt runs on a blocking thread, where dropping the future that asked for it stops nothing:
/// `cancel` is how a caller that gave up tells the loop to give up too.
#[derive(Debug, Clone)]
pub struct Limits {
    /// Counters to try at most, when the challenge names no bound of its own.
    pub max_number: u64,
    pub deadline: std::time::Instant,
    pub cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Limits {
    pub fn within(budget: std::time::Duration) -> Self {
        Self {
            // What the widget itself assumes when the server says nothing.
            max_number: 1_000_000,
            deadline: std::time::Instant::now() + budget,
            cancel: Default::default(),
        }
    }

    fn stop(&self) -> bool {
        self.cancel.load(std::sync::atomic::Ordering::Relaxed)
            || std::time::Instant::now() >= self.deadline
    }
}

/// A stated bound above this was not written for a browser, and counting to it would hold a core
/// for minutes.
const MAX_STATED_NUMBER: u64 = 50_000_000;

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 || !s.is_ascii() {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

impl Signed {
    /// Read a challenge from the JSON the server sent. `None` for anything that is not one.
    pub fn parse(json: &str) -> Option<Self> {
        let v: serde_json::Value = serde_json::from_str(json).ok()?;
        let text = |v: &serde_json::Value, k: &str| v.get(k)?.as_str().map(str::to_string);
        if let Some(p) = v.get("parameters") {
            let algorithm = text(p, "algorithm")?;
            let (kdf, digest) = algorithm.split_once('/')?;
            if !kdf.eq_ignore_ascii_case("PBKDF2") {
                // The other derivations the format allows are memory-hard, and the page's own
                // widget is the right thing to run them. "Not mine" hands it over.
                return None;
            }
            let key_length = p.get("keyLength").and_then(|n| n.as_u64()).unwrap_or(32) as usize;
            let cost = p.get("cost")?.as_u64()?;
            if !(1..=64).contains(&key_length) || cost == 0 || cost > 10_000_000 {
                return None;
            }
            let key_prefix = unhex(&text(p, "keyPrefix")?)?;
            if key_prefix.is_empty() || key_prefix.len() > key_length {
                return None;
            }
            return Some(Self::Derived {
                hash: Hash::parse(digest)?,
                cost: cost as u32,
                key_length,
                key_prefix,
                nonce: unhex(&text(p, "nonce")?)?,
                salt: unhex(&text(p, "salt")?)?,
            });
        }
        let challenge = text(&v, "challenge")?.to_ascii_lowercase();
        if challenge.is_empty() {
            return None;
        }
        Some(Self::Digest {
            hash: Hash::parse(&text(&v, "algorithm")?)?,
            challenge,
            salt: text(&v, "salt")?,
            maxnumber: v
                .get("maxnumber")
                .and_then(|n| n.as_u64())
                .unwrap_or(1_000_000),
        })
    }

    /// Count until the server's own check would pass, or until told to stop.
    ///
    /// `None` is a real answer: out of range, out of time, or cancelled. CPU-bound and
    /// synchronous, so it belongs on a blocking thread and never on the reactor.
    pub fn solve(&self, limits: &Limits) -> Option<Counter> {
        match self {
            Self::Digest {
                hash,
                challenge,
                salt,
                maxnumber,
            } => {
                let want = unhex(challenge)?;
                for n in 0..=(*maxnumber).min(MAX_STATED_NUMBER) {
                    if n % 4096 == 0 && limits.stop() {
                        return None;
                    }
                    if hash.digest(&[salt.as_bytes(), n.to_string().as_bytes()]) == want {
                        return Some(Counter {
                            number: n,
                            derived_key: String::new(),
                            iterations: n + 1,
                        });
                    }
                }
                None
            }
            Self::Derived { key_prefix, .. } => {
                // One derivation is `cost` rounds, so the clock is read before every one.
                for n in 0..=limits.max_number.min(u32::MAX as u64) {
                    if limits.stop() {
                        return None;
                    }
                    let key = self.key_for(n)?;
                    if key.starts_with(key_prefix) {
                        return Some(Counter {
                            number: n,
                            derived_key: hex(&key),
                            iterations: n + 1,
                        });
                    }
                }
                None
            }
        }
    }

    /// Would the server accept this counter? What a fixture server runs, and what a test asserts.
    pub fn accepts(&self, number: u64) -> bool {
        match self {
            Self::Digest {
                hash,
                challenge,
                salt,
                maxnumber,
            } => {
                number <= *maxnumber
                    && hex(&hash.digest(&[salt.as_bytes(), number.to_string().as_bytes()]))
                        == *challenge
            }
            Self::Derived { key_prefix, .. } => self
                .key_for(number)
                .is_some_and(|k| k.starts_with(key_prefix)),
        }
    }

    /// The key the derived form produces for a counter. `None` for the digest form.
    pub fn key_for(&self, number: u64) -> Option<Vec<u8>> {
        let Self::Derived {
            hash,
            cost,
            key_length,
            nonce,
            salt,
            ..
        } = self
        else {
            return None;
        };
        let mut password = nonce.clone();
        password.extend_from_slice(&u32::try_from(number).ok()?.to_be_bytes());
        let mut key = vec![0u8; *key_length];
        hash.derive(&password, salt, *cost, &mut key);
        Some(key)
    }
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
