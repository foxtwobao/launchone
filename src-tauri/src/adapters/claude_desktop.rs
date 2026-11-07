use super::*;
use serde_json::json;

pub struct ClaudeDesktop;
// LaunchOne's own stable profile; never replace another tool's library entry.
const PROFILE: &str = "ab84c181-85d0-4ad1-8b73-6be4382cec75";
const PROFILE_FIELDS: &[&str] = &[
    "inferenceProvider",
    "inferenceGatewayBaseUrl",
    "inferenceGatewayApiKey",
    "inferenceGatewayAuthScheme",
    "inferenceModels",
    "disableDeploymentModeChooser",
];

fn paths(context: &Context) -> Vec<(String, PathBuf)> {
    let root = if cfg!(windows) {
        context.home.join("AppData/Local")
    } else {
        context.home.join("Library/Application Support")
    };
    let library = root.join("Claude-3p/configLibrary");
    vec![
        (
            "claude-desktop-normal".into(),
            root.join("Claude/claude_desktop_config.json"),
        ),
        (
            "claude-desktop-thirdparty".into(),
            root.join("Claude-3p/claude_desktop_config.json"),
        ),
        (
            "claude-desktop-profile".into(),
            library.join(format!("{PROFILE}.json")),
        ),
        ("claude-desktop-library".into(), library.join("_meta.json")),
    ]
}
#[async_trait::async_trait]
impl AppAdapter for ClaudeDesktop {
    fn manifest(&self) -> AppManifest {
        AppManifest {
            id: "claude-desktop",
            name: "Claude",
            description: "Anthropic 桌面版",
            command: "",
            icon: "claude",
            supports_official: true,
            platforms: &["macos", "windows"],
        }
    }
    fn desktop(&self) -> Option<DesktopSpec> {
        Some(crate::desktop_app::CLAUDE)
    }
    fn path(&self, context: &Context) -> Result<PathBuf> {
        Ok(paths(context)[2].1.clone())
    }
    fn format(&self) -> Format {
        Format::Json
    }
    fn targets(&self, context: &Context) -> Result<Vec<(String, PathBuf)>> {
        Ok(paths(context))
    }
    fn fields(&self, mode: Mode, c: Option<&Credential>) -> Result<Vec<Field>> {
        if mode == Mode::Official {
            return Ok(PROFILE_FIELDS
                .iter()
                .map(|k| Field::new(&[k], None))
                .collect());
        }
        let c = credential(c)?;
        Ok(vec![
            Field::new(&["inferenceProvider"], Some(json!("gateway"))),
            Field::new(&["inferenceGatewayBaseUrl"], Some(json!(c.base_url))),
            Field::new(&["inferenceGatewayApiKey"], Some(json!(c.key))),
            Field::new(&["inferenceGatewayAuthScheme"], Some(json!("bearer"))),
            Field::new(&["inferenceModels"], Some(json!([{"name":c.model}]))),
            Field::new(&["disableDeploymentModeChooser"], Some(json!(true))),
        ])
    }
    async fn changes(
        &self,
        context: &Context,
        mode: Mode,
        credential: Option<&Credential>,
    ) -> Result<Vec<Change>> {
        let targets = paths(context);
        let mut changes = Vec::new();
        for (id, path) in &targets[..2] {
            changes.push(Change {
                id: id.clone(),
                path: path.clone(),
                format: Format::Json,
                fields: vec![Field::new(
                    &["deploymentMode"],
                    Some(json!(if mode == Mode::Tokenone { "3p" } else { "1p" })),
                )],
            });
        }
        changes.push(Change {
            id: targets[2].0.clone(),
            path: targets[2].1.clone(),
            format: Format::Json,
            fields: self.fields(mode, credential)?,
        });
        let meta = read_value(&targets[3].1, Format::Json)?;
        let mut entries = match meta.get("entries") {
            None => Vec::new(),
            Some(Value::Array(entries)) => entries.clone(),
            _ => return Err(message("Claude 配置库格式无效，未修改文件")),
        };
        // Only our own profile is added or removed; unrelated entries survive.
        entries.retain(|v| v.get("id").and_then(Value::as_str) != Some(PROFILE));
        if mode == Mode::Tokenone {
            entries.push(json!({"id":PROFILE,"name":"TokenOne"}));
        }
        let applied = if mode == Mode::Tokenone {
            Some(json!(PROFILE))
        } else {
            meta.get("appliedId")
                .filter(|v| v.as_str() != Some(PROFILE))
                .cloned()
        };
        changes.push(Change {
            id: targets[3].0.clone(),
            path: targets[3].1.clone(),
            format: Format::Json,
            fields: vec![
                Field::new(&["entries"], Some(json!(entries))),
                Field::new(&["appliedId"], applied),
            ],
        });
        Ok(changes)
    }
    fn current_mode(&self, context: &Context, endpoint: Option<&str>) -> Result<Option<Mode>> {
        let p = paths(context);
        let a = read_value(&p[0].1, Format::Json)?;
        let b = read_value(&p[1].1, Format::Json)?;
        if [a.get("deploymentMode"), b.get("deploymentMode")]
            .iter()
            .all(|m| m.is_none() || m.and_then(|v| v.as_str()) == Some("1p"))
        {
            return Ok(Some(Mode::Official));
        }
        if a["deploymentMode"] != "3p" || b["deploymentMode"] != "3p" {
            return Ok(None);
        }
        let meta = read_value(&p[3].1, Format::Json)?;
        if meta["appliedId"] != PROFILE
            || !meta["entries"]
                .as_array()
                .is_some_and(|entries| entries.iter().any(|e| e["id"] == PROFILE))
        {
            return Ok(None);
        }
        Ok(self.mode(&read_value(&p[2].1, Format::Json)?, endpoint))
    }
    fn mode(&self, value: &Value, endpoint: Option<&str>) -> Option<Mode> {
        if value["inferenceProvider"] == "gateway"
            && value["inferenceGatewayAuthScheme"] == "bearer"
            && value["inferenceGatewayApiKey"]
                .as_str()
                .is_some_and(|s| !s.is_empty())
            && same_endpoint(value.get("inferenceGatewayBaseUrl"), endpoint)
        {
            Some(Mode::Tokenone)
        } else {
            None
        }
    }
    fn install_script(&self, _: &str) -> Result<&'static str> {
        Err(message("请使用官方桌面安装流程"))
    }
    fn official_arguments(&self) -> &[&str] {
        &[]
    }
    fn official_status_arguments(&self) -> &[&str] {
        &[]
    }
    fn official_session(&self, _: &str, _: &str) -> bool {
        false
    }
    fn verify_request(&self, c: &Credential) -> (String, Value) {
        super::claude::Claude.verify_request(c)
    }
    fn request_headers(&self, key: &str) -> Vec<(&'static str, String)> {
        super::claude::Claude.request_headers(key)
    }
    fn valid_response(&self, value: &Value) -> bool {
        super::claude::Claude.valid_response(value)
    }
    fn check_conflicts(&self, _: Mode) -> Result<()> {
        Ok(())
    }
}
