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

#[tokio::test]
async fn desktop_switch_subscription_and_restore_are_distinct_and_preserve_other_profiles() {
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
            adapter
                .changes(&ctx, Mode::Tokenone, Some(&c))
                .await
                .unwrap(),
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
            adapter.changes(&ctx, Mode::Official, None).await.unwrap(),
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
    let batch = store.begin_restore("claude-desktop", changes).unwrap();
    store.finish_restore(batch).unwrap();
    assert_eq!(
        read_value(&targets[3].1, Format::Json).unwrap()["appliedId"],
        "previous"
    );
    assert!(targets.iter().all(|(id, _)| !store.can_restore(id)));
}
#[tokio::test]
async fn desktop_preflight_failure_changes_no_other_file() {
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
                .await
                .unwrap()
        )
        .is_err());
    assert_eq!(
        fs::read_to_string(&targets[0].1).unwrap(),
        "{\"keep\":true}"
    );
    assert!(!targets[1].1.exists());
}

#[tokio::test]
async fn explicit_restore_overwrites_changed_routing_and_preserves_projects() {
    let temp = tempfile::tempdir().unwrap();
    let context = Context::isolated(temp.path().into());
    let registry = Registry::default();
    let adapter = registry.get("codex").unwrap();
    let path = adapter.path(&context).unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        "model = 'original'\n[projects.local]\ntrust_level = 'trusted'\n",
    )
    .unwrap();
    let store = ConfigStore::new(temp.path().join("backups"));
    let batch = store
        .begin(
            "codex",
            adapter
                .changes(&context, Mode::Tokenone, Some(&credential()))
                .await
                .unwrap(),
        )
        .unwrap();
    store.commit(batch).unwrap();
    let edited = fs::read_to_string(&path)
        .unwrap()
        .replace("claude-sonnet-4-6", "changed-in-client")
        .replace("fixture-personal-key", "changed-key");
    let edited =
        format!("{edited}\n[desktop.ssh_connections.fixture]\nhostname = 'fixture.example.test'\n");
    fs::write(&path, &edited).unwrap();
    let batch = store
        .begin_restore(
            "codex",
            store
                .restore_changes(adapter.targets(&context).unwrap())
                .unwrap(),
        )
        .unwrap();
    store.finish_restore(batch).unwrap();
    let restored = read_value(&path, adapter.format()).unwrap();
    assert_eq!(restored["model"], "original");
    assert!(restored.get("model_provider").is_none());
    assert!(restored["model_providers"].get("tokenone").is_none());
    assert_eq!(restored["projects"]["local"]["trust_level"], "trusted");
    assert_eq!(
        restored["desktop"]["ssh_connections"]["fixture"]["hostname"],
        "fixture.example.test"
    );
    assert!(!store.can_restore("codex"));
}

#[tokio::test]
async fn explicit_multi_file_restore_can_recover_interruption_and_retry() {
    let temp = tempfile::tempdir().unwrap();
    let context = Context::isolated(temp.path().into());
    let registry = Registry::default();
    let adapter = registry.get("claude-desktop").unwrap();
    let targets = adapter.targets(&context).unwrap();
    fs::create_dir_all(targets[0].1.parent().unwrap()).unwrap();
    let original = r#"{"deploymentMode":"1p","mcpServers":{"mine":{"command":"fixture"}}}"#;
    fs::write(&targets[0].1, original).unwrap();
    let store = ConfigStore::new(temp.path().join("backups"));
    let batch = store
        .begin(
            "claude-desktop",
            adapter
                .changes(&context, Mode::Tokenone, Some(&credential()))
                .await
                .unwrap(),
        )
        .unwrap();
    store.commit(batch).unwrap();
    let edited = r#"{"deploymentMode":"changed-by-client","mcpServers":{"mine":{"command":"fixture"}},"laterSetting":true}"#;
    fs::write(&targets[0].1, edited).unwrap();
    let batch = store
        .begin_restore(
            "claude-desktop",
            store.restore_changes(targets.clone()).unwrap(),
        )
        .unwrap();
    drop(batch); // Interrupted before the operation was committed.
    store.recover().unwrap();
    assert_eq!(
        read_value(&targets[0].1, Format::Json).unwrap()["deploymentMode"],
        "changed-by-client"
    );
    assert!(targets.iter().all(|(id, _)| store.can_restore(id)));
    let batch = store
        .begin_restore(
            "claude-desktop",
            store.restore_changes(targets.clone()).unwrap(),
        )
        .unwrap();
    store.finish_restore(batch).unwrap();
    let restored = read_value(&targets[0].1, Format::Json).unwrap();
    assert_eq!(restored["deploymentMode"], "1p");
    assert_eq!(restored["laterSetting"], true);
    assert_eq!(restored["mcpServers"]["mine"]["command"], "fixture");
    assert!(targets.iter().all(|(id, _)| !store.can_restore(id)));
}
#[tokio::test]
async fn interrupted_multi_file_switch_recovers_and_does_not_clobber_later_edits() {
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
                    .await
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
#[tokio::test]
async fn failed_validation_reverts_all_files_and_keeps_original_backup() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = Context::isolated(dir.path().into());
    let registry = Registry::default();
    let adapter = registry.get("claude-desktop").unwrap();
    let store = ConfigStore::new(dir.path().join("backups"));
    let batch = store
        .begin(
            "claude-desktop",
            adapter.changes(&ctx, Mode::Official, None).await.unwrap(),
        )
        .unwrap();
    store.commit(batch).unwrap();
    let batch = store
        .begin(
            "claude-desktop",
            adapter
                .changes(&ctx, Mode::Tokenone, Some(&credential()))
                .await
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
#[tokio::test]
async fn desktop_mode_requires_both_deployment_files_and_active_own_profile() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = Context::isolated(dir.path().into());
    let registry = Registry::default();
    let adapter = registry.get("claude-desktop").unwrap();
    let store = ConfigStore::new(dir.path().join("backups"));
    let c = credential();
    let batch = store
        .begin(
            "claude-desktop",
            adapter
                .changes(&ctx, Mode::Tokenone, Some(&c))
                .await
                .unwrap(),
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

#[tokio::test]
async fn codex_switch_keeps_account_local_and_ssh_projects_and_completed_onboarding() {
    let temp = tempfile::tempdir().unwrap();
    let context = Context::isolated(temp.path().into());
    let registry = Registry::default();
    let adapter = registry.get("codex").unwrap();
    let path = adapter.path(&context).unwrap();
    let root = path.parent().unwrap();
    fs::create_dir_all(root).unwrap();
    let original = "model = 'original'\n[projects.\"/Users/fixture/local-project\"]\ntrust_level = 'trusted'\n[projects.\"/srv/ssh-project\"]\ntrust_level = 'trusted'\n[desktop.ssh_connections.fixture]\nhostname = 'fixture.example.test'\nuser = 'developer'\n";
    fs::write(&path, original).unwrap();
    let auth = json!({"auth_mode":"chatgpt", "tokens":{
        "id_token":"fixture-id-token", "access_token":"fixture-official-access",
        "refresh_token":"fixture-refresh", "account_id":"fixture-account"
    }, "last_refresh":"2026-11-07T13:00:00Z"});
    let state = json!({
        "electron-local-projects": {
            "local": {"rootPaths":["/Users/fixture/local-project"]},
            "remote": {"rootPaths":["/srv/ssh-project"],"hostId":"ssh-fixture"}
        },
        "electron-persisted-atom-state": {"fixture-completed-onboarding":true},
        "electron-account-renderer-state-v1": {"owner":"fixture-account-hash","inactive":{}}
    });
    let untouched = [
        (
            root.join("auth.json"),
            serde_json::to_vec_pretty(&auth).unwrap(),
        ),
        (
            root.join(".codex-global-state.json"),
            serde_json::to_vec_pretty(&state).unwrap(),
        ),
        (
            root.join("state_5.sqlite"),
            b"fixture-user-database".to_vec(),
        ),
        (
            temp.path().join(".ssh/config"),
            b"Host fixture\n  HostName fixture.example.test\n".to_vec(),
        ),
    ];
    for (file, bytes) in &untouched {
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, bytes).unwrap();
    }
    let store = ConfigStore::new(temp.path().join("backups"));
    let mut c = credential();
    c.model = "fixture-model".into();
    for mode in [Mode::Tokenone, Mode::Official, Mode::Tokenone] {
        let batch = store
            .begin(
                "codex",
                adapter.changes(&context, mode, Some(&c)).await.unwrap(),
            )
            .unwrap();
        store.commit(batch).unwrap();
        let config = read_value(&path, adapter.format()).unwrap();
        if mode == Mode::Tokenone {
            assert_eq!(
                config["model_providers"]["tokenone"]["requires_openai_auth"],
                true
            );
            assert_eq!(
                config["model_providers"]["tokenone"]["experimental_bearer_token"],
                c.key
            );
            assert_eq!(
                config["model_providers"]["tokenone"]["base_url"],
                c.base_url
            );
        }
        assert_eq!(
            config["projects"]["/srv/ssh-project"]["trust_level"],
            "trusted"
        );
        assert_eq!(
            config["desktop"]["ssh_connections"]["fixture"]["hostname"],
            "fixture.example.test"
        );
        for (file, bytes) in &untouched {
            assert_eq!(fs::read(file).unwrap(), *bytes);
        }
    }
    let batch = store
        .begin_restore(
            "codex",
            store
                .restore_changes(adapter.targets(&context).unwrap())
                .unwrap(),
        )
        .unwrap();
    store.finish_restore(batch).unwrap();
    assert_eq!(
        read_value(&path, adapter.format()).unwrap()["model"],
        "original"
    );
    for (file, bytes) in &untouched {
        assert_eq!(fs::read(file).unwrap(), *bytes);
    }
}

#[tokio::test]
async fn codex_does_not_require_official_login_for_a_new_tokenone_user() {
    for auth in [
        None,
        Some(json!({"last_refresh":"metadata-only"})),
        Some(json!({"auth_mode":"chatgpt", "tokens":{"access_token":"incomplete-fixture"}})),
        Some(json!({"auth_mode":"bedrockApiKey", "OPENAI_API_KEY":"stale-fixture-key"})),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let context = Context::isolated(temp.path().into());
        let registry = Registry::default();
        let adapter = registry.get("codex").unwrap();
        let path = adapter.path(&context).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        if let Some(auth) = auth {
            fs::write(path.parent().unwrap().join("auth.json"), auth.to_string()).unwrap();
        }
        let store = ConfigStore::new(temp.path().join("backups"));
        let batch = store
            .begin(
                "codex",
                adapter
                    .changes(&context, Mode::Tokenone, Some(&credential()))
                    .await
                    .unwrap(),
            )
            .unwrap();
        store.commit(batch).unwrap();
        assert_eq!(
            read_value(&path, adapter.format()).unwrap()["model_providers"]["tokenone"]
                ["requires_openai_auth"],
            false
        );
    }
}

#[tokio::test]
async fn malformed_official_auth_stops_switch_without_touching_any_file() {
    let temp = tempfile::tempdir().unwrap();
    let context = Context::isolated(temp.path().into());
    let registry = Registry::default();
    let adapter = registry.get("codex").unwrap();
    let path = adapter.path(&context).unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "model = 'original'\n").unwrap();
    fs::write(
        path.parent().unwrap().join("auth.json"),
        "invalid-auth-json",
    )
    .unwrap();
    assert!(adapter
        .changes(&context, Mode::Tokenone, Some(&credential()))
        .await
        .is_err());
    assert_eq!(fs::read_to_string(path).unwrap(), "model = 'original'\n");
}
