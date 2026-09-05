//! `svipall` — the same tool, from a shell.
//!
//! Agents reach for a CLI before they reach for an MCP server, and the reason is arithmetic: an MCP
//! connection loads every tool's JSON schema into the context before the first call, while
//! `svipall fetch <url>` costs the characters of the line. Same code underneath — this binary drives
//! the same `SvipallServer` the MCP transport does — for a fraction of the tokens.
//!
//! Everything prints JSON to stdout, one object, nothing else. Diagnostics go to stderr, so a
//! pipeline can read stdout without filtering. The exit code is 0 when the command ran, 1 when it
//! could not, and never depends on what a *site* said: a page that was blocked is a successful
//! report of a block.

use serde_json::{json, Value};
use svipall_mcp::server::SvipallServer;
use svipall_mcp::tools::*;

const USAGE: &str = "\
svipall — local-first web scraping, from a shell

USAGE:
    svipall <command> [arguments]
    svipall --version           What this build is: version, target triple, compiled-in features.

COMMANDS:
    fetch <url> [--query Q] [--out FILE] [--text] [--mobile] [--isolated] [--full] [--tables]
                [--scroll auto|N] [--schema auto|JSON]
                [--extraction markdown|text|html] [--cache auto|bypass|refresh] [--mode TIER]
                            Fetch a page, climbing the tier ladder as far as it needs to.
                            <url> may be file:///path (under local_roots) or raw: with --stdin.
                            --tables returns typed rows; with --out x.csv they go to the file.
                            --schema auto reads a listing's own repeated structure and returns
                            its rows, with the schema it worked out in induced_schema.
    crawl <url> [--pages N] [--depth N] [--query Q] [--out FILE] [--dfs] [--since-last]
                            Crawl a site. --out writes .csv/.json/.jsonl instead of printing.
    snapshot <url>          The page as roles, names and refs — a fraction of the tokens.
    capture <url> [--pattern P] [--bodies]
                            The JSON the page itself fetched: usually the site's real API.
    search <query> [--engine E]
                            Search without an API key. --engine all merges every engine.
    map <url>               A site's URLs from robots.txt, sitemaps and feeds.
    log [--summary] [--domain D]
                            What this installation has been doing, and which walls it met.
    notes <get|set|list|delete> [key] [value]
                            Remember something between runs.
    watch <add|list|check|remove> [url] [--every SECS] [--selector CSS]
                            Report when a page changes.
    profile <list|export|import> [name] [--file F] [--password P]
                            Move a logged-in profile between machines.
    route <list|add|remove|check> [domain] [--proxy URL | --proxies A,B,C] [--country CC | --countries CC,CC] [--check-url URL]
                            One exit per domain, or a pool the domain moves through as exits
                            get blocked. Subdomains inherit.
    status                  Learned tiers, cooldowns, routes, profiles, solver state.
    serve [--port N] [--bind ADDR]
                            The same server over HTTP: POST /v1/fetch, /v1/crawl, /v1/search …
                            one endpoint per tool, so any language can drive it. Loopback and a
                            bearer key by default; the key is printed once and kept in
                            ~/.svipall/api_key. Runs until ctrl-c.
    browser [status|install|update|remove]
                            Which browser runs, or download Chrome for Testing (~190 MB).
    doctor                  Whether this installation will work on this machine: which build this
                            is, which browser would run, which models are compiled in, whether the
                            dashboard port is free — and, for anything that is wrong, the command
                            that fixes it. What an installer runs when it has finished unpacking.
    hook <event>            Answer an AI harness's tool-call hook, reading its JSON on stdin.
                            `claude-web` declines Claude Code's WebFetch and WebSearch in favour of
                            svipall, and only while ~/.svipall/claude_strict exists.
    solver export-corpus --out DIR [--since DAYS] [--modality M] [--source model|zeroshot|human]
                            The challenges this machine has seen and how they were answered, as
                            images plus a manifest.jsonl, for training your own models.
    quality ask [--count N] [--since DAYS]
                            Put pages in front of a person at the dashboard to be rated.
    quality export-training --out FILE.jsonl [--since DAYS]
                            What this machine has fetched, weakly labelled from what happened
                            after each fetch, for training the page-substance classifier.
    quality train --in FILE.jsonl --out DIR [--epochs N] [--lr F]
                            Fit the classifier and write substance.bin + .json. Point --out at
                            ~/.svipall/models to install it.

Everything prints one JSON object to stdout. Errors go to stderr.
";

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "-h" || args[0] == "--help" || args[0] == "help" {
        eprint!("{USAGE}");
        std::process::exit(if args.is_empty() { 1 } else { 0 });
    }
    // Answered before the config is read, the logger is built or a directory is created. An
    // installer asks this of a binary it has just unpacked and may not yet own a home for, and a
    // package manager's smoke test asks it of a binary in a sandbox.
    if matches!(args[0].as_str(), "--version" | "-V" | "version") {
        println!(
            "{}",
            serde_json::to_string_pretty(&svipall_mcp::doctor::version_json()).unwrap_or_default()
        );
        std::process::exit(0);
    }
    // A hook runs before every matching tool call somebody makes, so it opens no database, starts
    // no browser pool and reads no cache. Straight from stdin to stdout.
    if args[0] == "hook" {
        let event = args.get(1).map(String::as_str).unwrap_or_default();
        match svipall_mcp::hooks::run(event) {
            Ok(value) => {
                println!("{}", serde_json::to_string(&value).unwrap_or_default());
                std::process::exit(0);
            }
            Err(e) => {
                // Never exit 2 here: that is the harness's "block", and a hook that cannot read
                // its own input has no business blocking anybody's tool call.
                eprintln!("svipall hook: {e}");
                println!("{{}}");
                std::process::exit(0);
            }
        }
    }
    // Warnings and progress on stderr, so stdout is only ever the answer.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("warn".parse().expect("static directive")),
        )
        .with_writer(std::io::stderr)
        .init();

    match run(&args).await {
        Ok(value) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&value).unwrap_or_default()
            );
        }
        Err(e) => {
            eprintln!("svipall: {e}");
            std::process::exit(1);
        }
    }
}

async fn run(args: &[String]) -> anyhow::Result<Value> {
    let cfg = svipall_core::config::load();
    svipall_core::ensure_dirs();
    // The page cache and the notes; without it everything still works, just without memory.
    let store = svipall_core::cache::Store::open()
        .ok()
        .map(std::sync::Arc::new);
    // Cloned rather than moved: `serve` still needs `rest_port` and `rest_bind` afterwards, and
    // `main.rs` already builds the server this way for the same reason.
    let server = SvipallServer::with_store(None, cfg.clone(), None, store);
    let flags = Flags::parse(&args[1..]);
    let positional = flags.positional.clone();
    let first = positional.first().cloned();

    let out = match args[0].as_str() {
        "fetch" => {
            let mut url = first.ok_or_else(|| anyhow::anyhow!("fetch needs a url"))?;
            // `svipall fetch raw: --stdin < page.html`: the markup comes down the pipe, not the
            // command line, which has a length limit and shell quoting.
            if flags.has("stdin") && url == "raw:" {
                let mut html = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut html)?;
                url.push_str(&html);
            }
            let out = server
                .fetch_json(WebFetchParams {
                    url,
                    query: flags.value("query"),
                    tables: flags.has("tables").then_some(true),
                    scroll: flags.value("scroll"),
                    out_file: flags.value("out"),
                    text_only: flags.has("text").then_some(true),
                    extraction: flags.value("extraction"),
                    cache: flags.value("cache"),
                    // `--full` turns off the main-content heuristic, which is the first thing to
                    // try when a page comes back with a title and no text.
                    main_content_only: flags.has("full").then_some(false),
                    mobile: flags.has("mobile").then_some(true),
                    isolated: flags.has("isolated").then_some(true),
                    // Debugging only: the ladder knows better, which is why the skill says never.
                    mode: flags.value("mode"),
                    // `--schema auto` for a listing nobody has selectors for, or a JSON object for
                    // one written by hand. A malformed object is reported by the extractor as a
                    // schema error rather than swallowed here, which is where every other bad
                    // schema is reported too.
                    schema: flags
                        .value("schema")
                        .map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::String(s))),
                    ..Default::default()
                })
                .await;
            Ok(out.value)
        }
        "crawl" => {
            let url = first.ok_or_else(|| anyhow::anyhow!("crawl needs a url"))?;
            Ok(server
                .crawl_json(WebCrawlParams {
                    url,
                    max_pages: flags.number("pages"),
                    max_depth: flags.number("depth"),
                    query: flags.value("query"),
                    out_file: flags.value("out"),
                    strategy: flags.has("dfs").then(|| "dfs".to_string()),
                    since_last_crawl: flags.has("since-last").then_some(true),
                    ..Default::default()
                })
                .await)
        }
        "snapshot" => {
            let url = first.ok_or_else(|| anyhow::anyhow!("snapshot needs a url"))?;
            server
                .snapshot_json(serde_json::from_value(json!({"url": url}))?)
                .await
        }
        "capture" => {
            let url = first.ok_or_else(|| anyhow::anyhow!("capture needs a url"))?;
            let mut p = json!({"url": url});
            if let Some(pattern) = flags.value("pattern") {
                p["pattern"] = json!(pattern);
            }
            if flags.has("bodies") {
                p["bodies"] = json!(true);
            }
            server.capture_json(serde_json::from_value(p)?).await
        }
        "search" => {
            let query = positional.join(" ");
            if query.trim().is_empty() {
                anyhow::bail!("search needs something to search for");
            }
            let engine = flags.value("engine");
            let fetcher = server.fetcher();
            let out = match engine.as_deref() {
                Some("all" | "merge") => {
                    svipall_mcp::search::search_all(fetcher.as_ref(), &query, 10).await
                }
                e => svipall_mcp::search::search(fetcher.as_ref(), &query, 10, e).await,
            };
            Ok(json!({
                "query": query,
                "engine": out.engine,
                "count": out.results.len(),
                "results": out.results,
            }))
        }
        "map" => {
            let url = first.ok_or_else(|| anyhow::anyhow!("map needs a url"))?;
            server
                .map_json(serde_json::from_value(json!({"url": url}))?)
                .await
        }
        "log" => server.log_json(WebLogParams {
            view: Some(
                if flags.has("summary") {
                    "summary"
                } else {
                    "recent"
                }
                .to_string(),
            ),
            domain: flags.value("domain"),
            since_secs: flags.number("since").map(|n| n as i64),
            limit: flags.number("limit"),
        }),
        "notes" => {
            let action = first.unwrap_or_else(|| "list".into());
            server.notes_json(WebNotesParams {
                action: Some(action),
                key: positional.get(1).cloned(),
                value: positional.get(2).cloned(),
                prefix: flags.value("prefix"),
            })
        }
        "watch" => {
            let action = first.unwrap_or_else(|| "list".into());
            server
                .watch_json(WebWatchParams {
                    action: Some(action),
                    url: positional.get(1).cloned(),
                    interval_secs: flags.number("every").map(|n| n as i64),
                    label: flags.value("label"),
                    css_selector: flags.value("selector"),
                })
                .await
        }
        "profile" => {
            let action = first.unwrap_or_else(|| "list".into());
            server.profile_json(WebProfileParams {
                action: Some(action),
                name: positional.get(1).cloned(),
                file: flags.value("file"),
                password: flags.value("password"),
            })
        }
        "browser" => {
            server
                .browser_setup_json(BrowserSetupParams {
                    action: first,
                    artifact: flags.value("artifact"),
                })
                .await
        }
        "route" => {
            let action = first.unwrap_or_else(|| "list".into());
            let split = |v: Option<String>| -> Option<Vec<String>> {
                v.map(|s| {
                    s.split(',')
                        .map(|x| x.trim().to_string())
                        .filter(|x| !x.is_empty())
                        .collect()
                })
            };
            let domain = positional.get(1).cloned();
            if !matches!(action.as_str(), "list" | "check") && domain.is_none() {
                anyhow::bail!("route {action} needs a domain");
            }
            server
                .route_json(WebRouteParams {
                    domain,
                    proxy: flags.value("proxy"),
                    country: flags.value("country"),
                    proxies: split(flags.value("proxies")),
                    countries: split(flags.value("countries")),
                    remove: (action == "remove").then_some(true),
                    check: (action == "check").then_some(true),
                    check_url: flags.value("check-url"),
                })
                .await
        }
        "solver" => {
            let action = first.unwrap_or_default();
            if action != "export-corpus" {
                anyhow::bail!("solver knows export-corpus, not {action:?}");
            }
            let out = flags
                .value("out")
                .ok_or_else(|| anyhow::anyhow!("export-corpus needs --out DIR"))?;
            let since_days = flags.number("since").unwrap_or(30) as i64;
            export_corpus(
                std::path::Path::new(&out),
                since_days,
                flags.value("modality"),
                flags.value("source"),
            )
        }
        "quality" => {
            let action = first.unwrap_or_default();
            // The panel's own database, opened directly: the CLI runs no solver, so the ratings a
            // person left are only reachable this way. Absent is fine — the log alone still works.
            let jobs = svipall_solver::db::Db::open().ok();
            if action == "ask" {
                let store = server
                    .store()
                    .ok_or_else(|| anyhow::anyhow!("no cache to choose pages from"))?;
                let db = jobs
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("no job database to put the pages in"))?;
                return svipall_mcp::quality_cli::ask(
                    store,
                    db,
                    flags.number("count").unwrap_or(20),
                    flags.number("since").unwrap_or(30) as i64,
                );
            }
            let out = flags
                .value("out")
                .ok_or_else(|| anyhow::anyhow!("quality {action} needs --out"))?;
            match action.as_str() {
                "export-training" => {
                    let store = server
                        .store()
                        .ok_or_else(|| anyhow::anyhow!("no cache to read a history from"))?;
                    svipall_mcp::quality_cli::export_training(
                        store,
                        jobs.as_ref(),
                        std::path::Path::new(&out),
                        flags.number("since").unwrap_or(30) as i64,
                    )
                }
                "train" => {
                    let input = flags
                        .value("in")
                        .ok_or_else(|| anyhow::anyhow!("train needs --in FILE.jsonl"))?;
                    svipall_mcp::quality_cli::train(
                        std::path::Path::new(&input),
                        std::path::Path::new(&out),
                        flags.number("epochs").unwrap_or(30),
                        flags
                            .value("lr")
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0.5f32),
                    )
                }
                other => {
                    anyhow::bail!("quality knows ask, export-training and train, not {other:?}")
                }
            }
        }
        "status" => server.status_json(serde_json::from_value(json!({}))?).await,
        // Reads this machine rather than a page, which is why it takes the config and nothing else.
        "doctor" => Ok(svipall_mcp::doctor::report(&cfg)),
        "serve" => {
            // The one command whose object is about itself rather than about a page, and the one
            // that answers when it *stops* rather than when it starts. Handled here rather than in
            // `main` so it inherits the `pool().shutdown()` below — which is exactly the cleanup a
            // long-running server needs on ctrl-c, and which routing around `run` would duplicate.
            let port = flags
                .number("port")
                .map(|n| n as u16)
                .unwrap_or(if cfg.rest_port != 0 {
                    cfg.rest_port
                } else {
                    8788
                });
            let bind = flags.value("bind").unwrap_or_else(|| cfg.rest_bind.clone());
            svipall_mcp::rest::serve(server.clone(), &bind, port).await?;
            Ok(json!({
                "served": format!("http://{bind}:{port}/v1/"),
                "routes": svipall_mcp::rest::ROUTES.len(),
                "stopped": "ctrl-c",
            }))
        }
        other => anyhow::bail!("unknown command '{other}'\n\n{USAGE}"),
    };
    // A browser the pool still holds keeps the process alive after the answer has been printed:
    // a fetch that finished in four seconds sat there for ten minutes. Measured, then fixed.
    server.pool().shutdown().await;
    // A one-shot command has no housekeeping loop, so this is where what it spent is written.
    svipall_core::reputation::flush();
    out
}

/// Flags that are yes-or-no and never take a value.
///
/// Listed, not inferred: see `Flags::parse`.
const SWITCHES: &[&str] = &[
    "text",
    "mobile",
    "isolated",
    "dfs",
    "since-last",
    "summary",
    "full",
    "tables",
    "stdin",
];

/// `--name value` and `--name`, plus whatever is left over.
///
/// Hand-written rather than a dependency: this is thirty lines, the grammar is fixed, and an
/// argument parser is the kind of thing that grows a feature every release.
#[derive(Debug, Default)]
struct Flags {
    named: std::collections::HashMap<String, String>,
    switches: std::collections::HashSet<String>,
    positional: Vec<String>,
}

impl Flags {
    fn parse(args: &[String]) -> Self {
        let mut out = Flags::default();
        let mut i = 0;
        while i < args.len() {
            let a = &args[i];
            match a.strip_prefix("--") {
                Some(name) if !name.is_empty() => {
                    // `--name=value` and `--name value` mean the same thing.
                    if let Some((k, v)) = name.split_once('=') {
                        out.named.insert(k.to_string(), v.to_string());
                    } else if SWITCHES.contains(&name) {
                        // Declared rather than guessed. Deciding by "is the next argument a flag"
                        // makes `svipall fetch --text https://x/` lose the URL: the switch swallows it,
                        // the command runs against nothing, and nothing about it looks wrong.
                        out.switches.insert(name.to_string());
                    } else if args.get(i + 1).is_some_and(|next| !next.starts_with("--")) {
                        out.named.insert(name.to_string(), args[i + 1].clone());
                        i += 1;
                    } else {
                        out.switches.insert(name.to_string());
                    }
                }
                _ => out.positional.push(a.clone()),
            }
            i += 1;
        }
        out
    }

    fn value(&self, name: &str) -> Option<String> {
        self.named.get(name).cloned()
    }

    fn has(&self, name: &str) -> bool {
        self.switches.contains(name) || self.named.contains_key(name)
    }

    fn number(&self, name: &str) -> Option<usize> {
        self.named.get(name)?.parse().ok()
    }
}

/// Write the corpus to `dir`: one image file per asset under `<modality>/`, and a
/// `manifest.jsonl` naming each file with what was asked, what was answered, by whom, and whether
/// it worked. The CLI has no solver state of its own, so it opens the job database directly.
fn export_corpus(
    dir: &std::path::Path,
    since_days: i64,
    modality: Option<String>,
    source: Option<String>,
) -> anyhow::Result<Value> {
    use std::io::Write as _;
    let db = svipall_solver::db::Db::open()?;
    let since = chrono::Utc::now().timestamp() - since_days.max(0) * 86_400;
    let rows = db.corpus(since, modality.as_deref(), source.as_deref());
    std::fs::create_dir_all(dir)?;
    let mut manifest = std::fs::File::create(dir.join("manifest.jsonl"))?;
    let (mut files, mut bytes, mut lines) = (0usize, 0usize, 0usize);
    for row in &rows {
        let modality = row.modality.clone().unwrap_or_else(|| "unknown".into());
        let sub = dir.join(&modality);
        std::fs::create_dir_all(&sub)?;
        let mut written: Vec<Value> = Vec::new();
        let mut assets: Vec<(String, Option<i64>, String, Vec<u8>)> = row
            .assets
            .iter()
            .filter_map(|a| {
                db.asset(&a.id)
                    .map(|(mime, b)| (a.kind.clone(), a.idx, mime, b))
            })
            .collect();
        if assets.is_empty() && row.has_image {
            if let Some(b) = db.image_bytes(&row.task_id) {
                assets.push(("image".into(), Some(0), "image/png".into(), b));
            }
        }
        for (kind, idx, mime, data) in assets {
            let ext = match mime.as_str() {
                "image/jpeg" => "jpg",
                "image/webp" => "webp",
                "audio/mpeg" => "mp3",
                "audio/wav" => "wav",
                _ => "png",
            };
            let name = format!("{}-{}-{}.{ext}", row.task_id, kind, idx.unwrap_or(0));
            std::fs::write(sub.join(&name), &data)?;
            files += 1;
            bytes += data.len();
            written.push(json!({"file": format!("{modality}/{name}"), "kind": kind, "idx": idx}));
        }
        let line = json!({
            "job_id": row.job_id, "task_id": row.task_id, "job_type": row.job_type,
            "widget": row.widget, "modality": modality,
            "prompt": row.payload.as_deref().and_then(|p| serde_json::from_str::<Value>(p).ok()),
            "answer": row.answer.as_deref().and_then(|a| serde_json::from_str::<Value>(a).ok()),
            "source": row.source, "ok": row.ok, "at": row.created_at, "files": written,
        });
        writeln!(manifest, "{line}")?;
        lines += 1;
    }
    Ok(json!({
        "out": dir.to_string_lossy(),
        "manifest": dir.join("manifest.jsonl").to_string_lossy(),
        "rows": lines,
        "files": files,
        "bytes": bytes,
        "since_days": since_days,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_flag_with_a_value_and_a_flag_without_one_are_told_apart() {
        let f = Flags::parse(&args(&["https://x.test/", "--query", "cache", "--text"]));
        assert_eq!(f.positional, vec!["https://x.test/".to_string()]);
        assert_eq!(f.value("query").as_deref(), Some("cache"));
        assert!(f.has("text"));
        assert!(!f.has("mobile"));
    }

    #[test]
    fn the_two_spellings_of_a_named_flag_mean_the_same_thing() {
        let a = Flags::parse(&args(&["--pages=30"]));
        let b = Flags::parse(&args(&["--pages", "30"]));
        assert_eq!(a.number("pages"), Some(30));
        assert_eq!(b.number("pages"), Some(30));
    }

    #[test]
    fn a_switch_never_swallows_the_argument_after_it() {
        // `svipall fetch --text https://x/` must still have a URL. Deciding by "is the next argument
        // a flag" loses it, the command runs against nothing, and nothing about it looks wrong.
        let f = Flags::parse(&args(&["--text", "--mobile", "url"]));
        assert!(f.has("text") && f.has("mobile"));
        assert_eq!(f.positional, vec!["url".to_string()]);

        let f = Flags::parse(&args(&["--text", "https://x.test/"]));
        assert!(f.has("text"));
        assert_eq!(f.positional, vec!["https://x.test/".to_string()]);
    }

    #[test]
    fn every_switch_the_commands_read_is_declared_as_one() {
        // A switch missing from the list behaves as a value flag and eats the next argument.
        for name in SWITCHES {
            let f = Flags::parse(&args(&[&format!("--{name}"), "positional"]));
            assert!(f.has(name), "{name}");
            assert_eq!(f.positional, vec!["positional".to_string()], "{name}");
        }
    }

    #[test]
    fn a_number_that_is_not_a_number_is_absent_rather_than_zero() {
        // Zero pages is a crawl that fetches nothing and reports success.
        let f = Flags::parse(&args(&["--pages", "lots"]));
        assert_eq!(f.number("pages"), None);
    }

    #[test]
    fn several_words_after_a_command_stay_in_order() {
        let f = Flags::parse(&args(&["rust", "async", "runtime", "--engine", "all"]));
        assert_eq!(f.positional.join(" "), "rust async runtime");
        assert_eq!(f.value("engine").as_deref(), Some("all"));
    }

    #[test]
    fn the_published_skill_and_the_binary_agree_on_what_exists() {
        // A skill that documents a command the binary does not have sends an agent down a path
        // that ends in "unknown command", and it will not try again.
        let skill = include_str!("../../../../skill/SKILL.md");
        for cmd in [
            "svipall fetch",
            "svipall crawl",
            "svipall snapshot",
            "svipall capture",
            "svipall search",
            "svipall map",
            "svipall log",
            "svipall notes",
            "svipall watch",
            "svipall status",
            "svipall route",
            "svipall solver",
            "svipall quality",
            "svipall serve",
            "svipall doctor",
        ] {
            assert!(skill.contains(cmd), "{cmd} is not in the skill");
        }
    }

    #[test]
    fn the_plugin_ships_the_same_skill_the_release_tarball_does() {
        // Two copies of the skill exist because they are consumed differently: the tarball ships
        // `SKILL.md` next to the binaries, and a Claude Code plugin has to carry its skills inside
        // itself. Nothing else keeps them equal, and a plugin quietly a version behind teaches an
        // agent commands this binary no longer has. `scripts/sync-plugin` is what makes them so.
        let canonical = include_str!("../../../../skill/SKILL.md");
        let shipped = include_str!("../../../../plugins/svipall/skills/svipall/SKILL.md");
        assert_eq!(
            canonical, shipped,
            "plugins/svipall/skills/svipall/SKILL.md has drifted from skill/SKILL.md;              run scripts/sync-plugin.sh (or .ps1)"
        );
    }

    #[test]
    fn the_usage_text_names_every_command_the_binary_answers_to() {
        // A command that works and is undocumented is a command nobody runs.
        for cmd in [
            "fetch",
            "crawl",
            "snapshot",
            "capture",
            "search",
            "map",
            "log",
            "notes",
            "watch",
            "profile",
            "status",
            "browser",
            "route",
            "solver",
            "quality",
            "serve",
            "doctor",
            "hook",
            "--version",
        ] {
            assert!(USAGE.contains(cmd), "{cmd} is missing from the usage text");
        }
    }
}
