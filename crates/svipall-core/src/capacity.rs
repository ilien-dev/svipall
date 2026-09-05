//! How much this machine can actually do at once.
//!
//! The pacer already adapts to the *remote* host: a fast site gets crawled fast, a hostile one gets
//! backed off. What nothing watched was the local end. `parallelism` was a fixed number from the
//! config, so a laptop that could comfortably run three browsers was asked to run six, and the
//! result was not a faster crawl — it was six browsers thrashing, every page timing out, and the
//! ladder escalating tiers because slow pages look like walls.
//!
//! That last part is why this belongs with the anti-bot code rather than in a performance section:
//! a machine under pressure produces *false wall detections*, teaches the wrong tier to the domain
//! memory, and burns cooldowns on sites that were never blocking anything.
//!
//! No dependency for this. The two numbers that matter — cores, and whether a browser tier is
//! involved — are already known, and a browser costs far more than an HTTP request.

/// What a concurrency decision needs to know.
#[derive(Debug, Clone, Copy)]
pub struct Load {
    /// Logical cores on this machine.
    pub cores: usize,
    /// Browsers currently open. Each one is a process tree, not a task.
    pub open_browsers: usize,
    /// What the operator asked for.
    pub configured: usize,
    /// Whether the work about to be scheduled needs a browser at all.
    pub needs_browser: bool,
}

/// Roughly what a Chromium process tree costs while it is rendering: enough that two per core is
/// already optimistic and four per core is a machine that has stopped responding.
const BROWSERS_PER_CORE: usize = 1;
/// HTTP work is cheap; the limit there is the remote host's patience, not this machine's.
const HTTP_PER_CORE: usize = 4;

/// How many things to run at once, given the machine and the kind of work.
///
/// Never returns zero — a limit of zero would deadlock the caller waiting for a slot that can
/// never open — and never returns more than the operator asked for. This tightens; it does not
/// grant permission the config withheld.
pub fn concurrency(load: Load) -> usize {
    let cores = load.cores.max(1);
    let ceiling = if load.needs_browser {
        // Browsers already open count against the budget: they are still holding memory whether or
        // not this batch is using them.
        (cores * BROWSERS_PER_CORE).saturating_sub(load.open_browsers)
    } else {
        cores * HTTP_PER_CORE
    };
    ceiling.clamp(1, load.configured.max(1))
}

/// Logical cores, or a conservative guess when the platform will not say.
pub fn cores() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(cores: usize, open: usize, configured: usize, browser: bool) -> Load {
        Load {
            cores,
            open_browsers: open,
            configured,
            needs_browser: browser,
        }
    }

    #[test]
    fn a_small_machine_is_not_asked_to_run_the_configured_number_of_browsers() {
        // The case this exists for: a 2-core laptop told to run 6 browsers does not crawl six
        // times faster, it times out and the ladder reads the timeouts as walls.
        assert_eq!(concurrency(load(2, 0, 6, true)), 2);
        assert_eq!(concurrency(load(4, 0, 6, true)), 4);
    }

    #[test]
    fn a_big_machine_still_respects_the_configured_ceiling() {
        // This tightens a limit; it never grants permission the operator withheld.
        assert_eq!(concurrency(load(32, 0, 4, true)), 4);
        assert_eq!(concurrency(load(32, 0, 4, false)), 4);
    }

    #[test]
    fn browsers_already_open_count_against_the_budget() {
        // They are holding memory whether or not this batch uses them.
        assert_eq!(concurrency(load(8, 0, 8, true)), 8);
        assert_eq!(concurrency(load(8, 5, 8, true)), 3);
    }

    #[test]
    fn the_limit_is_never_zero() {
        // Zero would deadlock the caller waiting for a slot that can never open.
        assert_eq!(concurrency(load(2, 99, 8, true)), 1);
        assert_eq!(concurrency(load(1, 0, 0, true)), 1);
        assert_eq!(concurrency(load(0, 0, 0, false)), 1);
    }

    #[test]
    fn http_work_is_allowed_far_more_than_browser_work() {
        // The limit on an HTTP fetch is the remote host's patience, not this machine's memory.
        let http = concurrency(load(4, 0, 64, false));
        let browser = concurrency(load(4, 0, 64, true));
        assert!(http > browser, "{http} should exceed {browser}");
    }

    #[test]
    fn this_machine_reports_a_plausible_core_count() {
        assert!(cores() >= 1);
    }
}
