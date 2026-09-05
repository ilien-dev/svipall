//! What this binary is, and whether it is going to work here.
//!
//! Three different callers need the same two answers before they can do anything: `install.sh`
//! after it has unpacked a tarball, a package manager's smoke test, and the Claude Code plugin
//! deciding whether to offer an install at all. Answering in prose would make each of them parse
//! prose; answering here makes all three read one JSON object.
//!
//! The split between [`Facts`] and [`problems`] is the whole point. Gathering facts touches this
//! machine — a browser is either installed or it is not — and a test that runs only where a
//! browser happens to be installed asserts nothing about the machine an installer lands on. So the
//! judgement is a pure function of facts, and the facts are collected separately.

use serde_json::{json, Value};
use std::path::PathBuf;
use svipall_core::config::Config;

/// What this build is: the answer to `svipall --version`.
///
/// Deliberately an object rather than a version string. `svipall 1.0.0` cannot tell a Homebrew
/// formula which tarball it is holding, and cannot tell a user why the http tier is refusing to
/// emulate a browser on a `--no-default-features` build.
pub fn version_json() -> Value {
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "target": env!("SVIPALL_TARGET"),
        "impersonation": svipall_http::impersonation_available(),
        "features": features(),
    })
}

/// The optional pieces that are compiled in, named the way `--features` names them.
fn features() -> Vec<&'static str> {
    let mut out = Vec::new();
    if cfg!(feature = "impersonate") {
        out.push("impersonate");
    }
    if cfg!(feature = "http3") {
        out.push("http3");
    }
    for (on, name) in [
        (cfg!(feature = "onnx-ocr"), "onnx-ocr"),
        (cfg!(feature = "onnx-grid"), "onnx-grid"),
        (cfg!(feature = "onnx-audio"), "onnx-audio"),
        (cfg!(feature = "onnx-detect"), "onnx-detect"),
        (cfg!(feature = "onnx-segment"), "onnx-segment"),
        (cfg!(feature = "onnx-zeroshot"), "onnx-zeroshot"),
    ] {
        if on {
            out.push(name);
        }
    }
    out
}

/// Everything about this installation that a judgement could depend on.
///
/// Plain fields rather than accessors: a test builds one by hand, spoils exactly one thing, and
/// asserts what [`problems`] says about it.
#[derive(Debug, Clone)]
pub struct Facts {
    pub version: String,
    pub target: String,
    /// This executable's own path, when the OS will say.
    pub exe: Option<PathBuf>,
    pub impersonation: bool,
    pub http_engine: String,
    pub home: PathBuf,
    pub home_writable: bool,
    pub config_present: bool,
    pub secrets_present: bool,
    /// Every browser the pool would consider, best first.
    pub browsers: Vec<PathBuf>,
    pub browser_major: Option<u16>,
    /// The newest major this machine has any evidence of; `None` means no evidence, and no
    /// evidence can never make something stale.
    pub newest_known_major: Option<u16>,
    pub embedded_models: Vec<String>,
    pub installed_models: Vec<String>,
    /// Whether any `onnx-*` feature is compiled in. Separate from having weights: the build script
    /// embeds whatever model files are on disk, and the inference code sits behind a feature flag,
    /// so a build can carry 58 MB of weights that nothing in it can read.
    pub inference: bool,
    pub dashboard_port: u16,
    pub dashboard_free: bool,
    pub rest_port: u16,
}

/// One thing that is wrong, and the command that fixes it.
///
/// `fix` is not optional and not decorative: the plugin reads it verbatim and shows it to a
/// person, so a problem without a next step becomes an agent inventing one.
#[derive(Debug, Clone)]
pub struct Problem {
    pub code: String,
    pub message: String,
    pub fix: String,
}

fn problem(code: &str, message: &str, fix: &str) -> Problem {
    Problem {
        code: code.into(),
        message: message.into(),
        fix: fix.into(),
    }
}

/// What is wrong with this installation, worst first. Empty means it is ready.
pub fn problems(f: &Facts) -> Vec<Problem> {
    let mut out = Vec::new();

    if !f.home_writable {
        // Everything else is downstream of this: no cache, no profiles, no learned tiers, and a
        // generated api key that lasts exactly one run.
        out.push(problem(
            "home_not_writable",
            &format!(
                "{} cannot be written to, so nothing is remembered between runs.",
                f.home.display()
            ),
            "Fix the permissions on that directory, or point SVIPALL_HOME at one this user owns.",
        ));
    }

    match f.browsers.first() {
        None => out.push(problem(
            "no_browser",
            "No Chromium-based browser was found, so only the http tier is available and any page \
             behind a challenge will stay blocked.",
            "Run `svipall browser install` (downloads Chrome for Testing, ~190 MB), or install \
             Chrome or Edge and set browser_path in the config.",
        )),
        Some(exe) => {
            if crate::browser::Brand::of(exe) == crate::browser::Brand::SelfDefending {
                out.push(problem(
                    "self_defending_browser",
                    &format!(
                        "The browser that would be used ({}) ships its own anti-fingerprinting, \
                         which contradicts the Chrome identity stated everywhere else.",
                        exe.display()
                    ),
                    "Run `svipall browser install` — the pool prefers the managed Chrome for \
                     Testing once it is there.",
                ));
            }
            if let (Some(in_use), Some(best)) = (f.browser_major, f.newest_known_major) {
                if best.saturating_sub(in_use) >= STALE_MAJORS {
                    out.push(problem(
                        "stale_browser",
                        &format!(
                            "The browser in use is Chrome {in_use}, and this machine knows of \
                             {best}; a user agent naming a Chrome that old is itself a signal."
                        ),
                        "Run `svipall browser update`.",
                    ));
                }
            }
        }
    }

    if f.embedded_models.is_empty() && f.installed_models.is_empty() {
        // The honest difference between a release tarball, a plain `cargo build`, and a container
        // image built without the export step. Silent, until a captcha goes to a person instead.
        out.push(problem(
            "no_models",
            "No captcha models are compiled in and none are installed, so image challenges go to \
             the human dashboard instead of being answered.",
            "Use a release build, or see docs/models.md to export and install them.",
        ));
    } else if !f.inference {
        // One problem per missing capability: with no weights at all, `no_models` already said it.
        out.push(problem(
            "models_not_readable",
            "This build carries captcha model weights but was compiled without any `onnx-*` \
             feature, so nothing in it can read them and image challenges still go to the human \
             dashboard.",
            "Use a release build, or rebuild with --features \
             onnx-ocr,onnx-grid,onnx-audio,onnx-detect,onnx-segment,onnx-zeroshot.",
        ));
    }

    if !f.impersonation {
        out.push(problem(
            "no_impersonation",
            "This build cannot emulate a browser's TLS and HTTP/2 fingerprint, so the http tier is \
             recognisable as a robot in the first packet.",
            "Install a release build, or rebuild with the default features (needs cmake, nasm, \
             perl and llvm).",
        ));
    }

    if !f.dashboard_free {
        out.push(problem(
            "dashboard_port_busy",
            &format!(
                "Port {} is already in use. Usually that is a svipall-mcp already running, which \
                 is fine; if it is something else, the dashboard will not start and the URL it \
                 reports would point at nothing.",
                f.dashboard_port
            ),
            "Nothing, if svipall-mcp is already running. Otherwise set dashboard_port in the \
             config, or SVIPALL_DASHBOARD_PORT, to a free port.",
        ));
    }

    out
}

/// How many majors behind the newest known build counts as stale. Same number `browser_advice`
/// uses, and for the same reason.
const STALE_MAJORS: u16 = 2;

/// The whole report, as the one JSON object `svipall doctor` prints.
pub fn report_from(f: &Facts) -> Value {
    let found = problems(f);
    json!({
        "ok": found.is_empty(),
        "version": f.version,
        "target": f.target,
        "exe": f.exe.as_ref().map(|p| p.display().to_string()),
        "impersonation": f.impersonation,
        "http_engine": f.http_engine,
        "home": {
            "path": f.home.display().to_string(),
            "writable": f.home_writable,
            "config_toml": f.config_present,
            "secrets_env": f.secrets_present,
        },
        "browser": {
            "in_use": f.browsers.first().map(|p| p.display().to_string()),
            "brand": f.browsers.first().map(|p| crate::browser::Brand::of(p).name()),
            "major": f.browser_major,
            "newest_known_major": f.newest_known_major,
            "candidates": f.browsers.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        },
        "models": {
            "embedded": f.embedded_models,
            "installed": f.installed_models,
            // Without this the two lists read as a capability, and on a build with no `onnx-*`
            // feature they are bytes nothing opens.
            "inference": f.inference,
        },
        "dashboard": { "port": f.dashboard_port, "free": f.dashboard_free },
        "rest": { "port": f.rest_port, "enabled": f.rest_port != 0 },
        "problems": found.iter().map(|p| json!({
            "code": p.code, "message": p.message, "fix": p.fix,
        })).collect::<Vec<_>>(),
    })
}

/// Ask this machine everything [`problems`] needs to know.
pub fn collect(cfg: &Config) -> Facts {
    let home = svipall_core::config::home_dir();
    let browsers = crate::browser::detect_all(cfg);
    let browser_major = browsers
        .first()
        .and_then(|p| crate::browser::browser_version(p));
    // Any other browser on this machine is evidence of what the stable channel is up to. The one
    // in use is included: a single browser can never be stale relative to itself.
    let newest_known_major = browsers
        .iter()
        .filter_map(|p| crate::browser::browser_version(p))
        .max();
    Facts {
        version: env!("CARGO_PKG_VERSION").into(),
        target: env!("SVIPALL_TARGET").into(),
        exe: std::env::current_exe().ok(),
        impersonation: svipall_http::impersonation_available(),
        http_engine: format!("{:?}", svipall_http::Engine::resolve(&cfg.http_engine))
            .to_ascii_lowercase(),
        home_writable: writable(&home),
        config_present: home.join("config.toml").is_file(),
        secrets_present: home.join("secrets.env").is_file(),
        home,
        browsers,
        browser_major,
        newest_known_major,
        embedded_models: svipall_models::compiled_in()
            .into_iter()
            .map(String::from)
            .collect(),
        installed_models: installed_models(),
        inference: cfg!(feature = "onnx"),
        dashboard_port: cfg.dashboard_port,
        dashboard_free: port_free(&cfg.dashboard_bind, cfg.dashboard_port),
        rest_port: cfg.rest_port,
    }
}

/// The report for this machine.
pub fn report(cfg: &Config) -> Value {
    report_from(&collect(cfg))
}

/// Model files the operator put in `~/.svipall/models`, by stem. A file without its `.json`
/// sidecar is not counted, because `model_source` will not load one either.
fn installed_models() -> Vec<String> {
    let dir = crate::model_source::models_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let ext = path.extension()?.to_str()?;
            if ext != "onnx" && ext != "bin" {
                return None;
            }
            let stem = path.file_stem()?.to_str()?.to_string();
            path.with_extension("json").is_file().then_some(stem)
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Writable by actually writing: asking the metadata answers a different question on Windows,
/// where the read-only bit says nothing about the ACL that will refuse the write.
fn writable(dir: &std::path::Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".svipall-write-probe");
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Free by binding it. A port is only ever free at a moment, and this is the same moment the
/// dashboard would have used.
fn port_free(bind: &str, port: u16) -> bool {
    std::net::TcpListener::bind((bind, port)).is_ok()
}
