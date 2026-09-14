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
