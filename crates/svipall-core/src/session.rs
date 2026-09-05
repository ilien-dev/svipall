//! Sessions, and knowing when to stop using one.
//!
//! A session is the triple that a site actually sees: the cookies (a browser profile), the machine
//! (an identity seed) and the exit (a proxy, or none). The first two are decided by the profile —
//! `profiles::identity_seed_for` files the machine under the same key as the cookie jar — and the
//! third by `exits`.
//!
//! What is left here is the part neither of those answers: **knowing when a session is spent**.
//! That is harder than it sounds, because a site rarely says so: it returns a `200` with an empty
//! body, or redirects to the home page, or simply gets slower. `Verdict::of` is where those are
//! named, and a session that collects enough of them is retired instead of being asked again.

/// What one response says about the health of the session that fetched it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// A real page.
    Ok,
    /// Slow down, but the session is fine.
    RateLimited,
    /// This session is burnt for this domain.
    Blocked,
}

/// Everything `Verdict::of` needs, so the decision is a pure function and can be tested without a
/// network.
#[derive(Debug, Clone)]
pub struct Response<'a> {
    pub status: u16,
    /// Text length after extraction, not HTML length: a challenge page can be large.
    pub text_len: usize,
    pub requested_path: &'a str,
    pub final_path: &'a str,
    /// Whether the classifier already named a wall.
    pub wall: bool,
    /// Round trip in milliseconds, and the rolling average for this domain.
    pub elapsed_ms: u64,
    pub typical_ms: u64,
}

impl Verdict {
    /// Read a response the way a person would: not by its status code alone.
    ///
    /// Three of these are the reason this function exists. A `200` with nothing in it, a request
    /// for a deep page that lands on the front page, and a response that suddenly takes many times
    /// longer than the domain's own average are all blocks that a status check calls success.
    pub fn of(r: &Response<'_>) -> Self {
        if r.wall {
            return Verdict::Blocked;
        }
        match r.status {
            429 => return Verdict::RateLimited,
            403 | 401 | 407 => return Verdict::Blocked,
            // 5xx is the server's problem, not this session's. Retrying elsewhere would move a
            // fault that follows the site, and burn a good session doing it.
            500..=599 => return Verdict::RateLimited,
            _ => {}
        }
        // A success with no content is the quietest block there is.
        if (200..300).contains(&r.status) && r.text_len < 200 {
            return Verdict::Blocked;
        }
        // Asked for something specific, landed on the front page.
        if is_deep(r.requested_path) && is_root(r.final_path) {
            return Verdict::Blocked;
        }
        // Far slower than this domain normally is, which is how a tarpit feels from the inside.
        if r.typical_ms > 0 && r.elapsed_ms > r.typical_ms.saturating_mul(8) && r.elapsed_ms > 5_000
        {
            return Verdict::RateLimited;
        }
        Verdict::Ok
    }
}

/// A path that is the front page rather than anything in particular.
pub fn is_root(path: &str) -> bool {
    let p = path.split(['?', '#']).next().unwrap_or(path);
    p.is_empty() || p == "/" || p == "/index.html"
}

/// A path that asks for something specific, so landing on the front page is not an answer.
pub fn is_deep(path: &str) -> bool {
    let p = path.split(['?', '#']).next().unwrap_or(path);
    p.trim_matches('/').contains('/') || p.trim_matches('/').len() > 12
}

/// One visitor's standing with a site, and what it takes to give up on it.
#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub proxy: Option<String>,
    /// Starts full and falls. Retirement is a threshold rather than a single failure, because one
    /// block can be the page rather than the session.
    pub health: i32,
    pub uses: u32,
    pub blocks: u32,
}

/// Health a session starts with, and the point below which it is not worth asking again.
pub const FULL_HEALTH: i32 = 100;
const RETIRE_BELOW: i32 = 40;
const BLOCK_COST: i32 = 35;
const RATE_LIMIT_COST: i32 = 10;
/// Successes heal, slowly. Without this a long crawl retires every session it owns purely through
/// accumulated bad luck.
const OK_GAIN: i32 = 3;

impl Session {
    pub fn new(id: impl Into<String>, proxy: Option<String>) -> Self {
        Self {
            id: id.into(),
            proxy,
            health: FULL_HEALTH,
            uses: 0,
            blocks: 0,
        }
    }

    pub fn record(&mut self, v: Verdict) {
        self.uses += 1;
        match v {
            Verdict::Ok => self.health = (self.health + OK_GAIN).min(FULL_HEALTH),
            Verdict::RateLimited => self.health -= RATE_LIMIT_COST,
            Verdict::Blocked => {
                self.health -= BLOCK_COST;
                self.blocks += 1;
            }
        }
    }

    pub fn is_usable(&self) -> bool {
        self.health >= RETIRE_BELOW
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn resp(status: u16, text_len: usize) -> Response<'static> {
        Response {
            status,
            text_len,
            requested_path: "/a/b/c",
            final_path: "/a/b/c",
            wall: false,
            elapsed_ms: 100,
            typical_ms: 100,
        }
    }

    #[test]
    fn a_success_with_no_content_is_a_block() {
        // The quietest block there is, and a status check calls it success.
        assert_eq!(Verdict::of(&resp(200, 12)), Verdict::Blocked);
        assert_eq!(Verdict::of(&resp(200, 5_000)), Verdict::Ok);
    }

    #[test]
    fn a_deep_request_that_lands_on_the_front_page_is_a_block() {
        let r = Response {
            final_path: "/",
            ..resp(200, 40_000)
        };
        assert_eq!(Verdict::of(&r), Verdict::Blocked);
    }

    #[test]
    fn a_front_page_request_that_returns_the_front_page_is_fine() {
        // The rule must not fire on the ordinary case it resembles.
        let r = Response {
            requested_path: "/",
            final_path: "/",
            ..resp(200, 40_000)
        };
        assert_eq!(Verdict::of(&r), Verdict::Ok);
    }

    #[test]
    fn a_sudden_collapse_in_speed_is_rate_limiting_not_a_block() {
        let r = Response {
            elapsed_ms: 30_000,
            typical_ms: 200,
            ..resp(200, 40_000)
        };
        assert_eq!(Verdict::of(&r), Verdict::RateLimited);
    }

    #[test]
    fn a_server_error_does_not_burn_the_session() {
        // A 500 follows the site, not the visitor. Retiring a session over it would move a fault
        // that was never ours and spend a good session doing it.
        assert_eq!(Verdict::of(&resp(503, 0)), Verdict::RateLimited);
        assert_eq!(Verdict::of(&resp(500, 0)), Verdict::RateLimited);
    }

    #[test]
    fn the_statuses_that_really_are_refusals_are_treated_as_such() {
        for s in [401, 403, 407] {
            assert_eq!(Verdict::of(&resp(s, 40_000)), Verdict::Blocked, "{s}");
        }
        assert_eq!(Verdict::of(&resp(429, 40_000)), Verdict::RateLimited);
    }

    #[test]
    fn a_named_wall_beats_every_other_signal() {
        let r = Response {
            wall: true,
            ..resp(200, 40_000)
        };
        assert_eq!(Verdict::of(&r), Verdict::Blocked);
    }

    #[test]
    fn one_block_does_not_retire_a_session_but_three_do() {
        // A single block can be the page rather than the session, so retirement is a threshold.
        let mut s = Session::new("s", None);
        s.record(Verdict::Blocked);
        assert!(s.is_usable(), "one block should not end a session");
        s.record(Verdict::Blocked);
        assert!(!s.is_usable(), "two blocks in a row is enough");
    }

    #[test]
    fn success_heals_but_never_past_full() {
        let mut s = Session::new("s", None);
        s.record(Verdict::RateLimited);
        let hurt = s.health;
        for _ in 0..100 {
            s.record(Verdict::Ok);
        }
        assert!(s.health > hurt);
        assert_eq!(s.health, FULL_HEALTH, "health must not run away upward");
    }
}
