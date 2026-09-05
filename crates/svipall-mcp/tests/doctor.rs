//! What an installer, a package manager and the Claude Code plugin ask this binary about itself.
//!
//! Every one of them needs the same two answers before it can do anything useful: *which build is
//! this* and *is it going to work here*. Both are asserted against synthetic facts rather than
//! against this machine, because a test that passes only where a browser happens to be installed
//! tells you nothing about the machine the installer actually lands on.

use svipall_mcp::doctor::{self, Facts};

/// Facts from a machine where everything is in place. Each test spoils exactly one of them.
fn healthy() -> Facts {
    Facts {
        version: "1.0.0".into(),
        target: "x86_64-unknown-linux-gnu".into(),
        exe: Some("/home/someone/.local/bin/svipall".into()),
        impersonation: true,
        http_engine: "auto".into(),
        home: "/home/someone/.svipall".into(),
        home_writable: true,
        config_present: false,
        secrets_present: false,
        browsers: vec!["/opt/google/chrome/chrome".into()],
        browser_major: Some(141),
        newest_known_major: Some(141),
        embedded_models: vec!["detect".into(), "segment".into()],
        installed_models: vec![],
        inference: true,
        dashboard_port: 8787,
        dashboard_free: true,
        rest_port: 0,
    }
}

fn codes(f: &Facts) -> Vec<String> {
    doctor::problems(f).into_iter().map(|p| p.code).collect()
}

#[test]
fn the_version_object_names_the_version_the_package_declares() {
    let v = doctor::version_json();
    assert_eq!(
        v["version"].as_str(),
        Some(env!("CARGO_PKG_VERSION")),
        "an installer that cannot read the version cannot tell an upgrade from a reinstall"
    );
}

#[test]
fn the_version_object_names_the_target_it_was_built_for() {
    let v = doctor::version_json();
    let target = v["target"].as_str().unwrap_or_default();
    // Homebrew, Scoop and winget all pick an artefact by target triple; "unknown" would send a
    // user the wrong build and the failure would look like a corrupt download.
    assert!(
        target.contains('-') && target != "unknown",
        "target was {target:?}"
    );
}

#[test]
fn the_version_object_says_whether_this_build_can_emulate_a_browser() {
    let v = doctor::version_json();
    assert!(
        v["impersonation"].is_boolean(),
        "a --no-default-features build is a different product and has to say so"
    );
}

#[test]
fn a_healthy_machine_reports_no_problems() {
    assert!(doctor::problems(&healthy()).is_empty());
    assert_eq!(
        doctor::report_from(&healthy())["ok"],
        serde_json::json!(true)
    );
}

#[test]
fn no_browser_is_a_problem_that_names_the_way_out() {
    let mut f = healthy();
    f.browsers.clear();
    f.browser_major = None;
    assert!(codes(&f).contains(&"no_browser".to_string()));
}

#[test]
fn a_browser_that_defends_itself_contradicts_the_identity_we_advertise() {
    // Brave randomises what the stealth script cannot: it is the binary talking, not the page.
    let mut f = healthy();
    f.browsers = vec!["/usr/bin/brave-browser".into()];
    assert!(codes(&f).contains(&"self_defending_browser".to_string()));
}

#[test]
fn a_browser_two_majors_behind_is_a_problem() {
    let mut f = healthy();
    f.browser_major = Some(139);
    f.newest_known_major = Some(141);
    assert!(codes(&f).contains(&"stale_browser".to_string()));
}

#[test]
fn a_build_with_no_models_says_so_rather_than_failing_a_captcha_quietly() {
    // This is the difference between a release tarball and a plain `cargo build`, and between a
    // release tarball and today's container image. It has to be visible.
    let mut f = healthy();
    f.embedded_models.clear();
    assert!(codes(&f).contains(&"no_models".to_string()));
}

#[test]
fn a_model_the_operator_installed_answers_for_the_one_that_is_missing() {
    let mut f = healthy();
    f.embedded_models.clear();
    f.installed_models = vec!["detect".into()];
    assert!(!codes(&f).contains(&"no_models".to_string()));
}

#[test]
fn weights_with_nothing_to_run_them_are_reported_as_inert() {
    // A build can carry 58 MB of ONNX weights and not one `onnx-*` feature to read them: the
    // build script embeds whatever files are on disk, the features are separate. `models.embedded`
    // then lists models that answer nothing, which reads as working and is not.
    let mut f = healthy();
    f.inference = false;
    assert!(codes(&f).contains(&"models_not_readable".to_string()));
}

#[test]
fn a_build_with_neither_weights_nor_inference_reports_the_more_basic_problem_only() {
    let mut f = healthy();
    f.inference = false;
    f.embedded_models.clear();
    let found = codes(&f);
    assert!(found.contains(&"no_models".to_string()));
    assert!(
        !found.contains(&"models_not_readable".to_string()),
        "two problems for one missing capability is noise"
    );
}

#[test]
fn a_home_it_cannot_write_to_is_a_problem() {
    let mut f = healthy();
    f.home_writable = false;
    assert!(codes(&f).contains(&"home_not_writable".to_string()));
}

#[test]
fn a_taken_dashboard_port_is_a_problem_because_the_url_would_point_at_nothing() {
    let mut f = healthy();
    f.dashboard_free = false;
    assert!(codes(&f).contains(&"dashboard_port_busy".to_string()));
}

#[test]
fn a_reqwest_only_build_says_it_cannot_emulate_a_browser() {
    let mut f = healthy();
    f.impersonation = false;
    assert!(codes(&f).contains(&"no_impersonation".to_string()));
}

#[test]
fn every_problem_carries_a_fix_a_person_can_run() {
    // A diagnosis without a next step is a diagnosis nobody acts on — and the plugin reads these
    // verbatim, so an empty `fix` becomes an agent guessing.
    let mut f = healthy();
    f.browsers.clear();
    f.browser_major = None;
    f.embedded_models.clear();
    f.inference = false;
    f.home_writable = false;
    f.dashboard_free = false;
    f.impersonation = false;
    let found = doctor::problems(&f);
    assert!(found.len() >= 5);
    for p in found {
        assert!(!p.code.is_empty(), "a problem with no code");
        assert!(!p.message.trim().is_empty(), "{} has no message", p.code);
        assert!(!p.fix.trim().is_empty(), "{} has no fix", p.code);
    }
}

#[test]
fn the_report_is_one_json_object_an_installer_can_read_without_a_parser_of_its_own() {
    let v = doctor::report_from(&healthy());
    for key in [
        "ok",
        "version",
        "target",
        "home",
        "browser",
        "models",
        "dashboard",
        "problems",
    ] {
        assert!(v.get(key).is_some(), "{key} is missing from the report");
    }
    assert!(v["problems"].is_array());
}
