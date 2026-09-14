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
    if !install || !report.update_available {
        return serde_json::to_value(report).context("serializing update report");
    }
    if report.channel != InstallChannel::Installer {
        report.note = Some(format!(
            "This installation belongs to the {:?} channel. Run install_command after confirmation; Svipall will not create a second installation.",
            report.channel
        ));
        return serde_json::to_value(report).context("serializing update report");
    }
    if cfg!(windows) {
        report.note = Some(
            "Windows cannot replace the svipall.exe process that is running this command. Close every harness using svipall-mcp, then run install_command in PowerShell; no files were changed by this command."
                .into(),
        );
        return serde_json::to_value(report).context("serializing update report");
    }

    install_release(&report).await?;
    report.installed = true;
    report.current = report.latest.clone();
    report.update_available = false;
    report.note = Some(
        "Updated the shared user-owned binaries. Restart harnesses with a running svipall-mcp process so they load the new executable."
            .into(),
    );
    serde_json::to_value(report).context("serializing update report")
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
