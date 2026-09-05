#![warn(clippy::all)] // Ours, unlike the rest of this crate: lint it properly.

//! The identity script every worker gets, before it runs a line of its own.
//!
//! `Page.addScriptToEvaluateOnNewDocument` covers documents. A `Worker` is a separate target with
//! its own realm, its own `WorkerNavigator`, and none of the document's overrides — so a page that
//! declares eight cores and four gigabytes in the document, and reports the host's real thirty-two
//! from inside a worker, has contradicted itself in one line of JavaScript. That cross-realm check
//! is cheap enough that any serious detector runs it.
//!
//! Workers already attach paused (`Target.setAutoAttach` with `waitForDebuggerOnStart`), so there
//! is a window between the attach and `Runtime.runIfWaitingForDebugger` in which the realm exists
//! and nothing of the page's has run yet. The script goes in there.
//!
//! The script itself is composed by the caller, which is the only place that knows the identity;
//! this module just holds it for the handler, the same arrangement `world` uses for the isolated
//! world name.

use std::sync::OnceLock;

static SCRIPT: OnceLock<String> = OnceLock::new();

/// Set the script every worker will be given. Only the first call has any effect: a browser wears
/// one identity for its whole life, and a worker started later must not disagree with one started
/// earlier.
pub fn set_init_script(js: String) -> bool {
    SCRIPT.set(js).is_ok()
}

/// What to evaluate in a freshly attached worker, if anything. `None` leaves the worker untouched,
/// which is what an unconfigured embedder gets.
pub fn init_script() -> Option<&'static str> {
    SCRIPT.get().map(String::as_str)
}

/// Whether a target type is a worker realm worth dressing.
///
/// `service_worker` is deliberately absent: the handler detaches from those immediately, so there
/// is nothing to evaluate in.
pub fn is_worker(target_type: &str) -> bool {
    matches!(target_type, "worker" | "shared_worker" | "worklet")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_realms_are_named_exactly() {
        assert!(is_worker("worker"));
        assert!(is_worker("shared_worker"));
        assert!(is_worker("worklet"));
        // The handler detaches from service workers, so there is no session to evaluate in.
        assert!(!is_worker("service_worker"));
        assert!(!is_worker("page"));
        assert!(!is_worker("iframe"));
    }
}
