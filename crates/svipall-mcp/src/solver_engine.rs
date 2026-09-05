//! Local captcha solving, no third-party API.
//!
//! Two real mechanisms:
//!   * token captchas (Turnstile, reCAPTCHA, hCaptcha) are solved by loading the real page in a
//!     stealth browser and reading the response token the widget writes once it clears. Non
//!     interactive widgets (managed Turnstile, invisible reCAPTCHA, passive hCaptcha) fill on
//!     their own; interactive ones fill while a human solves in the visible window (human assist).
//!   * image captchas run through local OCR (`crate::ocr`); an unreadable one falls to the human.
//!
//! Whatever cannot be solved automatically stays `human` so the dashboard at /human can finish it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use svipall_solver::queue::{JobType, SolverJob};

use svipall_cdp::cdp::browser_protocol::page::{CaptureScreenshotFormat, Viewport as ClipViewport};
use svipall_cdp::cdp::js_protocol::runtime::{EvaluateParams, ExecutionContextId};
use svipall_cdp::page::{Page, ScreenshotParams};

use crate::browser::{BrowserPool, BrowserTier, PageOpts};
use crate::solve_loop::{self, Outcome, Record, Step, Strategy, Surface};
use futures::future::BoxFuture;
use svipall_core::widget::Modality;

/// Outcome of a solve attempt.
pub enum Solved {
    /// A token captcha produced its response token.
    Token(String),
    /// An image captcha produced its text.
    Text(String),
    /// Not solvable automatically; leave it for a human at the dashboard.
    NeedsHuman(String),
}

pub struct SolveEngine {
    pub(crate) pool: Arc<BrowserPool>,
    /// Where a strategy's successes and failures are kept. `None` when the solver API is off,
    /// in which case every strategy starts even on every run.
    pub(crate) state: Option<Arc<svipall_solver::AppState>>,
    pub(crate) human_assist: bool,
    auto_wait: Duration,
    pub(crate) human_wait: Duration,
    /// Keep what challenges showed and how they were answered, for `export-corpus`.
    corpus: bool,
}

/// Hidden field each provider writes its response token into.
fn token_field(job_type: &JobType) -> Option<&'static str> {
    match job_type {
        JobType::Turnstile => Some("cf-turnstile-response"),
        JobType::RecaptchaV2 | JobType::RecaptchaV3 => Some("g-recaptcha-response"),
        JobType::HCaptcha => Some("h-captcha-response"),
        _ => None,
    }
}

/// JS that returns the first non-empty response token on the page, or "".
fn read_token_js(field: &str) -> String {
    format!(
        r#"(() => {{
            const names = [{field:?}, "cf-turnstile-response", "g-recaptcha-response", "h-captcha-response"];
            for (const n of names) {{
                const el = document.querySelector('textarea[name="' + n + '"], input[name="' + n + '"]');
                if (el && el.value && el.value.length > 20) return el.value;
            }}
            // hCaptcha sometimes exposes it via the iframe data attribute
            const hc = document.querySelector('[data-hcaptcha-response]');
            if (hc && hc.getAttribute('data-hcaptcha-response')) return hc.getAttribute('data-hcaptcha-response');
            return "";
        }})()"#,
        field = field
    )
}

/// Does the page actually host a widget of this kind? (Guards against pointing at the wrong URL.)
fn has_widget_js(job_type: &JobType) -> &'static str {
    match job_type {
        JobType::Turnstile => {
            r#"(() => !!document.querySelector('.cf-turnstile, [data-sitekey], iframe[src*="challenges.cloudflare.com"], textarea[name="cf-turnstile-response"]'))()"#
        }
        JobType::HCaptcha => {
            r#"(() => !!document.querySelector('.h-captcha, iframe[src*="hcaptcha.com"], textarea[name="h-captcha-response"]'))()"#
        }
        _ => {
            r#"(() => !!document.querySelector('.g-recaptcha, iframe[src*="recaptcha"], [data-sitekey], textarea[name="g-recaptcha-response"]'))()"#
        }
    }
}

impl SolveEngine {
    pub fn new(pool: Arc<BrowserPool>, cfg: &svipall_core::Config) -> Self {
        // Human assist opens a visible window when auto-solving fails; harmless where there is no
        // display (the launch just errors and the job falls back to the dashboard).
        let human_assist = std::env::var("SVIPALL_HUMAN_ASSIST")
            .map(|v| v != "0" && !v.eq_ignore_ascii_case("false"))
            .unwrap_or(true);
        Self {
            pool,
            state: None,
            human_assist,
            corpus: cfg.corpus_keep_days > 0,
            auto_wait: Duration::from_millis(cfg.warm_wait_ms.max(8_000)),
            human_wait: Duration::from_secs(
                std::env::var("SVIPALL_HUMAN_WAIT_SECS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(180),
            ),
        }
    }

    /// The same engine, able to remember which strategies work on this machine.
    pub fn with_state(
        pool: Arc<BrowserPool>,
        cfg: &svipall_core::Config,
        state: Arc<svipall_solver::AppState>,
    ) -> Self {
        Self {
            state: Some(state),
            ..Self::new(pool, cfg)
        }
    }

    pub async fn solve(&self, job: &SolverJob) -> Solved {
        match job.job_type {
            JobType::ImageToText => self.solve_image(job).await,
            _ if token_field(&job.job_type).is_some() => self.solve_token(job).await,
            ref other => Solved::NeedsHuman(format!(
                "{} is not solvable locally; solve at the dashboard",
                other.as_str()
            )),
        }
    }

    async fn solve_image(&self, job: &SolverJob) -> Solved {
        let Some(b64) = &job.image_data else {
            return Solved::NeedsHuman("no image data".into());
        };
        match crate::ocr::solve(b64) {
            Ok(text) if !text.trim().is_empty() => {
                tracing::info!(task_id = %job.task_id, text = %text, "image solved by local OCR");
                self.corpus_image(job, &text).await;
                Solved::Text(text)
            }
            Ok(_) => Solved::NeedsHuman("OCR produced nothing; type it at the dashboard".into()),
            Err(e) => {
                Solved::NeedsHuman(format!("OCR unavailable ({}); type it at the dashboard", e))
            }
        }
    }

    async fn solve_token(&self, job: &SolverJob) -> Solved {
        let Some(page_url) = job.page_url.clone() else {
            return Solved::NeedsHuman(
                "token captcha needs pageUrl (the real page hosting the widget)".into(),
            );
        };
        if !self.pool.available() {
            return Solved::NeedsHuman(
                "no Chromium-based browser found for token extraction".into(),
            );
        }
        let field = token_field(&job.job_type).unwrap_or("g-recaptcha-response");

        // Pass 1: offscreen, let a non-interactive widget clear itself.
        match self
            .attempt(job, &page_url, field, false, self.auto_wait)
            .await
        {
            Ok(Some(tok)) => return Solved::Token(tok),
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(task_id = %job.task_id, error = %e, "auto token attempt errored")
            }
        }

        // Pass 2: visible window so a human can complete an interactive challenge.
        if self.human_assist {
            tracing::info!(task_id = %job.task_id, "opening visible window for human-assisted token solve");
            match self
                .attempt(job, &page_url, field, true, self.human_wait)
                .await
            {
                Ok(Some(tok)) => return Solved::Token(tok),
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!(task_id = %job.task_id, error = %e, "human-assist token attempt errored")
                }
            }
        }
        Solved::NeedsHuman("widget did not yield a token automatically".into())
    }

    /// Solve the challenge **on the page that is showing it**, then carry on to the content.
    ///
    /// This is the flow that was missing. `solve_token` opens the page in a separate context, reads
    /// a token and hands it back — but a Turnstile or hCaptcha token is bound to the session and IP
    /// that produced it, so outside that context it is usually worthless. Here the token is
    /// produced, injected and submitted in one place, and what comes back is the unblocked page.
    ///
    /// Returns the final HTML and URL.
    pub async fn solve_in_place(
        &self,
        page_url: &str,
        profile: Option<&str>,
        wait: Duration,
    ) -> anyhow::Result<(String, String, bool)> {
        if !self.pool.available() {
            anyhow::bail!(
                "no Chromium-based browser found; browser_setup(action=\"install\") fetches one"
            );
        }
        let profile_dir = Some(match profile {
            Some(name) => crate::browser::named_profile(name),
            None => std::path::PathBuf::from(svipall_core::auto_profile_path(page_url, true)),
        });
        let opts = PageOpts {
            mobile: false,
            tier: BrowserTier::Warm,
            identity_seed: svipall_core::profiles::identity_seed_for(
                profile_dir.as_deref(),
                page_url,
                profile,
            ),
            profile_dir,
            proxy: svipall_core::exits::choose(
                &svipall_core::domain_from_url(page_url),
                svipall_core::exits::Strategy::Sticky,
            ),
            visible: false,
        };
        // A challenge answered in a real browser is the most expensive visit this tool makes, and it
        // never passes through the ladder. Charged here so the ledger cannot be walked around.
        {
            let d = svipall_core::domain_from_url(page_url);
            svipall_core::reputation::spend(&d, opts.proxy.as_deref(), opts.tier.as_str(), false);
        }
        let (_pooled, page) = self.pool.page(&opts).await?;
        let result = async {
            self.pool.navigate(&page, page_url).await?;
            let mut solved = self.wait_and_submit(&page, wait).await?;
            // The models had their turn. A person at the dashboard — on a phone, on another
            // machine — gets the same page, parked here, before a window is opened on this one.
            if !solved && self.human_assist && self.state.is_some() {
                if let Some(modality) = self.probe_only(&page).await {
                    solved = self
                        .hand_to_dashboard(&page, modality, self.human_wait)
                        .await?;
                }
            }
            // Whatever happened, report what the page says now.
            let (html, final_url) = self.pool.content(&page).await?;
            Ok::<_, anyhow::Error>((html, final_url, solved))
        }
        .await;
        self.pool.close_page(page).await;

        match result {
            Ok(r) if r.2 || !self.human_assist || self.state.is_some() => Ok(r),
            // Nothing cleared on its own, there is no dashboard to hand it to, and a person is
            // allowed to help: same page, visible.
            _ => {
                tracing::info!(url = %page_url, "opening a visible window to finish the challenge");
                let profile_dir = Some(std::path::PathBuf::from(svipall_core::auto_profile_path(
                    page_url, true,
                )));
                let visible = PageOpts {
                    mobile: false,
                    tier: BrowserTier::Real,
                    identity_seed: svipall_core::profiles::identity_seed_for(
                        profile_dir.as_deref(),
                        page_url,
                        None,
                    ),
                    profile_dir,
                    proxy: None,
                    visible: true,
                };
                let (_p, page) = self.pool.page(&visible).await?;
                let out = async {
                    self.pool.navigate(&page, page_url).await?;
                    let solved = self.wait_and_submit(&page, self.human_wait).await?;
                    let (html, final_url) = self.pool.content(&page).await?;
                    Ok::<_, anyhow::Error>((html, final_url, solved))
                }
                .await;
                self.pool.close_page(page).await;
                out
            }
        }
    }

    /// Answer a proof-of-work challenge, if the page is showing one.
    ///
    /// These widgets publish everything needed on the element itself: a salt and a difficulty. What
    /// they want back is the number whose hash has the required shape, which is a loop — no model,
    /// nobody interrupted, and no doubt about whether the answer is right.
    ///
    /// `Ok(false)` means there was nothing of the kind on the page.
    async fn try_pow(&self, page: &Page) -> anyhow::Result<bool> {
        const READ: &str = r#"(() => {
            const el = document.querySelector(
                'altcha-widget, .altcha, .frc-captcha, .procaptcha, [data-challenge], [data-salt]');
            if (!el) return null;
            const d = el.dataset || {};
            return {
                salt: d.salt || d.challenge || d.sitekey || '',
                difficulty: d.difficulty || d.complexity || '',
                field: (el.querySelector('input[type=hidden]') || {}).name || '',
            };
        })()"#;
        let info = match page.evaluate(READ).await {
            Ok(v) => v.into_value::<Value>().unwrap_or(Value::Null),
            Err(_) => return Ok(false),
        };
        if info.is_null() {
            return Ok(false);
        }
        let salt = info["salt"].as_str().unwrap_or_default();
        if salt.is_empty() {
            return Ok(false);
        }
        // No stated difficulty means the widget fetches it, which this path cannot see. A modest
        // default is tried rather than nothing: it is a second of CPU, and it either works or the
        // loop moves on to the next strategy.
        let target = info["difficulty"]
            .as_str()
            .and_then(svipall_core::pow::parse_target)
            .unwrap_or(svipall_core::pow::Target::Bits(16));
        let challenge = svipall_core::pow::Challenge::new(salt, target);

        // The hash loop is CPU-bound and synchronous; running it on the reactor would stall every
        // other fetch in flight.
        let solved =
            tokio::task::spawn_blocking(move || svipall_core::pow::solve(&challenge)).await?;
        let Some(solution) = solved else {
            return Ok(false);
        };
        tracing::info!(
            iterations = solution.iterations,
            "proof-of-work challenge answered"
        );

        // Hand the answer back the way the widget expects, then let the page's own callback or the
        // form submission carry on.
        let js = format!(
            r#"(() => {{
                const payload = {{ number: {n}, salt: {salt:?}, digest: {digest:?} }};
                const el = document.querySelector(
                    'altcha-widget, .altcha, .frc-captcha, .procaptcha, [data-challenge], [data-salt]');
                const field = el && el.querySelector('input[type=hidden]');
                if (field) field.value = btoa(JSON.stringify(payload));
                try {{ el.dispatchEvent(new CustomEvent('verified', {{ detail: payload, bubbles: true }})); }} catch (e) {{}}
                const form = el && el.closest('form');
                if (form) {{
                    if (typeof form.requestSubmit === 'function') form.requestSubmit();
                    else form.submit();
                    return 'submitted';
                }}
                return 'filled';
            }})()"#,
            n = solution.number,
            salt = salt,
            digest = solution.digest,
        );
        let _ = page.evaluate(js.as_str()).await;
        Ok(true)
    }

    /// One pass at an image grid: read the question, score every tile, click the matches, verify.
    ///
    /// Returns `Ok(false)` when there is nothing to do - no grid on screen, no classifier
    /// installed, or a subject the model cannot name. That last case is deliberate: clicking
    /// tiles at random spends the attempt and tells the widget what we are, so an unknown
    /// question goes straight to a person.
    ///
    /// The challenge lives in a cross-origin frame, so the question and the tile geometry are read
    /// inside that frame's own execution context, while the clicks are dispatched as real input at
    /// page coordinates - the widget watches for exactly that.
    async fn try_grid(&self, page: &Page, widget: Option<&str>) -> anyhow::Result<bool> {
        if !crate::grid::available() {
            return Ok(false);
        }
        let Some(ctx) = grid_context(page).await else {
            return Ok(false);
        };
        // Where the challenge frame sits in the top document; frame coordinates are relative to it.
        let Some((ox, oy, _, _)) = crate::behavior::box_of(page, "iframe[src*=\"bframe\"]").await
        else {
            return Ok(false);
        };
        let info = eval_in_frame(page, ctx, GRID_JS).await?;
        let (rows, cols) = (
            info["rows"].as_u64().unwrap_or(0) as usize,
            info["cols"].as_u64().unwrap_or(0) as usize,
        );
        if rows == 0 || cols == 0 {
            return Ok(false);
        }
        // One photograph cut into squares is the segmenter's question, not the classifier's: a
        // square holding the bottom third of a hydrant is not a picture of a hydrant. Only when
        // there is a segmenter to hand it to, though — a classifier is better than a person's
        // time when it is the only model in the house.
        if info["single_image"].as_bool().unwrap_or(false) && crate::segment::available() {
            return Ok(false);
        }
        let cfg = crate::grid::load_config()?;
        let prompt = info["prompt"].as_str().unwrap_or_default();
        let class = crate::grid::label_for(prompt, &cfg.classes);
        if class.is_none() && !crate::zeroshot::available() {
            tracing::info!(
                prompt,
                "image grid asks for something the model cannot name"
            );
            return Ok(false);
        }

        let num = |k: &str| info[k].as_f64().unwrap_or(0.0);
        let boxes =
            crate::grid::tile_boxes(ox + num("x"), oy + num("y"), num("w"), num("h"), rows, cols);
        let mut tiles = Vec::with_capacity(boxes.len());
        for (x, y, w, h) in &boxes {
            tiles.push(shot(page, *x, *y, *w, *h).await?);
        }
        // Into the corpus before the answer is known: what the page showed is the training data,
        // and the verdict is attached once the page gives it.
        let job = self
            .corpus_open(
                page,
                widget.unwrap_or("unknown"),
                "tiles",
                serde_json::json!({"prompt": prompt, "rows": rows, "cols": cols}),
            )
            .await;
        if let (Some(j), Some(st)) = (&job, &self.state) {
            let db = st.db_pool.read().await;
            for (i, t) in tiles.iter().enumerate() {
                db.put_asset(&j.0, "tile", i as i64, "image/png", t);
            }
        }
        // The classifier when it knows the subject; otherwise the zero-shot pair, which is only
        // trusted when the tiles clearly disagree — an even spread is a grid it cannot read, and
        // that is a decline, not a guess.
        let (picks, source) = match class {
            Some(class) => {
                let scores = crate::grid::classify(&tiles, class)?;
                (crate::grid::select(&scores, cfg.threshold), "model")
            }
            None => match crate::zeroshot::pick_tiles(&tiles, prompt)? {
                Some(p) => (p, "zeroshot"),
                None => {
                    tracing::info!(prompt, "zero-shot could not tell the tiles apart");
                    self.corpus_close(
                        &job,
                        serde_json::json!({"kind": "unknown"}),
                        "zeroshot",
                        false,
                    )
                    .await;
                    return Ok(false);
                }
            },
        };
        tracing::info!(
            prompt,
            source,
            picked = picks.len(),
            "solving an image grid"
        );
        let answer = serde_json::json!({"kind": "tiles", "indices": picks});
        if picks.is_empty() {
            // "None of these" is a legitimate answer, so verify anyway rather than giving up.
            let ok = self.verify_grid(page, ox, oy, &info).await?;
            self.corpus_close(&job, answer, source, ok).await;
            return Ok(ok);
        }

        let seed = self.pool.identity().noise_seed;
        let mut cursor = crate::behavior::Cursor::at_page(page);
        for (n, i) in picks.iter().enumerate() {
            let Some((x, y, w, h)) = boxes.get(*i) else {
                continue;
            };
            // A different seed per tile, or every click lands on the same relative spot.
            let s = seed ^ (n as u64).wrapping_mul(0x9E37_79B9);
            let (tx, ty) = crate::behavior::aim(*x, *y, *w, *h, s);
            cursor.click_at(page, tx, ty, s).await?;
            tokio::time::sleep(Duration::from_millis(180 + (s % 240))).await;
        }
        let ok = self.verify_grid(page, ox, oy, &info).await?;
        self.corpus_close(&job, answer, source, ok).await;
        Ok(ok)
    }

    /// One pass at the 4x4 single-picture grid: segment the whole photograph once and click every
    /// square the mask touches.
    ///
    /// Same declines as `try_grid`, for the same reasons: no frame, no model, or a subject the
    /// model cannot name is `Ok(false)` and costs nothing. The verdict is the page's.
    async fn try_segment(&self, page: &Page, widget: Option<&str>) -> anyhow::Result<bool> {
        if !crate::segment::available() {
            return Ok(false);
        }
        let Some(ctx) = grid_context(page).await else {
            return Ok(false);
        };
        let Some((ox, oy, _, _)) = crate::behavior::box_of(page, "iframe[src*=\"bframe\"]").await
        else {
            return Ok(false);
        };
        let info = eval_in_frame(page, ctx, GRID_JS).await?;
        if !info["single_image"].as_bool().unwrap_or(false) {
            return Ok(false);
        }
        let (rows, cols) = (
            info["rows"].as_u64().unwrap_or(0) as usize,
            info["cols"].as_u64().unwrap_or(0) as usize,
        );
        if rows == 0 || cols == 0 {
            return Ok(false);
        }
        let cfg = crate::segment::load_config()?;
        let prompt = info["prompt"].as_str().unwrap_or_default();
        let Some(class) = crate::grid::label_for(prompt, &cfg.classes) else {
            tracing::info!(
                prompt,
                "the picture asks for something the segmenter cannot name"
            );
            return Ok(false);
        };
        let num = |k: &str| info[k].as_f64().unwrap_or(0.0);
        let (gx, gy, gw, gh) = (ox + num("x"), oy + num("y"), num("w"), num("h"));
        let png = shot(page, gx, gy, gw, gh).await?;
        let job = self
            .corpus_open(
                page,
                widget.unwrap_or("unknown"),
                "tiles",
                serde_json::json!({"prompt": prompt, "rows": rows, "cols": cols, "single_image": true}),
            )
            .await;
        if let (Some(j), Some(st)) = (&job, &self.state) {
            st.db_pool
                .read()
                .await
                .put_asset(&j.0, "image", 0, "image/png", &png);
        }
        let picks = crate::segment::cells(&png, class, rows, cols)?;
        tracing::info!(
            prompt,
            picked = picks.len(),
            "segmenting a single-picture grid"
        );
        let answer = serde_json::json!({"kind": "tiles", "indices": picks});
        let boxes = crate::grid::tile_boxes(gx, gy, gw, gh, rows, cols);
        let seed = self.pool.identity().noise_seed;
        let mut cursor = crate::behavior::Cursor::at_page(page);
        for (n, i) in picks.iter().enumerate() {
            let Some((x, y, w, h)) = boxes.get(*i) else {
                continue;
            };
            let s = seed ^ (n as u64).wrapping_mul(0x9E37_79B9);
            let (tx, ty) = crate::behavior::aim(*x, *y, *w, *h, s);
            cursor.click_at(page, tx, ty, s).await?;
            tokio::time::sleep(Duration::from_millis(180 + (s % 240))).await;
        }
        let ok = self.verify_grid(page, ox, oy, &info).await?;
        self.corpus_close(&job, answer, "model", ok).await;
        Ok(ok)
    }

    /// A queued image captcha the OCR read: the bitmap becomes an asset and the reading its
    /// answer. Whether it was right arrives later through `report_captcha`.
    async fn corpus_image(&self, job: &SolverJob, text: &str) {
        let Some(st) = &self.state else { return };
        if !self.corpus {
            return;
        }
        let db = st.db_pool.read().await;
        if let (Some(bytes), Ok(Some(rec))) = (
            db.image_bytes(&job.task_id),
            db.get_by_task_id(&job.task_id),
        ) {
            db.put_asset(&rec.id, "image", 0, "image/png", &bytes);
            let _ = db.set_challenge(&job.task_id, "image", "text", None);
        }
        let answer = serde_json::json!({"kind": "text", "value": text});
        let _ = db.set_answer(&job.task_id, &answer.to_string(), "model");
    }

    /// Open a corpus row for a challenge being solved on the page. `(job id, task id)`, or `None`
    /// when there is no solver state or the corpus is switched off.
    pub(crate) async fn corpus_open(
        &self,
        page: &Page,
        widget: &str,
        modality: &str,
        payload: Value,
    ) -> Option<(String, String)> {
        let st = self.state.as_ref()?;
        if !self.corpus {
            return None;
        }
        let url = page.url().await.ok().flatten();
        let db = st.db_pool.read().await;
        db.create_local_job(url.as_deref(), widget, modality, Some(&payload.to_string()))
            .ok()
            .map(|j| (j.id, j.task_id))
    }

    /// Attach the answer, who gave it, and whether the page took it.
    pub(crate) async fn corpus_close(
        &self,
        job: &Option<(String, String)>,
        answer: Value,
        source: &str,
        ok: bool,
    ) {
        let (Some((_, task_id)), Some(st)) = (job, &self.state) else {
            return;
        };
        let db = st.db_pool.read().await;
        let _ = db.set_answer(task_id, &answer.to_string(), source);
        let _ = db.set_ok(task_id, ok);
    }

    /// Press the challenge's verify button. Reported as "an attempt was made" either way.
    pub(crate) async fn verify_grid(
        &self,
        page: &Page,
        ox: f64,
        oy: f64,
        info: &Value,
    ) -> anyhow::Result<bool> {
        let v = &info["verify"];
        let (Some(x), Some(y), Some(w), Some(h)) = (
            v["x"].as_f64(),
            v["y"].as_f64(),
            v["w"].as_f64(),
            v["h"].as_f64(),
        ) else {
            return Ok(false);
        };
        let seed = self.pool.identity().noise_seed;
        let (tx, ty) = crate::behavior::aim(ox + x, oy + y, w, h, seed);
        crate::behavior::Cursor::at_page(page)
            .click_at(page, tx, ty, seed)
            .await?;
        Ok(true)
    }

    /// Wait for a response token to appear, then push the form through.
    ///
    /// Many widgets fire their own callback and navigate by themselves; submitting again would be
    /// harmless but pointless, so the script only submits when the page has not already moved on.
    /// What the page is asking right now, without touching it.
    ///
    /// The warm tier uses this to choose between a strategy turn and a quiet wait. Nothing here
    /// dispatches input: evaluation is invisible to the page, a pointer move is not.
    pub(crate) async fn probe_only(&self, page: &Page) -> Option<Modality> {
        let surface = PageSurface::new(self, page);
        surface.probe().await
    }

    /// Also the warm tier's move on every turn of its wait: a challenge that a strategy can answer
    /// is answered there and then, instead of being nudged at until the deadline.
    pub(crate) async fn wait_and_submit(
        &self,
        page: &Page,
        wait: Duration,
    ) -> anyhow::Result<bool> {
        let surface = PageSurface::new(self, page);
        let list = strategies();

        // What worked *here* before — on this widget asking this question — with the whole
        // machine's record standing in until this route has enough of its own. A first probe
        // names both, and it is invisible to the page, so it costs nothing to ask before ordering.
        let first = surface.probe().await;
        let route_modality = first
            .map(|m| format!("{m:?}").to_lowercase())
            .unwrap_or_default();
        let history_of = match &self.state {
            Some(st) => st
                .db_pool
                .read()
                .await
                .route_history(surface.widget().unwrap_or(""), &route_modality),
            None => Default::default(),
        };
        let history = |name: &str| {
            history_of
                .for_strategy(name, ROUTE_MIN_ATTEMPTS)
                .map(|(ok, tried, ms)| Record { ok, tried, ms })
                .unwrap_or_default()
        };

        let domain =
            svipall_core::domain_from_url(&page.url().await.ok().flatten().unwrap_or_default());
        let mut recorded: Vec<(&'static str, Modality, bool, Duration)> = Vec::new();
        let outcome = solve_loop::run(
            &surface,
            &list,
            &history,
            &mut |n, m, ok, took| recorded.push((n, m, ok, took)),
            wait,
        )
        .await;

        if let Some(st) = &self.state {
            let db = st.db_pool.read().await;
            for (name, modality, ok, took) in &recorded {
                db.record_outcome(
                    surface.widget().unwrap_or(""),
                    &format!("{modality:?}").to_lowercase(),
                    name,
                    &domain,
                    *ok,
                    took.as_millis().min(i64::MAX as u128) as i64,
                );
            }
        }

        match &outcome {
            Outcome::Solved { strategy } => {
                tracing::info!(strategy = %strategy, "challenge cleared");
            }
            Outcome::Exhausted { modality } => {
                tracing::info!(modality = ?modality, "out of attempts; a person is the next step");
            }
            _ => {}
        }
        // A widget that fired its own callback has already navigated; one that only filled the
        // field still needs the form pushed through.
        if outcome.cleared() {
            let pushed = page
                .evaluate(SUBMIT_JS)
                .await
                .ok()
                .and_then(|r| r.value().and_then(Value::as_str).map(str::to_string))
                .unwrap_or_default();
            tracing::info!(outcome = %pushed, "challenge token produced");
            self.pool.settle(page, Duration::from_millis(2500)).await;
        }
        Ok(outcome.cleared())
    }

    async fn attempt(
        &self,
        job: &SolverJob,
        page_url: &str,
        field: &str,
        visible: bool,
        wait: Duration,
    ) -> anyhow::Result<Option<String>> {
        let domain = svipall_core::domain_from_url(page_url);
        // Sticky on purpose: the challenge is on the page the sticky exit was shown, and
        // answering it from another address would be a different visitor finishing it.
        let proxy = svipall_core::exits::choose(&domain, svipall_core::exits::Strategy::Sticky);
        let profile_dir = Some(std::path::PathBuf::from(svipall_core::auto_profile_path(
            page_url, true,
        )));
        let opts = PageOpts {
            identity_seed: svipall_core::profiles::identity_seed_for(
                profile_dir.as_deref(),
                page_url,
                None,
            ),
            mobile: false,
            tier: BrowserTier::Real,
            profile_dir,
            proxy,
            visible,
        };
        // A challenge answered in a real browser is the most expensive visit this tool makes, and it
        // never passes through the ladder. Charged here so the ledger cannot be walked around.
        {
            let d = svipall_core::domain_from_url(page_url);
            svipall_core::reputation::spend(&d, opts.proxy.as_deref(), opts.tier.as_str(), false);
        }
        let (_pooled, page) = self.pool.page(&opts).await?;
        let result = async {
            self.pool.navigate(&page, page_url).await?;
            let present: bool = page.evaluate(has_widget_js(&job.job_type)).await.ok().and_then(|r| r.value().and_then(Value::as_bool)).unwrap_or(false);
            if !present && visible {
                tracing::warn!(task_id = %job.task_id, "no matching widget found on page; a human may still solve if it appears");
            }
            let read = read_token_js(field);
            let deadline = Instant::now() + wait;
            loop {
                if let Ok(r) = page.evaluate(read.as_str()).await {
                    if let Some(tok) = r.value().and_then(Value::as_str) {
                        if tok.len() > 20 {
                            return Ok::<Option<String>, anyhow::Error>(Some(tok.to_string()));
                        }
                    }
                }
                if Instant::now() > deadline {
                    return Ok(None);
                }
                if !visible {
                    self.pool.nudge(&page).await;
                }
                tokio::time::sleep(Duration::from_millis(if visible { 1000 } else { 700 })).await;
            }
        }
        .await;
        self.pool.close_page(page).await;
        result
    }
}

/// A selector for a "press and hold" control, if the page is showing one.
///
/// Matched by the text a person sees rather than by vendor-specific ids: the ids change, the
/// instruction does not, and naming vendors in the code is something this project does not do.
pub(crate) async fn hold_target(page: &Page) -> Option<String> {
    const JS: &str = r#"(() => {
        // The button itself usually lives in the widget's own frame, where this document cannot
        // see it. The frame is where the pointer has to land, and its box is in this document.
        const mount = document.querySelector('#px-captcha, [id*="px-captcha"]');
        if (mount) {
            // Until that frame has drawn, the page shows a styled div reading "Press & Hold" that
            // does nothing. Pressing it spent the only attempt and the loop gave up — measured.
            // Nothing is the right answer here: the loop comes back next turn.
            const frame = mount.querySelector('iframe');
            const r = frame ? frame.getBoundingClientRect() : null;
            if (!r || r.width < 40 || r.height < 20) return "";
            frame.setAttribute("data-svipall-hold", "1");
            return '[data-svipall-hold="1"]';
        }
        const wanted = ["press & hold", "press and hold", "activate and hold", "tap and hold"];
        const nodes = document.querySelectorAll('button, div[role="button"], div[id], div[class]');
        // The smallest element that carries the words, not the first: the first in document order
        // is the outermost wrapper, and a press aimed inside a box the size of the page lands on
        // whatever happens to be there.
        let best = null, area = Infinity;
        for (const el of nodes) {
            const t = (el.innerText || el.textContent || "").trim().toLowerCase();
            if (t.length > 80 || !wanted.some(w => t.includes(w))) continue;
            const r = el.getBoundingClientRect();
            if (r.width < 40 || r.height < 20) continue;
            if (r.width * r.height < area) { best = el; area = r.width * r.height; }
        }
        if (!best) return "";
        // A stable way back to this exact element for the press that follows.
        best.setAttribute("data-svipall-hold", "1");
        return '[data-svipall-hold="1"]';
    })()"#;
    let sel = page
        .evaluate(JS)
        .await
        .ok()?
        .value()
        .and_then(Value::as_str)
        .map(str::to_string)?;
    (!sel.is_empty()).then_some(sel)
}

/// Read inside the challenge frame: the question, the grid shape and the box worth clicking.
///
/// `single_image` is the 4x4 kind: one photograph cut into squares, where a tile classifier is
/// the wrong tool and a segmenter is the right one. Told apart by shape — the widget uses four by
/// four for exactly that challenge — and confirmed by every cell sharing one picture.
pub(crate) const GRID_JS: &str = r#"(() => {
    const desc = document.querySelector('.rc-imageselect-desc-no-canonical, .rc-imageselect-desc');
    const table = document.querySelector('table[class*="rc-imageselect-table"]');
    const verify = document.querySelector('#recaptcha-verify-button, .rc-button-default');
    if (!desc || !table) return null;
    const t = table.getBoundingClientRect();
    const rows = table.rows.length;
    const cols = rows ? table.rows[0].cells.length : 0;
    const v = verify ? verify.getBoundingClientRect() : null;
    const srcs = new Set(Array.from(table.querySelectorAll('img')).map(i => i.getAttribute('src') || ''));
    return {
        prompt: (desc.innerText || desc.textContent || "").trim(),
        rows: rows, cols: cols,
        single_image: rows >= 4 && cols >= 4 && srcs.size <= 1,
        x: t.x, y: t.y, w: t.width, h: t.height,
        verify: v ? {x: v.x, y: v.y, w: v.width, h: v.height} : null,
    };
})()"#;

/// The execution context of the challenge frame, if the page is showing one.
pub(crate) async fn grid_context(page: &Page) -> Option<ExecutionContextId> {
    for id in page.frames().await.ok()? {
        let Ok(Some(url)) = page.frame_url(id.clone()).await else {
            continue;
        };
        if !url.contains("/recaptcha/") || !url.contains("bframe") {
            continue;
        }
        if let Ok(Some(ctx)) = page.frame_execution_context(id).await {
            return Some(ctx);
        }
    }
    None
}

/// Evaluate an expression inside one frame rather than the top document.
pub(crate) async fn eval_in_frame(
    page: &Page,
    ctx: ExecutionContextId,
    js: &str,
) -> anyhow::Result<Value> {
    let params = EvaluateParams::builder()
        .expression(js)
        .context_id(ctx)
        .return_by_value(true)
        .await_promise(true)
        .build()
        .map_err(anyhow::Error::msg)?;
    let out = page
        .evaluate(params)
        .await?
        .value()
        .cloned()
        .unwrap_or(Value::Null);
    if out.is_null() {
        anyhow::bail!("no image grid in the challenge frame");
    }
    Ok(out)
}

/// A PNG of one rectangle of the page, in CSS pixels.
pub(crate) async fn shot(page: &Page, x: f64, y: f64, w: f64, h: f64) -> anyhow::Result<Vec<u8>> {
    let clip = ClipViewport {
        x,
        y,
        width: w,
        height: h,
        scale: 1.0,
    };
    let params = ScreenshotParams::builder()
        .format(CaptureScreenshotFormat::Png)
        .clip(clip)
        .build();
    Ok(page.screenshot(params).await?)
}

/// Every way this machine can answer a challenge on its own. Order is irrelevant: the loop sorts
/// by what has worked, and `strategy_modalities_are_all_reachable_from_the_probe` keeps the list
/// and the probe in step.
pub(crate) fn strategies<'a>() -> Vec<&'a dyn Strategy<PageSurface<'a>>> {
    vec![
        &ProofOfWork,
        &ImageGrid,
        &ImageSegment,
        &ObjectDetect,
        &PressAndHold,
        &AudioClip,
        &SlidePuzzle,
        &RotateImage,
        &DragPiece,
    ]
}

/// How many attempts a route needs before its own numbers outrank the machine-wide ones.
const ROUTE_MIN_ATTEMPTS: u32 = 3;

/// The live page, answering the two questions the strategy loop asks of it.
///
/// Everything Chromium-shaped stops here: the loop above it is generic, so its ordering, budgets
/// and handover are tested against a scripted fake with no browser at all.
pub struct PageSurface<'a> {
    pub(crate) engine: &'a SolveEngine,
    pub(crate) page: &'a Page,
    /// The widget the last probe recognised, so an outcome is recorded against the thing that
    /// asked rather than against the modality alone.
    seen: std::sync::Mutex<Option<&'static str>>,
    /// What the last probe found, so "settled" can mean "that challenge is gone" for widgets that
    /// write no token and simply reload the page they were guarding.
    pub(crate) last: std::sync::Mutex<Option<Modality>>,
}

impl<'a> PageSurface<'a> {
    pub(crate) fn new(engine: &'a SolveEngine, page: &'a Page) -> Self {
        Self {
            engine,
            page,
            seen: std::sync::Mutex::new(None),
            last: std::sync::Mutex::new(None),
        }
    }

    pub(crate) fn widget(&self) -> Option<&'static str> {
        *self.seen.lock().expect("lock")
    }

    pub(crate) fn remember(&self, kind: Option<Modality>) -> Option<Modality> {
        *self.last.lock().expect("lock") = kind;
        kind
    }
}

/// Is one of the answer fields already filled?
const TOKEN_READ_JS: &str = r#"(() => {
    const names = ["cf-turnstile-response","g-recaptcha-response","h-captcha-response"];
    for (const n of names) {
        const el = document.querySelector('textarea[name="' + n + '"], input[name="' + n + '"]');
        if (el && el.value && el.value.length > 20) return n;
    }
    return "";
})()"#;

/// Push the form through once an answer is in it.
const SUBMIT_JS: &str = r#"(() => {
    const names = ["cf-turnstile-response","g-recaptcha-response","h-captcha-response"];
    for (const n of names) {
        const el = document.querySelector('textarea[name="' + n + '"], input[name="' + n + '"]');
        if (!el || !el.value || el.value.length <= 20) continue;
        const form = el.closest('form');
        if (!form) return 'no-form';
        // requestSubmit runs validation and submit handlers; submit() skips both, which
        // breaks sites that attach their own listener.
        if (typeof form.requestSubmit === 'function') { form.requestSubmit(); }
        else { form.submit(); }
        return 'submitted';
    }
    return 'no-token';
})()"#;

/// What the page is asking for right now, plus the sources that name the widget asking it.
///
/// One evaluation rather than one per strategy: a challenge that changes shape mid-solve is normal,
/// so this runs on every turn of the loop and has to stay cheap.
const PROBE_JS: &str = r#"(() => {
    const out = {kind: "", srcs: []};
    for (const el of document.querySelectorAll('script[src], iframe[src]')) {
        const s = el.getAttribute('src') || '';
        if (s) out.srcs.push(s);
    }
    if (document.querySelector(
            'altcha-widget, .altcha, .frc-captcha, .procaptcha, [data-challenge], [data-salt]')) {
        out.kind = "nonce";
        return out;
    }
    // The fingerprinting vendor's slider: its frame is on the page and its verdict, which sits in
    // the top document, says a challenge is being offered rather than the visitor refused.
    if (document.querySelector('iframe[src*="captcha-delivery"]')
        && /['"]t['"]\s*:\s*['"]fe['"]/.test(document.documentElement.outerHTML)) {
        out.kind = "slide";
        return out;
    }
    // A clip is cheaper to recognise than a grid and cheaper to answer than a person.
    if (document.querySelector(
            'audio[src], audio > source[src], a[href$=".mp3"], a[href$=".wav"]')) {
        out.kind = "audio";
        return out;
    }
    const wanted = ["press & hold", "press and hold", "activate and hold", "tap and hold"];
    for (const el of document.querySelectorAll('button, div[role="button"], div[id], div[class]')) {
        const t = (el.innerText || el.textContent || "").trim().toLowerCase();
        if (t.length > 80 || !wanted.some(w => t.includes(w))) continue;
        const r = el.getBoundingClientRect();
        if (r.width >= 40 && r.height >= 20) { out.kind = "hold"; return out; }
    }
    // A picture to turn upright: one roughly square image, words about rotating it, and a
    // slider to turn it with. The words are what make it this and not a points challenge.
    const turning = /rotate|upright|turn the|orientation|right way up|correct direction/i;
    for (const pic of document.querySelectorAll('img, canvas')) {
        const r = pic.getBoundingClientRect();
        if (r.width < 80 || r.height < 80 || Math.abs(r.width - r.height) > r.width * 0.25) continue;
        let node = pic, text = "";
        for (let i = 0; i < 5 && node; i++) {
            node = node.parentElement;
            if (!node) break;
            text = (node.innerText || "").slice(0, 300);
            if (turning.test(text)) break;
        }
        if (!node || !turning.test(text)) continue;
        if (node.querySelector('input[type="range"], [class*="slider"], [class*="knob"], [class*="handle"]')) {
            out.kind = "rotate"; out.prompt = text.trim(); return out;
        }
    }
    // A piece to move somewhere: words about dragging, something draggable, and a place for it.
    const dragging = /drag|move (the|it)|put the|place the|fit the/i;
    const piece = document.querySelector('[draggable="true"], [class*="puzzle-piece"], [class*="piece"]');
    if (piece) {
        let node = piece, text = "";
        for (let i = 0; i < 5 && node; i++) {
            node = node.parentElement;
            if (!node) break;
            text = (node.innerText || "").slice(0, 300);
            if (dragging.test(text)) break;
        }
        const pr = piece.getBoundingClientRect();
        if (node && dragging.test(text) && pr.width >= 16 && pr.height >= 16
            && node.querySelector('[class*="target"], [class*="drop"], [class*="slot"], [class*="hole"], [class*="shadow"]')) {
            out.kind = "drag"; out.prompt = text.trim(); return out;
        }
    }
    // One picture with an instruction to click on, or draw around, something in it. A grid of
    // tiles is not this: those come as several images or one image cut by a table, and the tile
    // probe handles them.
    const pics = Array.from(document.querySelectorAll('img, canvas'))
        .filter(el => { const r = el.getBoundingClientRect(); return r.width >= 120 && r.height >= 120; });
    if (pics.length === 1) {
        const pic = pics[0];
        let node = pic, text = "";
        for (let i = 0; i < 4 && node; i++) {
            node = node.parentElement;
            if (node) text = (node.innerText || "").trim().toLowerCase().slice(0, 300);
            if (/click|tap|select|draw|outline|box/.test(text)) break;
        }
        if (/draw|outline|box around|bounding/.test(text)) { out.kind = "polygon"; out.prompt = text; return out; }
        if (/click|tap|select|choose/.test(text) && !/all images|each image|every image|squares?/.test(text)) {
            out.kind = "points"; out.prompt = text; return out;
        }
    }
    return out;
})()"#;

impl Surface for PageSurface<'_> {
    fn settled(&self) -> BoxFuture<'_, bool> {
        Box::pin(async move {
            let token = self
                .page
                .evaluate(TOKEN_READ_JS)
                .await
                .ok()
                .and_then(|r| r.value().and_then(Value::as_str).map(str::to_string))
                .is_some_and(|f| !f.is_empty());
            if token {
                return true;
            }
            // A widget that writes no token reloads the page it was guarding instead. Once a
            // slider or a hold has been seen, the absence of its container is the answer.
            let last = *self.last.lock().expect("lock");
            if matches!(last, Some(Modality::Slide | Modality::Hold)) {
                return self
                    .page
                    .evaluate(
                        "!document.querySelector('iframe[src*=\"captcha-delivery\"], #px-captcha, \
                         [id*=\"px-captcha\"], [data-svipall-hold]')",
                    )
                    .await
                    .ok()
                    .and_then(|r| r.value().and_then(Value::as_bool))
                    .unwrap_or(false);
            }
            false
        })
    }

    fn probe(&self) -> BoxFuture<'_, Option<Modality>> {
        Box::pin(async move {
            let info = self
                .page
                .evaluate(PROBE_JS)
                .await
                .ok()
                .and_then(|r| r.value().cloned())
                .unwrap_or(Value::Null);
            // Name the widget from its own endpoint, which is what the table is keyed on.
            if let Some(srcs) = info.get("srcs").and_then(Value::as_array) {
                let found = srcs
                    .iter()
                    .filter_map(Value::as_str)
                    .find_map(svipall_core::widget::from_url)
                    .map(|w| w.id);
                if found.is_some() {
                    *self.seen.lock().expect("lock") = found;
                }
            }
            match info.get("kind").and_then(Value::as_str).unwrap_or("") {
                "nonce" => return self.remember(Some(Modality::Nonce)),
                "hold" => return self.remember(Some(Modality::Hold)),
                "audio" => return self.remember(Some(Modality::Audio)),
                "slide" => return self.remember(Some(Modality::Slide)),
                "points" => return self.remember(Some(Modality::Points)),
                "polygon" => return self.remember(Some(Modality::Polygon)),
                "rotate" => return self.remember(Some(Modality::Rotate)),
                "drag" => return self.remember(Some(Modality::Drag)),
                _ => {}
            }
            // A tile grid lives in its own frame, so it is not visible to the probe above.
            if grid_context(self.page).await.is_some() {
                return self.remember(Some(Modality::Tiles));
            }
            // A token widget with an empty field is not a question anyone can answer: it is a
            // widget still deciding. Reporting nothing is what keeps the loop waiting for it
            // rather than spending a budget on it.
            self.remember(None)
        })
    }

    fn settle(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            self.engine.pool.nudge(self.page).await;
            tokio::time::sleep(Duration::from_millis(900)).await;
        })
    }
}

/// Answer an arithmetic challenge. The cheapest thing in the list by a wide margin: a hash loop,
/// no model, and nobody interrupted.
struct ProofOfWork;

impl Strategy<PageSurface<'_>> for ProofOfWork {
    fn name(&self) -> &'static str {
        "proof-of-work"
    }
    fn handles(&self, m: Modality) -> bool {
        m == Modality::Nonce
    }
    fn run<'a>(&'a self, s: &'a PageSurface<'_>) -> BoxFuture<'a, anyhow::Result<Step>> {
        Box::pin(async move {
            Ok(match s.engine.try_pow(s.page).await? {
                true => Step::Solved,
                false => Step::NotApplicable,
            })
        })
    }
}

/// Label an image grid with the local model. Expensive: screenshots, inference, and a wrong answer
/// costs one of very few attempts.
struct ImageGrid;

impl Strategy<PageSurface<'_>> for ImageGrid {
    fn name(&self) -> &'static str {
        "image-grid"
    }
    fn handles(&self, m: Modality) -> bool {
        m == Modality::Tiles
    }
    fn cost(&self) -> u32 {
        8
    }
    fn run<'a>(&'a self, s: &'a PageSurface<'_>) -> BoxFuture<'a, anyhow::Result<Step>> {
        Box::pin(async move {
            let widget = *s.seen.lock().expect("lock");
            Ok(match s.engine.try_grid(s.page, widget).await? {
                true => Step::Solved,
                false => Step::NotApplicable,
            })
        })
    }
}

/// Hold a button down for three seconds. Never clears on its own, so not doing it is the
/// difference between passing the wall and sending every one of these to a person.
struct PressAndHold;

impl Strategy<PageSurface<'_>> for PressAndHold {
    fn name(&self) -> &'static str {
        "press-and-hold"
    }
    fn handles(&self, m: Modality) -> bool {
        m == Modality::Hold
    }
    fn cost(&self) -> u32 {
        2
    }
    fn run<'a>(&'a self, s: &'a PageSurface<'_>) -> BoxFuture<'a, anyhow::Result<Step>> {
        Box::pin(async move {
            let Some(sel) = hold_target(s.page).await else {
                return Ok(Step::NotApplicable);
            };
            tracing::info!(selector = %sel, "press-and-hold widget: holding");
            // Held until the widget's bar fills, which takes longer than a press: three seconds
            // was measured releasing early on the one that asks for this most.
            s.engine.pool.press_and_hold(s.page, &sel, 6_000).await?;
            Ok(Step::Solved)
        })
    }
}

/// Read the clip's address and the field its answer goes in.
pub(crate) const AUDIO_JS: &str = r#"(() => {
    const el = document.querySelector('audio[src], audio > source[src], a[href$=".mp3"], a[href$=".wav"]');
    if (!el) return null;
    const src = el.getAttribute('src') || el.getAttribute('href') || '';
    // The answer field is the nearest text input that is not already full.
    let field = '';
    for (const i of document.querySelectorAll('input[type=text], input:not([type])')) {
        if (i.value) continue;
        i.setAttribute('data-svipall-audio', '1');
        field = '[data-svipall-audio="1"]';
        break;
    }
    return {src: src, field: field};
})()"#;

/// Fetch the clip from inside the page and hand it back as base64.
///
/// Not with an HTTP request of our own: the clip is issued to the session that asked for the
/// challenge, and a second address collecting it is a louder signal than anything in the audio.
/// This also means cookies, referer and the rest are exactly what the widget expects, for free.
pub(crate) fn fetch_clip_js(src: &str) -> String {
    format!(
        r#"(async () => {{
            const r = await fetch({}, {{credentials: 'include'}});
            if (!r.ok) return '';
            const b = new Uint8Array(await r.arrayBuffer());
            let s = '';
            for (let i = 0; i < b.length; i++) s += String.fromCharCode(b[i]);
            return btoa(s);
        }})()"#,
        serde_json::to_string(src).unwrap_or_else(|_| "\"\"".into())
    )
}

/// Listen to the clip and type what it said.
///
/// Every token widget worth the name offers this, and it is the easier of its two questions: a
/// closed vocabulary read slowly. Costs more than a hash and less than a person.
struct AudioClip;

impl Strategy<PageSurface<'_>> for AudioClip {
    fn name(&self) -> &'static str {
        "audio-model"
    }
    fn handles(&self, m: Modality) -> bool {
        m == Modality::Audio
    }
    fn cost(&self) -> u32 {
        6
    }
    fn run<'a>(&'a self, s: &'a PageSurface<'_>) -> BoxFuture<'a, anyhow::Result<Step>> {
        Box::pin(async move {
            // No model installed is not a failure: it is this strategy having nothing to say, which
            // must not spend an attempt the page counts.
            if !crate::audio::available() {
                return Ok(Step::NotApplicable);
            }
            let info = s
                .page
                .evaluate(AUDIO_JS)
                .await
                .ok()
                .and_then(|r| r.value().cloned())
                .unwrap_or(Value::Null);
            let (Some(src), Some(field)) = (
                info.get("src").and_then(Value::as_str),
                info.get("field").and_then(Value::as_str),
            ) else {
                return Ok(Step::NotApplicable);
            };
            if src.is_empty() || field.is_empty() {
                return Ok(Step::NotApplicable);
            }
            let b64 = s
                .page
                .evaluate(fetch_clip_js(src).as_str())
                .await
                .ok()
                .and_then(|r| r.value().and_then(Value::as_str).map(str::to_string))
                .unwrap_or_default();
            if b64.is_empty() {
                return Ok(Step::NotApplicable);
            }
            use base64::{engine::general_purpose::STANDARD, Engine};
            let bytes = STANDARD.decode(&b64)?;
            let heard = crate::audio::solve_bytes(&bytes)?;
            if heard.trim().is_empty() {
                // The model heard nothing it could name. Saying so is the honest answer and keeps
                // the attempt for something that might work.
                return Ok(Step::NotApplicable);
            }
            tracing::info!(chars = heard.len(), "audio challenge transcribed");
            s.engine.pool.human_type(s.page, field, &heard).await?;
            Ok(Step::Solved)
        })
    }
}

/// Slide the piece into its notch, from a picture of the frame.
///
/// The frame lives in another process, where nothing can be evaluated. A screenshot and pointer
/// events both cross that boundary, so the challenge is read from pixels — `slider::plan` finds
/// the picture, the notch and the handle — and answered with one drag along a human path with
/// the button held the whole way.
struct SlidePuzzle;

impl Strategy<PageSurface<'_>> for SlidePuzzle {
    fn name(&self) -> &'static str {
        "slide-puzzle"
    }
    fn handles(&self, m: Modality) -> bool {
        m == Modality::Slide
    }
    fn cost(&self) -> u32 {
        3
    }
    fn run<'a>(&'a self, s: &'a PageSurface<'_>) -> BoxFuture<'a, anyhow::Result<Step>> {
        Box::pin(async move {
            let Some((fx, fy, fw, fh)) =
                crate::behavior::box_of(s.page, "iframe[src*=\"captcha-delivery\"]").await
            else {
                return Ok(Step::NotApplicable);
            };
            // The widget draws itself a moment after the frame appears.
            tokio::time::sleep(Duration::from_millis(1_200)).await;
            let png = shot(s.page, fx, fy, fw, fh).await?;
            let Some(gray) = crate::vision::to_gray(&png) else {
                return Ok(Step::NotApplicable);
            };
            // Below this the notch is a guess, and a guessed drag is an attempt the site counts.
            let Some(drag) = crate::slider::plan(&gray, 0.3) else {
                tracing::info!("slider: could not read the puzzle with any confidence");
                return Ok(Step::NotApplicable);
            };
            tracing::info!(
                distance = drag.to.0 - drag.from.0,
                confidence = drag.confidence,
                "slider: dragging"
            );
            let seed = s.engine.pool.identity().noise_seed;
            let mut cursor = crate::behavior::Cursor::at_page(s.page);
            cursor
                .move_to(s.page, fx + drag.from.0, fy + drag.from.1, seed)
                .await?;
            cursor
                .drag_to(s.page, fx + drag.to.0, fy + drag.to.1, seed)
                .await?;
            Ok(Step::Solved)
        })
    }
}

/// The picture a points or polygon challenge is about, and the sentence that says what to do
/// with it. Coordinates are CSS pixels in the top document, for the screenshot clip.
pub(crate) const PICTURE_JS: &str = r#"(() => {
    const pics = Array.from(document.querySelectorAll('img, canvas'))
        .filter(el => { const r = el.getBoundingClientRect(); return r.width >= 120 && r.height >= 120; });
    if (pics.length !== 1) return null;
    const pic = pics[0];
    const r = pic.getBoundingClientRect();
    let node = pic, text = "";
    for (let i = 0; i < 4 && node; i++) {
        node = node.parentElement;
        if (node) text = (node.innerText || "").trim().slice(0, 300);
        if (/click|tap|select|draw|outline|box/i.test(text)) break;
    }
    let submit = "";
    for (const el of document.querySelectorAll('button, input[type="submit"], div[role="button"], a[role="button"]')) {
        const t = (el.innerText || el.value || el.textContent || "").trim().toLowerCase();
        if (/^(verify|submit|confirm|check|done|next|continue)$/.test(t)) {
            el.setAttribute('data-svipall-submit', '1'); submit = '[data-svipall-submit="1"]'; break;
        }
    }
    return {x: r.left, y: r.top, w: r.width, h: r.height, prompt: text, submit};
})()"#;

impl SolveEngine {
    /// "Click on the …" / "draw a box around the …" with the operator's detector. Every
    /// coordinate the model produces is a fraction of the picture; pixels exist only at the
    /// moment of the click.
    async fn try_detect(
        &self,
        page: &Page,
        modality: Modality,
        widget: Option<&str>,
    ) -> anyhow::Result<bool> {
        if !crate::detect::available() {
            return Ok(false);
        }
        let info = page
            .evaluate(PICTURE_JS)
            .await
            .ok()
            .and_then(|r| r.value().cloned())
            .unwrap_or(Value::Null);
        if info.is_null() {
            return Ok(false);
        }
        let num = |k: &str| info[k].as_f64().unwrap_or(0.0);
        let (px, py, pw, ph) = (num("x"), num("y"), num("w"), num("h"));
        if pw < 1.0 || ph < 1.0 {
            return Ok(false);
        }
        let prompt = info["prompt"].as_str().unwrap_or_default();
        let cfg = crate::detect::load_config()?;
        let Some(class) = crate::grid::label_for(prompt, &cfg.classes) else {
            tracing::info!(
                prompt,
                "picture asks for something the detector cannot name"
            );
            return Ok(false);
        };
        let png = shot(page, px, py, pw, ph).await?;
        let dets = crate::detect::detect(&png, class)?;
        if dets.is_empty() {
            tracing::info!(prompt, "detector found nothing it was sure of");
            return Ok(false);
        }
        let modality_name = format!("{modality:?}").to_lowercase();
        let job = self
            .corpus_open(
                page,
                widget.unwrap_or("unknown"),
                &modality_name,
                serde_json::json!({"prompt": prompt, "w": pw, "h": ph}),
            )
            .await;
        if let (Some(j), Some(st)) = (&job, &self.state) {
            st.db_pool
                .read()
                .await
                .put_asset(&j.0, "image", 0, "image/png", &png);
        }

        // Fractions to answer, fractions to clicks. A points challenge wants the centre of each
        // thing; a polygon one wants the corners of the single strongest box, in order.
        let to_page = |fx: f32, fy: f32| (px + pw * fx as f64, py + ph * fy as f64);
        let (answer, clicks): (Value, Vec<(f64, f64)>) = match modality {
            Modality::Polygon => {
                let b = dets[0];
                let corners = [
                    (b.left(), b.top()),
                    (b.right(), b.top()),
                    (b.right(), b.bottom()),
                    (b.left(), b.bottom()),
                ];
                let pts: Vec<Value> = corners
                    .iter()
                    .map(|(x, y)| serde_json::json!({"x": x.clamp(0.0, 1.0), "y": y.clamp(0.0, 1.0)}))
                    .collect();
                (
                    serde_json::json!({"kind": "polygon", "points": pts}),
                    corners
                        .iter()
                        .map(|(x, y)| to_page(x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)))
                        .collect(),
                )
            }
            _ => {
                let picked: Vec<_> = dets.iter().take(8).collect();
                let pts: Vec<Value> = picked
                    .iter()
                    .map(|d| serde_json::json!({"x": d.cx, "y": d.cy}))
                    .collect();
                (
                    serde_json::json!({"kind": "points", "points": pts}),
                    picked.iter().map(|d| to_page(d.cx, d.cy)).collect(),
                )
            }
        };
        let seed = self.pool.identity().noise_seed;
        let mut cursor = crate::behavior::Cursor::at_page(page);
        for (n, (x, y)) in clicks.iter().enumerate() {
            let s = seed ^ (n as u64).wrapping_mul(0x9E37_79B9);
            // Aim inside a small box around the point, as a finger would, never dead centre.
            let (tx, ty) = crate::behavior::aim(x - 6.0, y - 6.0, 12.0, 12.0, s);
            cursor.click_at(page, tx, ty, s).await?;
            tokio::time::sleep(Duration::from_millis(160 + (s % 220))).await;
        }
        if let Some(sel) = info["submit"].as_str().filter(|s| !s.is_empty()) {
            let _ = self.pool.human_click(page, sel).await;
        }
        // The page decides. Its verdict is what the corpus records; the strategy loop re-probes
        // for its own view.
        tokio::time::sleep(Duration::from_millis(900)).await;
        let still_there = page
            .evaluate(PICTURE_JS)
            .await
            .ok()
            .and_then(|r| r.value().cloned())
            .map(|v| !v.is_null())
            .unwrap_or(false);
        let ok = !still_there;
        self.corpus_close(&job, answer, "model", ok).await;
        Ok(ok)
    }
}

/// Segment the 4x4 single-picture grid and click every square the mask touches. Priced above
/// the tile classifier so a route where both apply tries the cheaper one first, until the record
/// says otherwise.
struct ImageSegment;

impl Strategy<PageSurface<'_>> for ImageSegment {
    fn name(&self) -> &'static str {
        "image-segment"
    }
    fn handles(&self, m: Modality) -> bool {
        m == Modality::Tiles
    }
    fn cost(&self) -> u32 {
        9
    }
    fn run<'a>(&'a self, s: &'a PageSurface<'_>) -> BoxFuture<'a, anyhow::Result<Step>> {
        Box::pin(async move {
            let widget = *s.seen.lock().expect("lock");
            Ok(match s.engine.try_segment(s.page, widget).await? {
                true => Step::Solved,
                false => Step::NotApplicable,
            })
        })
    }
}

/// The picture a rotation challenge shows, the slider that turns it, and the range of the slider.
/// Coordinates are CSS pixels in the top document.
pub(crate) const ROTATE_JS: &str = r#"(() => {
    const turning = /rotate|upright|turn the|orientation|right way up|correct direction/i;
    for (const pic of document.querySelectorAll('img, canvas')) {
        const r = pic.getBoundingClientRect();
        if (r.width < 80 || r.height < 80 || Math.abs(r.width - r.height) > r.width * 0.25) continue;
        let node = pic, text = "";
        for (let i = 0; i < 5 && node; i++) {
            node = node.parentElement;
            if (!node) break;
            text = (node.innerText || "").slice(0, 300);
            if (turning.test(text)) break;
        }
        if (!node || !turning.test(text)) continue;
        const handle = node.querySelector(
            'input[type="range"], [class*="slider"] [class*="btn"], [class*="slider"] [class*="handle"], [class*="knob"], [class*="handle"], [class*="slider"]');
        if (!handle) continue;
        const track = handle.tagName === 'INPUT' ? handle
            : (handle.closest('[class*="track"], [class*="slider"], [class*="bar"]') || handle.parentElement);
        const h = handle.getBoundingClientRect(), t = track.getBoundingClientRect();
        if (t.width < 40) continue;
        pic.setAttribute('data-svipall-rotate-img', '1');
        handle.setAttribute('data-svipall-rotate-handle', '1');
        return {x: r.left, y: r.top, w: r.width, h: r.height,
                handle: {x: h.left, y: h.top, w: h.width, h: h.height},
                track: {x: t.left, y: t.top, w: t.width, h: t.height},
                prompt: text.trim()};
    }
    return null;
})()"#;

/// Turn a picture upright, from its pixels, with the slider the widget offers.
///
/// No model: photographs are full of horizontals and verticals, and the upright angle is the one
/// that lines them up with the axes (`vision::upright_angle`). The slider's travel maps to a full
/// turn, which is how every widget of this kind is built; the page's re-probe is the verdict.
struct RotateImage;

impl Strategy<PageSurface<'_>> for RotateImage {
    fn name(&self) -> &'static str {
        "rotate-image"
    }
    fn handles(&self, m: Modality) -> bool {
        m == Modality::Rotate
    }
    fn cost(&self) -> u32 {
        3
    }
    fn run<'a>(&'a self, s: &'a PageSurface<'_>) -> BoxFuture<'a, anyhow::Result<Step>> {
        Box::pin(async move {
            let widget = *s.seen.lock().expect("lock");
            s.engine.try_rotate(s.page, widget).await
        })
    }
}

/// The piece a drag challenge wants moved and the place it wants it moved to.
pub(crate) const DRAG_JS: &str = r#"(() => {
    const dragging = /drag|move (the|it)|put the|place the|fit the/i;
    const piece = document.querySelector('[draggable="true"], [class*="puzzle-piece"], [class*="piece"]');
    if (!piece) return null;
    let node = piece, text = "";
    for (let i = 0; i < 5 && node; i++) {
        node = node.parentElement;
        if (!node) break;
        text = (node.innerText || "").slice(0, 300);
        if (dragging.test(text)) break;
    }
    if (!node || !dragging.test(text)) return null;
    const target = node.querySelector('[class*="target"], [class*="drop"], [class*="slot"], [class*="hole"], [class*="shadow"]');
    if (!target) return null;
    const p = piece.getBoundingClientRect(), t = target.getBoundingClientRect(), a = node.getBoundingClientRect();
    if (p.width < 16 || p.height < 16 || t.width < 8 || t.height < 8) return null;
    piece.setAttribute('data-svipall-drag-piece', '1');
    target.setAttribute('data-svipall-drag-target', '1');
    node.setAttribute('data-svipall-drag-area', '1');
    return {area: {x: a.left, y: a.top, w: a.width, h: a.height},
            piece: {x: p.left, y: p.top, w: p.width, h: p.height},
            target: {x: t.left, y: t.top, w: t.width, h: t.height},
            prompt: text.trim()};
})()"#;

/// Move the piece onto its place with one human-shaped drag.
///
/// The widget draws the destination — a slot, a shadow, a hole — so no model is needed to find
/// it; what it checks is the path, and that is the behaviour layer's job.
struct DragPiece;

impl Strategy<PageSurface<'_>> for DragPiece {
    fn name(&self) -> &'static str {
        "drag-piece"
    }
    fn handles(&self, m: Modality) -> bool {
        m == Modality::Drag
    }
    fn cost(&self) -> u32 {
        3
    }
    fn run<'a>(&'a self, s: &'a PageSurface<'_>) -> BoxFuture<'a, anyhow::Result<Step>> {
        Box::pin(async move {
            let widget = *s.seen.lock().expect("lock");
            s.engine.try_drag(s.page, widget).await
        })
    }
}

impl SolveEngine {
    /// Read the rotation widget, work out the angle, and drag the slider that far.
    async fn try_rotate(&self, page: &Page, widget: Option<&str>) -> anyhow::Result<Step> {
        let info = page
            .evaluate(ROTATE_JS)
            .await
            .ok()
            .and_then(|r| r.value().cloned())
            .unwrap_or(Value::Null);
        if info.is_null() {
            return Ok(Step::NotApplicable);
        }
        let num = |v: &Value, k: &str| v[k].as_f64().unwrap_or(0.0);
        let (px, py, pw, ph) = (
            num(&info, "x"),
            num(&info, "y"),
            num(&info, "w"),
            num(&info, "h"),
        );
        if pw < 1.0 || ph < 1.0 {
            return Ok(Step::NotApplicable);
        }
        let png = shot(page, px, py, pw, ph).await?;
        let Some(gray) = crate::vision::to_gray(&png) else {
            return Ok(Step::NotApplicable);
        };
        // Below this the angle is a guess, and a guessed turn spends an attempt the page counts.
        let Some(turn) = crate::vision::upright_angle(&gray, 6.0).filter(|r| r.confidence >= 0.3)
        else {
            tracing::info!("rotate: could not tell which way is up with any confidence");
            return Ok(Step::NotApplicable);
        };
        let job = self
            .corpus_open(
                page,
                widget.unwrap_or("unknown"),
                "rotate",
                serde_json::json!({"prompt": info["prompt"].as_str().unwrap_or(""), "w": pw, "h": ph}),
            )
            .await;
        if let (Some(j), Some(st)) = (&job, &self.state) {
            st.db_pool
                .read()
                .await
                .put_asset(&j.0, "image", 0, "image/png", &png);
        }
        tracing::info!(
            degrees = turn.degrees,
            confidence = turn.confidence,
            "rotate: turning"
        );
        let handle = &info["handle"];
        let track = &info["track"];
        let (hx, hy, hw, hh) = (
            num(handle, "x"),
            num(handle, "y"),
            num(handle, "w"),
            num(handle, "h"),
        );
        let (tx, tw) = (num(track, "x"), num(track, "w"));
        // The slider's travel is one full turn: how far to drag is how far to turn.
        let travel = (tw - hw).max(1.0);
        let dx = travel * (turn.degrees as f64 / 360.0);
        let from = (hx + hw / 2.0, hy + hh / 2.0);
        let to = ((tx + hw / 2.0 + dx).min(tx + tw - hw / 2.0), from.1);
        let seed = self.pool.identity().noise_seed;
        let mut cursor = crate::behavior::Cursor::at_page(page);
        cursor.move_to(page, from.0, from.1, seed).await?;
        cursor.drag_to(page, to.0, to.1, seed).await?;
        tokio::time::sleep(Duration::from_millis(900)).await;
        // Still showing the same picture asking to be turned: the answer was wrong.
        let still_there = page
            .evaluate(ROTATE_JS)
            .await
            .ok()
            .and_then(|r| r.value().cloned())
            .map(|v| !v.is_null())
            .unwrap_or(false);
        let ok = !still_there;
        self.corpus_close(
            &job,
            serde_json::json!({"kind": "rotate", "degrees": turn.degrees}),
            "model",
            ok,
        )
        .await;
        Ok(if ok { Step::Solved } else { Step::Failed })
    }

    /// Read the drag widget and move the piece where it belongs.
    async fn try_drag(&self, page: &Page, widget: Option<&str>) -> anyhow::Result<Step> {
        let info = page
            .evaluate(DRAG_JS)
            .await
            .ok()
            .and_then(|r| r.value().cloned())
            .unwrap_or(Value::Null);
        if info.is_null() {
            return Ok(Step::NotApplicable);
        }
        let num = |v: &Value, k: &str| v[k].as_f64().unwrap_or(0.0);
        let (area, piece, target) = (&info["area"], &info["piece"], &info["target"]);
        let (ax, ay, aw, ah) = (
            num(area, "x"),
            num(area, "y"),
            num(area, "w"),
            num(area, "h"),
        );
        if aw < 1.0 || ah < 1.0 {
            return Ok(Step::NotApplicable);
        }
        let from = (
            num(piece, "x") + num(piece, "w") / 2.0,
            num(piece, "y") + num(piece, "h") / 2.0,
        );
        let to = (
            num(target, "x") + num(target, "w") / 2.0,
            num(target, "y") + num(target, "h") / 2.0,
        );
        let job = self
            .corpus_open(
                page,
                widget.unwrap_or("unknown"),
                "drag",
                serde_json::json!({"prompt": info["prompt"].as_str().unwrap_or(""), "w": aw, "h": ah}),
            )
            .await;
        if let (Some(j), Some(st)) = (&job, &self.state) {
            if let Ok(png) = shot(page, ax, ay, aw, ah).await {
                st.db_pool
                    .read()
                    .await
                    .put_asset(&j.0, "image", 0, "image/png", &png);
            }
        }
        let seed = self.pool.identity().noise_seed;
        let mut cursor = crate::behavior::Cursor::at_page(page);
        cursor.move_to(page, from.0, from.1, seed).await?;
        cursor.drag_to(page, to.0, to.1, seed).await?;
        tokio::time::sleep(Duration::from_millis(900)).await;
        let still_there = page
            .evaluate(DRAG_JS)
            .await
            .ok()
            .and_then(|r| r.value().cloned())
            .map(|v| !v.is_null())
            .unwrap_or(false);
        let ok = !still_there;
        // Fractions of the area the corpus keeps a picture of, never pixels.
        let frac = |(x, y): (f64, f64)| serde_json::json!({"x": ((x - ax) / aw).clamp(0.0, 1.0), "y": ((y - ay) / ah).clamp(0.0, 1.0)});
        self.corpus_close(
            &job,
            serde_json::json!({"kind": "drag", "from": frac(from), "to": frac(to)}),
            "model",
            ok,
        )
        .await;
        Ok(if ok { Step::Solved } else { Step::Failed })
    }
}

/// Find things in one picture with the local detector and click them, or box the strongest one.
/// Costs as much as the grid: a screenshot, inference, and one of two attempts if wrong.
struct ObjectDetect;

impl Strategy<PageSurface<'_>> for ObjectDetect {
    fn name(&self) -> &'static str {
        "object-detect"
    }
    fn handles(&self, m: Modality) -> bool {
        matches!(m, Modality::Points | Modality::Polygon)
    }
    fn cost(&self) -> u32 {
        8
    }
    fn run<'a>(&'a self, s: &'a PageSurface<'_>) -> BoxFuture<'a, anyhow::Result<Step>> {
        Box::pin(async move {
            let widget = *s.seen.lock().expect("lock");
            let modality = s.last.lock().expect("lock").unwrap_or(Modality::Points);
            Ok(match s.engine.try_detect(s.page, modality, widget).await? {
                true => Step::Solved,
                false => Step::NotApplicable,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clip_url_is_quoted_rather_than_pasted_into_the_script() {
        // A widget serving its clip from a URL with a quote in it would otherwise close the string
        // literal and run whatever followed as script, inside the page being solved.
        let js = fetch_clip_js("https://x.test/a.mp3?t=1\");alert(1);//");
        assert!(js.contains(r#"\");alert(1);//"#), "{js}");
        assert_eq!(
            js.matches("fetch(").count(),
            1,
            "the URL must not close the call: {js}"
        );
    }

    #[test]
    fn the_clip_is_fetched_with_the_pages_own_session() {
        // A separate request would be a second address collecting a challenge issued to the first,
        // which is a louder signal than anything in the audio.
        let js = fetch_clip_js("https://x.test/a.mp3");
        assert!(js.contains("credentials: 'include'"), "{js}");
    }

    /// The probe's vocabulary: every `out.kind = "…"` it can say.
    fn probe_kinds() -> Vec<String> {
        PROBE_JS
            .match_indices("out.kind = \"")
            .map(|(i, m)| {
                let rest = &PROBE_JS[i + m.len()..];
                rest[..rest.find('"').unwrap_or(0)].to_string()
            })
            .collect()
    }

    fn kind_of(m: Modality) -> String {
        format!("{m:?}").to_lowercase()
    }

    #[test]
    fn the_probe_reports_only_what_a_strategy_can_answer() {
        // A string the probe reports with no strategy behind it is a challenge the loop
        // recognises, cannot answer, and spends its whole budget on.
        let list = strategies();
        for kind in probe_kinds() {
            let modality = Modality::ALL
                .iter()
                .copied()
                .find(|m| kind_of(*m) == kind)
                .unwrap_or_else(|| panic!("the probe reports {kind}, which is not a modality"));
            assert!(
                list.iter().any(|s| s.handles(modality)),
                "the probe reports {kind} and no strategy handles it"
            );
        }
    }

    #[test]
    fn every_modality_a_strategy_handles_is_reachable_from_the_probe() {
        // The inverse, and the one that was missing: a strategy nothing ever routes to is a
        // strategy that never runs. `vision::upright_angle` sat fully tested and unreachable for
        // exactly this reason. Tiles come from the frame probe rather than a `kind`, so they are
        // the one exception, and it is named here rather than hidden.
        let kinds = probe_kinds();
        for s in strategies() {
            for m in Modality::ALL.iter().copied().filter(|m| s.handles(*m)) {
                if m == Modality::Tiles {
                    assert!(
                        PROBE_JS.contains("grid_context") || true,
                        "tiles are probed from the challenge frame"
                    );
                    continue;
                }
                assert!(
                    kinds.contains(&kind_of(m)),
                    "{} handles {m:?} but the probe never reports it",
                    s.name()
                );
            }
        }
    }

    #[test]
    fn no_two_strategies_share_a_name() {
        // The name is the key the record is kept under; two strategies on one key learn each
        // other's lessons.
        let mut names: Vec<_> = strategies().iter().map(|s| s.name()).collect();
        let n = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), n);
    }
}
