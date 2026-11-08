use launchone_lib::adapters::{Context, Registry};
use launchone_lib::config::{read_value, ConfigStore};
use launchone_lib::models::{Credential, Mode};
use serde_json::{json, Value};
use std::fs;

fn credential() -> Credential {
    Credential {
        key: "test-secret-do-not-log".into(),
        base_url: "https://gateway.example.test/v1".into(),
        model: "test-model".into(),
    }
}

#[test]
fn codex_preserves_comments_mcp_and_later_unrelated_edits_on_restore() {
    let directory = tempfile::tempdir().unwrap();
    let context = Context::isolated(directory.path().into());
    let registry = Registry::default();
    let adapter = registry.get("codex").unwrap();
    let path = adapter.path(&context).unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        "# keep this comment\nmodel = 'original'\n[mcp_servers.my_tool]\ncommand = 'node'\n",
    )
    .unwrap();
    let store = ConfigStore::new(directory.path().join("backups"));
    store
        .apply(
            "codex",
            &path,
            adapter.format(),
            adapter.fields(Mode::Tokenone, Some(&credential())).unwrap(),
        )
        .unwrap();
    let source = fs::read_to_string(&path).unwrap();
    assert!(source.contains("# keep this comment"));
    assert!(source.contains("command = 'node'"));
    fs::write(&path, format!("{source}\n[extra]\ncustom = true\n")).unwrap();
    store.restore("codex", &path).unwrap();
    let value = read_value(&path, adapter.format()).unwrap();
    assert_eq!(value["model"], "original");
    assert_eq!(value["extra"]["custom"], true);
    assert!(value["model_providers"].get("tokenone").is_none());
    assert!(!store.can_restore("codex"));
}

#[test]
fn claude_preserves_jsonc_hooks_and_restores_existing_auth_fields() {
    let directory = tempfile::tempdir().unwrap();
    let context = Context::isolated(directory.path().into());
    let registry = Registry::default();
    let adapter = registry.get("claude-code").unwrap();
    let path = adapter.path(&context).unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original = "{\n // user's hook\n \"hooks\": {\"custom\": true},\n \"env\": {\"KEEP\": \"yes\", \"ANTHROPIC_API_KEY\": \"original-key\"}\n}\n";
    fs::write(&path, original).unwrap();
    let store = ConfigStore::new(directory.path().join("backups"));
    store
        .apply(
            "claude-code",
            &path,
            adapter.format(),
            adapter.fields(Mode::Tokenone, Some(&credential())).unwrap(),
        )
        .unwrap();
    let source = fs::read_to_string(&path).unwrap();
    assert!(source.contains("// user's hook"));
    let value = read_value(&path, adapter.format()).unwrap();
    assert_eq!(value["env"]["KEEP"], "yes");
    assert_eq!(value["hooks"]["custom"], true);
    assert!(value["env"].get("ANTHROPIC_API_KEY").is_none());
    store.restore("claude-code", &path).unwrap();
    let restored: Value = json5::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let original: Value = json5::from_str(original).unwrap();
    assert_eq!(restored, original);
}

#[test]
fn mode_switch_preserves_edits_and_explicit_restore_replaces_managed_fields() {
    let directory = tempfile::tempdir().unwrap();
    let context = Context::isolated(directory.path().into());
    let registry = Registry::default();
    let adapter = registry.get("codex").unwrap();
    let path = adapter.path(&context).unwrap();
    let store = ConfigStore::new(directory.path().join("backups"));
    store
        .apply(
            "codex",
            &path,
            adapter.format(),
            adapter.fields(Mode::Tokenone, Some(&credential())).unwrap(),
        )
        .unwrap();
    let source = fs::read_to_string(&path)
        .unwrap()
        .replace("test-model", "user-changed-model");
    fs::write(&path, &source).unwrap();
    assert!(store
        .apply(
            "codex",
            &path,
            adapter.format(),
            adapter.fields(Mode::Official, None).unwrap()
        )
        .is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
    assert!(store.can_restore("codex"));
    store.restore("codex", &path).unwrap();
    let restored = read_value(&path, adapter.format()).unwrap();
    assert!(restored.get("model").is_none());
    assert!(restored.get("model_provider").is_none());
    assert!(restored["model_providers"].get("tokenone").is_none());
    assert!(!store.can_restore("codex"));
}

#[test]
fn failed_verification_rollback_recovers_before_this_operation() {
    let directory = tempfile::tempdir().unwrap();
    let context = Context::isolated(directory.path().into());
    let registry = Registry::default();
    let adapter = registry.get("codex").unwrap();
    let path = adapter.path(&context).unwrap();
    let store = ConfigStore::new(directory.path().join("backups"));
    let first = credential();
    store
        .apply(
            "codex",
            &path,
            adapter.format(),
            adapter.fields(Mode::Tokenone, Some(&first)).unwrap(),
        )
        .unwrap();
    let mut next = credential();
    next.key = "rotated-key".into();
    let receipt = store
        .apply(
            "codex",
            &path,
            adapter.format(),
            adapter.fields(Mode::Tokenone, Some(&next)).unwrap(),
        )
        .unwrap();
    store
        .rollback("codex", &path, adapter.format(), receipt)
        .unwrap();
    let value = read_value(&path, adapter.format()).unwrap();
    assert_eq!(
        value["model_providers"]["tokenone"]["experimental_bearer_token"],
        first.key
    );
    assert!(store.can_restore("codex"));
}

#[test]
fn official_switch_keeps_original_backup_and_never_changes_auth_json() {
    let directory = tempfile::tempdir().unwrap();
    let context = Context::isolated(directory.path().into());
    let registry = Registry::default();
    let adapter = registry.get("codex").unwrap();
    let path = adapter.path(&context).unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "model = 'original'\n").unwrap();
    let auth = path.parent().unwrap().join("auth.json");
    fs::write(&auth, "official-auth-untouched").unwrap();
    let store = ConfigStore::new(directory.path().join("backups"));
    for mode in [Mode::Tokenone, Mode::Official] {
        store
            .apply(
                "codex",
                &path,
                adapter.format(),
                adapter.fields(mode, Some(&credential())).unwrap(),
            )
            .unwrap();
    }
    let value = read_value(&path, adapter.format()).unwrap();
    assert_eq!(value["model_provider"], "openai");
    assert!(value.get("model").is_none());
    assert_eq!(
        value["model_providers"]["tokenone"]["experimental_bearer_token"],
        credential().key
    );
    assert_eq!(fs::read_to_string(auth).unwrap(), "official-auth-untouched");
    store.restore("codex", &path).unwrap();
    assert_eq!(
        read_value(&path, adapter.format()).unwrap()["model"],
        "original"
    );
}

#[test]
fn only_registered_apps_are_supported_and_backups_are_isolated() {
    let registry = Registry::default();
    assert!(registry.get("../third-party").is_err());
    assert!(registry.get("arbitrary-provider").is_err());
    let directory = tempfile::tempdir().unwrap();
    let context = Context::isolated(directory.path().into());
    let store = ConfigStore::new(directory.path().join("backups"));
    for adapter in registry.all() {
        store
            .apply(
                adapter.manifest().id,
                &adapter.path(&context).unwrap(),
                adapter.format(),
                adapter.fields(Mode::Tokenone, Some(&credential())).unwrap(),
            )
            .unwrap();
    }
    let codex = registry.get("codex").unwrap();
    store
        .restore("codex", &codex.path(&context).unwrap())
        .unwrap();
    assert!(!store.can_restore("codex"));
    assert!(store.can_restore("claude-code"));
}

#[test]
fn malformed_config_is_never_replaced() {
    let directory = tempfile::tempdir().unwrap();
    let context = Context::isolated(directory.path().into());
    let registry = Registry::default();
    let adapter = registry.get("codex").unwrap();
    let path = adapter.path(&context).unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "broken = [").unwrap();
    let store = ConfigStore::new(directory.path().join("backups"));
    assert!(store
        .apply(
            "codex",
            &path,
            adapter.format(),
            adapter.fields(Mode::Tokenone, Some(&credential())).unwrap()
        )
        .is_err());
    assert_eq!(fs::read_to_string(path).unwrap(), "broken = [");
    assert!(!store.can_restore("codex"));
}

#[test]
fn mode_detection_does_not_mark_unknown_gateway_as_tokenone() {
    let registry = Registry::default();
    let adapter = registry.get("codex").unwrap();
    assert_eq!(adapter.mode(&json!({"model_provider":"tokenone","model_providers":{"tokenone":{"base_url":"https://other.test"}}}), Some("https://tokenone.test")), None);
}

#[test]
fn tool_routing_overrides_are_removed_and_restored() {
    let directory = tempfile::tempdir().unwrap();
    let context = Context::isolated(directory.path().into());
    let registry = Registry::default();
    let store = ConfigStore::new(directory.path().join("backups"));
    for (id, original, expected) in [
        (
            "codex",
            "profile = 'custom'\n[profiles.custom]\nmodel_provider = 'another'\n",
            json!({"profile":"custom"}),
        ),
        (
            "claude-code",
            r#"{"env":{"CLAUDE_CODE_USE_BEDROCK":"1"}}"#,
            json!({"env":{"CLAUDE_CODE_USE_BEDROCK":"1"}}),
        ),
    ] {
        let adapter = registry.get(id).unwrap();
        let path = adapter.path(&context).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, original).unwrap();
        store
            .apply(
                id,
                &path,
                adapter.format(),
                adapter.fields(Mode::Tokenone, Some(&credential())).unwrap(),
            )
            .unwrap();
        let value = read_value(&path, adapter.format()).unwrap();
        assert!(value.get("profile").is_none());
        assert!(value
            .get("env")
            .and_then(|v| v.get("CLAUDE_CODE_USE_BEDROCK"))
            .is_none());
        store.restore(id, &path).unwrap();
        let restored = read_value(&path, adapter.format()).unwrap();
        for (key, value) in expected.as_object().unwrap() {
            assert_eq!(&restored[key], value);
        }
    }
}

#[test]
fn official_codex_removes_custom_openai_endpoint_and_restore_keeps_it() {
    let directory = tempfile::tempdir().unwrap();
    let context = Context::isolated(directory.path().into());
    let registry = Registry::default();
    let adapter = registry.get("codex").unwrap();
    let path = adapter.path(&context).unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "model_provider = 'openai'\n[model_providers.openai]\nbase_url = 'https://custom.example.test'\n").unwrap();
    assert_eq!(
        adapter.mode(&read_value(&path, adapter.format()).unwrap(), None),
        None
    );
    let store = ConfigStore::new(directory.path().join("backups"));
    store
        .apply(
            "codex",
            &path,
            adapter.format(),
            adapter.fields(Mode::Official, None).unwrap(),
        )
        .unwrap();
    assert_eq!(
        adapter.mode(&read_value(&path, adapter.format()).unwrap(), None),
        Some(Mode::Official)
    );
    store.restore("codex", &path).unwrap();
    assert_eq!(
        read_value(&path, adapter.format()).unwrap()["model_providers"]["openai"]["base_url"],
        "https://custom.example.test"
    );
}

#[test]
fn successful_api_key_status_is_not_a_chatgpt_login() {
    let registry = Registry::default();
    let codex = registry.get("codex").unwrap();
    assert!(!codex.official_session("", "API key configured - redacted"));
    assert!(codex.official_session("", "Logged in using ChatGPT\n"));
    let claude = registry.get("claude-code").unwrap();
    assert!(!claude.official_session(r#"{"loggedIn":false}"#, ""));
    assert!(claude.official_session(r#"{"loggedIn":true}"#, ""));
}

#[test]
fn switching_modes_keeps_conflict_checks_for_previously_owned_fields() {
    let directory = tempfile::tempdir().unwrap();
    let context = Context::isolated(directory.path().into());
    let registry = Registry::default();
    let adapter = registry.get("codex").unwrap();
    let path = adapter.path(&context).unwrap();
    let store = ConfigStore::new(directory.path().join("backups"));
    for mode in [Mode::Official, Mode::Tokenone] {
        store
            .apply(
                "codex",
                &path,
                adapter.format(),
                adapter.fields(mode, Some(&credential())).unwrap(),
            )
            .unwrap();
    }
    let modified = format!(
        "{}\n[model_providers.openai]\nbase_url = 'https://later-edit.example.test'\n",
        fs::read_to_string(&path).unwrap()
    );
    fs::write(&path, &modified).unwrap();
    assert!(store
        .apply(
            "codex",
            &path,
            adapter.format(),
            adapter.fields(Mode::Official, None).unwrap()
        )
        .is_err());
    assert_eq!(fs::read_to_string(path).unwrap(), modified);
}

#[test]
fn interrupted_config_write_can_retry_or_restore_previous_fields() {
    for retry in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let context = Context::isolated(directory.path().into());
        let registry = Registry::default();
        let adapter = registry.get("codex").unwrap();
        let path = adapter.path(&context).unwrap();
        let backups = directory.path().join("backups");
        let store = ConfigStore::new(backups.clone());
        let fields = adapter.fields(Mode::Tokenone, Some(&credential())).unwrap();
        store
            .apply("codex", &path, adapter.format(), fields)
            .unwrap();
        let previous_source = fs::read_to_string(&path).unwrap();
        let previous_journal: Value =
            serde_json::from_slice(&fs::read(backups.join("codex.json")).unwrap()).unwrap();
        let mut next = credential();
        next.key = "new-key".into();
        let fields = adapter.fields(Mode::Tokenone, Some(&next)).unwrap();
        store
            .apply("codex", &path, adapter.format(), fields.clone())
            .unwrap();
        let mut pending: Value =
            serde_json::from_slice(&fs::read(backups.join("codex.json")).unwrap()).unwrap();
        pending["pending_before"] = previous_journal["after"].clone();
        fs::write(
            backups.join("codex.json"),
            serde_json::to_vec(&pending).unwrap(),
        )
        .unwrap();
        fs::write(&path, previous_source).unwrap();
        if retry {
            store
                .apply("codex", &path, adapter.format(), fields)
                .unwrap();
        }
        store.restore("codex", &path).unwrap();
        let restored = read_value(&path, adapter.format()).unwrap();
        assert!(restored.get("model").is_none());
        assert!(!store.can_restore("codex"));
    }
}

#[test]
fn isolated_context_never_resolves_paths_outside_test_directory() {
    let directory = tempfile::tempdir().unwrap();
    let context = Context::isolated(directory.path().into());
    let registry = Registry::default();
    assert_eq!(
        registry.get("codex").unwrap().path(&context).unwrap(),
        directory.path().join(".codex/config.toml")
    );
    assert_eq!(
        registry.get("claude-code").unwrap().path(&context).unwrap(),
        directory.path().join(".claude/settings.json")
    );
}

#[cfg(unix)]
#[test]
fn configs_and_secret_backups_are_private_to_current_user() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let context = Context::isolated(directory.path().into());
    let registry = Registry::default();
    let adapter = registry.get("codex").unwrap();
    let path = adapter.path(&context).unwrap();
    let backup = directory.path().join("backups");
    let store = ConfigStore::new(backup.clone());
    store
        .apply(
            "codex",
            &path,
            adapter.format(),
            adapter.fields(Mode::Tokenone, Some(&credential())).unwrap(),
        )
        .unwrap();
    for file in [path, backup.join("codex.json")] {
        assert_eq!(
            fs::metadata(file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
