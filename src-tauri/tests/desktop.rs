use launchone_lib::{
    adapters::{Context, Registry},
    config::{read_value, ConfigStore, Format},
    desktop_app::{mac_installation, CODEX},
    models::{Credential, Mode},
};
use serde_json::json;
use std::fs;
fn credential() -> Credential {
    Credential {
        key: "fixture-personal-key".into(),
        base_url: "https://tokenone.example.test".into(),
        model: "claude-sonnet-4-6".into(),
    }
}

#[test]
fn desktop_switch_subscription_and_restore_are_distinct_and_preserve_other_profiles() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = Context::isolated(dir.path().into());
    let registry = Registry::default();
    let adapter = registry.get("claude-desktop").unwrap();
    let targets = adapter.targets(&ctx).unwrap();
    fs::create_dir_all(targets[0].1.parent().unwrap()).unwrap();
    fs::write(
        &targets[0].1,
        r#"{"deploymentMode":"1p","mcpServers":{"mine":{"command":"my-tool"}}}"#,
    )
    .unwrap();
    fs::create_dir_all(targets[3].1.parent().unwrap()).unwrap();
    fs::write(
        &targets[3].1,
        r#"{"entries":[{"id":"previous","name":"keep"}],"appliedId":"previous"}"#,
    )
    .unwrap();
    let store = ConfigStore::new(dir.path().join("backups"));
    let c = credential();
    let batch = store
        .begin(
            "claude-desktop",
            adapter.changes(&ctx, Mode::Tokenone, Some(&c)).unwrap(),
        )
        .unwrap();
    assert_eq!(
        adapter.current_mode(&ctx, Some(&c.base_url)).unwrap(),
        Some(Mode::Tokenone)
    );
    store.commit(batch).unwrap();
    assert_eq!(
        read_value(&targets[0].1, Format::Json).unwrap()["mcpServers"]["mine"]["command"],
        "my-tool"
    );
    let batch = store
        .begin(
            "claude-desktop",
            adapter.changes(&ctx, Mode::Official, None).unwrap(),
        )
        .unwrap();
    store.commit(batch).unwrap();
    assert_eq!(
        adapter.current_mode(&ctx, None).unwrap(),
        Some(Mode::Official)
    );
    assert!(read_value(&targets[2].1, Format::Json)
        .unwrap()
        .get("inferenceGatewayApiKey")
        .is_none());
    assert_eq!(
        read_value(&targets[3].1, Format::Json).unwrap()["entries"],
        json!([{"id":"previous","name":"keep"}])
    );
    assert!(targets.iter().all(|(id, _)| store.can_restore(id)));
    // Original backup is not replaced by the intermediate subscription mode.
    let changes = store.restore_changes(targets.clone()).unwrap();
    let batch = store.begin("claude-desktop", changes).unwrap();
    store.finish_restore(batch).unwrap();
    assert_eq!(
        read_value(&targets[3].1, Format::Json).unwrap()["appliedId"],
        "previous"
    );
    assert!(targets.iter().all(|(id, _)| !store.can_restore(id)));
}
#[test]
fn desktop_preflight_failure_changes_no_other_file() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = Context::isolated(dir.path().into());
    let registry = Registry::default();
    let adapter = registry.get("claude-desktop").unwrap();
    let targets = adapter.targets(&ctx).unwrap();
    fs::create_dir_all(targets[0].1.parent().unwrap()).unwrap();
    fs::write(&targets[0].1, "{\"keep\":true}").unwrap();
    fs::create_dir_all(targets[2].1.parent().unwrap()).unwrap();
    fs::write(&targets[2].1, "not json").unwrap();
    let store = ConfigStore::new(dir.path().join("backups"));
    assert!(store
        .begin(
            "claude-desktop",
            adapter
                .changes(&ctx, Mode::Tokenone, Some(&credential()))
                .unwrap()
        )
        .is_err());
    assert_eq!(
        fs::read_to_string(&targets[0].1).unwrap(),
        "{\"keep\":true}"
    );
    assert!(!targets[1].1.exists());
}
#[test]
fn interrupted_multi_file_switch_recovers_and_does_not_clobber_later_edits() {
    for conflict in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Context::isolated(dir.path().into());
        let registry = Registry::default();
        let adapter = registry.get("claude-desktop").unwrap();
        let targets = adapter.targets(&ctx).unwrap();
        let backups = dir.path().join("backups");
        let store = ConfigStore::new(backups.clone());
        let batch = store
            .begin(
                "claude-desktop",
                adapter
                    .changes(&ctx, Mode::Tokenone, Some(&credential()))
                    .unwrap(),
            )
            .unwrap();
        // A process exit releases the lock, but leaves the durable rollback intent.
        drop(batch);
        if conflict {
            fs::write(&targets[0].1, r#"{"deploymentMode":"changed-by-user"}"#).unwrap();
        }
        let reopened = ConfigStore::new(backups);
        if conflict {
            assert!(reopened.recover().is_err());
            assert_eq!(
                read_value(&targets[1].1, Format::Json).unwrap()["deploymentMode"],
                "3p"
            );
        } else {
            reopened.recover().unwrap();
            assert_eq!(
                adapter.current_mode(&ctx, None).unwrap(),
                Some(Mode::Official)
            );
            assert!(read_value(&targets[2].1, Format::Json)
                .unwrap()
                .get("inferenceGatewayApiKey")
                .is_none());
        }
    }
}
#[test]
fn failed_validation_reverts_all_files_and_keeps_original_backup() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = Context::isolated(dir.path().into());
    let registry = Registry::default();
    let adapter = registry.get("claude-desktop").unwrap();
    let store = ConfigStore::new(dir.path().join("backups"));
    let batch = store
        .begin(
            "claude-desktop",
            adapter.changes(&ctx, Mode::Official, None).unwrap(),
        )
        .unwrap();
    store.commit(batch).unwrap();
    let batch = store
        .begin(
            "claude-desktop",
            adapter
                .changes(&ctx, Mode::Tokenone, Some(&credential()))
                .unwrap(),
        )
        .unwrap();
    store.revert(batch).unwrap();
    assert_eq!(
        adapter.current_mode(&ctx, None).unwrap(),
        Some(Mode::Official)
    );
    assert!(adapter
        .targets(&ctx)
        .unwrap()
        .iter()
        .all(|(id, _)| store.can_restore(id)));
}
#[test]
fn desktop_mode_requires_both_deployment_files_and_active_own_profile() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = Context::isolated(dir.path().into());
    let registry = Registry::default();
    let adapter = registry.get("claude-desktop").unwrap();
    let store = ConfigStore::new(dir.path().join("backups"));
    let c = credential();
    let batch = store
        .begin(
            "claude-desktop",
            adapter.changes(&ctx, Mode::Tokenone, Some(&c)).unwrap(),
        )
        .unwrap();
    store.commit(batch).unwrap();
    assert_eq!(
        adapter
            .current_mode(&ctx, Some("https://other.example.test"))
            .unwrap(),
        None
    );
    let targets = adapter.targets(&ctx).unwrap();
    fs::write(&targets[3].1, r#"{"appliedId":"someone-else"}"#).unwrap();
    assert_eq!(adapter.current_mode(&ctx, Some(&c.base_url)).unwrap(), None);
}
#[test]
fn mac_detection_uses_bundle_identity_and_executable_not_cli_or_settings() {
    let dir = tempfile::tempdir().unwrap();
    let app = dir.path().join("Renamed.app");
    let contents = app.join("Contents");
    fs::create_dir_all(contents.join("MacOS")).unwrap();
    let write = |identity: &str| {
        let mut d = plist::Dictionary::new();
        d.insert("CFBundleIdentifier".into(), identity.into());
        d.insert("CFBundleExecutable".into(), "app".into());
        d.insert("CFBundleShortVersionString".into(), "1.2.3".into());
        plist::Value::Dictionary(d)
            .to_file_binary(contents.join("Info.plist"))
            .unwrap();
    };
    write("com.openai.codex");
    assert!(mac_installation(CODEX, &app).is_none());
    fs::write(contents.join("MacOS/app"), "fixture").unwrap();
    assert_eq!(
        mac_installation(CODEX, &app).unwrap().version.as_deref(),
        Some("1.2.3")
    );
    write("com.openai.chat");
    assert!(!mac_installation(CODEX, &app).unwrap().gateway_supported);
}
