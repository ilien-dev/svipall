//! Work that outlives the request that asked for it.
//!
//! A crawl of two hundred pages is minutes. Over MCP that is fine — there is a caller waiting on
//! the tool call — but an HTTP client that has to hold a connection open for it is a client that
//! loses the whole crawl to a proxy timeout. So a job can be handed an id instead: it runs here,
//! its progress is durable, and it can be polled, followed, stopped and — after the process running
//! it is gone — picked up where it stopped.
//!
//! The resumability is not new. `crawl_queue` has survived a kill since crawls were written. What
//! was missing was any way to tell a crawl that *died* from one that finished, because
//! `crawl.status` is set to `running` per batch and nothing ever clears it. The `job` row is that
//! missing fact, and the heartbeat is what makes it answerable.
//!
//! This lives here rather than in `rest` because the MCP binary has the same jobs, and because a
//! future `svipall daemon` runs this same loop with nobody connected at all: turning a due watch
//! into work is `runner.submit(...)`, and everything else — durability, cancellation, one crawl per
//! site, restart-after-kill — already applies.

use crate::progress::{CrawlEvent, ProgressSink};
use crate::server::SvipallServer;
use crate::tools::WebCrawlParams;
use futures::future::BoxFuture;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use svipall_core::cache::{JobRow, Store};
use tokio::sync::{broadcast, Notify, Semaphore};

/// How long a job may go without a heartbeat before another run treats it as dead.
///
/// Comfortably longer than one page's timeout (45 s by default) and far shorter than one BFS level,
/// which is what makes a heartbeat necessary in the first place: `crawl.updated_at` is written per
/// batch, and a level of two hundred pages can be half an hour between writes.
const SILENT_FOR_SECS: i64 = 300;

/// How long a finished job's row is kept. Long enough to still be there on Monday.
const KEEP_SECS: i64 = 7 * 86_400;

/// Events a subscriber can fall behind by before it is told it has.
const EVENT_BUFFER: usize = 256;

/// What a job is.
///
/// One variant today. `job.kind` is `TEXT` and this is an enum, so adding `Watch` for a daemon
/// needs no migration and no change to anything below.
pub enum JobKind {
    Crawl(Box<WebCrawlParams>),
}

impl JobKind {
    fn name(&self) -> &'static str {
        match self {
            JobKind::Crawl(_) => "crawl",
        }
    }
    fn params_json(&self) -> String {
        match self {
            JobKind::Crawl(p) => serde_json::to_string(p).unwrap_or_else(|_| "{}".into()),
        }
    }
    /// The site this job is aimed at, known before it runs. This is what `next_queued_job` keys the
    /// one-crawl-per-site rule on; `crawl.domain` cannot serve, because that row does not exist
    /// until the crawl has already started.
    fn domain(&self) -> String {
        match self {
            JobKind::Crawl(p) => svipall_core::domain_from_url(&p.url),
        }
    }
    /// The id this is continuing, when it is continuing one.
    fn resuming(&self) -> Option<String> {
        match self {
            JobKind::Crawl(p) => p.crawl_id.clone(),
        }
    }
}

/// A job that is running right now, in this process.
struct Live {
    cancel: Arc<AtomicBool>,
    events: broadcast::Sender<CrawlEvent>,
}

/// The one thing that runs long work, wherever svipall is running.
///
/// Built by each binary rather than held inside `SvipallServer`: the server is `Clone` and is
/// constructed in tests and in the bench, and giving it a runner would spawn background tasks in
/// every one of them.
#[derive(Clone)]
pub struct JobRunner {
    server: SvipallServer,
    /// This run of this process. A `running` row owned by another run *and* gone quiet is a job
    /// whose process died. Both halves are needed: two svipall processes can share one database.
    run: Arc<str>,
    slots: Arc<Semaphore>,
    live: Arc<StdMutex<HashMap<String, Live>>>,
    /// Rung by `submit`, so a queued job starts now rather than on the next tick.
    wake: Arc<Notify>,
}

impl JobRunner {
    pub fn new(server: SvipallServer, max_jobs: usize) -> Self {
        Self {
            server,
            run: uuid::Uuid::new_v4().simple().to_string().into(),
            slots: Arc::new(Semaphore::new(max_jobs.max(1))),
            live: Arc::new(StdMutex::new(HashMap::new())),
            wake: Arc::new(Notify::new()),
        }
    }

    fn store(&self) -> Option<Arc<Store>> {
        self.server.store().cloned()
    }

    /// Queue work and return its id. Nothing is fetched before this returns.
    ///
    /// The row is written here, synchronously, so a client that polls the id it was just handed
    /// always finds it. A 202 whose id then 404s is the one race worth designing out.
    pub fn submit(&self, kind: JobKind) -> anyhow::Result<String> {
        let Some(store) = self.store() else {
            anyhow::bail!("the page cache is unavailable, so a job could not be recorded");
        };
        // A resume carries the id it is resuming, and for a crawl that id *is* the job. Minting a
        // fresh one here would file the continuation under a new handle and hand the crawl a
        // `crawl_id` it has never seen — which starts the site over rather than continuing it.
        let id = kind.resuming().unwrap_or_else(|| {
            // The same recipe `resume_or_start` uses, so the two agree on what an id looks like.
            uuid::Uuid::new_v4().simple().to_string()[..16].to_string()
        });
        store.create_job(&id, kind.name(), &kind.domain(), &kind.params_json())?;
        self.wake.notify_one();
        Ok(id)
    }

    /// Ask a job to stop. Returns the state it was in, or `None` when there is no such job.
    ///
    /// Both halves matter: the flag in the row survives this process, and the flag in memory is
    /// what a running crawl actually reads between pages.
    pub fn cancel(&self, id: &str) -> Option<String> {
        let was = self.store()?.request_cancel(id)?;
        if let Some(live) = self
            .live
            .lock()
            .ok()
            .and_then(|l| l.get(id).map(|live| live.cancel.clone()))
        {
            live.store(true, Ordering::Relaxed);
        }
        Some(was)
    }

    /// Events from now on, for a job running in this process. `None` when it is not.
    pub fn subscribe(&self, id: &str) -> Option<broadcast::Receiver<CrawlEvent>> {
        self.live
            .lock()
            .ok()?
            .get(id)
            .map(|live| live.events.subscribe())
    }

    /// Take over what a dead run left behind, then keep house. Call once per process.
    ///
    /// Its own tick rather than the housekeeping loop in `svipall-mcp`'s `main`, because that loop
    /// exists in one binary and this has to run in both. Moving the browser reaping and log
    /// trimming in here is the obvious next step and a separate concern.
    pub fn start(&self) -> tokio::task::JoinHandle<()> {
        let me = self.clone();
        tokio::spawn(async move {
            if let Some(store) = me.store() {
                let adopted = store.adopt_orphaned_jobs(&me.run, SILENT_FOR_SECS);
                if adopted > 0 {
                    tracing::info!(
                        "{adopted} job(s) were left running by a process that is gone; \
                         they are resumable"
                    );
                }
            }
            let mut ticks: u64 = 0;
            loop {
                me.dispatch().await;
                tokio::select! {
                    _ = me.wake.notified() => {}
                    _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {
                        ticks += 1;
                        if ticks.is_multiple_of(30) {
                            if let Some(store) = me.store() {
                                let dropped = store.expire_jobs(KEEP_SECS);
                                if dropped > 0 {
                                    tracing::debug!("forgot {dropped} finished job(s)");
                                }
                            }
                        }
                    }
                }
            }
        })
    }

    /// Start every queued job there is room for.
    ///
    /// One loop, so `next_queued_job`'s one-crawl-per-domain rule is decided in one place and by
    /// the database rather than by an in-memory set that a restart would forget.
    async fn dispatch(&self) {
        let Some(store) = self.store() else { return };
        while let Ok(permit) = self.slots.clone().try_acquire_owned() {
            let Some(job) = store.next_queued_job() else {
                break;
            };
            // A compare-and-swap: if another process took it between the read and here, move on.
            if !store.start_job(&job.id, &self.run).unwrap_or(false) {
                continue;
            }
            let me = self.clone();
            tokio::spawn(async move {
                me.run_job(job).await;
                drop(permit);
                // A finished job frees both a slot and, more to the point, its site: a job held back
                // by the one-crawl-per-domain rule must start now rather than on the next tick.
                me.wake.notify_one();
            });
        }
    }

    async fn run_job(&self, job: JobRow) {
        let Some(store) = self.store() else { return };
        let (events, _) = broadcast::channel(EVENT_BUFFER);
        // A cancel that arrived while the job was still queued must not be forgotten.
        let cancel = Arc::new(AtomicBool::new(store.cancel_requested(&job.id)));
        if let Ok(mut live) = self.live.lock() {
            live.insert(
                job.id.clone(),
                Live {
                    cancel: cancel.clone(),
                    events: events.clone(),
                },
            );
        }

        let sink = JobSink {
            store: store.clone(),
            id: job.id.clone(),
            events,
            cancel: cancel.clone(),
        };

        let params: WebCrawlParams = serde_json::from_str(&job.params_json).unwrap_or_default();
        let params = WebCrawlParams {
            // The runner owns the id, so the job and the crawl it drives are the same handle.
            crawl_id: Some(job.id.clone()),
            ..params
        };
        let summary = self.server.crawl_json_with(params, Some(&sink)).await;

        let stopped_by = summary
            .get("stopped_by")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let state = match stopped_by {
            "cancelled" => "cancelled",
            // The frontier drained: there was nothing left to fetch.
            "frontier_empty" => "finished",
            // A budget ran out — pages, time, tokens, saturation, a site's share. The crawl did
            // what it was asked and there is more of the site left, which is why the id resumes.
            _ => "stopped",
        };
        let _ = store.finish_job(&job.id, state, Some(&summary.to_string()), None);
        if let Ok(mut live) = self.live.lock() {
            live.remove(&job.id);
        }
    }
}

/// The sink a running job reports to: the job row, and whoever is watching it.
///
/// `crawl_json` is infallible, so the only way this task ends without `finish_job` is a panic —
/// and that case heals itself: the heartbeat stops, and the next run's `adopt_orphaned_jobs` moves
/// the row to `interrupted`, which is resumable.
struct JobSink {
    store: Arc<Store>,
    id: String,
    events: broadcast::Sender<CrawlEvent>,
    cancel: Arc<AtomicBool>,
}

impl ProgressSink for JobSink {
    fn report<'a>(&'a self, event: &'a CrawlEvent) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            // Per page, which is what makes this a liveness signal rather than a per-batch one.
            self.store.beat(&self.id);
            // `send` errors when nobody is subscribed, which is the ordinary case for a job nobody
            // is watching. A listener must never be able to slow or stop a crawl.
            let _ = self.events.send(event.clone());
        })
    }

    fn should_stop(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}
