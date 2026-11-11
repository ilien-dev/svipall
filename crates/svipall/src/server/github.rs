//! The network half of reading a code-hosting URL through its API (`svipall_core::github` is the
//! pure half: which URL maps to which endpoint, and how the answer reads).
//!
//! One hook, `github_or_page`, sits where the ladder starts, after the operator's origin policy
//! and robots.txt and inside the fetch timeout. A URL with no API shape goes on to the ladder as it
//! always did. One that has one is answered from the API (or, for a file, from the raw host), and
//! anything the API will not serve â€” a rate limit, a 404, a network error â€” goes to the ordinary
//! page instead, saying why. Nothing here ever writes a cooldown: a quota is the API's own word
//! about when to come back, not a wall.
//!
//! An API URL fetched by name (search, anything) is answered as it is, with the token and the
//! quota handled the same way.
//!
//! Unauthenticated by default, 60 requests an hour. A token named `GITHUB_TOKEN` in
//! `~/.svipall/secrets.env` is sent to the API origin only â€” never to the raw host or the page â€”
//! and is never logged or put in a result.

use super::*;
use svipall_core::github::{self as gh, Kind, RateLimit, Target};

/// Where the three hosts are. The defaults are the real ones; the variables exist so the tests can
/// replay captured answers from a local server, the page fallback included.
struct Origins {
    /// An extra origin treated as the site, besides its own hosts.
    web: Option<String>,
    api: String,
    raw: String,
}

fn env_origin(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().trim_end_matches('/').to_string())
        .filter(|v| !v.is_empty())
}

fn origins() -> Origins {
    Origins {
        web: env_origin("SVIPALL_GITHUB_WEB"),
        api: env_origin("SVIPALL_GITHUB_API").unwrap_or_else(|| gh::API_ORIGIN.into()),
        raw: env_origin("SVIPALL_GITHUB_RAW").unwrap_or_else(|| gh::RAW_ORIGIN.into()),
    }
}

/// A call about the page's markup, or a request that is not a plain read, is a call for the page.
/// This is also how a caller forces the page: any explicit `mode`.
fn wants_the_page(p: &WebFetchParams, want_links: bool) -> bool {
    want_links
        || p.mode.as_deref().is_some_and(|m| m != "auto")
        || p.css_selector.is_some()
        || p.schema.is_some()
        || p.tables.unwrap_or(false)
        || p.extraction.as_deref() == Some("html")
        || p.include_links.unwrap_or(false)
        || p.scroll.is_some()
        || p.profile.is_some()
        || p.body.is_some()
        || p.method
            .as_deref()
            .is_some_and(|m| !m.eq_ignore_ascii_case("GET"))
        || p.headers.as_ref().is_some_and(|h| !h.is_empty())
}

/// An API URL fetched by name: read as it is, but with the token and the quota understood. Only a
/// plain GET; anything else (a POST, a forced tier, a page-structure parameter) takes the ladder.
fn direct_api(p: &WebFetchParams, want_links: bool, o: &Origins) -> bool {
    p.url.starts_with(&format!("{}/", o.api))
        && !want_links
        && p.mode.as_deref().is_none_or(|m| m == "auto")
        && p.css_selector.is_none()
        && p.schema.is_none()
        && !p.tables.unwrap_or(false)
        && p.extraction.as_deref() != Some("html")
        && p.body.is_none()
        && p.method
            .as_deref()
            .is_none_or(|m| m.eq_ignore_ascii_case("GET"))
}

/// Why the API did not answer.
enum Failure {
    RateLimited(Option<RateLimit>, u16),
    Status(u16),
    Network(String),
}

/// What goes on the page result when the API was tried and the page answered instead.
struct Fallback {
    detail: Value,
    note: String,
}

impl Fallback {
    fn from(f: Failure, source: &str, authenticated: bool) -> Self {
        match f {
            Failure::RateLimited(rate, status) => {
                let until = rate.as_ref().map(RateLimit::reset_at);
                let mut detail = json!({"status": status});
                if let Some(r) = &rate {
                    detail["rate_limited_until"] = json!(r.reset_at());
                    detail["remaining"] = json!(r.remaining);
                    detail["limit"] = json!(r.limit);
                    if let Some(res) = &r.resource {
                        detail["resource"] = json!(res);
                    }
                }
                let mut note = match until {
                    Some(t) => format!("The {source} is rate limited until {t}."),
                    None => format!("The {source} is rate limited."),
                };
                note.push_str(
                    " This is the HTML page instead, which is not a wall and started no cooldown.",
                );
                if !authenticated {
                    note.push_str(&token_hint(rate.as_ref()));
                }
                Self { detail, note }
            }
            Failure::Status(status) => Self {
                detail: json!({"status": status}),
                note: format!(
                    "The {source} answered {status} for this URL (private, renamed, or a branch \
                     name with a slash in it); this is the HTML page instead."
                ),
            },
            Failure::Network(e) => Self {
                detail: json!({"error": e}),
                note: format!(
                    "The {source} could not be reached ({e}); this is the HTML page instead."
                ),
            },
        }
    }
}

/// The API origin, one request at a time, keeping what the caller is told about it.
struct Api {
    fetcher: Arc<dyn HttpFetcher>,
    base: String,
    token: Option<String>,
    attempts: Vec<String>,
    rate: Option<RateLimit>,
    calls: u32,
}

/// The token goes to the real API host, or to a loopback stand-in for it in tests; an origin
/// redirected anywhere else gets no token.
fn token_may_go_to(api_origin: &str) -> bool {
    url::Url::parse(api_origin)
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
        .is_some_and(|h| {
            h == "api.github.com" || h == "localhost" || h.starts_with("127.") || h == "[::1]"
        })
}

fn api_token(api_origin: &str) -> Option<String> {
    if !token_may_go_to(api_origin) {
        return None;
    }
    crate::secrets::load()
        .remove(gh::TOKEN_SECRET)
        .filter(|t| !t.trim().is_empty())
}

/// What the note adds when there was no token.
fn token_hint(rate: Option<&RateLimit>) -> String {
    let limit = match rate {
        Some(r) => format!(
            "{} requests{} per window",
            r.limit,
            r.resource
                .as_deref()
                .map(|res| format!(" of the {res} kind"))
                .unwrap_or_default()
        ),
        None => "a small number of requests".into(),
    };
    format!(
        " Without a token this address is allowed {limit}; a token named {} in \
         ~/.svipall/secrets.env raises it.",
        gh::TOKEN_SECRET
    )
}

impl Api {
    /// One request, whatever it answers. `headers` are the caller's; an `Authorization` among
    /// them is the caller's explicit choice and replaces the named token.
    async fn exchange(
        &mut self,
        path: &str,
        headers: &[(String, String)],
    ) -> Result<svipall_http::HttpResponse, Failure> {
        let mut req = HttpRequest::get(format!("{}{path}", self.base));
        req.set_header("accept", "application/vnd.github+json");
        req.set_header("user-agent", &self.fetcher.identity().user_agent);
        req.set_header("x-github-api-version", "2022-11-28");
        if let Some(t) = &self.token {
            req.set_header("authorization", &format!("Bearer {t}"));
        }
        for (k, v) in headers {
            if !k.eq_ignore_ascii_case("host") {
                req.set_header(k, v);
            }
        }
        let started = Instant::now();
        let sent = self.fetcher.send(req).await;
        self.calls += 1;
        let r = match sent {
            Ok(r) => r,
            Err(e) => {
                let mut e = format!("{e:#}");
                // An engine error never quotes a header today; this keeps it that way.
                if let Some(t) = self.token.as_deref().filter(|t| !t.is_empty()) {
                    e = e.replace(t, "[token]");
                }
                self.attempts.push(format!("github-api: {path} error"));
                return Err(Failure::Network(e));
            }
        };
        self.attempts.push(format!(
            "github-api: {path} {} ({}ms)",
            r.status,
            started.elapsed().as_millis()
        ));
        if let Some(rate) = RateLimit::from_headers(&r.headers) {
            self.rate = Some(rate);
        }
        Ok(r)
    }

    /// One request whose answer must be a success to be of use.
    async fn send(
        &mut self,
        path: &str,
        accept: &str,
    ) -> Result<svipall_http::HttpResponse, Failure> {
        let r = self
            .exchange(path, &[("accept".into(), accept.into())])
            .await?;
        let rate = RateLimit::from_headers(&r.headers);
        if gh::is_rate_limited(r.status, rate.as_ref(), &String::from_utf8_lossy(&r.body)) {
            return Err(Failure::RateLimited(rate, r.status));
        }
        if !(200..300).contains(&r.status) {
            return Err(Failure::Status(r.status));
        }
        Ok(r)
    }

    async fn json(&mut self, path: &str) -> Result<(Value, bool), Failure> {
        let r = self.send(path, "application/vnd.github+json").await?;
        let next = gh::has_next(r.header("link"));
        let v = serde_json::from_slice(&r.body)
            .map_err(|e| Failure::Network(format!("the answer was not JSON: {e}")))?;
        Ok((v, next))
    }

    /// `Ok(None)` for a 404, which for a README or an empty repository's files is an answer.
    async fn optional_json(&mut self, path: &str) -> Result<Option<Value>, Failure> {
        match self.json(path).await {
            Ok((v, _)) => Ok(Some(v)),
            Err(Failure::Status(404)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn optional_text(&mut self, path: &str, accept: &str) -> Result<Option<String>, Failure> {
        match self.send(path, accept).await {
            Ok(r) => Ok(Some(r.text())),
            Err(Failure::Status(404)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Every page of a list, up to `MAX_PAGES` of 100. The flag says whether that was all of it.
    async fn all(&mut self, path: &str) -> Result<(Vec<Value>, bool), Failure> {
        let first = format!("{path}?per_page=100");
        let mut items = Vec::new();
        for page in 1..=gh::MAX_PAGES {
            let url = if page == 1 {
                first.clone()
            } else {
                format!("{first}&page={page}")
            };
            let (v, next) = self.json(&url).await?;
            if let Value::Array(a) = v {
                items.extend(a);
            }
            if !next {
                return Ok((items, true));
            }
        }
        Ok((items, false))
    }
}

/// A rendered document and what the result says about it.
struct Doc {
    title: String,
    final_url: String,
    markdown: String,
    extra: serde_json::Map<String, Value>,
}

fn html_url(v: &Value, fallback: &str) -> String {
    v.get("html_url")
        .and_then(Value::as_str)
        .unwrap_or(fallback)
        .to_string()
}

impl SvipallServer {
    /// The hook at the start of the fetch path: the API for a URL that has an API shape, the
    /// ladder for everything else and for whatever the API would not serve.
    pub(super) async fn github_or_page(
        &self,
        p: &WebFetchParams,
        want_links: bool,
    ) -> FetchOutcome {
        let o = origins();
        let Some(target) = gh::parse(&p.url, o.web.as_deref()) else {
            if direct_api(p, want_links, &o) {
                return self.github_direct(p, want_links, &o).await;
            }
            return self.fetch_inner(p, want_links).await;
        };
        if wants_the_page(p, want_links) {
            return self.github_page(p, want_links, None).await;
        }
        match self.github_read(p, &target, &o).await {
            Ok(out) => out,
            Err(why) => self.github_page(p, want_links, Some(why)).await,
        }
    }

    /// The ordinary page, labelled as such, with the reason the API did not answer if it was asked.
    async fn github_page(
        &self,
        p: &WebFetchParams,
        want_links: bool,
        why: Option<Fallback>,
    ) -> FetchOutcome {
        let mut out = self.fetch_inner(p, want_links).await;
        if let Some(obj) = out.value.as_object_mut() {
            obj.insert("via".into(), json!("page"));
            if let Some(f) = why {
                obj.insert("github_api".into(), f.detail);
                let note = match obj.get("note").and_then(Value::as_str) {
                    Some(n) => format!("{} {n}", f.note),
                    None => f.note,
                };
                obj.insert("note".into(), json!(note));
            }
        }
        out
    }

    fn github_proxy(&self, p: &WebFetchParams, origin: &str) -> Option<String> {
        let d = domain_from_url(origin);
        p.proxy.clone().or_else(|| {
            if is_local(&d) {
                None
            } else {
                self.exit_for(&d)
            }
        })
    }

    /// Text that must come back as it is (a source file, an API's JSON): verbatim when it fits the
    /// budget, because the block-wise budget normalises blank lines and the indentation a block
    /// starts with, which is right for prose and wrong here. Larger than the budget, with `query`,
    /// `cursor` or `out_file`, it goes through the same budget as any page.
    fn github_verbatim(&self, value: &mut Value, content: String, p: &WebFetchParams) {
        let max = p.max_tokens.unwrap_or(self.cfg.max_tokens_per_fetch).max(1);
        let tokens = budget::estimate_tokens(&content);
        if p.out_file.is_none()
            && p.cursor.is_none()
            && p.query.as_deref().is_none_or(|q| q.trim().is_empty())
            && tokens <= max
        {
            value["chars"] = json!(content.chars().count());
            value["tokens_estimated"] = json!(tokens);
            value["content"] = json!(content);
        } else {
            self.github_deliver(value, content, p);
        }
    }

    /// An API URL the caller named: the answer as it is, status and all. A quota refusal says
    /// until when, and is the API's own word rather than a wall: no cooldown, no tier climbed.
    async fn github_direct(
        &self,
        p: &WebFetchParams,
        want_links: bool,
        o: &Origins,
    ) -> FetchOutcome {
        let path = p.url[o.api.len()..]
            .split('#')
            .next()
            .unwrap_or_default()
            .to_string();
        let mut headers: Vec<(String, String)> = p
            .headers
            .iter()
            .flatten()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        headers.sort();
        let token = api_token(&o.api);
        let authenticated = token.is_some();
        let mut api = Api {
            fetcher: self.fetcher_for(self.github_proxy(p, &o.api).as_deref()),
            base: o.api.clone(),
            token,
            attempts: Vec::new(),
            rate: None,
            calls: 0,
        };
        let r = match api.exchange(&path, &headers).await {
            Ok(r) if r.content_type.contains("json") || r.content_type.starts_with("text/") => r,
            // Not reachable, or not text (an archive behind a redirect): the ordinary ladder.
            _ => return self.fetch_inner(p, want_links).await,
        };
        let body = r.text();
        let mut github = json!({"kind": "api"});
        if authenticated {
            github["authenticated"] = json!(true);
        }
        let mut value = json!({
            "url": p.url, "final_url": r.final_url, "status": r.status, "tier_used": "http",
            "via": "github-api", "content_type": r.content_type, "attempts": api.attempts,
            "github": github,
        });
        let rate = api.rate.clone();
        if let Some(r) = &rate {
            let mut rl =
                json!({"limit": r.limit, "remaining": r.remaining, "reset_at": r.reset_at()});
            if let Some(res) = &r.resource {
                rl["resource"] = json!(res);
            }
            value["rate_limit"] = rl;
        }
        if gh::is_rate_limited(r.status, rate.as_ref(), &body) {
            let mut note = match &rate {
                Some(rl) => {
                    value["rate_limited_until"] = json!(rl.reset_at());
                    format!("The API is rate limited until {}.", rl.reset_at())
                }
                None => "The API is rate limited.".into(),
            };
            note.push_str(" This is its own answer, not a wall; no cooldown was started.");
            if !authenticated {
                note.push_str(&token_hint(rate.as_ref()));
            }
            value["note"] = json!(note);
        } else if !(200..300).contains(&r.status) {
            value["note"] = json!(format!(
                "The API answered {}; the body is its own explanation.",
                r.status
            ));
        }
        let final_url = r.final_url.clone();
        self.github_verbatim(&mut value, body, p);
        FetchOutcome {
            value,
            links: Vec::new(),
            final_url,
            blocked_body: None,
        }
    }

    /// The `query` filter, then the token budget (or `out_file`), as every other path applies them.
    fn github_deliver(&self, value: &mut Value, markdown: String, p: &WebFetchParams) {
        let content = match &p.query {
            Some(q) if !q.trim().is_empty() => extraction::bm25_filter(&markdown, q, 40),
            _ => markdown,
        };
        self.budget_into(value, content, p);
    }

    async fn github_read(
        &self,
        p: &WebFetchParams,
        t: &Target,
        o: &Origins,
    ) -> Result<FetchOutcome, Fallback> {
        if let Kind::Blob { path, lines, .. } = &t.kind {
            return self.github_blob(p, t, o, path, *lines).await;
        }
        let cache_mode = p
            .cache
            .as_deref()
            .and_then(CacheMode::parse)
            .unwrap_or(CacheMode::ReadWrite);
        // As for pages: a local origin is cached only when the caller named a cache mode.
        let cacheable = !is_local(&domain_from_url(&p.url)) || p.cache.is_some();
        let primary = t.primary_path();
        // A key of its own, so the rendered document and the page of the same URL never answer for
        // each other.
        let key = format!(
            "{}{primary}{}svipall_view=markdown",
            o.api,
            if primary.contains('?') { '&' } else { '?' }
        );
        if cacheable && cache_mode.may_read() {
            if let Some(hit) = self
                .store
                .as_ref()
                .and_then(|s| s.get(&key))
                .filter(CachedPage::is_fresh)
            {
                let mut value = json!({
                    "url": p.url, "final_url": hit.final_url, "status": hit.status,
                    "tier_used": "http", "via": "github-api", "title": hit.title,
                    "from_cache": true,
                    "attempts": [format!("cache: hit, {}s old", hit.age_secs())],
                    "github": {"kind": t.kind_name()},
                });
                let final_url = hit.final_url.clone();
                self.github_deliver(&mut value, hit.markdown, p);
                return Ok(FetchOutcome {
                    value,
                    links: Vec::new(),
                    final_url,
                    blocked_body: None,
                });
            }
        }

        let token = api_token(&o.api);
        let authenticated = token.is_some();
        let mut api = Api {
            fetcher: self.fetcher_for(self.github_proxy(p, &o.api).as_deref()),
            base: o.api.clone(),
            token,
            attempts: Vec::new(),
            rate: None,
            calls: 0,
        };
        let doc = match Self::github_document(&mut api, t, &p.url).await {
            Ok(d) => d,
            Err(f) => return Err(Fallback::from(f, "API", authenticated)),
        };

        if cacheable && cache_mode.may_write() {
            if let Some(store) = &self.store {
                if let Err(e) = store.put(
                    &key,
                    &doc.final_url,
                    200,
                    "github-api",
                    None,
                    None,
                    "text/markdown",
                    Some(&doc.title),
                    &doc.markdown,
                    600,
                    None,
                ) {
                    tracing::debug!("cache write skipped: {e}");
                }
            }
        }

        let mut github = json!({"kind": t.kind_name(), "api_calls": api.calls});
        if authenticated {
            github["authenticated"] = json!(true);
        }
        for (k, v) in doc.extra {
            github[k] = v;
        }
        let mut value = json!({
            "url": p.url, "final_url": doc.final_url, "status": 200, "tier_used": "http",
            "via": "github-api", "title": doc.title, "attempts": api.attempts, "github": github,
        });
        if let Some(r) = &api.rate {
            let mut rl =
                json!({"limit": r.limit, "remaining": r.remaining, "reset_at": r.reset_at()});
            if let Some(res) = &r.resource {
                rl["resource"] = json!(res);
            }
            value["rate_limit"] = rl;
        }
        self.github_deliver(&mut value, doc.markdown, p);
        Ok(FetchOutcome {
            value,
            links: Vec::new(),
            final_url: doc.final_url,
            blocked_body: None,
        })
    }

    async fn github_document(api: &mut Api, t: &Target, url: &str) -> Result<Doc, Failure> {
        let repo_path = t.repo_path();
        let mut extra = serde_json::Map::new();
        let doc = match &t.kind {
            Kind::Root => {
                let (repo, _) = api.json(&repo_path).await?;
                let contents = api
                    .optional_json(&gh::contents_path(&repo_path, "", None))
                    .await?;
                let readme = api
                    .optional_text(&format!("{repo_path}/readme"), "application/vnd.github.raw")
                    .await?;
                Doc {
                    title: repo["full_name"].as_str().unwrap_or(url).to_string(),
                    final_url: html_url(&repo, url),
                    markdown: gh::render_repo(&repo, contents.as_ref(), readme.as_deref()),
                    extra,
                }
            }
            Kind::Tree { git_ref, path } => {
                let (contents, _) = api.json(&t.primary_path()).await?;
                // A file named by a tree URL: say so rather than pretend it is a directory.
                let listing = if contents.is_array() {
                    contents
                } else {
                    Value::Array(vec![contents])
                };
                Doc {
                    title: format!("{}/{}/{path} at {git_ref}", t.owner, t.repo),
                    final_url: url.to_string(),
                    markdown: gh::render_listing(t, git_ref, path, &listing),
                    extra,
                }
            }
            Kind::Issue(_) | Kind::Pull(_) => {
                let (issue, _) = api.json(&t.primary_path()).await?;
                let number = issue["number"].as_u64().unwrap_or(0);
                let total = issue["comments"].as_u64().unwrap_or(0);
                let (comments, complete) = if total > 0 {
                    api.all(&format!("{repo_path}/issues/{number}/comments"))
                        .await?
                } else {
                    (Vec::new(), true)
                };
                extra.insert(
                    "comments".into(),
                    json!({"fetched": comments.len(), "total": total}),
                );
                let markdown = if issue.get("pull_request").is_some() {
                    let pulls = format!("{repo_path}/pulls/{number}");
                    let (pull, _) = api.json(&pulls).await?;
                    let (files, files_complete) = if pull["changed_files"].as_u64().unwrap_or(0) > 0
                    {
                        api.all(&format!("{pulls}/files")).await?
                    } else {
                        (Vec::new(), true)
                    };
                    let (review_comments, _) = if pull["review_comments"].as_u64().unwrap_or(0) > 0
                    {
                        api.all(&format!("{pulls}/comments")).await?
                    } else {
                        (Vec::new(), true)
                    };
                    let (reviews, _) = api.all(&format!("{pulls}/reviews")).await?;
                    extra.insert(
                        "pull_request".into(),
                        json!({"files": files.len(), "reviews": reviews.len(),
                               "review_comments": review_comments.len()}),
                    );
                    gh::render_thread(
                        &issue,
                        &comments,
                        complete,
                        Some(&gh::PullParts {
                            pull: Some(&pull),
                            files: &files,
                            files_complete,
                            reviews: &reviews,
                            review_comments: &review_comments,
                        }),
                    )
                } else {
                    gh::render_thread(&issue, &comments, complete, None)
                };
                Doc {
                    title: format!("{} (#{number})", issue["title"].as_str().unwrap_or("")),
                    final_url: html_url(&issue, url),
                    markdown,
                    extra,
                }
            }
            Kind::Releases { page } => {
                let (list, next) = api.json(&t.primary_path()).await?;
                let list = list.as_array().cloned().unwrap_or_default();
                let next_url = next.then(|| {
                    format!(
                        "{}/{}/{}/releases?page={}",
                        gh::WEB_ORIGIN,
                        t.owner,
                        t.repo,
                        page + 1
                    )
                });
                Doc {
                    title: format!("Releases of {}/{}", t.owner, t.repo),
                    final_url: url.to_string(),
                    markdown: gh::render_releases(t, &list, *page, next_url.as_deref()),
                    extra,
                }
            }
            Kind::Release(_) | Kind::LatestRelease => {
                let (r, _) = api.json(&t.primary_path()).await?;
                let tag = r["tag_name"].as_str().unwrap_or_default();
                let title = match r["name"].as_str() {
                    Some(n) if !n.is_empty() && n != tag => format!("{tag}: {n}"),
                    _ => tag.to_string(),
                };
                Doc {
                    title,
                    final_url: html_url(&r, url),
                    markdown: gh::render_release(&r),
                    extra,
                }
            }
            Kind::Commit(_) => {
                let (c, more_files) = api.json(&t.primary_path()).await?;
                let mut markdown = gh::render_commit(&c);
                if more_files {
                    markdown.push_str("(more files than the API puts on one page; only the first page is read here)\n");
                }
                Doc {
                    title: c["commit"]["message"]
                        .as_str()
                        .and_then(|m| m.lines().next())
                        .unwrap_or("commit")
                        .to_string(),
                    final_url: html_url(&c, url),
                    markdown,
                    extra,
                }
            }
            Kind::Blob { .. } => unreachable!("blobs are read from the raw host"),
        };
        Ok(doc)
    }

    /// A file: its bytes from the raw host, which costs no API quota.
    ///
    /// A text file comes back as it is (`github_verbatim`). A file that is not text goes through the ordinary http tier at its raw URL, so a binary, a PDF or an `out_file`
    /// is handled as for any URL. A line anchor returns those lines, and says how many there are.
    async fn github_blob(
        &self,
        p: &WebFetchParams,
        t: &Target,
        o: &Origins,
        path: &str,
        lines: Option<(usize, usize)>,
    ) -> Result<FetchOutcome, Fallback> {
        let raw_url = format!("{}{}", o.raw, t.primary_path());
        let raw_fault = |f: Failure| Fallback::from(f, "raw file host", true);

        let fetcher = self.fetcher_for(self.github_proxy(p, &o.raw).as_deref());
        let mut req = HttpRequest::get(raw_url.clone());
        req.set_header("user-agent", &fetcher.identity().user_agent);
        let started = Instant::now();
        let r = fetcher
            .send(req)
            .await
            .map_err(|e| raw_fault(Failure::Network(format!("{e:#}"))))?;
        let attempt = format!(
            "github-raw: {} {} ({}ms)",
            t.primary_path(),
            r.status,
            started.elapsed().as_millis()
        );
        if !(200..300).contains(&r.status) {
            return Err(raw_fault(Failure::Status(r.status)));
        }
        let Some(text) = String::from_utf8(r.body).ok().filter(|t| !t.contains('\0')) else {
            let mut out = self
                .fetch_inner(
                    &WebFetchParams {
                        url: raw_url.clone(),
                        ..p.clone()
                    },
                    false,
                )
                .await;
            let status = out.value["status"].as_u64().unwrap_or(0) as u16;
            if out.value.get("blocked_reason").is_some() || !(200..300).contains(&status) {
                return Err(raw_fault(Failure::Status(status)));
            }
            if let Some(obj) = out.value.as_object_mut() {
                obj.insert("url".into(), json!(p.url));
                obj.insert("final_url".into(), json!(raw_url));
                obj.insert("via".into(), json!("github-raw"));
                if lines.is_some() {
                    obj.insert(
                        "lines_ignored".into(),
                        json!("the file is not text, so the line anchor does not apply"),
                    );
                }
            }
            return Ok(out);
        };

        let mut value = json!({
            "url": p.url, "final_url": raw_url, "status": 200, "tier_used": "http",
            "via": "github-raw", "title": path, "attempts": [attempt],
        });
        let content = match lines {
            None => text,
            Some((from, to)) => {
                let (content, total) = gh::slice_lines(&text, from, to);
                value["lines"] = json!({"from": from, "to": to.min(total), "of": total});
                if from > total {
                    value["note"] = json!(format!(
                        "The file has {total} lines, so lines {from}-{to} are empty. The whole \
                         file is the same URL without the #L anchor."
                    ));
                }
                content
            }
        };
        self.github_verbatim(&mut value, content, p);
        Ok(FetchOutcome {
            value,
            links: Vec::new(),
            final_url: raw_url,
            blocked_body: None,
        })
    }
}
