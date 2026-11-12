//! What a tab found behind a challenge.
//!
//! A challenge that lets the visitor through replaces itself with whatever the site answers
//! next, and that answer is not always the page. Measured on a manuals site: the interstitial
//! cleared by itself in six seconds and the site's own server then answered a bare
//! `403 Forbidden`. Nothing on that page is a challenge, so the wait kept waiting on it for a
//! minute and a half and the fetch climbed to the next tier to do it again. Measured on a test
//! page the other way round: the challenge cleared, the page behind it was 78 characters long,
//! and it was reported as the refusal an earlier tier had got.
//!
//! Both are the same question, asked each time the page is looked at: was a challenge here, and
//! what stands in its place now. This is the memory that answers it. It is pure; the caller says
//! what it sees and how many documents the tab has loaded (`wire::DocumentWatch`).

use svipall_core::classify::WallKind;

/// What stands where a challenge was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Behind {
    /// No challenge was seen, or it is still there, or the page that replaced it has only just
    /// been seen.
    Unknown,
    /// The challenge is gone and a page answered in its place. A short one is still the page.
    Page,
    /// The challenge is gone and the site's own server refused, with this status.
    Refused(u16),
}

/// The memory of one tab's way through a challenge.
#[derive(Debug, Default)]
pub struct Passage {
    /// How many documents the tab had loaded when a challenge was last seen.
    challenged_at: Option<u32>,
    /// A refusal seen once: the document it was seen on and its status. Reported the second time,
    /// because a challenge reloading into itself is, for a moment, an empty page with a 403.
    pending: Option<(u32, u16)>,
    /// What was last reported, so the caller can read it after the looking is over.
    last: Option<Behind>,
}

/// Is this a wall that asks something of the visitor, as opposed to one that just says no?
pub fn asks(kind: &WallKind) -> bool {
    matches!(
        kind,
        WallKind::Cloudflare | WallKind::Hold | WallKind::Vendor | WallKind::Generic
    )
}

/// Is a page a browser read a wall, by the rule the browser tiers deliver on?
///
/// An `Empty` verdict means "this needs a browser to render it". Read by a browser, answered
/// with a 2xx and holding any text at all, it is a short page and not a wall.
pub fn walled(refused: bool, kind: &WallKind, status: u16, text: &str) -> bool {
    let short_page =
        *kind == WallKind::Empty && (200..300).contains(&status) && !text.trim().is_empty();
    refused && !short_page
}

/// Is this status a server declining to serve? Not a missing page, which is an answer about the
/// address, and not a request to slow down, which is an answer about the pace.
fn declines(status: u16) -> bool {
    (400..600).contains(&status) && !matches!(status, 404 | 407 | 410 | 429 | 503)
}

impl Passage {
    /// One look at the page. `documents` is how many document responses the tab has had,
    /// `status` the latest one's, `kind` and `walled` what the classifier said of the page.
    pub fn look(
        &mut self,
        documents: u32,
        status: Option<u16>,
        kind: &WallKind,
        walled: bool,
    ) -> Behind {
        let behind = self.judge(documents, status, kind, walled);
        self.last = Some(behind);
        behind
    }

    fn judge(
        &mut self,
        documents: u32,
        status: Option<u16>,
        kind: &WallKind,
        walled: bool,
    ) -> Behind {
        if asks(kind) {
            self.challenged_at = Some(documents);
            self.pending = None;
            return Behind::Unknown;
        }
        let Some(at) = self.challenged_at else {
            return Behind::Unknown;
        };
        let replaced = documents > at;
        match status {
            Some(s) if replaced && declines(s) => {
                if self.pending == Some((documents, s)) {
                    return Behind::Refused(s);
                }
                self.pending = Some((documents, s));
                Behind::Unknown
            }
            Some(s) if !walled && replaced && (200..300).contains(&s) => Behind::Page,
            // Nobody was counting documents: the wall being gone is all there is to go by.
            None if !walled => Behind::Page,
            _ => Behind::Unknown,
        }
    }

    /// Was a challenge seen on this tab at all?
    pub fn challenged(&self) -> bool {
        self.challenged_at.is_some()
    }

    /// What the last look found.
    pub fn behind(&self) -> Behind {
        self.last.unwrap_or(Behind::Unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_page_where_the_challenge_was_is_the_page() {
        let mut p = Passage::default();
        assert_eq!(
            p.look(1, Some(403), &WallKind::Cloudflare, true),
            Behind::Unknown
        );
        assert!(p.challenged());
        assert_eq!(p.look(2, Some(200), &WallKind::None, false), Behind::Page);
        assert_eq!(p.behind(), Behind::Page);
    }

    #[test]
    fn a_few_words_a_browser_read_with_a_2xx_are_a_page_and_with_a_403_are_not() {
        assert!(!walled(
            true,
            &WallKind::Empty,
            200,
            "Tiny. You got through."
        ));
        assert!(walled(true, &WallKind::Empty, 403, "403 Forbidden"));
        assert!(walled(true, &WallKind::Empty, 200, "  "));
        assert!(walled(true, &WallKind::Cloudflare, 200, "Just a moment"));
        assert!(!walled(false, &WallKind::None, 200, "A page."));
    }

    #[test]
    fn a_refusal_where_the_challenge_was_is_reported_on_the_second_look() {
        let mut p = Passage::default();
        p.look(1, Some(403), &WallKind::Cloudflare, true);
        // A bare "403 Forbidden" reads as an empty page or a status, never as a challenge.
        assert_eq!(
            p.look(2, Some(403), &WallKind::Empty, true),
            Behind::Unknown
        );
        assert_eq!(
            p.look(2, Some(403), &WallKind::Status, true),
            Behind::Refused(403)
        );
    }

    #[test]
    fn a_challenge_reloading_into_itself_is_not_a_refusal() {
        let mut p = Passage::default();
        p.look(1, Some(403), &WallKind::Cloudflare, true);
        // Mid-reload: the new document has answered 403 and has nothing in it yet.
        assert_eq!(
            p.look(2, Some(403), &WallKind::Empty, true),
            Behind::Unknown
        );
        // Then it is the challenge again, and the count starts over from there.
        assert_eq!(
            p.look(2, Some(403), &WallKind::Cloudflare, true),
            Behind::Unknown
        );
        assert_eq!(
            p.look(2, Some(403), &WallKind::Empty, true),
            Behind::Unknown
        );
        assert_eq!(
            p.look(2, Some(403), &WallKind::Empty, true),
            Behind::Unknown,
            "the same document that held the challenge is not what replaced it"
        );
    }

    #[test]
    fn a_page_that_never_showed_a_challenge_says_nothing() {
        let mut p = Passage::default();
        assert_eq!(
            p.look(1, Some(403), &WallKind::Status, true),
            Behind::Unknown
        );
        assert_eq!(
            p.look(1, Some(403), &WallKind::Status, true),
            Behind::Unknown
        );
        assert_eq!(
            p.look(1, Some(200), &WallKind::None, false),
            Behind::Unknown
        );
        assert!(!p.challenged());
    }

    #[test]
    fn a_missing_page_or_a_request_to_slow_down_is_not_a_refusal() {
        for status in [404, 410, 429, 503] {
            let mut p = Passage::default();
            p.look(1, Some(403), &WallKind::Hold, true);
            p.look(2, Some(status), &WallKind::Status, true);
            assert_eq!(
                p.look(2, Some(status), &WallKind::Status, true),
                Behind::Unknown,
                "{status}"
            );
        }
    }

    #[test]
    fn without_a_document_count_the_wall_being_gone_is_the_page() {
        let mut p = Passage::default();
        p.look(0, None, &WallKind::Generic, true);
        assert_eq!(p.look(0, None, &WallKind::Empty, true), Behind::Unknown);
        assert_eq!(p.look(0, None, &WallKind::None, false), Behind::Page);
    }
}
