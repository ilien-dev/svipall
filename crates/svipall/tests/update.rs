//! Updating is explicit: checking is read-only, and installation happens only after a harness has
//! shown the two versions and obtained the user's choice.  These tests keep the version comparison
//! and channel advice independent of GitHub and of this developer machine.

use std::path::Path;
use svipall::update::{self, InstallChannel};

mod support;
use support::{Reply, Site};

#[test]
fn an_older_installation_reports_the_current_and_latest_versions() {
    let report = update::report_from_release(
        "1.0.3",
        r#"{"tag_name":"v1.0.5","html_url":"https://example.test/v1.0.5"}"#,
        Path::new("/home/alice/.local/bin/svipall"),
    )
    .expect("valid release");

    assert_eq!(report.current, "1.0.3");
    assert_eq!(report.latest, "1.0.5");
    assert!(report.update_available);
    assert_eq!(report.channel, InstallChannel::Installer);
    assert!(report.affects_all_harnesses);
    assert!(!report.installed, "a version check must never install");
}

#[test]
fn semantic_versions_are_compared_as_versions_not_strings() {
    let report = update::report_from_release(
        "1.0.9",
        r#"{"tag_name":"v1.0.10","html_url":"https://example.test/v1.0.10"}"#,
        Path::new("/home/alice/.local/bin/svipall"),
    )
    .expect("valid release");
    assert!(report.update_available);
}

#[test]
fn a_current_installation_does_not_offer_a_reinstall_as_an_update() {
    let report = update::report_from_release(
        "1.0.5",
        r#"{"tag_name":"v1.0.5","html_url":"https://example.test/v1.0.5"}"#,
        Path::new("/home/alice/.local/bin/svipall"),
    )
    .expect("valid release");
    assert!(!report.update_available);
}

#[test]
fn the_update_channel_is_not_guessed_for_an_unknown_install_location() {
    assert_eq!(
        update::channel_for(Path::new("/opt/custom/svipall")),
        InstallChannel::Unknown
    );
}

#[test]
fn npm_updates_the_package_at_its_existing_prefix() {
    let report = update::report_from_release(
        "1.0.3",
        r#"{"tag_name":"v1.0.5","html_url":"https://example.test/v1.0.5"}"#,
        Path::new("/work/app/node_modules/svipall/vendor/svipall"),
    )
    .expect("valid release");

    assert_eq!(report.channel, InstallChannel::Npm);
    assert_eq!(
        report.install_command,
        "npm install --prefix \"/work/app\" svipall@1.0.5"
    );
}

#[test]
fn npx_refreshes_its_cache_without_creating_a_global_installation() {
    let report = update::report_from_release(
        "1.0.3",
        r#"{"tag_name":"v1.0.5","html_url":"https://example.test/v1.0.5"}"#,
        Path::new("/home/alice/.npm/_npx/abc/node_modules/svipall/vendor/svipall"),
    )
    .expect("valid release");

    assert_eq!(report.channel, InstallChannel::Npm);
    assert_eq!(
        report.install_command,
        "npx --yes --package=svipall@1.0.5 svipall --version"
    );
}

#[tokio::test]
async fn the_live_check_reads_release_metadata_without_installing() {
    let site = Site::start(vec![(
        "/latest",
        Reply::plain(r#"{"tag_name":"v99.0.0","html_url":"https://example.test/v99.0.0"}"#),
    )])
    .await;
    std::env::set_var("SVIPALL_UPDATE_URL", site.url("/latest"));
    let report = update::check().await.expect("check the fixture release");
    std::env::remove_var("SVIPALL_UPDATE_URL");

    assert_eq!(site.hits("/latest"), 1);
    assert_eq!(report.latest, "99.0.0");
    assert!(report.update_available);
    assert!(!report.installed);
}

// ---- The notice: one line saying a newer version exists and the one step that installs it. ----

use update::Harness;

#[test]
fn no_notice_when_the_installed_version_is_current_or_newer() {
    assert_eq!(
        update::notice("1.2.0", "1.2.0", Harness::ClaudePlugin),
        None
    );
    assert_eq!(update::notice("1.3.0", "1.2.0", Harness::Unknown), None);
    assert_eq!(update::notice("garbage", "1.2.0", Harness::Unknown), None);
}

#[test]
fn the_claude_plugin_notice_names_its_own_command() {
    let n = update::notice("1.1.1", "1.2.0", Harness::ClaudePlugin).expect("outdated");
    assert!(n.contains("1.2.0") && n.contains("1.1.1"), "{n}");
    assert!(n.contains("/svipall:update"), "{n}");
}

#[test]
fn every_other_harness_gets_its_command_and_the_prompt_that_needs_no_skill() {
    for (harness, command) in [
        (Harness::ClaudeCode, "/svipall-update"),
        (Harness::Codex, "$svipall-update"),
        (Harness::Cursor, "/svipall-update"),
        (Harness::OpenCode, "/svipall-update"),
    ] {
        let n = update::notice("1.1.1", "1.2.0", harness).expect("outdated");
        assert!(n.contains(command), "{harness:?}: {n}");
        assert!(n.contains(update::UPDATE_GUIDE), "{harness:?}: {n}");
    }
    let n = update::notice("1.1.1", "1.2.0", Harness::Unknown).expect("outdated");
    assert!(n.contains(update::UPDATE_GUIDE), "{n}");
    assert!(update::UPDATE_GUIDE.ends_with("/docs/update.md"));
}

#[test]
fn an_mcp_client_is_named_by_its_initialize_handshake() {
    assert_eq!(Harness::from_mcp_client("claude-code"), Harness::ClaudeCode);
    assert_eq!(Harness::from_mcp_client("codex-mcp-client"), Harness::Codex);
    assert_eq!(Harness::from_mcp_client("cursor-vscode"), Harness::Cursor);
    assert_eq!(Harness::from_mcp_client("opencode"), Harness::OpenCode);
    assert_eq!(Harness::from_mcp_client("something-else"), Harness::Unknown);
}

#[test]
fn the_latest_version_is_remembered_for_a_day_and_then_asked_again() {
    let home = std::env::temp_dir().join(format!("svipall-update-cache-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();

    assert_eq!(
        update::remembered(&home, 1_000),
        (None, true),
        "nothing known is stale"
    );
    update::remember(&home, "1.2.0", 1_000).unwrap();
    assert_eq!(
        update::remembered(&home, 1_000 + 3_600),
        (Some("1.2.0".into()), false)
    );
    assert_eq!(
        update::remembered(&home, 1_000 + 86_401),
        (Some("1.2.0".into()), true)
    );
    let _ = std::fs::remove_dir_all(&home);
}

// ---- One command: `svipall update` installs, and it refreshes the skills copied by hand. ----

#[test]
fn a_bare_update_installs_and_only_check_is_read_only() {
    let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert!(update::wants_install(&args(&[])));
    assert!(update::wants_install(&args(&["--install"])));
    assert!(!update::wants_install(&args(&["--check"])));
    assert!(!update::wants_install(&args(&["check"])));
}

#[test]
fn the_skills_copied_by_hand_are_found_in_every_harness_and_scope() {
    let root = std::env::temp_dir().join(format!("svipall-skill-targets-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (home, project) = (root.join("home"), root.join("project"));
    for file in [
        home.join(".claude/skills/svipall/SKILL.md"),
        home.join(".claude/skills/svipall-update/SKILL.md"),
        home.join(".agents/skills/svipall/SKILL.md"),
        project.join(".cursor/skills/svipall/SKILL.md"),
        project.join(".opencode/commands/svipall-update.md"),
    ] {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "old").unwrap();
    }

    let mut found = update::skill_copies(&home, &project);
    found.sort();
    let mut expected = vec![
        (
            home.join(".claude/skills/svipall/SKILL.md"),
            "skill/SKILL.md",
        ),
        (
            home.join(".claude/skills/svipall-update/SKILL.md"),
            "skills/svipall-update/SKILL.md",
        ),
        (
            home.join(".agents/skills/svipall/SKILL.md"),
            "skill/SKILL.md",
        ),
        (
            project.join(".cursor/skills/svipall/SKILL.md"),
            "skill/SKILL.md",
        ),
        (
            project.join(".opencode/commands/svipall-update.md"),
            "integrations/opencode/commands/svipall-update.md",
        ),
    ];
    expected.sort();
    assert_eq!(
        found, expected,
        "only files that exist are refreshed; none is created"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_update_ends_by_saying_every_session_must_be_reopened() {
    let note = update::finished_note("1.2.0").to_lowercase();
    assert!(
        note.contains("1.2.0") && note.contains("close") && note.contains("open"),
        "{note}"
    );
}

#[test]
fn the_notice_is_shown_once_per_session() {
    let home = std::env::temp_dir().join(format!("svipall-update-session-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    assert!(update::first_in_session(&home, "a"));
    assert!(!update::first_in_session(&home, "a"));
    assert!(update::first_in_session(&home, "b"));
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn outside_the_plugin_the_agent_is_told_to_put_the_notice_at_the_end() {
    let n = update::for_the_agent("Svipall 1.2.0 is available.");
    assert!(n.to_lowercase().contains("end of your"), "{n}");
}
