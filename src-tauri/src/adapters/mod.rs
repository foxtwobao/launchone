mod claude;
mod claude_desktop;
mod codex;

use crate::config::{read_value, Change, Field, Format};
use crate::desktop_app::DesktopSpec;
use crate::error::{message, Result};
use crate::models::{AppManifest, Credential, Mode};
use serde_json::Value;
use std::path::PathBuf;

pub struct Context {
    pub home: PathBuf,
    use_environment: bool,
}

impl Context {
    pub fn new(home: PathBuf) -> Self {
        Self {
            home,
            use_environment: true,
        }
    }
    pub fn isolated(home: PathBuf) -> Self {
        Self {
            home,
            use_environment: false,
        }
    }
    pub fn config_directory(&self, variable: &str, fallback: &str) -> Result<PathBuf> {
        let path = if self.use_environment {
            std::env::var_os(variable).map(PathBuf::from)
        } else {
            None
        }
        .unwrap_or_else(|| self.home.join(fallback));
        if !path.is_absolute() {
            return Err(message("工具的自定义配置目录必须使用绝对路径"));
        }
        Ok(path)
    }
}

#[async_trait::async_trait]
pub trait AppAdapter: Send + Sync {
    fn manifest(&self) -> AppManifest;
    fn desktop(&self) -> Option<DesktopSpec> {
        None
    }
    fn targets(&self, context: &Context) -> Result<Vec<(String, PathBuf)>> {
        Ok(vec![(self.manifest().id.into(), self.path(context)?)])
    }
    async fn changes(
        &self,
        context: &Context,
        mode: Mode,
        credential: Option<&Credential>,
    ) -> Result<Vec<Change>> {
        Ok(vec![Change {
            id: self.manifest().id.into(),
            path: self.path(context)?,
            format: self.format(),
            fields: self.fields(mode, credential)?,
        }])
    }
    fn current_mode(&self, context: &Context, endpoint: Option<&str>) -> Result<Option<Mode>> {
        Ok(self.mode(&read_value(&self.path(context)?, self.format())?, endpoint))
    }
    fn path(&self, context: &Context) -> Result<PathBuf>;
    fn format(&self) -> Format;
    fn fields(&self, mode: Mode, credential: Option<&Credential>) -> Result<Vec<Field>>;
    fn mode(&self, value: &Value, endpoint: Option<&str>) -> Option<Mode>;
    fn install_script(&self, platform: &str) -> Result<&'static str>;
    fn official_arguments(&self) -> &[&str];
    fn official_status_arguments(&self) -> &[&str];
    fn official_session(&self, stdout: &str, stderr: &str) -> bool;
    fn verify_request(&self, credential: &Credential) -> (String, Value);
    fn request_headers(&self, key: &str) -> Vec<(&'static str, String)>;
    fn valid_response(&self, value: &Value) -> bool;
    fn check_conflicts(&self, mode: Mode) -> Result<()>;
}

pub struct Registry {
    adapters: Vec<Box<dyn AppAdapter>>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            adapters: vec![
                Box::new(codex::Codex),
                Box::new(claude_desktop::ClaudeDesktop),
                Box::new(claude::Claude),
            ],
        }
    }
}

impl Registry {
    pub fn all(&self) -> impl Iterator<Item = &dyn AppAdapter> {
        self.adapters.iter().map(|a| a.as_ref())
    }
    pub fn get(&self, id: &str) -> Result<&dyn AppAdapter> {
        self.all()
            .find(|a| a.manifest().id == id)
            .ok_or_else(|| message("此应用尚未支持"))
    }
}

pub fn get<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().try_fold(value, |v, key| v.get(*key))
}

pub fn same_endpoint(actual: Option<&Value>, expected: Option<&str>) -> bool {
    matches!((actual.and_then(Value::as_str), expected), (Some(a), Some(b)) if a.trim_end_matches('/') == b.trim_end_matches('/'))
}

pub fn credential(value: Option<&Credential>) -> Result<&Credential> {
    value.ok_or_else(|| message("尚未获取 TokenOne 凭据"))
}

pub fn reject_environment(keys: &[&str]) -> Result<()> {
    if keys
        .iter()
        .any(|key| std::env::var_os(key).is_some_and(|v| !v.is_empty()))
    {
        return Err(message(
            "检测到已有接入环境变量，可能覆盖配置。请清理对应工具的 API 地址或 Key 环境变量后重试",
        ));
    }
    Ok(())
}

pub fn ensure_supported(adapter: &dyn AppAdapter) -> Result<()> {
    let platform = std::env::consts::OS;
    let architecture = std::env::consts::ARCH;
    if adapter.manifest().platforms.contains(&platform)
        && ((platform == "windows" && architecture == "x86_64")
            || (platform == "macos" && matches!(architecture, "x86_64" | "aarch64")))
    {
        Ok(())
    } else {
        Err(message(
            "当前系统暂不支持自动安装和配置，请使用 macOS 或 Windows x64",
        ))
    }
}
