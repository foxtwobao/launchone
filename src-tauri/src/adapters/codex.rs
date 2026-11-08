use super::*;
use serde_json::json;

pub struct Codex;

#[async_trait::async_trait]
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
    async fn changes(
        &self,
        context: &Context,
        mode: Mode,
        credential: Option<&Credential>,
    ) -> Result<Vec<Change>> {
        let path = self.path(context)?;
        let mut fields = self.fields(mode, credential)?;
        if mode == Mode::Tokenone {
            let config = read_value(&path, Format::Toml)?;
            let preserve_account = has_existing_account(context, &path, &config).await?;
            // This controls the desktop account identity, independently of the
            // explicit provider token used for model requests. Keeping the same
            // account also keeps its project list, SSH projects and onboarding.
            let provider = fields
                .iter_mut()
                .find(|f| f.path == ["model_providers", "tokenone"])
                .and_then(|f| f.value.as_mut())
                .ok_or_else(|| message("无法生成 TokenOne 配置"))?;
            provider["requires_openai_auth"] = json!(preserve_account);
        }
        Ok(vec![Change {
            id: "codex".into(),
            path,
            format: Format::Toml,
            fields,
        }])
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
            // Keep the TokenOne provider registered while official mode is
            // active. Codex sessions remember the provider they were created
            // with and need that route when reopened from desktop history.
            Mode::Official => (Some(json!("openai")), None, None),
        };
        let mut fields = vec![
            Field::new(&["profile"], None),
            Field::new(&["model_provider"], provider),
            Field::new(&["model"], model),
        ];
        if mode == Mode::Tokenone {
            fields.push(Field::new(&["model_providers", "tokenone"], table));
        } else {
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

async fn has_existing_account(
    context: &Context,
    path: &std::path::Path,
    config: &Value,
) -> Result<bool> {
    let directory = path
        .parent()
        .ok_or_else(|| message("无法读取应用配置目录"))?;
    let file = || -> Result<Option<Value>> {
        match std::fs::read(directory.join("auth.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|_| message("现有官方登录文件无法解析，未修改配置")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(message("无法读取现有官方登录状态，未修改配置")),
        }
    };
    match config
        .get("cli_auth_credentials_store")
        .and_then(Value::as_str)
        .unwrap_or("file")
    {
        "file" => Ok(file()?.as_ref().is_some_and(has_openai_account)),
        "ephemeral" => Ok(false),
        "keyring" | "auto" => {
            // Let the installed official engine resolve its native credential
            // backend. Never reimplement or migrate its keychain/secrets format.
            if !context.use_environment {
                return Err(message("隔离配置不能访问系统凭据存储"));
            }
            crate::desktop_app::codex_has_account(&context.home, directory).await
        }
        _ => Err(message("无法识别现有官方登录存储方式，未修改配置")),
    }
}

fn has_openai_account(auth: &Value) -> bool {
    let present = |value: Option<&Value>| {
        value
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty())
    };
    let chatgpt = auth.get("tokens").is_some_and(|tokens| {
        present(tokens.get("access_token")) && present(tokens.get("id_token"))
    });
    match auth.get("auth_mode").and_then(Value::as_str) {
        Some("apikey") => present(auth.get("OPENAI_API_KEY")),
        Some("chatgpt" | "chatgptAuthTokens") => chatgpt,
        None if present(auth.get("OPENAI_API_KEY")) => true,
        None => chatgpt,
        _ => false,
    }
}
