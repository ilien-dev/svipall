//! How a long job says how it is going, to whoever is listening.
//!
//! There was one listener and it was rmcp: `Progress` held a `Peer` and a token, and a crawl called
//! it directly. Three things want to hear it now — an MCP client that passed a progress token, an
//! HTTP client watching an event stream, and the job row a poller reads — and none of them may be
//! able to stop the crawl. So this is a trait, and `report` returns nothing at all: the
//! infallibility is in the signature rather than in a comment above a `let _ =`.
//!
//! `BoxFuture` rather than `#[async_trait]`, matching `solve_loop::Strategy` in this same crate:
//! `futures` is already a direct dependency and `async-trait` is not, and the shape is the one a
//! reader has already met a few files away.

use futures::future::BoxFuture;
use std::sync::Arc;

/// One thing that happened during a crawl, small enough to send on every page.
///
/// Deliberately not the page. An event carries what a progress bar and a log line need — which URL,
/// how far along, which tier answered, whether a wall answered instead — and never the markdown,
/// because a crawl of two hundred pages would otherwise stream its whole result twice. Everything
/// here is read off the value the ladder already built; nothing is recomputed.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CrawlEvent {
    /// The job this belongs to. For a crawl it is also the `crawl_id`.
    pub job_id: String,
    pub kind: EventKind,
    /// Pages fetched so far, including everything a resumed run inherited.
    pub pages_done: usize,
    /// The page cap, not a prediction of how many pages exist: a crawl that runs out of links stops
    /// early, and "40 of 200" when there were only 40 is a bar that never fills.
    pub total: Option<usize>,
    /// Still queued.
    pub queued: usize,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub url: String,
    /// `tier_used` from the fetch: http, browser, stealth, real, warm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    /// `blocked_reason`, when a wall answered instead of the page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_by: Option<String>,
    /// Only on `Finished`: frontier_empty, max_pages, time, budget, saturation, cancelled, …
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped_by: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// The crawl has begun. `pages_done` is what a resume inherited, which is not always zero.
    Started,
    /// A page arrived and was kept.
    Page,
    /// A page arrived and was a near-duplicate of one already seen.
    Duplicate,
    Finished,
}

impl CrawlEvent {
    /// A bare event with the counters filled in; the callers below add what their kind carries.
    pub fn new(job_id: &str, kind: EventKind, pages_done: usize, queued: usize) -> Self {
        Self {
            job_id: job_id.to_string(),
            kind,
            pages_done,
            total: None,
            queued,
            url: String::new(),
            tier: None,
            status: None,
            blocked_by: None,
            stopped_by: None,
        }
    }

    /// What an MCP client sees. Byte-identical to the string this reported before there was a
    /// trait here, because a refactor that changes what a client reads is not a refactor.
    pub fn message(&self) -> String {
        format!("{} pages, {} queued", self.pages_done, self.queued)
    }

    /// Fill `tier`, `status` and `blocked_by` from a fetch result, without copying its content.
    pub fn from_fetch(mut self, url: &str, value: &serde_json::Value) -> Self {
        self.url = url.to_string();
        self.tier = value
            .get("tier_used")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        self.status = value
            .get("status")
            .and_then(|v| v.as_u64())
            .map(|s| s as u16);
        self.blocked_by = value
            .get("blocked_reason")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        self
    }
}

/// Somewhere to say how a long job is going.
///
/// `report` cannot fail and cannot be refused. A listener that has gone away, a channel with no
/// receivers, a database that would not write — none of them is the crawl's problem, and none of
/// them may become one.
pub trait ProgressSink: Send + Sync {
    fn report<'a>(&'a self, event: &'a CrawlEvent) -> BoxFuture<'a, ()>;

    /// Has somebody asked this job to stop?
    ///
    /// It lives on the same trait as `report` because it is answered in exactly the same two
    /// places — after a page, and at the top of a batch — and a second `Option<&dyn …>` threaded
    /// to those same call sites would be a parameter that is always `Some` or always `None` in
    /// step with this one. A sink is "the thing following this job"; following it includes being
    /// able to call it off.
    ///
    /// Cooperative on purpose. A crawl stops at a page boundary and leaves through its ordinary
    /// exit, so the frontier is written and the id can be resumed. Aborting the task instead would
    /// skip `close_page`, which leaks a CDP page into the browser pool.
    fn should_stop(&self) -> bool {
        false
    }
}

/// Several sinks as one, so the crawl has a single call site whatever is listening.
pub struct Fan(pub Vec<Arc<dyn ProgressSink>>);

impl ProgressSink for Fan {
    fn report<'a>(&'a self, event: &'a CrawlEvent) -> BoxFuture<'a, ()> {
        // In order rather than concurrently: one of these is a synchronous broadcast send and one
        // is a SQLite update measured in microseconds. Ordering is worth more here than the
        // microseconds a join would save.
        Box::pin(async move {
            for s in &self.0 {
                s.report(event).await;
            }
        })
    }

    fn should_stop(&self) -> bool {
        self.0.iter().any(|s| s.should_stop())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Counting(AtomicUsize);
    impl ProgressSink for Counting {
        fn report<'a>(&'a self, _e: &'a CrawlEvent) -> BoxFuture<'a, ()> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Box::pin(async {})
        }
    }

    #[test]
    fn an_mcp_client_still_sees_the_same_progress_message_it_saw_before() {
        // The wire format an MCP client reads. A refactor is allowed to change how the message is
        // produced and not what it says, and this is the fence around that.
        let e = CrawlEvent::new("abc", EventKind::Page, 3, 4);
        assert_eq!(e.message(), "3 pages, 4 queued");
    }

    #[test]
    fn an_event_carries_what_answered_and_never_the_page() {
        // The stream must not cost what the result costs: a crawl of two hundred pages would
        // otherwise send its whole output twice.
        let fetched = json!({
            "url": "https://x.test/a", "status": 403, "tier_used": "warm",
            "blocked_reason": "cloudflare wall (cf-mitigated)",
            "content": "a very long page ".repeat(500),
        });
        let e =
            CrawlEvent::new("abc", EventKind::Page, 1, 2).from_fetch("https://x.test/a", &fetched);
        assert_eq!(e.tier.as_deref(), Some("warm"));
        assert_eq!(e.status, Some(403));
        assert!(e.blocked_by.is_some());
        let wire = serde_json::to_string(&e).expect("serialise");
        assert!(
            !wire.contains("a very long page"),
            "the page leaked: {wire}"
        );
        assert!(
            wire.len() < 512,
            "an event grew too big to send per page: {wire}"
        );
    }

    #[tokio::test]
    async fn a_fan_reports_to_every_sink() {
        let a = Arc::new(Counting(AtomicUsize::new(0)));
        let b = Arc::new(Counting(AtomicUsize::new(0)));
        let fan = Fan(vec![a.clone(), b.clone()]);
        fan.report(&CrawlEvent::new("abc", EventKind::Started, 0, 1))
            .await;
        assert_eq!(a.0.load(Ordering::Relaxed), 1);
        assert_eq!(b.0.load(Ordering::Relaxed), 1);
    }
}
