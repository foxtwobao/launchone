use super::*;
use serde_json::json;

pub struct Codex;

impl AppAdapter for Codex {
    fn manifest(&self) -> AppManifest {
        AppManifest {
            id: "codex",
            name: "ChatGPT / Codex",
            description: "OpenAI 桌面版 · TokenOne 用于 Codex 任务",
            command: "codex",
            icon: "codex",
            supports_official: true,
            platforms: &["macos", "windows"],
        }
    }
    fn desktop(&self) -> Option<crate::desktop_app::DesktopSpec> {
        Some(crate::desktop_app::CODEX)
    }
    fn path(&self, context: &Context) -> Result<PathBuf> {
        Ok(context
            .config_directory("CODEX_HOME", ".codex")?
            .join("config.toml"))
    }
    fn format(&self) -> Format {
        Format::Toml
    }
    fn fields(&self, mode: Mode, value: Option<&Credential>) -> Result<Vec<Field>> {
        let (provider, model, table) = match mode {
            Mode::Tokenone => {
                let c = credential(value)?;
                (
                    Some(json!("tokenone")),
                    Some(json!(c.model)),
                    Some(json!({
                        "name": "TokenOne", "base_url": c.base_url, "wire_api": "responses",
                        "experimental_bearer_token": c.key, "requires_openai_auth": false
                    })),
                )
            }
            Mode::Official => (Some(json!("openai")), None, None),
        };
        let mut fields = vec![
            Field::new(&["profile"], None),
            Field::new(&["model_provider"], provider),
            Field::new(&["model"], model),
            Field::new(&["model_providers", "tokenone"], table),
        ];
        if mode == Mode::Official {
            fields.push(Field::new(&["model_providers", "openai"], None));
        }
        Ok(fields)
    }
    fn mode(&self, value: &Value, endpoint: Option<&str>) -> Option<Mode> {
        if get(value, &["profile"]).is_some() {
            return None;
        }
        match get(value, &["model_provider"]).and_then(Value::as_str) {
            Some("tokenone")
                if same_endpoint(
                    get(value, &["model_providers", "tokenone", "base_url"]),
                    endpoint,
                ) =>
            {
                Some(Mode::Tokenone)
            }
            None | Some("openai") if get(value, &["model_providers", "openai"]).is_none() => {
                Some(Mode::Official)
            }
            _ => None,
        }
    }
    fn install_script(&self, platform: &str) -> Result<&'static str> {
        match platform {
            "macos" => Ok("curl -fsSL https://chatgpt.com/codex/install.sh -o \"$1/codex-install.sh\" && /bin/sh \"$1/codex-install.sh\""),
            "windows" => Ok("$ErrorActionPreference='Stop'; irm https://chatgpt.com/codex/install.ps1 | iex"),
            _ => Err(message("该平台暂不支持自动安装")),
        }
    }
    fn official_arguments(&self) -> &[&str] {
        &["login"]
    }
    fn official_status_arguments(&self) -> &[&str] {
        &["login", "status"]
    }
    fn official_session(&self, stdout: &str, stderr: &str) -> bool {
        stdout
            .lines()
            .chain(stderr.lines())
            .any(|line| line.trim() == "Logged in using ChatGPT")
    }
    fn verify_request(&self, c: &Credential) -> (String, Value) {
        (
            format!("{}/responses", c.base_url),
            json!({ "model": c.model, "input": "Reply OK.", "max_output_tokens": 16, "store": false }),
        )
    }
    fn request_headers(&self, key: &str) -> Vec<(&'static str, String)> {
        vec![("authorization", format!("Bearer {key}"))]
    }
    fn valid_response(&self, value: &Value) -> bool {
        value.get("output").is_some_and(Value::is_array)
            && value.get("status").and_then(Value::as_str) == Some("completed")
            && value.get("error").is_none_or(Value::is_null)
    }
    fn check_conflicts(&self, _: Mode) -> Result<()> {
        reject_environment(&["OPENAI_API_KEY", "OPENAI_BASE_URL", "CODEX_API_KEY"])
    }
}
