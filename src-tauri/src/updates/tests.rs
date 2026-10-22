use super::*;
#[test]
fn versions_reject_prereleases_build_metadata_and_downgrades() {
    assert!(policy::stable_newer("1.0.0", "1.0.1"));
    for version in ["0.9.9", "1.0.0", "1.1.0-beta.1", "1.1.0+build", "01.1.0"] {
        assert!(!policy::stable_newer("1.0.0", version));
    }
}
#[test]
fn package_limit_is_enforced_before_append_without_content_length() {
    let mut bytes = vec![1; 7];
    assert!(transport::append_limited(&mut bytes, &[2, 2], 8).is_err());
    assert_eq!(bytes.len(), 7);
    transport::append_limited(&mut bytes, &[2], 8).unwrap();
    assert_eq!(bytes.len(), 8);
}
#[test]
fn redirect_policy_rejects_credentials_plaintext_unknown_hosts_and_sixth_hop() {
    for url in [
        "http://github.com/x",
        "https://github.com.evil.test/x",
        "https://user@github.com/x",
        "https://github.com:444/x",
    ] {
        assert!(!transport::redirect_allowed(
            &tauri::Url::parse(url).unwrap(),
            1
        ));
    }
    let url = tauri::Url::parse("https://release-assets.githubusercontent.com/x").unwrap();
    assert!(transport::redirect_allowed(&url, 5));
    assert!(!transport::redirect_allowed(&url, 6));
}
#[test]
fn platform_never_borrows_another_installer_or_architecture() {
    use tauri::utils::config::BundleType;
    for (os, arch, bundle, format) in [
        ("windows", "x86_64", BundleType::Msi, "msi"),
        ("linux", "x86_64", BundleType::Deb, "deb"),
        ("macos", "aarch64", BundleType::App, "dmg"),
    ] {
        let cap = platform::profile(os, arch, Some(bundle), false);
        assert_eq!(cap.mode, "manual");
        assert_eq!(cap.target, None);
        assert_eq!(cap.manual_format.as_deref(), Some(format));
    }
    assert_eq!(
        platform::profile("windows", "aarch64", Some(BundleType::Nsis), false).mode,
        "unavailable"
    );
    assert_eq!(
        platform::profile("windows", "x86_64", Some(BundleType::Nsis), false).mode,
        "inApp"
    );
}
#[test]
fn result_survives_background_checks_and_disk_reload() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let mut saved = Saved::default();
    saved.pending_install = Some(PendingInstall {
        id: "attempt".into(),
        source_version: "1.0.0".into(),
        target_version: "1.0.1".into(),
        handed_off: true,
        started_at: 0,
    });
    assert!(policy::startup_result(&mut saved, "1.0.0"));
    assert_eq!(saved.unread_result.as_ref().unwrap().outcome, "incomplete");
    store::persist(&path, &saved).unwrap();
    let (loaded, healthy) = store::load(&path);
    assert!(healthy);
    assert!(loaded.pending_install.is_none());
    assert_eq!(loaded.unread_result.unwrap().id, "attempt");
    std::fs::write(&path, "corrupt").unwrap();
    assert!(!store::load(&path).1);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "corrupt");
}
#[test]
fn completion_requires_the_target_or_a_newer_stable_running_version() {
    for (actual, outcome) in [
        ("1.0.0", "incomplete"),
        ("1.0.1", "completed"),
        ("1.1.0", "completed"),
        ("1.0.1-beta.1", "incomplete"),
        ("invalid", "incomplete"),
    ] {
        let mut saved = Saved::default();
        saved.pending_install = Some(PendingInstall {
            id: "attempt".into(),
            source_version: "1.0.0".into(),
            target_version: "1.0.1".into(),
            handed_off: true,
            started_at: 0,
        });
        assert!(policy::startup_result(&mut saved, actual));
        assert_eq!(saved.unread_result.as_ref().unwrap().outcome, outcome);
        assert!(!policy::startup_result(&mut saved, actual));
        assert_eq!(saved.unread_result.unwrap().id, "attempt");
    }
}
#[test]
fn failed_checks_back_off_and_never_overflow() {
    assert_eq!(policy::backoff(0), DAY);
    assert_eq!(policy::backoff(1), 15 * 60);
    assert_eq!(policy::backoff(2), 60 * 60);
    assert_eq!(policy::backoff(3), 6 * 60 * 60);
    assert_eq!(policy::backoff(u32::MAX), 6 * 60 * 60);
}
#[test]
fn blockers_and_resource_identity_are_not_only_counts() {
    let state = crate::connection::tests::state();
    let (review, a) = install::preflight(&state, 0).unwrap();
    assert!(review.blockers.is_empty());
    let mutation = state
        .files
        .begin_mutation("server", &["/file".into()], "编辑")
        .unwrap();
    let (review, b) = install::preflight(&state, 0).unwrap();
    assert!(!review.blockers.is_empty());
    assert_ne!(a, b);
    drop(mutation);
    let (_, c) = install::preflight(&state, 1).unwrap();
    assert_ne!(a, c);
}

#[test]
fn suppression_and_automatic_check_are_independent() {
    let candidate = Candidate {
        release: Release {
            id: "candidate".into(),
            version: "1.0.1".into(),
            published_at: None,
            notes: String::new(),
        },
        update: None,
        fingerprint: "package".into(),
        manual_url: None,
        capability: platform::profile(
            "windows",
            "x86_64",
            Some(tauri::utils::config::BundleType::Nsis),
            false,
        ),
    };
    let mut saved = Saved {
        auto_check: false,
        ..Saved::default()
    };
    assert!(policy::auto_download(&saved, &candidate, true));
    saved.suppressed_version = Some("1.0.1".into());
    saved.suppression = Some("cancelled".into());
    assert!(!policy::auto_download(&saved, &candidate, true));
    saved.suppressed_version = Some("1.0.0".into());
    assert!(policy::auto_download(&saved, &candidate, true));
    saved.skipped_version = Some("1.0.1".into());
    assert!(!policy::auto_download(&saved, &candidate, true));
    saved.skipped_version = None;
    assert!(!policy::auto_download(&saved, &candidate, false));
}
#[test]
fn unknown_schema_and_state_write_failures_preserve_prior_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    std::fs::write(&path, r#"{"schemaVersion":77}"#).unwrap();
    assert!(!store::load(&path).1);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        r#"{"schemaVersion":77}"#
    );
    assert!(store::persist(&dir.path().join("missing/state.json"), &Saved::default()).is_err());
}
#[test]
fn diagnostic_versions_and_targets_cannot_contain_arbitrary_data() {
    let value = crate::diagnostics::event::safe_details(
        serde_json::json!({"currentVersion":"1.0.0", "targetVersion":"https://secret.test/token", "updateTarget":"windows-x86_64-nsis", "url":"https://secret.test/token"}),
    );
    assert_eq!(value["currentVersion"], "1.0.0");
    assert!(value.get("targetVersion").is_none());
    assert!(value.get("url").is_none());
}
