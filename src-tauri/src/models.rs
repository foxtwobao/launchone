use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Tokenone,
    Official,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OpenOutcome {
    Opened,
    AlreadyRunning,
    RestartRequired,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppManifest {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub command: &'static str,
    pub icon: &'static str,
    pub supports_official: bool,
    pub platforms: &'static [&'static str],
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub manifest: AppManifest,
    pub installed: bool,
    pub tokenone_supported: bool,
    pub restart_required: bool,
    pub version: Option<String>,
    pub mode: Option<Mode>,
    pub can_restore: bool,
    pub supported: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    #[serde(alias = "sub")]
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub apps: Vec<AppStatus>,
    pub account: Option<Account>,
    pub login_ready: bool,
    pub tokenone_ready: bool,
    pub desktop: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultFailure {
    pub app_id: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub app_id: String,
    pub stage: &'static str,
    pub message: String,
}

// Deliberately not Serialize/Debug: secrets never cross the IPC boundary or enter logs.
pub struct Credential {
    pub key: String,
    pub base_url: String,
    pub model: String,
}
