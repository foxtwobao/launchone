use crate::error::{actionable, message, RecoveryAction, Result};
use crate::models::{Account, Credential};
use crate::settings::{http_client, validate_service_url, Settings};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tokio::time::Instant;

const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

fn credential_namespace() -> &'static str {
    if option_env!("LAUNCHONE_BUILD_ENV") == Some("lab") {
        "com.tokenone.launchone.lab"
    } else {
        "com.tokenone.launchone"
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Session {
    service_binding: String,
    account: Account,
    access_token: String,
    refresh_token: Option<String>,
    expires_at: u64,
}

#[derive(Deserialize)]
struct Discovery {
    issuer: String,
    device_authorization_endpoint: String,
    token_endpoint: String,
    userinfo_endpoint: String,
}

#[derive(Deserialize)]
struct DeviceAuthorization {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: Option<u64>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
}

#[derive(Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum KeyResponse {
    Ready {
        key: String,
    },
    AuthorizationRequired {
        id: String,
        authorization_url: String,
        expires_in: u64,
    },
    Pending,
}

pub struct Auth {
    settings: Settings,
    session: Mutex<Option<Session>>,
    cancelled: AtomicBool,
    #[cfg(test)]
    isolated: bool,
}

impl Auth {
    pub fn new(settings: Settings) -> Self {
        let saved = keyring::Entry::new(credential_namespace(), "idone")
            .ok()
            .and_then(|e| e.get_password().ok())
            .and_then(|s| serde_json::from_str::<Session>(&s).ok())
            .filter(|s| s.service_binding == service_binding(&settings));
        Self {
            settings,
            session: Mutex::new(saved),
            cancelled: AtomicBool::new(false),
            #[cfg(test)]
            isolated: false,
        }
    }

    // Tests never read or write the host's system credential store.
    #[cfg(test)]
    pub(crate) fn isolated(settings: Settings) -> Self {
        Self {
            settings,
            session: Mutex::new(None),
            cancelled: AtomicBool::new(false),
            isolated: true,
        }
    }

    pub async fn account(&self) -> Option<Account> {
        self.session
            .lock()
            .await
            .as_ref()
            .map(|s| s.account.clone())
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    fn check_cancelled(&self) -> Result<()> {
        if self.cancelled.load(Ordering::SeqCst) {
            return Err(message("已取消操作"));
        }
        Ok(())
    }

    async fn wait(&self, duration: Duration) -> Result<()> {
        let until = Instant::now() + duration;
        while Instant::now() < until {
            self.check_cancelled()?;
            tokio::time::sleep(
                until
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(100)),
            )
            .await;
        }
        self.check_cancelled()
    }

    async fn discovery(&self, client: &Client) -> Result<Discovery> {
        let issuer = self
            .settings
            .issuer
            .as_deref()
            .ok_or_else(|| message("IDONE 尚未配置"))?;
        let response = client
            .get(format!(
                "{}/.well-known/openid-configuration",
                issuer.trim_end_matches('/')
            ))
            .send()
            .await
            .map_err(|_| message("无法连接 IDONE"))?
            .error_for_status()
            .map_err(|_| message("无法读取 IDONE 登录配置"))?;
        let discovery: Discovery = response
            .json()
            .await
            .map_err(|_| message("IDONE 尚未开放设备登录"))?;
        if discovery.issuer != issuer {
            return Err(message("IDONE 签发方不匹配"));
        }
        for endpoint in [
            &discovery.device_authorization_endpoint,
            &discovery.token_endpoint,
            &discovery.userinfo_endpoint,
        ] {
            same_origin(endpoint, issuer, false)?;
        }
        Ok(discovery)
    }

    pub async fn login(
        &self,
        open_browser: impl FnOnce(&str) -> Result<()>,
        progress: impl Fn(&str),
    ) -> Result<Account> {
        if !self.settings.login_ready() {
            return Err(message("IDONE 登录服务尚未接入，请联系应用提供方"));
        }
        self.cancelled.store(false, Ordering::SeqCst);
        let client = http_client()?;
        let discovery = self.discovery(&client).await?;
        let client_id = self.settings.client_id.as_deref().unwrap();
        let response = client
            .post(&discovery.device_authorization_endpoint)
            .form(&[
                ("client_id", client_id),
                ("scope", "openid profile email offline_access"),
            ])
            .send()
            .await
            .map_err(|_| message("无法发起 IDONE 设备登录"))?;
        let status = response.status();
        let value: Value = response
            .json()
            .await
            .map_err(|_| message("IDONE 设备登录响应无效"))?;
        if value.get("error").is_some() || !status.is_success() {
            return Err(message("此 IDONE 客户端尚未启用设备登录，请联系应用提供方"));
        }
        let device: DeviceAuthorization =
            serde_json::from_value(value).map_err(|_| message("IDONE 设备登录响应无效"))?;
        let mut interval = device.interval.unwrap_or(5);
        if device.device_code.is_empty()
            || device.user_code.is_empty()
            || device.user_code.len() > 128
            || !(1..=600).contains(&device.expires_in)
            || !(1..=60).contains(&interval)
        {
            return Err(message("IDONE 设备登录响应无效"));
        }
        // IDONE embeds the user code in verification_uri, rather than supplying verification_uri_complete.
        same_origin(
            &device.verification_uri,
            self.settings.issuer.as_deref().unwrap(),
            true,
        )?;
        self.check_cancelled()?;
        let deadline = Instant::now() + Duration::from_secs(device.expires_in);
        progress(&format!("请在浏览器核对确认码：{}", device.user_code));
        open_browser(&device.verification_uri)?;
        let token = loop {
            if Instant::now() >= deadline {
                return Err(message("设备登录已过期，请重试"));
            }
            self.wait(
                Duration::from_secs(interval)
                    .min(deadline.saturating_duration_since(Instant::now())),
            )
            .await?;
            if Instant::now() >= deadline {
                return Err(message("设备登录已过期，请重试"));
            }
            let response = client
                .post(&discovery.token_endpoint)
                .form(&[
                    ("grant_type", DEVICE_GRANT),
                    ("client_id", client_id),
                    ("device_code", &device.device_code),
                ])
                .timeout(
                    deadline
                        .saturating_duration_since(Instant::now())
                        .min(Duration::from_secs(45)),
                )
                .send()
                .await
                .map_err(|_| message("无法连接 IDONE 登录服务，请重试"))?;
            let status = response.status();
            let value: Value = response
                .json()
                .await
                .map_err(|_| message("IDONE 授权响应无效"))?;
            match value.get("error").and_then(Value::as_str) {
                Some("authorization_pending") => continue,
                Some("slow_down") => {
                    interval += 5;
                    continue;
                }
                Some("access_denied") => return Err(message("设备登录已被拒绝或取消")),
                Some("expired_token") => return Err(message("设备登录已过期，请重试")),
                Some(_) => return Err(message("IDONE 设备授权失败，请重试或联系应用提供方")),
                None if !status.is_success() => return Err(message("IDONE 设备授权失败，请重试")),
                None => break valid_token(value)?,
            }
        };
        self.check_cancelled()?;
        let response = client
            .get(&discovery.userinfo_endpoint)
            .bearer_auth(&token.access_token)
            .send()
            .await
            .map_err(|_| message("无法连接 IDONE 账户服务"))?
            .error_for_status()
            .map_err(|_| message("IDONE 账户校验失败，请重新登录"))?;
        let info: Value = response
            .json()
            .await
            .map_err(|_| message("IDONE 账户响应无效"))?;
        let sub = info
            .get("sub")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| message("IDONE 未返回有效身份"))?;
        if info.get("iss").and_then(Value::as_str) != self.settings.issuer.as_deref() {
            return Err(message("IDONE 账户签发方不匹配"));
        }
        let name = ["name", "preferred_username", "displayName"]
            .iter()
            .find_map(|key| {
                info.get(key)
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty())
            })
            .unwrap_or(sub);
        let account = Account {
            id: sub.into(),
            name: name.into(),
        };
        let session = Session {
            service_binding: service_binding(&self.settings),
            account: account.clone(),
            access_token: token.access_token,
            refresh_token: token.refresh_token,
            expires_at: now() + token.expires_in,
        };
        self.check_cancelled()?;
        self.persist(&session)?;
        *self.session.lock().await = Some(session);
        Ok(account)
    }

    pub async fn logout(&self) -> Result<()> {
        #[cfg(test)]
        if self.isolated {
            *self.session.lock().await = None;
            return Ok(());
        }
        let entry = keyring::Entry::new(credential_namespace(), "idone")
            .map_err(|_| message("无法访问系统凭据存储"))?;
        match entry.delete_credential() {
            Ok(_) | Err(keyring::Error::NoEntry) => {}
            Err(_) => return Err(message("无法清除本地登录信息，请重试")),
        }
        *self.session.lock().await = None;
        Ok(())
    }

    async fn active_session(&self, client: &Client) -> Result<Session> {
        let mut session = self
            .session
            .lock()
            .await
            .clone()
            .ok_or_else(|| actionable("请先登录 IDONE", RecoveryAction::Login))?;
        if session.expires_at <= now() + 30 {
            let refresh = session
                .refresh_token
                .as_ref()
                .ok_or_else(|| actionable("登录已过期，请重新登录 IDONE", RecoveryAction::Login))?;
            let discovery = self.discovery(client).await?;
            let response = client
                .post(&discovery.token_endpoint)
                .form(&[
                    ("grant_type", "refresh_token"),
                    ("refresh_token", refresh),
                    (
                        "client_id",
                        self.settings.client_id.as_deref().unwrap_or_default(),
                    ),
                ])
                .send()
                .await
                .map_err(|_| actionable("无法刷新 IDONE 登录，请重试", RecoveryAction::Retry))?;
            if !response.status().is_success() {
                return Err(
                    if response.status().is_client_error() && response.status().as_u16() != 429 {
                        actionable("登录已过期，请重新登录 IDONE", RecoveryAction::Login)
                    } else {
                        actionable("暂时无法刷新 IDONE 登录，请重试", RecoveryAction::Retry)
                    },
                );
            }
            let token = valid_token(
                response
                    .json()
                    .await
                    .map_err(|_| message("IDONE 刷新响应无效"))?,
            )?;
            session.access_token = token.access_token;
            if token.refresh_token.is_some() {
                session.refresh_token = token.refresh_token;
            }
            session.expires_at = now() + token.expires_in;
            self.persist(&session)?;
            *self.session.lock().await = Some(session.clone());
        }
        Ok(session)
    }

    pub async fn credential(
        &self,
        app: &str,
        open_browser: impl FnOnce(&str) -> Result<()>,
        progress: impl Fn(&str),
    ) -> Result<Credential> {
        self.cancelled.store(false, Ordering::SeqCst);
        let (base_url, model) = self.settings.endpoint(app)?;
        let client = http_client()?;
        let session = self.active_session(&client).await?;
        let origin = self
            .settings
            .service_url
            .as_deref()
            .ok_or_else(|| message("TokenOne 接入服务尚未配置"))?;
        let initial = client
            .post(format!("{}/v1/credentials", origin.trim_end_matches('/')))
            .bearer_auth(&session.access_token)
            .json(&json!({}))
            .send()
            .await
            .map_err(|_| message("无法连接 TokenOne，请检查网络后重试"))?;
        let mut reply = read_key_response(initial).await?;
        if let KeyResponse::AuthorizationRequired {
            id,
            authorization_url,
            expires_in,
        } = reply
        {
            if id.len() != 43
                || !id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
                || !(1..=300).contains(&expires_in)
            {
                return Err(message("模型授权响应无效"));
            }
            same_origin(&authorization_url, origin, true)?;
            self.check_cancelled()?;
            progress("请在浏览器中完成模型服务授权");
            open_browser(&authorization_url)?;
            let deadline = Instant::now() + Duration::from_secs(expires_in);
            reply = loop {
                self.wait(
                    Duration::from_secs(2).min(deadline.saturating_duration_since(Instant::now())),
                )
                .await?;
                if Instant::now() >= deadline {
                    return Err(actionable("模型授权已过期，请重试", RecoveryAction::Retry));
                }
                let response = client
                    .get(format!(
                        "{}/v1/authorizations/{id}",
                        origin.trim_end_matches('/')
                    ))
                    .bearer_auth(&session.access_token)
                    .timeout(
                        deadline
                            .saturating_duration_since(Instant::now())
                            .min(Duration::from_secs(45)),
                    )
                    .send()
                    .await
                    .map_err(|_| message("暂时无法查询模型授权，请重试"))?;
                match read_key_response(response).await? {
                    KeyResponse::Pending => continue,
                    ready @ KeyResponse::Ready { .. } => break ready,
                    _ => return Err(message("模型授权状态无效，请重试")),
                }
            };
        }
        self.check_cancelled()?;
        let KeyResponse::Ready { key } = reply else {
            return Err(message("TokenOne 尚未完成模型授权"));
        };
        if key.trim().is_empty() || key.contains(['\r', '\n']) {
            return Err(message("TokenOne 未返回有效的调用 Key"));
        }
        Ok(Credential {
            key,
            base_url,
            model,
        })
    }

    fn persist(&self, session: &Session) -> Result<()> {
        #[cfg(test)]
        if self.isolated {
            return Ok(());
        }
        keyring::Entry::new(credential_namespace(), "idone")
            .and_then(|entry| {
                entry.set_password(&serde_json::to_string(session).expect("serializable session"))
            })
            .map_err(|_| message("系统凭据存储不可用，无法安全保存登录信息"))
    }
}

async fn read_key_response(response: reqwest::Response) -> Result<KeyResponse> {
    let status = response.status();
    let value: Value = response
        .json()
        .await
        .map_err(|_| message("TokenOne 接入服务响应无效"))?;
    if !status.is_success() {
        let (text, action) = match value.get("error").and_then(Value::as_str) {
            Some("UNAUTHORIZED") => ("登录已过期，请重新登录 IDONE", RecoveryAction::Login),
            Some("TOKENONE_USER_NOT_FOUND") => {
                ("请先在 TokenOne 完成开户，再重试", RecoveryAction::Portal)
            }
            Some("TOKENONE_USER_DISABLED") => {
                ("TokenOne 账户已停用，请联系服务方", RecoveryAction::Portal)
            }
            Some("TOKENONE_GROUP_FORBIDDEN") => (
                "账户尚无此模型服务的使用权限，请联系服务方",
                RecoveryAction::Portal,
            ),
            Some("TOKENONE_KEY_QUOTA_EXHAUSTED") => (
                "TokenOne 调用 Key 的额度已用完，请检查账户",
                RecoveryAction::Portal,
            ),
            Some("APP_KEY_UNAVAILABLE") => (
                "TokenOne 调用 Key 已停用或过期，请检查账户",
                RecoveryAction::Portal,
            ),
            Some("IDENTITY_BINDING_CONFLICT") => (
                "模型账户与登录身份不一致，请联系服务方",
                RecoveryAction::Portal,
            ),
            Some("AUTHORIZATION_FLOW_INVALID" | "AUTHORIZATION_RETRY_REQUIRED") => {
                ("模型授权未完成或已过期，请重试", RecoveryAction::Retry)
            }
            _ => (
                "模型服务暂时不可用，请稍后重试；如持续失败，请联系服务方",
                RecoveryAction::Retry,
            ),
        };
        return Err(actionable(text, action));
    }
    serde_json::from_value(value).map_err(|_| message("TokenOne 接入服务响应无效"))
}

fn valid_token(value: Value) -> Result<TokenResponse> {
    let token: TokenResponse = serde_json::from_value(value)
        .map_err(|_| message("IDONE 授权失败或响应无效，请重新登录"))?;
    if token.access_token.is_empty() || token.expires_in == 0 {
        return Err(message("IDONE 未返回有效的登录凭据"));
    }
    Ok(token)
}

fn same_origin(value: &str, expected: &str, allow_query: bool) -> Result<()> {
    let base = validate_service_url(expected)?;
    let url = url::Url::parse(value).map_err(|_| message("授权地址无效"))?;
    if url.origin() != base.origin()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || (!allow_query && url.query().is_some())
    {
        return Err(message("授权地址与发布方配置不匹配"));
    }
    Ok(())
}

fn service_binding(settings: &Settings) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(
        serde_json::to_vec(&[&settings.issuer, &settings.service_url, &settings.client_id])
            .expect("serializable settings"),
    ))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Bytes,
        extract::State,
        http::{Request, StatusCode},
        response::IntoResponse,
        routing::any,
        Json, Router,
    };
    use std::sync::{Arc, Mutex as StdMutex};

    struct Fixture {
        base: String,
        calls: Arc<StdMutex<Vec<(String, Value)>>>,
        task: tokio::task::JoinHandle<()>,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn fixture(scenario: &'static str) -> Fixture {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let calls = Arc::new(StdMutex::new(Vec::new()));
        let shared = (base.clone(), scenario, calls.clone());
        let app = Router::new().fallback(any(mock)).with_state(shared);
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Fixture { base, calls, task }
    }

    async fn mock(
        State((base, scenario, calls)): State<(
            String,
            &'static str,
            Arc<StdMutex<Vec<(String, Value)>>>,
        )>,
        request: Request<axum::body::Body>,
    ) -> impl IntoResponse {
        let (parts, body) = request.into_parts();
        let path = parts.uri.path().to_string();
        let body: Bytes = axum::body::to_bytes(body, 16_384).await.unwrap();
        let fields: Value = if parts
            .headers
            .get("content-type")
            .is_some_and(|v| v == "application/json")
        {
            serde_json::from_slice(&body).unwrap()
        } else {
            Value::Object(
                url::form_urlencoded::parse(&body)
                    .map(|(k, v)| (k.into_owned(), Value::String(v.into_owned())))
                    .collect(),
            )
        };
        let mut calls = calls.lock().unwrap();
        calls.push((path.clone(), fields.clone()));
        let value = match path.as_str() {
            "/.well-known/openid-configuration" => json!({ "issuer": base,
                "device_authorization_endpoint": format!("{base}/device"), "token_endpoint": format!("{base}/token"), "userinfo_endpoint": format!("{base}/userinfo") }),
            "/device" => {
                assert_eq!(fields["client_id"], "fixture-public-client");
                assert!(fields.get("client_secret").is_none());
                json!({ "device_code": "private-device-code", "user_code": "ABCD", "verification_uri": if scenario == "unsafe-url" { "https://attacker.test/confirm".into() } else { format!("{base}/confirm/ABCD?cancelToken=fixture") },
                    "expires_in": if scenario == "expired" { 1 } else { 60 }, "interval": 1 })
            }
            "/token" => {
                assert_eq!(fields["client_id"], "fixture-public-client");
                assert!(fields.get("client_secret").is_none());
                if fields["grant_type"] == "refresh_token" {
                    assert_eq!(fields["refresh_token"], "fixture-refresh-token");
                    json!({"access_token":"fixture-refreshed-token", "refresh_token":"fixture-new-refresh", "expires_in":3600})
                } else {
                    assert_eq!(fields["grant_type"], DEVICE_GRANT);
                    assert_eq!(fields["device_code"], "private-device-code");
                    let polls = calls.iter().filter(|(p, _)| p == "/token").count();
                    match scenario {
                        "denied" => {
                            return (
                                StatusCode::BAD_REQUEST,
                                Json(json!({"error":"access_denied"})),
                            )
                        }
                        "slow" if polls == 1 => {
                            return (StatusCode::BAD_REQUEST, Json(json!({"error":"slow_down"})))
                        }
                        "login" if polls == 1 => {
                            return (
                                StatusCode::BAD_REQUEST,
                                Json(json!({"error":"authorization_pending"})),
                            )
                        }
                        _ => {
                            json!({"access_token":"fixture-user-token", "refresh_token":"fixture-refresh-token", "expires_in":3600})
                        }
                    }
                }
            }
            "/userinfo" => {
                assert_eq!(parts.headers["authorization"], "Bearer fixture-user-token");
                json!({"iss":base, "sub":"fixture-user", "name":"测试用户"})
            }
            "/v1/credentials" => {
                assert_eq!(fields, json!({}));
                let expected = if scenario == "refresh" {
                    "Bearer fixture-refreshed-token"
                } else {
                    "Bearer fixture-user-token"
                };
                assert_eq!(parts.headers["authorization"], expected);
                if scenario == "authorization" {
                    json!({"status":"authorization_required", "id":"a".repeat(43), "authorization_url":format!("{base}/authorize/{}", "a".repeat(43)), "expires_in":10})
                } else if scenario == "quota" {
                    return (
                        StatusCode::CONFLICT,
                        Json(
                            json!({"error":"TOKENONE_KEY_QUOTA_EXHAUSTED", "secret":"must not escape"}),
                        ),
                    );
                } else {
                    json!({"status":"ready", "key":"sk-fixture-model"})
                }
            }
            p if p.starts_with("/v1/authorizations/") => {
                json!({"status":"ready", "key":"sk-fixture-model"})
            }
            _ => return (StatusCode::NOT_FOUND, Json(json!({"error":"NOT_FOUND"}))),
        };
        (StatusCode::OK, Json(value))
    }

    fn auth(fixture: &Fixture) -> Auth {
        Auth::isolated(Settings {
            issuer: Some(fixture.base.clone()),
            service_url: Some(fixture.base.clone()),
            client_id: Some("fixture-public-client".into()),
            update_public_key: None,
            tokenone_portal_url: None,
            endpoints: [(
                "codex".into(),
                (format!("{}/v1", fixture.base), "fixture-model".into()),
            )]
            .into(),
        })
    }
    async fn seed(auth: &Auth, expired: bool) {
        *auth.session.lock().await = Some(Session {
            service_binding: service_binding(&auth.settings),
            account: Account {
                id: "fixture-user".into(),
                name: "测试用户".into(),
            },
            access_token: "fixture-user-token".into(),
            refresh_token: Some("fixture-refresh-token".into()),
            expires_at: if expired { 0 } else { now() + 3600 },
        });
    }

    #[tokio::test]
    async fn device_login_polls_pending_and_saves_only_the_verified_user() {
        let f = fixture("login").await;
        let a = auth(&f);
        let account = a
            .login(
                |url| {
                    assert!(url.starts_with(&format!("{}/confirm/ABCD?", f.base)));
                    Ok(())
                },
                |text| {
                    assert!(text.contains("ABCD"));
                    assert!(!text.contains("private-device-code"));
                },
            )
            .await
            .unwrap();
        assert_eq!(account.id, "fixture-user");
        assert_eq!(a.account().await.unwrap().name, "测试用户");
        assert_eq!(
            f.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|(path, _)| path == "/token")
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn device_login_honors_slow_down() {
        let f = fixture("slow").await;
        let a = auth(&f);
        let start = Instant::now();
        a.login(|_| Ok(()), |_| {}).await.unwrap();
        assert!(start.elapsed() >= Duration::from_secs(7));
    }

    #[tokio::test]
    async fn device_denial_expiry_and_cancellation_do_not_save_a_session() {
        for scenario in ["denied", "expired", "cancel"] {
            let f = fixture(scenario).await;
            let a = auth(&f);
            let result = a
                .login(
                    |_| {
                        if scenario == "cancel" {
                            a.cancel();
                        }
                        Ok(())
                    },
                    |_| {},
                )
                .await;
            assert!(result.is_err());
            assert!(a.account().await.is_none());
        }
    }

    #[tokio::test]
    async fn rejects_cross_origin_verification_before_opening_browser() {
        let f = fixture("unsafe-url").await;
        let a = auth(&f);
        assert!(a
            .login(|_| panic!("must not open untrusted URL"), |_| {})
            .await
            .is_err());
    }

    #[tokio::test]
    async fn refreshes_without_a_client_secret_then_reads_personal_key() {
        let f = fixture("refresh").await;
        let a = auth(&f);
        seed(&a, true).await;
        let key = a
            .credential(
                "codex",
                |_| panic!("existing grant needs no browser"),
                |_| {},
            )
            .await
            .unwrap();
        assert_eq!(key.key, "sk-fixture-model");
        assert_eq!(key.model, "fixture-model");
        assert_eq!(
            a.session
                .lock()
                .await
                .as_ref()
                .unwrap()
                .refresh_token
                .as_deref(),
            Some("fixture-new-refresh")
        );
    }

    #[tokio::test]
    async fn first_model_authorization_opens_browser_and_polls_without_exposing_key() {
        let f = fixture("authorization").await;
        let a = auth(&f);
        seed(&a, false).await;
        let key = a
            .credential(
                "codex",
                |url| {
                    assert!(url.starts_with(&format!("{}/authorize/", f.base)));
                    Ok(())
                },
                |text| assert!(!text.contains("sk-")),
            )
            .await
            .unwrap();
        assert_eq!(key.key, "sk-fixture-model");
    }

    #[tokio::test]
    async fn quota_errors_do_not_open_browser_or_expose_raw_responses() {
        let f = fixture("quota").await;
        let a = auth(&f);
        seed(&a, false).await;
        let error = a
            .credential(
                "codex",
                |_| panic!("quota error is not authorization"),
                |_| {},
            )
            .await
            .err()
            .unwrap();
        assert!(error.to_string().contains("额度"));
        let safe = serde_json::to_value(&error).unwrap();
        assert_eq!(safe["action"], "portal");
        assert!(!safe.to_string().contains("must not escape"));
    }

    #[tokio::test]
    async fn missing_login_has_explicit_login_recovery() {
        let f = fixture("credentials").await;
        let a = auth(&f);
        let error = a
            .credential("codex", |_| panic!("must not open a browser"), |_| {})
            .await
            .err()
            .unwrap();
        assert_eq!(error.recovery_action(), Some(RecoveryAction::Login));
        assert!(f.calls.lock().unwrap().is_empty());
    }
}
