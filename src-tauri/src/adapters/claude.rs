use super::*;
use serde_json::json;

pub struct Claude;

#[async_trait::async_trait]
impl AppAdapter for Claude {
    fn manifest(&self) -> AppManifest {
        AppManifest {
            id: "claude-code",
            name: "Claude Code",
            description: "Anthropic 的编程助手",
            command: "claude",
            icon: "claude",
            supports_official: true,
            platforms: &["macos", "windows"],
        }
    }
    fn path(&self, context: &Context) -> Result<PathBuf> {
        Ok(context
            .config_directory("CLAUDE_CONFIG_DIR", ".claude")?
            .join("settings.json"))
    }
    fn format(&self) -> Format {
        Format::Json
    }
    fn fields(&self, mode: Mode, value: Option<&Credential>) -> Result<Vec<Field>> {
        let c = match mode {
            Mode::Tokenone => Some(credential(value)?),
            Mode::Official => None,
        };
        let mut fields = vec![
            Field::new(&["env", "ANTHROPIC_BASE_URL"], c.map(|c| json!(c.base_url))),
            Field::new(&["env", "ANTHROPIC_AUTH_TOKEN"], c.map(|c| json!(c.key))),
            Field::new(&["env", "ANTHROPIC_API_KEY"], None),
            Field::new(&["apiKeyHelper"], None),
            Field::new(&["model"], c.map(|c| json!(c.model))),
        ];
        for key in [
            "ANTHROPIC_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "CLAUDE_CODE_SUBAGENT_MODEL",
        ] {
            fields.push(Field::new(&["env", key], c.map(|c| json!(c.model))));
        }
        for key in [
            "CLAUDE_CODE_USE_BEDROCK",
            "CLAUDE_CODE_USE_VERTEX",
            "CLAUDE_CODE_USE_FOUNDRY",
        ] {
            fields.push(Field::new(&["env", key], None));
        }
        Ok(fields)
    }
    fn mode(&self, value: &Value, endpoint: Option<&str>) -> Option<Mode> {
        if [
            "CLAUDE_CODE_USE_BEDROCK",
            "CLAUDE_CODE_USE_VERTEX",
            "CLAUDE_CODE_USE_FOUNDRY",
        ]
        .iter()
        .any(|key| {
            get(value, &["env", key])
                .is_some_and(|v| v.as_str() == Some("1") || v.as_bool() == Some(true))
        }) {
            return None;
        }
        if same_endpoint(get(value, &["env", "ANTHROPIC_BASE_URL"]), endpoint) {
            Some(Mode::Tokenone)
        } else if get(value, &["env", "ANTHROPIC_BASE_URL"]).is_none()
            && get(value, &["env", "ANTHROPIC_API_KEY"]).is_none()
            && get(value, &["env", "ANTHROPIC_AUTH_TOKEN"]).is_none()
            && value.get("apiKeyHelper").is_none()
        {
            Some(Mode::Official)
        } else {
            None
        }
    }
    fn install_script(&self, platform: &str) -> Result<&'static str> {
        match platform {
            "macos" => Ok("curl -fsSL https://claude.ai/install.sh -o \"$1/claude-install.sh\" && /bin/bash \"$1/claude-install.sh\""),
            "windows" => Ok("$ErrorActionPreference='Stop'; irm https://claude.ai/install.ps1 | iex"),
            _ => Err(message("该平台暂不支持自动安装")),
        }
    }
    fn official_arguments(&self) -> &[&str] {
        &["auth", "login"]
    }
    fn official_status_arguments(&self) -> &[&str] {
        &["auth", "status", "--json"]
    }
    fn official_session(&self, stdout: &str, _: &str) -> bool {
        serde_json::from_str::<Value>(stdout)
            .is_ok_and(|v| v.get("loggedIn").and_then(Value::as_bool) == Some(true))
    }
    fn verify_request(&self, c: &Credential) -> (String, Value) {
        (
            format!("{}/v1/messages", c.base_url),
            json!({ "model": c.model, "max_tokens": 16, "messages": [{ "role": "user", "content": "Reply OK." }] }),
        )
    }
    fn request_headers(&self, key: &str) -> Vec<(&'static str, String)> {
        vec![
            ("authorization", format!("Bearer {key}")),
            ("anthropic-version", "2023-06-01".into()),
        ]
    }
    fn valid_response(&self, value: &Value) -> bool {
        value.get("type").and_then(Value::as_str) == Some("message")
            && value.get("content").is_some_and(Value::is_array)
    }
    fn check_conflicts(&self, _: Mode) -> Result<()> {
        reject_environment(&[
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
            "CLAUDE_CODE_USE_BEDROCK",
            "CLAUDE_CODE_USE_VERTEX",
            "CLAUDE_CODE_USE_FOUNDRY",
        ])
    }
}
