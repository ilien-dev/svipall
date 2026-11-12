//! Code-hosting URLs mapped to the API that serves them, and the API's answers as markdown.
//!
//! A repository, issue or pull-request page on github.com is built by script. Read as HTML it
//! came back as empty tables, and agents fell back to a command-line client to read what the
//! public REST API serves in one call. This module is the pure half of reading it that way: which
//! URL shapes map to which endpoints, and how the JSON reads as a document. The network half lives
//! in the server, which falls back to the ordinary page for everything `parse` returns `None` for.
//!
//! Nothing is dropped from what was fetched: every comment, review and file that came back is in
//! the markdown, and where the API had more than was read (a page cap) the document says so.

use serde_json::Value;

/// The web origin whose URLs are mapped.
pub const WEB_ORIGIN: &str = "https://github.com";
/// Where the REST API answers. The token, when there is one, goes here and nowhere else.
pub const API_ORIGIN: &str = "https://api.github.com";
/// Where a file's bytes are served as they are, outside the API's request quota.
pub const RAW_ORIGIN: &str = "https://raw.githubusercontent.com";
/// The name in `~/.svipall/secrets.env` whose value is sent as the API token.
pub const TOKEN_SECRET: &str = "GITHUB_TOKEN";
/// Pages of 100 read from one paginated list (comments, files, reviews) before stopping.
pub const MAX_PAGES: u32 = 10;

/// First path segments that are the site's own pages, never an owner.
const RESERVED: &[&str] = &[
    "about",
    "apps",
    "collections",
    "contact",
    "customer-stories",
    "dashboard",
    "enterprise",
    "events",
    "explore",
    "features",
    "issues",
    "login",
    "logout",
    "marketplace",
    "new",
    "notifications",
    "orgs",
    "organizations",
    "pricing",
    "pulls",
    "search",
    "security",
    "sessions",
    "settings",
    "signup",
    "site",
    "sponsors",
    "stars",
    "topics",
    "trending",
    "users",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub owner: String,
    pub repo: String,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// The repository's front page.
    Root,
    /// A directory. `git_ref` is the first segment after `tree`: a branch whose name has a slash
    /// is ambiguous here, the API answers 404, and the server falls back to the page.
    Tree {
        git_ref: String,
        path: String,
    },
    /// A file, with the `#L10-L40` range when the URL carries one.
    Blob {
        git_ref: String,
        path: String,
        lines: Option<(usize, usize)>,
    },
    Issue(u64),
    Pull(u64),
    Releases {
        page: u32,
    },
    Release(String),
    LatestRelease,
    Commit(String),
}

fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// `L10-L40`, `L10`, `L10C3-L40C9`: the line range a blob URL's fragment names.
pub fn line_range(fragment: &str) -> Option<(usize, usize)> {
    let line = |s: &str| -> Option<usize> {
        let s = s.strip_prefix('L')?;
        let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse().ok().filter(|n| *n > 0)
    };
    match fragment.split_once('-') {
        Some((a, b)) => {
            let (a, b) = (line(a)?, line(b)?);
            Some((a.min(b), a.max(b)))
        }
        None => line(fragment).map(|n| (n, n)),
    }
}

/// Map a URL to what the API serves for it, or `None` when it has no API shape here and the page
/// is the only reading. `web_origin` is an extra origin treated as the site (tests point it at a
/// local server); the site's own hosts always match.
pub fn parse(url: &str, web_origin: Option<&str>) -> Option<Target> {
    let u = url::Url::parse(url).ok()?;
    if !matches!(u.scheme(), "http" | "https") {
        return None;
    }
    let host = u.host_str()?.to_ascii_lowercase();
    let on_site = matches!(host.as_str(), "github.com" | "www.github.com")
        || web_origin
            .and_then(|o| url::Url::parse(o).ok())
            .is_some_and(|o| {
                o.host_str() == u.host_str()
                    && o.port_or_known_default() == u.port_or_known_default()
            });
    if !on_site {
        return None;
    }
    let segs: Vec<&str> = u.path_segments()?.filter(|s| !s.is_empty()).collect();
    if segs.len() < 2 {
        return None;
    }
    let owner = segs[0];
    let repo = segs[1].strip_suffix(".git").unwrap_or(segs[1]);
    if RESERVED.contains(&owner.to_ascii_lowercase().as_str())
        || !valid_name(owner)
        || !valid_name(repo)
    {
        return None;
    }
    let number = |s: &str| s.parse::<u64>().ok().filter(|n| *n > 0);
    let kind = match segs.get(2).copied() {
        None => Kind::Root,
        Some("tree") => {
            let git_ref = segs.get(3)?.to_string();
            Kind::Tree {
                git_ref,
                path: segs[4..].join("/"),
            }
        }
        Some("blob") if segs.len() >= 5 => Kind::Blob {
            git_ref: segs[3].to_string(),
            path: segs[4..].join("/"),
            lines: u.fragment().and_then(line_range),
        },
        Some("issues") if segs.len() == 4 => Kind::Issue(number(segs[3])?),
        Some("pull") if segs.len() == 4 || segs.len() == 5 => {
            if let Some(tab) = segs.get(4) {
                if !matches!(*tab, "files" | "commits" | "checks") {
                    return None;
                }
            }
            Kind::Pull(number(segs[3])?)
        }
        Some("releases") => match (segs.get(3).copied(), segs.len()) {
            (None, _) => Kind::Releases {
                page: u
                    .query_pairs()
                    .find(|(k, _)| k == "page")
                    .and_then(|(_, v)| v.parse().ok())
                    .filter(|p| *p > 0)
                    .unwrap_or(1),
            },
            (Some("latest"), 4) => Kind::LatestRelease,
            (Some("tag"), n) if n >= 5 => Kind::Release(segs[4..].join("/")),
            _ => return None,
        },
        Some("commit") if segs.len() == 4 => {
            let sha = segs[3];
            if !(7..=40).contains(&sha.len()) || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }
            Kind::Commit(sha.to_string())
        }
        _ => return None,
    };
    Some(Target {
        owner: owner.to_string(),
        repo: repo.to_string(),
        kind,
    })
}

impl Target {
    pub fn repo_path(&self) -> String {
        format!("/repos/{}/{}", self.owner, self.repo)
    }

    /// The endpoint whose answer is the document. For a blob this is the raw-host path instead.
    pub fn primary_path(&self) -> String {
        let r = self.repo_path();
        match &self.kind {
            Kind::Root => r,
            Kind::Tree { git_ref, path } => contents_path(&r, path, Some(git_ref)),
            Kind::Blob { git_ref, path, .. } => {
                format!("/{}/{}/{git_ref}/{path}", self.owner, self.repo)
            }
            Kind::Issue(n) | Kind::Pull(n) => format!("{r}/issues/{n}"),
            Kind::Releases { page: 1 } => format!("{r}/releases?per_page=10"),
            Kind::Releases { page } => format!("{r}/releases?per_page=10&page={page}"),
            Kind::Release(tag) => format!("{r}/releases/tags/{tag}"),
            Kind::LatestRelease => format!("{r}/releases/latest"),
            Kind::Commit(sha) => format!("{r}/commits/{sha}"),
        }
    }

    /// What kind of document this is, for the result's `github.kind`.
    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            Kind::Root => "repository",
            Kind::Tree { .. } => "tree",
            Kind::Blob { .. } => "blob",
            Kind::Issue(_) => "issue",
            Kind::Pull(_) => "pull_request",
            Kind::Releases { .. } => "releases",
            Kind::Release(_) | Kind::LatestRelease => "release",
            Kind::Commit(_) => "commit",
        }
    }
}

/// `/repos/o/r/contents[/path][?ref=x]`.
pub fn contents_path(repo_path: &str, path: &str, git_ref: Option<&str>) -> String {
    let mut s = format!("{repo_path}/contents");
    if !path.is_empty() {
        s.push('/');
        s.push_str(path);
    }
    if let Some(r) = git_ref {
        s.push_str("?ref=");
        s.push_str(r);
    }
    s
}

/// What the API said about its quota on its last answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimit {
    pub limit: u64,
    pub remaining: u64,
    /// Unix seconds.
    pub reset: i64,
    pub resource: Option<String>,
}

impl RateLimit {
    pub fn from_headers(headers: &[(String, String)]) -> Option<Self> {
        let get = |name: &str| {
            headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.trim().to_string())
        };
        Some(Self {
            limit: get("x-ratelimit-limit")?.parse().ok()?,
            remaining: get("x-ratelimit-remaining")?.parse().ok()?,
            reset: get("x-ratelimit-reset")?.parse().ok()?,
            resource: get("x-ratelimit-resource"),
        })
    }

    pub fn reset_at(&self) -> String {
        rfc3339(self.reset)
    }
}

/// A refusal for quota rather than for access: 429, or 403 with the quota spent or saying so.
/// A 403 for anything else (a blocked repository, a missing scope) is not one.
pub fn is_rate_limited(status: u16, rate: Option<&RateLimit>, body: &str) -> bool {
    status == 429
        || (status == 403
            && (rate.is_some_and(|r| r.remaining == 0)
                || body.to_ascii_lowercase().contains("rate limit")))
}

/// When a quota refusal may be asked again, and which limit it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refusal {
    /// Unix seconds.
    pub until: i64,
    /// Too many requests too quickly rather than the hourly quota spent. A token does not lift it.
    pub secondary: bool,
}

/// The time to come back after a quota refusal, in the order the API documents: its
/// `Retry-After`; else the quota's reset, when the quota is spent; else a minute.
pub fn refusal(
    headers: &[(String, String)],
    rate: Option<&RateLimit>,
    body: &str,
    now: i64,
) -> Refusal {
    let retry_after = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("retry-after"))
        .and_then(|(_, v)| crate::throttle::parse_retry_after(v))
        .map(|d| d.as_secs() as i64);
    let spent = rate.filter(|r| r.remaining == 0);
    let secondary = retry_after.is_some()
        || rate.is_some_and(|r| r.remaining > 0)
        || body.to_ascii_lowercase().contains("secondary rate limit");
    let until = match (retry_after, spent) {
        (Some(s), _) => now + s,
        (None, Some(r)) => r.reset,
        (None, None) => now + 60,
    };
    Refusal { until, secondary }
}

/// Does a `Link` header name a next page?
pub fn has_next(link: Option<&str>) -> bool {
    link.is_some_and(|l| l.split(',').any(|part| part.contains("rel=\"next\"")))
}

/// Unix seconds as `YYYY-MM-DDTHH:MM:SSZ`, without pulling in a date crate for one format.
pub fn rfc3339(unix: i64) -> String {
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

/// Lines `from..=to` (1-based) of a text, and how many lines it has.
pub fn slice_lines(text: &str, from: usize, to: usize) -> (String, usize) {
    let total = text.lines().count();
    let picked: Vec<&str> = text
        .lines()
        .skip(from.saturating_sub(1))
        .take(to.saturating_sub(from) + 1)
        .collect();
    (picked.join("\n"), total)
}

// ---- rendering ------------------------------------------------------------------------------

fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn n(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn login(v: &Value) -> String {
    match v
        .get("user")
        .or_else(|| v.get("author"))
        .and_then(|u| u.get("login"))
    {
        Some(Value::String(l)) => format!("@{l}"),
        _ => "@ghost".into(),
    }
}

fn body_or(v: &Value, key: &str, none: &str) -> String {
    match v.get(key).and_then(Value::as_str).map(str::trim) {
        Some(b) if !b.is_empty() => b.replace("\r\n", "\n"),
        _ => none.to_string(),
    }
}

/// `+1 5, heart 1`, or nothing when nobody reacted.
pub fn reactions(v: &Value) -> Option<String> {
    let r = v.get("reactions")?;
    let parts: Vec<String> = [
        "+1", "-1", "laugh", "hooray", "confused", "heart", "rocket", "eyes",
    ]
    .iter()
    .filter_map(|k| {
        let c = r.get(*k).and_then(Value::as_u64).unwrap_or(0);
        (c > 0).then(|| format!("{k} {c}"))
    })
    .collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// A fence longer than any run of backticks inside, so a patch that contains one stays inside.
fn fence(text: &str, lang: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let f = "`".repeat(longest.max(2) + 1);
    format!("{f}{lang}\n{}\n{f}", text.trim_end_matches('\n'))
}

fn size(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.1} KB", b as f64 / 1024.0),
        b => format!("{b} B"),
    }
}

/// One line per entry of a contents listing, directories first, as the site shows them.
pub fn listing_lines(contents: &Value) -> Vec<String> {
    let Some(items) = contents.as_array() else {
        return Vec::new();
    };
    let mut items: Vec<&Value> = items.iter().collect();
    items.sort_by_key(|i| (s(i, "type") != "dir", s(i, "name").to_ascii_lowercase()));
    items
        .into_iter()
        .map(|i| match s(i, "type") {
            "dir" => format!("- {}/", s(i, "name")),
            "symlink" => format!("- {} (symlink)", s(i, "name")),
            "submodule" => format!("- {} (submodule)", s(i, "name")),
            _ => format!("- {} ({})", s(i, "name"), size(n(i, "size"))),
        })
        .collect()
}

/// The repository's front page: what it is, the facts people look for, its files and README.
pub fn render_repo(repo: &Value, contents: Option<&Value>, readme: Option<&str>) -> String {
    let mut out = format!("# {}\n\n", s(repo, "full_name"));
    let description = body_or(repo, "description", "");
    if !description.is_empty() {
        out.push_str(&description);
        out.push_str("\n\n");
    }
    let licence = repo
        .get("license")
        .filter(|l| !l.is_null())
        .map(|l| match s(l, "spdx_id") {
            "" | "NOASSERTION" => s(l, "name").to_string(),
            id => id.to_string(),
        })
        .unwrap_or_else(|| "none stated".into());
    let mut facts = vec![
        format!(
            "- stars: {}, forks: {}, watchers: {}, open issues and pull requests: {}",
            n(repo, "stargazers_count"),
            n(repo, "forks_count"),
            n(repo, "subscribers_count"),
            n(repo, "open_issues_count")
        ),
        format!("- licence: {licence}"),
        format!("- default branch: {}", s(repo, "default_branch")),
        format!("- last push: {}", s(repo, "pushed_at")),
        format!("- created: {}", s(repo, "created_at")),
    ];
    if !s(repo, "language").is_empty() {
        facts.push(format!("- language: {}", s(repo, "language")));
    }
    if let Some(topics) = repo.get("topics").and_then(Value::as_array) {
        let t: Vec<&str> = topics.iter().filter_map(Value::as_str).collect();
        if !t.is_empty() {
            facts.push(format!("- topics: {}", t.join(", ")));
        }
    }
    if !s(repo, "homepage").is_empty() {
        facts.push(format!("- homepage: {}", s(repo, "homepage")));
    }
    if repo.get("archived") == Some(&Value::Bool(true)) {
        facts.push("- archived: read-only".into());
    }
    if let Some(parent) = repo.get("parent") {
        facts.push(format!("- fork of: {}", s(parent, "full_name")));
    }
    out.push_str(&facts.join("\n"));
    out.push_str("\n\n## Files\n\n");
    match contents.map(listing_lines) {
        Some(lines) if !lines.is_empty() => out.push_str(&lines.join("\n")),
        _ => out.push_str("(no files: the repository is empty or its listing was not served)"),
    }
    out.push_str("\n\n## README\n\n");
    match readme.map(str::trim) {
        Some(r) if !r.is_empty() => out.push_str(&r.replace("\r\n", "\n")),
        _ => out.push_str("(no README)"),
    }
    out.push('\n');
    out
}

/// A directory listing.
pub fn render_listing(t: &Target, git_ref: &str, path: &str, contents: &Value) -> String {
    let place = if path.is_empty() {
        format!("{}/{}", t.owner, t.repo)
    } else {
        format!("{}/{}/{path}", t.owner, t.repo)
    };
    let lines = listing_lines(contents);
    format!(
        "# {place} at {git_ref}\n\n{} entries\n\n{}\n",
        lines.len(),
        lines.join("\n")
    )
}

/// The parts of a pull request beyond its issue.
#[derive(Debug, Default)]
pub struct PullParts<'a> {
    pub pull: Option<&'a Value>,
    pub files: &'a [Value],
    pub files_complete: bool,
    pub reviews: &'a [Value],
    pub review_comments: &'a [Value],
}

/// An issue or pull request: its facts, its description, and every comment, review and inline
/// review comment that was fetched, in the order they were written.
pub fn render_thread(
    issue: &Value,
    comments: &[Value],
    comments_complete: bool,
    pr: Option<&PullParts>,
) -> String {
    let number = n(issue, "number");
    let mut out = format!("# {} (#{number})\n\n", s(issue, "title"));
    let state = match s(issue, "state_reason") {
        "" => s(issue, "state").to_string(),
        why => format!("{} ({why})", s(issue, "state")),
    };
    let labels: Vec<&str> = issue
        .get("labels")
        .and_then(Value::as_array)
        .map(|a| a.iter().map(|l| s(l, "name")).collect())
        .unwrap_or_default();
    let mut facts = vec![
        format!(
            "- {}: {state}",
            if pr.is_some() {
                "pull request"
            } else {
                "issue"
            }
        ),
        format!(
            "- author: {}, opened {}",
            login(issue),
            s(issue, "created_at")
        ),
    ];
    if !s(issue, "closed_at").is_empty() {
        facts.push(format!("- closed: {}", s(issue, "closed_at")));
    }
    facts.push(format!(
        "- labels: {}",
        if labels.is_empty() {
            "none".to_string()
        } else {
            labels.join(", ")
        }
    ));
    if let Some(m) = issue.get("milestone").filter(|m| !m.is_null()) {
        facts.push(format!("- milestone: {}", s(m, "title")));
    }
    let assignees: Vec<String> = issue
        .get("assignees")
        .and_then(Value::as_array)
        .map(|a| a.iter().map(|u| format!("@{}", s(u, "login"))).collect())
        .unwrap_or_default();
    if !assignees.is_empty() {
        facts.push(format!("- assignees: {}", assignees.join(", ")));
    }
    facts.push(format!("- comments: {}", n(issue, "comments")));
    if let Some(r) = reactions(issue) {
        facts.push(format!("- reactions: {r}"));
    }
    if let Some(pull) = pr.and_then(|p| p.pull) {
        let head = s(&pull["head"], "label");
        facts.push(format!("- branches: {} <- {head}", s(&pull["base"], "ref")));
        facts.push(match s(pull, "merged_at") {
            "" if pull.get("draft") == Some(&Value::Bool(true)) => "- merged: no (draft)".into(),
            "" => "- merged: no".into(),
            at => format!("- merged: {at}"),
        });
        facts.push(format!(
            "- commits: {}, +{} -{}, changed files: {}",
            n(pull, "commits"),
            n(pull, "additions"),
            n(pull, "deletions"),
            n(pull, "changed_files")
        ));
    }
    out.push_str(&facts.join("\n"));
    out.push_str("\n\n");
    out.push_str(&body_or(issue, "body", "(no description)"));
    out.push_str("\n\n");

    if let Some(p) = pr {
        if !p.files.is_empty() {
            out.push_str(&format!("## Changed files ({})\n\n", p.files.len()));
            for f in p.files {
                let renamed = match s(f, "previous_filename") {
                    "" => String::new(),
                    old => format!(" (from `{old}`)"),
                };
                out.push_str(&format!(
                    "- `{}`{renamed} {} +{} -{}\n",
                    s(f, "filename"),
                    s(f, "status"),
                    n(f, "additions"),
                    n(f, "deletions")
                ));
            }
            if !p.files_complete {
                out.push_str(&format!(
                    "\n(the first {} files; the API serves no more than that here)\n",
                    p.files.len()
                ));
            }
            out.push('\n');
        }
    }

    // One timeline: comments, reviews and inline review comments, oldest first.
    let mut timeline: Vec<(String, String)> = Vec::new();
    for c in comments {
        let mut head = format!("### {}, {}", login(c), s(c, "created_at"));
        if let Some(r) = reactions(c) {
            head.push_str(&format!("\n\nreactions: {r}"));
        }
        timeline.push((
            s(c, "created_at").to_string(),
            format!("{head}\n\n{}", body_or(c, "body", "(empty)")),
        ));
    }
    if let Some(p) = pr {
        for r in p.reviews {
            let when = s(r, "submitted_at").to_string();
            let body = body_or(r, "body", "");
            let mut block = format!("### {}, review {}, {when}", login(r), s(r, "state"));
            if !body.is_empty() {
                block.push_str("\n\n");
                block.push_str(&body);
            }
            timeline.push((when, block));
        }
        for c in p.review_comments {
            let line = c
                .get("line")
                .or_else(|| c.get("original_line"))
                .and_then(Value::as_u64)
                .map(|l| format!(" line {l}"))
                .unwrap_or_default();
            let mut head = format!(
                "### {}, on `{}`{line}, {}",
                login(c),
                s(c, "path"),
                s(c, "created_at")
            );
            if let Some(r) = reactions(c) {
                head.push_str(&format!("\n\nreactions: {r}"));
            }
            let hunk = s(c, "diff_hunk");
            let hunk = if hunk.is_empty() {
                String::new()
            } else {
                format!("\n\n{}", fence(hunk, "diff"))
            };
            timeline.push((
                s(c, "created_at").to_string(),
                format!("{head}{hunk}\n\n{}", body_or(c, "body", "(empty)")),
            ));
        }
    }
    timeline.sort_by(|a, b| a.0.cmp(&b.0));
    out.push_str(&format!("## Thread ({} entries)\n\n", timeline.len()));
    if timeline.is_empty() {
        out.push_str("(no comments)\n");
    }
    for (_, block) in &timeline {
        out.push_str(block);
        out.push_str("\n\n");
    }
    if !comments_complete {
        out.push_str(&format!(
            "(the first {} of {} comments: the rest are past the {} pages read here)\n",
            comments.len(),
            n(issue, "comments"),
            MAX_PAGES
        ));
    }
    out
}

fn release_block(r: &Value, level: &str) -> String {
    let tag = s(r, "tag_name");
    let name = s(r, "name");
    let mut head = format!("{level} {tag}");
    if !name.is_empty() && name != tag {
        head.push_str(&format!(": {name}"));
    }
    let mut flags = Vec::new();
    if r.get("prerelease") == Some(&Value::Bool(true)) {
        flags.push("prerelease");
    }
    if r.get("draft") == Some(&Value::Bool(true)) {
        flags.push("draft");
    }
    let mut out = format!(
        "{head}\n\n- published: {} by {}{}\n- target: {}\n",
        s(r, "published_at"),
        login(r),
        if flags.is_empty() {
            String::new()
        } else {
            format!(" ({})", flags.join(", "))
        },
        s(r, "target_commitish")
    );
    if let Some(rx) = reactions(r) {
        out.push_str(&format!("- reactions: {rx}\n"));
    }
    out.push('\n');
    out.push_str(&body_or(r, "body", "(no release notes)"));
    out.push('\n');
    // The notes first, then the files: a long asset list would push them out of a small budget.
    if let Some(assets) = r
        .get("assets")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
    {
        out.push_str(&format!("\nAssets ({}):\n\n", assets.len()));
        for a in assets {
            out.push_str(&format!(
                "- {} ({}, {} downloads) {}\n",
                s(a, "name"),
                size(n(a, "size")),
                n(a, "download_count"),
                s(a, "browser_download_url")
            ));
        }
    }
    out
}

pub fn render_release(r: &Value) -> String {
    release_block(r, "#")
}

/// One page of releases, newest first; `next` is the URL of the page after it, when there is one.
pub fn render_releases(t: &Target, list: &[Value], page: u32, next: Option<&str>) -> String {
    let mut out = format!(
        "# Releases of {}/{} (page {page}, {} releases)\n\n",
        t.owner,
        t.repo,
        list.len()
    );
    if list.is_empty() {
        out.push_str("(no releases)\n");
    }
    for r in list {
        out.push_str(&release_block(r, "##"));
        out.push('\n');
    }
    if let Some(next) = next {
        out.push_str(&format!("(older releases: {next})\n"));
    }
    out
}

/// A commit: who, when, the message, and every file's patch.
pub fn render_commit(c: &Value) -> String {
    let sha = s(c, "sha");
    let commit = &c["commit"];
    let message = body_or(commit, "message", "(no message)");
    let mut out = format!("# Commit {}\n\n", &sha[..sha.len().min(12)]);
    let who = |role: &str| {
        let p = &commit[role];
        let account = c
            .get(role)
            .and_then(|a| a.get("login"))
            .and_then(Value::as_str)
            .map(|l| format!(" (@{l})"))
            .unwrap_or_default();
        format!("- {role}: {}{account}, {}", s(p, "name"), s(p, "date"))
    };
    out.push_str(&format!(
        "- sha: {sha}\n{}\n{}\n",
        who("author"),
        who("committer")
    ));
    let parents: Vec<&str> = c
        .get("parents")
        .and_then(Value::as_array)
        .map(|a| a.iter().map(|p| s(p, "sha")).collect())
        .unwrap_or_default();
    if !parents.is_empty() {
        out.push_str(&format!("- parents: {}\n", parents.join(", ")));
    }
    let files = c.get("files").and_then(Value::as_array);
    out.push_str(&format!(
        "- stats: +{} -{}, changed files: {}\n\n{message}\n\n",
        n(&c["stats"], "additions"),
        n(&c["stats"], "deletions"),
        files.map_or(0, Vec::len)
    ));
    for f in files.into_iter().flatten() {
        out.push_str(&format!(
            "## {} ({} +{} -{})\n\n",
            s(f, "filename"),
            s(f, "status"),
            n(f, "additions"),
            n(f, "deletions")
        ));
        match f.get("patch").and_then(Value::as_str) {
            Some(p) => out.push_str(&fence(p, "diff")),
            None => out.push_str("(no patch: binary, or too large for the API to include)"),
        }
        out.push_str("\n\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_refusal_says_when_to_come_back_in_the_documented_order() {
        let h = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        let rate = |remaining| RateLimit {
            limit: 60,
            remaining,
            reset: 5_000,
            resource: Some("core".into()),
        };
        // The quota spent: its reset, and not a secondary limit.
        let primary = refusal(&h(&[]), Some(&rate(0)), "API rate limit exceeded", 1_000);
        assert_eq!(
            primary,
            Refusal {
                until: 5_000,
                secondary: false
            }
        );
        // Retry-After wins over the reset, and says secondary.
        let secondary = refusal(&h(&[("Retry-After", "60")]), Some(&rate(57)), "", 1_000);
        assert_eq!(
            secondary,
            Refusal {
                until: 1_060,
                secondary: true
            }
        );
        // No header and requests left: a minute, as documented.
        let bare = refusal(&h(&[]), Some(&rate(12)), "", 1_000);
        assert_eq!(
            bare,
            Refusal {
                until: 1_060,
                secondary: true
            }
        );
        // No quota headers at all, the message naming the limit.
        let named = refusal(
            &h(&[]),
            None,
            "You have exceeded a secondary rate limit",
            1_000,
        );
        assert!(named.secondary && named.until == 1_060);
    }

    fn t(owner: &str, repo: &str, kind: Kind) -> Option<Target> {
        Some(Target {
            owner: owner.into(),
            repo: repo.into(),
            kind,
        })
    }

    #[test]
    fn every_shape_maps() {
        let p = |u: &str| parse(u, None);
        assert_eq!(p("https://github.com/o/r"), t("o", "r", Kind::Root));
        assert_eq!(p("https://www.github.com/o/r/"), t("o", "r", Kind::Root));
        assert_eq!(p("https://github.com/o/r.git"), t("o", "r", Kind::Root));
        assert_eq!(
            p("https://github.com/o/r?tab=readme-ov-file"),
            t("o", "r", Kind::Root),
            "a tab is the same front page"
        );
        assert_eq!(
            p("https://github.com/o/r/tree/main"),
            t(
                "o",
                "r",
                Kind::Tree {
                    git_ref: "main".into(),
                    path: String::new()
                }
            )
        );
        assert_eq!(
            p("https://github.com/o/r/tree/v1.0/src/bin"),
            t(
                "o",
                "r",
                Kind::Tree {
                    git_ref: "v1.0".into(),
                    path: "src/bin".into()
                }
            )
        );
        assert_eq!(
            p("https://github.com/o/r/blob/main/src/lib.rs"),
            t(
                "o",
                "r",
                Kind::Blob {
                    git_ref: "main".into(),
                    path: "src/lib.rs".into(),
                    lines: None
                }
            )
        );
        assert_eq!(
            p("https://github.com/o/r/blob/main/a.rs#L10-L40"),
            t(
                "o",
                "r",
                Kind::Blob {
                    git_ref: "main".into(),
                    path: "a.rs".into(),
                    lines: Some((10, 40))
                }
            )
        );
        assert_eq!(
            p("https://github.com/o/r/issues/12"),
            t("o", "r", Kind::Issue(12))
        );
        assert_eq!(
            p("https://github.com/o/r/issues/12#issuecomment-99"),
            t("o", "r", Kind::Issue(12))
        );
        assert_eq!(
            p("https://github.com/o/r/pull/7"),
            t("o", "r", Kind::Pull(7))
        );
        assert_eq!(
            p("https://github.com/o/r/pull/7/files"),
            t("o", "r", Kind::Pull(7))
        );
        assert_eq!(
            p("https://github.com/o/r/pull/7/commits"),
            t("o", "r", Kind::Pull(7))
        );
        assert_eq!(
            p("https://github.com/o/r/releases"),
            t("o", "r", Kind::Releases { page: 1 })
        );
        assert_eq!(
            p("https://github.com/o/r/releases?page=3"),
            t("o", "r", Kind::Releases { page: 3 })
        );
        assert_eq!(
            p("https://github.com/o/r/releases/tag/v1.2.3"),
            t("o", "r", Kind::Release("v1.2.3".into()))
        );
        assert_eq!(
            p("https://github.com/o/r/releases/latest"),
            t("o", "r", Kind::LatestRelease)
        );
        assert_eq!(
            p("https://github.com/o/r/commit/87c8571326d60b90391d0555c9675b5ac6f044b1"),
            t(
                "o",
                "r",
                Kind::Commit("87c8571326d60b90391d0555c9675b5ac6f044b1".into())
            )
        );
        assert_eq!(
            p("https://github.com/o/r/commit/87c8571"),
            t("o", "r", Kind::Commit("87c8571".into()))
        );
    }

    #[test]
    fn what_has_no_api_shape_falls_through_to_the_page() {
        for u in [
            "https://github.com/",
            "https://github.com/o",
            "https://github.com/settings/profile",
            "https://github.com/orgs/o/repositories",
            "https://github.com/topics/rust",
            "https://github.com/search?q=x",
            "https://github.com/o/r/issues",
            "https://github.com/o/r/issues/new",
            "https://github.com/o/r/issues/abc",
            "https://github.com/o/r/pulls",
            "https://github.com/o/r/pull/7/checks/extra",
            "https://github.com/o/r/pull/7/whatever",
            "https://github.com/o/r/discussions/5",
            "https://github.com/o/r/wiki",
            "https://github.com/o/r/actions",
            "https://github.com/o/r/blob/main",
            "https://github.com/o/r/tree",
            "https://github.com/o/r/commit/not-a-sha",
            "https://github.com/o/r/commits/main",
            "https://github.com/o/r/releases/tag",
            "https://github.com/o/r/raw/main/a.rs",
            // The API and the raw host are already the clean source and stay as they are.
            "https://api.github.com/repos/o/r",
            "https://api.github.com/search/issues?q=x",
            "https://raw.githubusercontent.com/o/r/main/a.rs",
            "https://gist.github.com/o/abc",
            "https://example.com/o/r",
            "ftp://github.com/o/r",
            "not a url",
        ] {
            assert_eq!(parse(u, None), None, "{u}");
        }
    }

    #[test]
    fn an_injected_origin_is_treated_as_the_site_and_only_that_origin() {
        let origin = Some("http://127.0.0.1:4321");
        assert_eq!(
            parse("http://127.0.0.1:4321/o/r/issues/3", origin),
            t("o", "r", Kind::Issue(3))
        );
        assert_eq!(parse("http://127.0.0.1:9999/o/r/issues/3", origin), None);
        assert_eq!(parse("http://127.0.0.1:4321/o/r/issues/3", None), None);
    }

    #[test]
    fn line_anchors() {
        assert_eq!(line_range("L10-L40"), Some((10, 40)));
        assert_eq!(line_range("L7"), Some((7, 7)));
        assert_eq!(line_range("L10C3-L12C9"), Some((10, 12)));
        assert_eq!(line_range("L40-L10"), Some((10, 40)));
        assert_eq!(line_range("readme"), None);
        assert_eq!(line_range("L0"), None);
    }

    #[test]
    fn endpoints() {
        let p = |u: &str| parse(u, None).unwrap().primary_path();
        assert_eq!(p("https://github.com/o/r"), "/repos/o/r");
        assert_eq!(
            p("https://github.com/o/r/tree/main/src"),
            "/repos/o/r/contents/src?ref=main"
        );
        assert_eq!(
            p("https://github.com/o/r/tree/main"),
            "/repos/o/r/contents?ref=main"
        );
        assert_eq!(
            p("https://github.com/o/r/blob/main/src/a.rs#L1"),
            "/o/r/main/src/a.rs"
        );
        assert_eq!(p("https://github.com/o/r/pull/7"), "/repos/o/r/issues/7");
        assert_eq!(
            p("https://github.com/o/r/releases"),
            "/repos/o/r/releases?per_page=10"
        );
        assert_eq!(
            p("https://github.com/o/r/releases?page=2"),
            "/repos/o/r/releases?per_page=10&page=2"
        );
        assert_eq!(
            p("https://github.com/o/r/releases/tag/v1"),
            "/repos/o/r/releases/tags/v1"
        );
        assert_eq!(
            p("https://github.com/o/r/releases/latest"),
            "/repos/o/r/releases/latest"
        );
        assert_eq!(
            p("https://github.com/o/r/commit/abcdef1"),
            "/repos/o/r/commits/abcdef1"
        );
    }

    #[test]
    fn a_quota_refusal_is_told_apart_from_an_access_refusal() {
        let spent = RateLimit {
            limit: 60,
            remaining: 0,
            reset: 0,
            resource: None,
        };
        let left = RateLimit {
            remaining: 12,
            ..spent.clone()
        };
        assert!(is_rate_limited(403, Some(&spent), "{}"));
        assert!(is_rate_limited(429, None, ""));
        assert!(is_rate_limited(
            403,
            None,
            "{\"message\":\"API rate limit exceeded for x\"}"
        ));
        assert!(is_rate_limited(403, Some(&left), "secondary rate limit"));
        assert!(!is_rate_limited(
            403,
            Some(&left),
            "{\"message\":\"Repository access blocked\"}"
        ));
        assert!(!is_rate_limited(404, Some(&spent), ""));
        assert!(!is_rate_limited(200, Some(&spent), "rate limit"));
    }

    #[test]
    fn rate_limit_headers_and_reset_time() {
        let h = vec![
            ("X-RateLimit-Limit".to_string(), "60".to_string()),
            ("x-ratelimit-remaining".to_string(), "0".to_string()),
            ("X-RateLimit-Reset".to_string(), "1794433434".to_string()),
            ("X-RateLimit-Resource".to_string(), "core".to_string()),
        ];
        let r = RateLimit::from_headers(&h).unwrap();
        assert_eq!((r.limit, r.remaining), (60, 0));
        assert_eq!(r.reset_at(), "2026-11-11T21:43:54Z");
        assert_eq!(r.resource.as_deref(), Some("core"));
        assert_eq!(RateLimit::from_headers(&[]), None);
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn link_header_next() {
        assert!(has_next(Some(
            "<https://api.github.com/x?page=2>; rel=\"next\", <https://api.github.com/x?page=5>; rel=\"last\""
        )));
        assert!(!has_next(Some(
            "<https://api.github.com/x?page=1>; rel=\"prev\""
        )));
        assert!(!has_next(None));
    }

    #[test]
    fn lines_are_sliced_one_based_and_inclusive() {
        let text = "a\nb\nc\nd\n";
        assert_eq!(slice_lines(text, 2, 3), ("b\nc".to_string(), 4));
        assert_eq!(slice_lines(text, 4, 9), ("d".to_string(), 4));
        assert_eq!(slice_lines(text, 9, 9), (String::new(), 4));
    }

    #[test]
    fn a_thread_keeps_every_comment_in_order_with_its_reactions() {
        let issue = json!({"number": 5, "title": "T", "state": "open", "body": "B",
            "user": {"login": "a"}, "created_at": "2024-01-01T00:00:00Z", "comments": 2,
            "labels": [{"name": "bug"}], "reactions": {"+1": 3, "heart": 0}});
        let comments = vec![
            json!({"user": {"login": "b"}, "created_at": "2024-01-02T00:00:00Z", "body": "first",
                   "reactions": {"+1": 0, "rocket": 2}}),
            json!({"user": null, "created_at": "2024-01-03T00:00:00Z", "body": null}),
        ];
        let md = render_thread(&issue, &comments, true, None);
        assert!(md.contains("# T (#5)"));
        assert!(md.contains("- labels: bug"));
        assert!(md.contains("- reactions: +1 3"));
        let first = md.find("@b, 2024-01-02").unwrap();
        let second = md.find("@ghost, 2024-01-03").unwrap();
        assert!(first < second);
        assert!(md.contains("reactions: rocket 2"));
        assert!(
            md.contains("(empty)"),
            "a comment without a body is still there"
        );

        let partial = render_thread(&issue, &comments[..1], false, None);
        assert!(partial.contains("the first 1 of 2 comments"), "{partial}");
    }

    #[test]
    fn a_patch_with_backticks_stays_inside_its_fence() {
        let f = fence("a ```` b", "diff");
        assert!(f.starts_with("`````diff\n"), "{f}");
        assert!(f.ends_with("\n`````"));
    }
}
