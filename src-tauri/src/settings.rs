use crate::error::{message, Result};
use std::collections::HashMap;

#[derive(Clone, Default)]
pub struct Settings {
    pub issuer: Option<String>,
    pub service_url: Option<String>,
    pub client_id: Option<String>,
    pub endpoints: HashMap<String, (String, String)>,
}

impl Settings {
    pub fn from_environment() -> Self {
        // Production settings are embedded by the publisher. Runtime environment
        // overrides are development-only, never renderer-controlled providers.
        macro_rules! setting {
            ($key:literal) => {{
                let embedded = option_env!($key).map(str::to_string);
                #[cfg(debug_assertions)]
                let embedded = std::env::var($key)
                    .ok()
                    .filter(|s| !s.trim().is_empty())
                    .or(embedded);
                embedded.filter(|s| !s.trim().is_empty())
            }};
        }
        let mut endpoints = HashMap::new();
        if let (Some(url), Some(model)) = (
            setting!("LAUNCHONE_CODEX_BASE_URL"),
            setting!("LAUNCHONE_CODEX_MODEL"),
        ) {
            endpoints.insert("codex".into(), (url, model));
        }
        if let (Some(url), Some(model)) = (
            setting!("LAUNCHONE_CLAUDE_BASE_URL"),
            setting!("LAUNCHONE_CLAUDE_MODEL"),
        ) {
            endpoints.insert("claude-desktop".into(), (url.clone(), model.clone()));
            endpoints.insert("claude-code".into(), (url, model));
        }
        Self {
            issuer: setting!("LAUNCHONE_IDONE_ISSUER"),
            service_url: setting!("LAUNCHONE_SERVICE_URL"),
            client_id: setting!("LAUNCHONE_CLIENT_ID"),
            endpoints,
        }
    }

    pub fn login_ready(&self) -> bool {
        [&self.issuer, &self.service_url]
            .iter()
            .all(|v| v.as_ref().is_some_and(|s| validate_service_url(s).is_ok()))
            && self.client_id.as_ref().is_some_and(|s| !s.is_empty())
    }

    pub fn tokenone_ready(&self) -> bool {
        self.login_ready()
            && !self.endpoints.is_empty()
            && self
                .endpoints
                .values()
                .all(|(u, m)| validate_service_url(u).is_ok() && !m.is_empty())
    }

    pub fn endpoint(&self, app: &str) -> Result<(String, String)> {
        let (url, model) = self
            .endpoints
            .get(app)
            .ok_or_else(|| message("该工具的 TokenOne 接入尚未配置"))?;
        validate_service_url(url)?;
        Ok((url.trim_end_matches('/').into(), model.clone()))
    }
}

pub fn validate_service_url(value: &str) -> Result<url::Url> {
    let url = url::Url::parse(value).map_err(|_| message("服务地址无效"))?;
    let local = cfg!(debug_assertions)
        && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
    if (url.scheme() != "https" && !(local && url.scheme() == "http"))
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.query().is_some()
    {
        return Err(message("服务地址必须使用 HTTPS，且不能包含凭据或查询参数"));
    }
    Ok(url)
}

pub fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(45))
        .build()
        .map_err(|_| message("无法初始化网络连接"))
}
