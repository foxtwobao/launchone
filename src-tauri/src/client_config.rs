use crate::config::atomic_write;
use crate::error::{message, Result};
use crate::settings::{tokenone_endpoints, validate_service_url, Settings};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

const MAX_BYTES: usize = 16 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ClientConfig {
    #[serde(rename = "LAUNCHONE_IDONE_ISSUER")]
    issuer: String,
    #[serde(rename = "LAUNCHONE_CLIENT_ID")]
    client_id: String,
    #[serde(rename = "LAUNCHONE_TOKENONE_URL")]
    tokenone_url: String,
    #[serde(rename = "LAUNCHONE_CODEX_MODEL")]
    codex_model: String,
    #[serde(rename = "LAUNCHONE_CLAUDE_MODEL")]
    claude_model: String,
    #[serde(rename = "LAUNCHONE_TOKENONE_PORTAL_URL", default)]
    portal_url: String,
}

impl ClientConfig {
    fn apply(&self, mut settings: Settings) -> Result<Settings> {
        validate_service_url(&self.issuer)?;
        let client_id = text(&self.client_id, 1024)?;
        let codex_model = text(&self.codex_model, 256)?;
        let claude_model = text(&self.claude_model, 256)?;
        let tokenone_url = text(&self.tokenone_url, 2048)?;
        // Public model traffic and account links always use HTTPS, even in dev.
        if validate_service_url(tokenone_url)?.scheme() != "https" {
            return Err(message("TokenOne 地址必须使用 HTTPS"));
        }
        let endpoints = tokenone_endpoints(
            tokenone_url,
            Some(codex_model.into()),
            Some(claude_model.into()),
        )?;
        let portal = self.portal_url.trim();
        if !portal.is_empty()
            && (portal.len() > 2048 || validate_service_url(portal)?.scheme() != "https")
        {
            return Err(message("TokenOne 账户入口配置无效"));
        }
        settings.issuer = Some(self.issuer.clone());
        settings.client_id = Some(client_id.into());
        settings.endpoints = endpoints;
        settings.tokenone_portal_url = (!portal.is_empty()).then(|| portal.into());
        Ok(settings)
    }
}

fn text(value: &str, limit: usize) -> Result<&str> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > limit || value.chars().any(char::is_control) {
        return Err(message("接入配置无效"));
    }
    Ok(value)
}

fn cache_path(directory: &Path, profile: &str, service: &str) -> PathBuf {
    // Lab and production share backups, but never share their service config.
    let binding = serde_json::to_vec(&[profile, service]).expect("serializable binding");
    directory
        .join("client-config")
        .join(format!("{:x}.json", Sha256::digest(binding)))
}

fn read_cache(path: &Path) -> Result<ClientConfig> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BYTES {
        return Err(message("接入配置过大"));
    }
    Ok(serde_json::from_slice(&bytes)?)
}

async fn fetch(service: &str) -> Result<ClientConfig> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|_| message("无法读取接入配置"))?;
    let mut response = client
        .get(format!("{service}/v1/client-config"))
        .header(reqwest::header::CACHE_CONTROL, "no-cache")
        .send()
        .await
        .map_err(|_| message("无法读取接入配置"))?
        .error_for_status()
        .map_err(|_| message("无法读取接入配置"))?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| message("无法读取接入配置"))?
    {
        if bytes.len() + chunk.len() > MAX_BYTES {
            return Err(message("接入配置过大"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&bytes)?)
}

/// Refresh once at startup, then keep a consistent config for the whole login.
/// Only fully validated public values replace the last working cache.
pub async fn load(settings: Settings, directory: &Path, profile: &str) -> Settings {
    let Some(service) = settings.service_url.as_deref() else {
        return settings;
    };
    if validate_service_url(service).is_err() {
        return settings;
    }
    let service = service.trim_end_matches('/');
    let path = cache_path(directory, profile, service);
    if let Ok(config) = fetch(service).await {
        if let Ok(resolved) = config.apply(settings.clone()) {
            if let Ok(bytes) = serde_json::to_vec(&config) {
                let _ = atomic_write(&path, &bytes);
            }
            return resolved;
        }
    }
    read_cache(&path)
        .and_then(|config| config.apply(settings.clone()))
        .unwrap_or(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::State,
        http::{HeaderMap, StatusCode},
        routing::get,
        Router,
    };
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};

    struct Fixture {
        service: String,
        reply: Arc<Mutex<(StatusCode, String)>>,
        task: tokio::task::JoinHandle<()>,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.task.abort();
        }
    }
    impl Fixture {
        fn settings(&self) -> Settings {
            Settings {
                service_url: Some(self.service.clone()),
                update_public_key: Some("fixed-update-key".into()),
                ..Settings::default()
            }
        }
    }

    fn config(model: &str) -> Value {
        json!({
            "LAUNCHONE_IDONE_ISSUER": "https://idone.example.test",
            "LAUNCHONE_CLIENT_ID": "launchone-client",
            "LAUNCHONE_TOKENONE_URL": "https://tokenone.example.test/",
            "LAUNCHONE_CODEX_MODEL": model,
            "LAUNCHONE_CLAUDE_MODEL": "claude-model",
            "LAUNCHONE_TOKENONE_PORTAL_URL": "https://tokenone.example.test/account"
        })
    }

    async fn start_fixture() -> Fixture {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let service = format!("http://{}", listener.local_addr().unwrap());
        let reply = Arc::new(Mutex::new((
            StatusCode::OK,
            config("first-model").to_string(),
        )));
        let app =
            Router::new()
                .route(
                    "/v1/client-config",
                    get(
                        |State(reply): State<Arc<Mutex<(StatusCode, String)>>>,
                         headers: HeaderMap| async move {
                            assert!(!headers.contains_key("authorization"));
                            let (status, body) = reply.lock().unwrap().clone();
                            (status, body)
                        },
                    ),
                )
                .with_state(reply.clone());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Fixture {
            service,
            reply,
            task,
        }
    }

    #[tokio::test]
    async fn startup_fetches_current_defaults_and_keeps_embedded_trust_settings() {
        let fixture = start_fixture().await;
        let directory = tempfile::tempdir().unwrap();
        let first = load(fixture.settings(), directory.path(), "lab").await;
        assert!(first.login_ready() && first.tokenone_ready());
        assert_eq!(first.endpoint("codex").unwrap().1, "first-model");
        assert_eq!(
            first.endpoint("codex").unwrap().0,
            "https://tokenone.example.test/v1"
        );
        assert_eq!(
            first.endpoint("claude-desktop").unwrap().0,
            "https://tokenone.example.test"
        );
        assert_eq!(first.service_url.as_deref(), Some(fixture.service.as_str()));
        assert_eq!(first.update_public_key.as_deref(), Some("fixed-update-key"));

        *fixture.reply.lock().unwrap() = (StatusCode::OK, config("changed-model").to_string());
        let next = load(fixture.settings(), directory.path(), "lab").await;
        assert_eq!(next.endpoint("codex").unwrap().1, "changed-model");
        assert_eq!(next.client_id, first.client_id);
    }

    #[tokio::test]
    async fn offline_uses_last_good_config_only_for_the_same_service_and_build() {
        let fixture = start_fixture().await;
        let directory = tempfile::tempdir().unwrap();
        load(fixture.settings(), directory.path(), "lab").await;
        *fixture.reply.lock().unwrap() = (StatusCode::SERVICE_UNAVAILABLE, "unavailable".into());
        assert!(load(fixture.settings(), directory.path(), "lab")
            .await
            .tokenone_ready());
        assert!(!load(fixture.settings(), directory.path(), "production")
            .await
            .login_ready());
        let other = start_fixture().await;
        *other.reply.lock().unwrap() = (StatusCode::SERVICE_UNAVAILABLE, "unavailable".into());
        let unavailable = load(other.settings(), directory.path(), "lab").await;
        assert!(!unavailable.login_ready());
        assert_eq!(
            unavailable.update_public_key.as_deref(),
            Some("fixed-update-key")
        );
        assert!(unavailable.service_url.is_some());
    }

    #[tokio::test]
    async fn invalid_or_oversized_replies_never_replace_a_valid_cache() {
        let fixture = start_fixture().await;
        let directory = tempfile::tempdir().unwrap();
        load(fixture.settings(), directory.path(), "lab").await;
        let mut wrong = config("bad-model");
        wrong["LAUNCHONE_TOKENONE_URL"] = json!("https://tokenone.example.test/v1");
        let mut extra = config("bad-model");
        extra["LAUNCHONE_SERVICE_URL"] = json!("https://other.example.test");
        let mut missing = config("bad-model");
        missing
            .as_object_mut()
            .unwrap()
            .remove("LAUNCHONE_CLIENT_ID");
        for body in [
            wrong.to_string(),
            extra.to_string(),
            missing.to_string(),
            "x".repeat(MAX_BYTES + 1),
        ] {
            *fixture.reply.lock().unwrap() = (StatusCode::OK, body);
            let cached = load(fixture.settings(), directory.path(), "lab").await;
            assert_eq!(cached.endpoint("codex").unwrap().1, "first-model");
            let fresh = load(fixture.settings(), directory.path(), "fresh").await;
            assert!(!fresh.login_ready());
        }
    }
}
