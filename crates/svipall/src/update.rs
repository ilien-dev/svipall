//! Check and update the two release binaries installed on this machine.
//!
//! A check never writes and an install never happens implicitly.  Agent integrations use the
//! report to show the current and latest versions, explain that one user-owned installation is
//! shared by every harness, and ask before calling the install path.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use svipall_http::{HttpFetcher, HttpRequest};

const LATEST_RELEASE: &str = "https://api.github.com/repos/ilien-dev/svipall/releases/latest";
const RAW_RELEASE: &str = "https://raw.githubusercontent.com/ilien-dev/svipall";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallChannel {
    Installer,
    Homebrew,
    Scoop,
    Cargo,
    Npm,
    Container,
    SystemPackage,
    Unknown,
}

#[derive(Debug, Serialize)]
pub struct UpdateReport {
    pub current: String,
    pub latest: String,
    pub update_available: bool,
    pub channel: InstallChannel,
    pub executable: PathBuf,
    pub affects_all_harnesses: bool,
    pub installed: bool,
    pub release_url: String,
    pub install_command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
}

/// Identify only locations with an unambiguous owner.  An unknown path stays unknown: choosing a
/// second install channel behind the user's back is how two copies end up competing on PATH.
pub fn channel_for(executable: &Path) -> InstallChannel {
    let normalized = executable
        .to_string_lossy()
        .replace('\\', "/")
        .to_lowercase();
    if normalized.ends_with("/.local/bin/svipall")
        || normalized.ends_with("/programs/svipall/svipall.exe")
    {
        InstallChannel::Installer
    } else if normalized.contains("/homebrew/") || normalized.contains("/cellar/svipall/") {
        InstallChannel::Homebrew
    } else if normalized.contains("/scoop/apps/svipall/") {
        InstallChannel::Scoop
    } else if normalized.ends_with("/.cargo/bin/svipall")
        || normalized.ends_with("/.cargo/bin/svipall.exe")
    {
        InstallChannel::Cargo
    } else if normalized.contains("/node_modules/") || normalized.contains("/_npx/") {
        InstallChannel::Npm
    } else if normalized == "/usr/bin/svipall" || normalized == "/usr/local/bin/svipall" {
        if Path::new("/.dockerenv").exists() {
            InstallChannel::Container
        } else {
            InstallChannel::SystemPackage
        }
    } else {
        InstallChannel::Unknown
    }
}

pub fn report_from_release(
    current: &str,
    release_json: &str,
    executable: &Path,
) -> Result<UpdateReport> {
    let release: GithubRelease = serde_json::from_str(release_json)
        .context("the latest release response is not valid JSON")?;
    let latest = release.tag_name.trim_start_matches('v').to_string();
    let installed_version = semver::Version::parse(current)
        .with_context(|| format!("installed version {current:?} is not semantic versioning"))?;
    let latest_version = semver::Version::parse(&latest)
        .with_context(|| format!("release version {latest:?} is not semantic versioning"))?;
    let channel = channel_for(executable);
    Ok(UpdateReport {
        current: current.to_string(),
        latest: latest.clone(),
        update_available: latest_version > installed_version,
        channel,
        executable: executable.to_path_buf(),
        affects_all_harnesses: true,
        installed: false,
        release_url: release.html_url,
        install_command: command_for(channel, executable, &latest),
        note: Some(
            "The user-owned svipall and svipall-mcp binaries are shared by every configured harness on this machine."
                .into(),
        ),
    })
}

fn command_for(channel: InstallChannel, executable: &Path, latest: &str) -> String {
    let prefix = executable.parent().unwrap_or_else(|| Path::new("."));
    match channel {
        InstallChannel::Installer if cfg!(windows) => format!(
            "$p=Join-Path $env:TEMP 'svipall-install.ps1'; irm {RAW_RELEASE}/v{latest}/install.ps1 -OutFile $p; & $p -Version v{latest} -Prefix \"{}\" -Yes",
            prefix.display()
        ),
        InstallChannel::Installer => format!(
            "curl -fsSL {RAW_RELEASE}/v{latest}/install.sh | sh -s -- --version v{latest} --prefix \"{}\" --yes --no-path",
            prefix.display()
        ),
        InstallChannel::Homebrew => "brew upgrade ilien-dev/svipall/svipall".into(),
        InstallChannel::Scoop => "scoop update svipall".into(),
        InstallChannel::Cargo => "cargo install svipall --force".into(),
        InstallChannel::Npm => npm_command(executable, latest),
        InstallChannel::Container => "docker pull ghcr.io/ilien-dev/svipall:latest".into(),
        InstallChannel::SystemPackage => {
            "update svipall with the package manager that owns this executable".into()
        }
        InstallChannel::Unknown => {
            "identify the existing installation channel before updating".into()
        }
    }
}

fn npm_command(executable: &Path, latest: &str) -> String {
    let display = executable.to_string_lossy().replace('\\', "/");
    let normalized = display.to_lowercase();

    // npx owns a disposable cache rather than a durable global package.  An explicit version
    // refreshes that cache without inventing a second system-wide installation.
    if normalized.contains("/_npx/") {
        return format!("npx --yes --package=svipall@{latest} svipall --version");
    }

    // npm's executable lives below <prefix>/node_modules/svipall.  Point npm back at that exact
    // prefix, so a project-local package remains local instead of silently becoming global.
    if let Some((prefix, _)) = display.rsplit_once("/node_modules/") {
        return format!("npm install --prefix \"{prefix}\" svipall@{latest}");
    }
    format!("npm install svipall@{latest}")
}

fn latest_url() -> String {
    std::env::var("SVIPALL_UPDATE_URL").unwrap_or_else(|_| LATEST_RELEASE.into())
}

fn raw_release_base() -> String {
    std::env::var("SVIPALL_RAW_RELEASE_URL").unwrap_or_else(|_| RAW_RELEASE.into())
}

async fn http() -> Result<Arc<dyn HttpFetcher>> {
    let cfg = svipall_core::config::load_in(&svipall_core::config::home_dir()).unwrap_or_default();
    let identity = svipall_core::IdentityProfile::resolve(None, &cfg);
    svipall_http::build(svipall_http::FetcherConfig::new(identity))
}

async fn get(http: &Arc<dyn HttpFetcher>, url: &str) -> Result<Vec<u8>> {
    let mut req = HttpRequest::get(url.to_string());
    req.headers = http.identity().nav_headers();
    let resp = http.send(req).await?;
    if resp.status >= 400 {
        bail!("HTTP {} for {url}", resp.status);
    }
    Ok(resp.body)
}

pub async fn check() -> Result<UpdateReport> {
    let executable = std::env::current_exe().context("locating the running svipall executable")?;
    let body = get(&http().await?, &latest_url()).await?;
    report_from_release(
        env!("CARGO_PKG_VERSION"),
        &String::from_utf8_lossy(&body),
        &executable,
    )
}

/// Run the explicit update surface.  `install=false` is a read-only check.  The installer channel
/// reuses the release's own installer so archive selection, checksums and atomic replacement remain
/// one implementation.  Other channels return their manager's command instead of mixing installs.
pub async fn run(install: bool) -> Result<serde_json::Value> {
    let mut report = check().await?;
    let home = svipall_core::config::home_dir();
    if home.is_dir() {
        let _ = remember(&home, &report.latest, now());
    }
    if !install {
        return serde_json::to_value(report).context("serializing update report");
    }
    if !report.update_available {
        // Nothing newer, but a skill copied by hand may still be older than this binary: that
        // is how a Windows update, which the PowerShell command finished, gets its skills.
        let refreshed = refresh_skill_copies(&report.current).await;
        let mut value = serde_json::to_value(report).context("serializing update report")?;
        value["skills_refreshed"] = serde_json::json!(refreshed);
        return Ok(value);
    }
    if report.channel != InstallChannel::Installer {
        report.note = Some(format!(
            "This installation belongs to the {:?} channel. Run install_command; Svipall will not create a second installation. Then run `svipall update` once more to refresh the skills.",
            report.channel
        ));
        return serde_json::to_value(report).context("serializing update report");
    }
    if cfg!(windows) {
        report.note = Some(
            "Windows cannot replace the svipall.exe process that is running this command. Close every harness using svipall-mcp, run install_command in PowerShell, then `svipall update` once more to refresh the skills; no files were changed by this command."
                .into(),
        );
        return serde_json::to_value(report).context("serializing update report");
    }

    install_release(&report).await?;
    report.installed = true;
    report.current = report.latest.clone();
    report.update_available = false;
    report.note = Some(finished_note(&report.latest));
    let refreshed = refresh_skill_copies(&report.latest).await;
    let mut value = serde_json::to_value(report).context("serializing update report")?;
    value["skills_refreshed"] = serde_json::json!(refreshed);
    Ok(value)
}

async fn install_release(report: &UpdateReport) -> Result<()> {
    let ext = if cfg!(windows) { "ps1" } else { "sh" };
    let url = format!(
        "{}/v{}/install.{ext}",
        raw_release_base().trim_end_matches('/'),
        report.latest
    );
    let script = get(&http().await?, &url)
        .await
        .with_context(|| format!("downloading the v{} installer", report.latest))?;
    let path = std::env::temp_dir().join(format!(
        "svipall-update-{}-{}.{}",
        std::process::id(),
        uuid::Uuid::new_v4(),
        ext
    ));
    std::fs::write(&path, script).with_context(|| format!("writing {}", path.display()))?;

    let prefix = report
        .executable
        .parent()
        .context("the running executable has no installation directory")?;
    #[cfg(unix)]
    let output = std::process::Command::new("sh")
        .arg(&path)
        .args(["--version", &format!("v{}", report.latest), "--prefix"])
        .arg(prefix)
        .args(["--yes", "--no-path"])
        .output();
    #[cfg(windows)]
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&path)
        .args(["-Version", &format!("v{}", report.latest), "-Prefix"])
        .arg(prefix)
        .arg("-Yes")
        .output();
    let _ = std::fs::remove_file(&path);
    let output = output.context("starting the release installer")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        bail!(
            "the release installer failed ({}): {}{}",
            output.status,
            stdout.trim(),
            stderr.trim()
        );
    }
    Ok(())
}

// ---- The notice ----------------------------------------------------------------------------
//
// A newer release is announced where the person will see it, once, with the one step that
// installs it: the plugin's `SessionStart` hook in Claude Code, the first tool result of an MCP
// session elsewhere, and a stderr line from the CLI a skill drives. The latest version is looked
// up at most once a day and remembered in `~/.svipall`; `update_check = false` turns all of it off.

/// Instructions any agent can follow, whether or not an updater skill was installed.
pub const UPDATE_GUIDE: &str =
    "https://raw.githubusercontent.com/ilien-dev/svipall/main/docs/update.md";
const REMEMBERED: &str = "update-check.json";
const ASK_AGAIN_AFTER: u64 = 24 * 60 * 60;

/// Where the notice will be read, which decides the command it names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Harness {
    ClaudePlugin,
    ClaudeCode,
    Codex,
    Cursor,
    OpenCode,
    Unknown,
}

impl Harness {
    /// From `clientInfo.name` in the MCP `initialize` handshake.
    pub fn from_mcp_client(name: &str) -> Self {
        let name = name.to_lowercase();
        if name.contains("claude") {
            Self::ClaudeCode
        } else if name.contains("codex") {
            Self::Codex
        } else if name.contains("cursor") {
            Self::Cursor
        } else if name.contains("opencode") {
            Self::OpenCode
        } else {
            Self::Unknown
        }
    }

    /// For the CLI, which has no handshake: only Claude Code says so in the environment.
    pub fn from_env() -> Self {
        if std::env::var_os("CLAUDECODE").is_some() {
            Self::ClaudeCode
        } else {
            Self::Unknown
        }
    }
}

/// "Svipall X is available" and the single step that installs it, or None when nothing is newer.
pub fn notice(current: &str, latest: &str, harness: Harness) -> Option<String> {
    let (Ok(have), Ok(new)) = (
        semver::Version::parse(current),
        semver::Version::parse(latest),
    ) else {
        return None;
    };
    if new <= have {
        return None;
    }
    let prompt = format!("ask the agent: \"Update Svipall by following {UPDATE_GUIDE}\"");
    let step = match harness {
        Harness::ClaudePlugin => "run /svipall:update".to_string(),
        Harness::Codex => format!("run $svipall-update, or {prompt}"),
        Harness::ClaudeCode | Harness::Cursor | Harness::OpenCode => {
            format!("run /svipall-update, or {prompt}")
        }
        Harness::Unknown => prompt,
    };
    Some(format!(
        "Svipall {latest} is available; this machine runs {current}. To update, {step}."
    ))
}

#[derive(Serialize, Deserialize)]
struct Remembered {
    latest: String,
    checked_at: u64,
}

/// The remembered latest version, and whether it is old enough to ask again.
pub fn remembered(home: &Path, now: u64) -> (Option<String>, bool) {
    let found = std::fs::read(home.join(REMEMBERED))
        .ok()
        .and_then(|raw| serde_json::from_slice::<Remembered>(&raw).ok());
    match found {
        Some(r) => {
            let stale = now.saturating_sub(r.checked_at) > ASK_AGAIN_AFTER;
            (Some(r.latest), stale)
        }
        None => (None, true),
    }
}

pub fn remember(home: &Path, latest: &str, now: u64) -> Result<()> {
    let body = serde_json::to_vec(&Remembered {
        latest: latest.to_string(),
        checked_at: now,
    })?;
    std::fs::write(home.join(REMEMBERED), body).context("remembering the latest version")
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

fn notices_enabled() -> bool {
    let home = svipall_core::config::home_dir();
    svipall_core::config::load_in(&home)
        .map(|c| c.update_check)
        .unwrap_or(true)
}

/// The notice for this machine from what is remembered. When that is a day old, a detached
/// `svipall update --check` refreshes it for next time: nobody waits on the network for a notice.
pub fn pending_notice(harness: Harness) -> Option<String> {
    if !notices_enabled() {
        return None;
    }
    let (latest, stale) = remembered(&svipall_core::config::home_dir(), now());
    if stale {
        refresh_detached();
    }
    notice(env!("CARGO_PKG_VERSION"), &latest?, harness)
}

fn refresh_detached() {
    let Some(exe) = std::env::current_exe()
        .ok()
        .map(|e| e.with_file_name(svipall_exe()))
    else {
        return;
    };
    let _ = std::process::Command::new(exe)
        .args(["update", "--check"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

fn svipall_exe() -> &'static str {
    if cfg!(windows) {
        "svipall.exe"
    } else {
        "svipall"
    }
}

/// For `svipall-mcp`, once per process, appended to the first tool result that finds one. Inside
/// the Claude Code plugin the `SessionStart` hook already said it, so the server stays quiet.
pub fn mcp_notice_once(client: Option<&str>) -> Option<String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    static SAID: AtomicBool = AtomicBool::new(false);
    if SAID.load(Ordering::Relaxed)
        || std::env::var("SVIPALL_INTEGRATION").as_deref() == Ok("claude-plugin")
    {
        return None;
    }
    let harness = client.map_or(Harness::Unknown, Harness::from_mcp_client);
    let text = pending_notice(harness)?;
    (!SAID.swap(true, Ordering::Relaxed)).then(|| for_the_agent(&text))
}

/// Outside the plugin nothing but the agent reaches the person, so the notice asks it to close
/// its answer with the line, where it is read last.
pub fn for_the_agent(notice: &str) -> String {
    format!(
        "svipall update notice: at the end of your final answer this turn, after everything else, \
         add this line for the user: {notice}"
    )
}

/// True the first time a Claude Code session asks; the plugin shows its notice once per session.
pub fn first_in_session(home: &Path, session: &str) -> bool {
    let file = home.join("update-noticed-session");
    if std::fs::read_to_string(&file).is_ok_and(|seen| seen == session) {
        return false;
    }
    let _ = std::fs::write(&file, session);
    true
}

/// At `svipall-mcp` startup: refresh what is remembered in the background when it is a day old.
pub async fn refresh_if_stale() {
    let home = svipall_core::config::home_dir();
    if !notices_enabled() || !remembered(&home, now()).1 {
        return;
    }
    if let Ok(report) = check().await {
        let _ = remember(&home, &report.latest, now());
    }
}

// ---- One command ---------------------------------------------------------------------------

/// `svipall update` installs; only `--check` (or `check`) is read-only.
pub fn wants_install(args: &[String]) -> bool {
    !args.iter().any(|a| a == "--check" || a == "check")
}

/// The skill files a person copied by hand, paired with their path in the repository. Only files
/// that exist are listed: an update refreshes an integration, it never creates one.
pub fn skill_copies(home: &Path, project: &Path) -> Vec<(PathBuf, &'static str)> {
    const SKILL_DIRS: [&str; 4] = [
        ".claude/skills",
        ".agents/skills",
        ".cursor/skills",
        ".opencode/skills",
    ];
    let mut roots: Vec<PathBuf> = SKILL_DIRS.iter().map(|d| home.join(d)).collect();
    roots.push(home.join(".config/opencode/skills"));
    roots.extend(SKILL_DIRS.iter().map(|d| project.join(d)));
    let mut files = Vec::new();
    for root in &roots {
        files.push((root.join("svipall/SKILL.md"), "skill/SKILL.md"));
        files.push((
            root.join("svipall-update/SKILL.md"),
            "skills/svipall-update/SKILL.md",
        ));
    }
    for commands in [
        home.join(".config/opencode/commands"),
        project.join(".opencode/commands"),
    ] {
        files.push((
            commands.join("svipall-update.md"),
            "integrations/opencode/commands/svipall-update.md",
        ));
    }
    files.retain(|(path, _)| path.is_file());
    files.dedup();
    files
}

async fn refresh_skill_copies(version: &str) -> Vec<String> {
    let home = dirs::home_dir().unwrap_or_default();
    let project = std::env::current_dir().unwrap_or_default();
    let copies = skill_copies(&home, &project);
    if copies.is_empty() {
        return Vec::new();
    }
    let Ok(http) = http().await else {
        return Vec::new();
    };
    let base = raw_release_base();
    let mut refreshed = Vec::new();
    for (path, source) in copies {
        let url = format!("{}/v{version}/{source}", base.trim_end_matches('/'));
        if let Ok(body) = get(&http, &url).await {
            if std::fs::write(&path, body).is_ok() {
                refreshed.push(path.display().to_string());
            }
        }
    }
    refreshed
}

/// The last thing an update says.
pub fn finished_note(version: &str) -> String {
    format!(
        "Svipall is now {version}. Close every session that uses Svipall (Claude Code, Codex, \
         Cursor, OpenCode...) and open it again so it runs the new version."
    )
}
