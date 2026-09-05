//! Knowing when a page that loads as you scroll has stopped loading.
//!
//! A feed or a listing renders one screen and fetches the next when the reader nears the bottom.
//! Reading the document after `load` gets the first screen and nothing else. The only signal
//! that more arrived is that the document grew — taller, or with more text — so this watches
//! exactly that, decides when growth has stopped, and gives a "load more" button one chance
//! before giving up. It is a pure state machine so the decision can be tested without a browser.

/// What the caller should do after reporting one measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Scroll again.
    Continue,
    /// Nothing grew for a while; click a "load more" control if the page has one, then measure
    /// again. Offered once.
    TryLoadMore,
    /// Done: the page is as long as it is going to get, or the round budget is spent.
    Stop,
}

/// Rounds without growth before the page is considered settled.
pub const QUIET_ROUNDS: u32 = 2;
/// Default cap on scroll rounds. Each round is most of a viewport, so forty is a very long page.
pub const DEFAULT_MAX_ROUNDS: u32 = 40;

#[derive(Debug, Clone)]
pub struct GrowthWatch {
    max_rounds: u32,
    rounds: u32,
    quiet: u32,
    last_height: u64,
    last_text: u64,
    load_more_tried: bool,
}

impl GrowthWatch {
    pub fn new(max_rounds: u32) -> Self {
        Self {
            max_rounds: max_rounds.max(1),
            rounds: 0,
            quiet: 0,
            last_height: 0,
            last_text: 0,
            load_more_tried: false,
        }
    }

    /// Report the document's scroll height and text length after a scroll.
    pub fn observe(&mut self, height: u64, text_len: u64) -> Decision {
        let grew = height > self.last_height || text_len > self.last_text;
        self.last_height = self.last_height.max(height);
        self.last_text = self.last_text.max(text_len);
        self.rounds += 1;
        if grew {
            self.quiet = 0;
        } else {
            self.quiet += 1;
        }
        if self.rounds >= self.max_rounds {
            return Decision::Stop;
        }
        if self.quiet < QUIET_ROUNDS {
            return Decision::Continue;
        }
        if !self.load_more_tried {
            self.load_more_tried = true;
            self.quiet = 0;
            return Decision::TryLoadMore;
        }
        Decision::Stop
    }

    /// How many rounds have been measured.
    pub fn rounds(&self) -> u32 {
        self.rounds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn growth_keeps_going_and_two_quiet_rounds_end_it() {
        let mut w = GrowthWatch::new(20);
        assert_eq!(
            w.observe(1000, 500),
            Decision::Continue,
            "first reading is growth from 0"
        );
        assert_eq!(w.observe(2000, 900), Decision::Continue);
        assert_eq!(
            w.observe(2000, 900),
            Decision::Continue,
            "one quiet round is not settled"
        );
        assert_eq!(w.observe(2000, 900), Decision::TryLoadMore);
    }

    #[test]
    fn a_load_more_button_earns_the_page_another_look_but_only_once() {
        let mut w = GrowthWatch::new(20);
        w.observe(1000, 500);
        w.observe(1000, 500);
        assert_eq!(w.observe(1000, 500), Decision::TryLoadMore);
        // The click loaded more: growth resumes.
        assert_eq!(w.observe(3000, 1500), Decision::Continue);
        assert_eq!(w.observe(3000, 1500), Decision::Continue);
        assert_eq!(w.observe(3000, 1500), Decision::Stop, "no second chance");
    }

    #[test]
    fn text_growth_counts_even_when_height_does_not() {
        let mut w = GrowthWatch::new(20);
        w.observe(1000, 500);
        w.observe(1000, 500);
        assert_eq!(
            w.observe(1000, 800),
            Decision::Continue,
            "a virtualised list grows in text, not height"
        );
    }

    #[test]
    fn the_round_cap_is_final() {
        let mut w = GrowthWatch::new(3);
        assert_eq!(w.observe(1, 1), Decision::Continue);
        assert_eq!(w.observe(2, 2), Decision::Continue);
        assert_eq!(w.observe(3, 3), Decision::Stop);
        assert_eq!(w.rounds(), 3);
    }
}
