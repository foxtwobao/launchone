use crate::error::{message, Result};
use std::collections::HashMap;

#[derive(Clone, Default)]
pub struct Settings {
    pub issuer: Option<String>,
    pub service_url: Option<String>,
    pub tokenone_portal_url: Option<String>,
    pub client_id: Option<String>,
    pub update_public_key: Option<String>,
    pub endpoints: HashMap<String, (String, String)>,
}

impl Settings {
    pub fn from_environment() -> Self {
        // Only the service entry and updater trust key are embedded. Business
        // settings are fetched from that service before constructing Auth.
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
        Self {
            service_url: setting!("LAUNCHONE_SERVICE_URL"),
            update_public_key: setting!("LAUNCHONE_UPDATE_PUBLIC_KEY"),
            ..Self::default()
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

    pub fn tokenone_portal_url(&self) -> Result<url::Url> {
        let value = self
            .tokenone_portal_url
            .as_deref()
            .ok_or_else(|| message("TokenOne 账户入口尚未配置，请联系服务方"))?;
        let url = validate_service_url(value)?;
        if url.scheme() != "https" {
            return Err(message("TokenOne 账户入口必须使用 HTTPS"));
        }
        Ok(url)
    }
}

#[cfg(any(feature = "desktop", test))]
pub(crate) fn tokenone_endpoints(
    base: &str,
    codex_model: Option<String>,
    claude_model: Option<String>,
) -> Result<HashMap<String, (String, String)>> {
    let url = validate_service_url(base)?;
    if url.path() != "/" {
        return Err(message(
            "TokenOne 地址只需填写根地址，接口路径由应用自动处理",
        ));
    }
    let base = url.as_str().trim_end_matches('/');
    let mut endpoints = HashMap::new();
    if let Some(model) = codex_model {
        endpoints.insert("codex".into(), (format!("{base}/v1"), model));
    }
    if let Some(model) = claude_model {
        endpoints.insert("claude-desktop".into(), (base.into(), model.clone()));
        endpoints.insert("claude-code".into(), (base.into(), model));
    }
    Ok(endpoints)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_tokenone_address_configures_both_protocols_with_or_without_trailing_slash() {
        for base in [
            "https://tokenone.example.com",
            "https://tokenone.example.com/",
        ] {
            let settings = Settings {
                endpoints: tokenone_endpoints(
                    base,
                    Some("codex-model".into()),
                    Some("claude-model".into()),
                )
                .unwrap(),
                ..Settings::default()
            };
            let (codex, model) = settings.endpoint("codex").unwrap();
            assert_eq!(
                format!("{codex}/responses"),
                "https://tokenone.example.com/v1/responses"
            );
            assert_eq!(model, "codex-model");
            for app in ["claude-desktop", "claude-code"] {
                let (claude, model) = settings.endpoint(app).unwrap();
                assert_eq!(claude, "https://tokenone.example.com");
                assert_eq!(
                    format!("{claude}/v1/messages"),
                    "https://tokenone.example.com/v1/messages"
                );
                assert_eq!(model, "claude-model");
            }
        }
    }

    #[test]
    fn tokenone_root_rejects_api_paths_credentials_and_query_parameters() {
        for base in [
            "https://tokenone.example.com/v1",
            "https://tokenone.example.com/v1/",
            "https://tokenone.example.com/messages",
            "https://user:secret@tokenone.example.com",
            "https://tokenone.example.com/?token=secret",
            "https://tokenone.example.com/#secret",
            "http://tokenone.example.com",
        ] {
            assert!(
                tokenone_endpoints(
                    base,
                    Some("codex-model".into()),
                    Some("claude-model".into())
                )
                .is_err(),
                "{base}"
            );
        }
    }

    #[test]
    fn portal_accepts_only_explicit_https_urls_without_credentials_or_tokens() {
        assert!(Settings::default().tokenone_portal_url().is_err());
        for value in [
            "http://localhost",
            "http://example.com",
            "https://user:password@example.com",
            "https://example.com/?token=secret",
            "https://example.com/#secret",
            "javascript:alert(1)",
        ] {
            let settings = Settings {
                tokenone_portal_url: Some(value.into()),
                ..Settings::default()
            };
            assert!(settings.tokenone_portal_url().is_err(), "{value}");
        }
        let settings = Settings {
            tokenone_portal_url: Some("https://tokenone.example.com/account".into()),
            ..Settings::default()
        };
        assert_eq!(
            settings.tokenone_portal_url().unwrap().as_str(),
            "https://tokenone.example.com/account"
        );
    }
}
