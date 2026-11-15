use crate::adapters::{ensure_supported, AppAdapter, Context, Registry};
use crate::auth::Auth;
use crate::config::ConfigStore;
use crate::desktop_app;
use crate::error::{actionable, message, RecoveryAction, Result};
use crate::models::{AppStatus, Credential, DefaultFailure, Mode, OpenOutcome, Progress, Snapshot};
use crate::preferences::Preferences;
use crate::settings::{http_client, Settings};
use serde_json::Value;
use std::path::PathBuf;
use tokio::sync::Mutex;

pub struct Runtime {
    pub registry: Registry,
    pub settings: Settings,
    pub auth: Auth,
    pub operation: Mutex<()>,
    context: Context,
    store: ConfigStore,
    preferences: Preferences,
    installer_cache: PathBuf,
}
impl Runtime {
    pub fn new(home: PathBuf, data: PathBuf, settings: Settings) -> Self {
        Self {
            registry: Registry::default(),
            auth: Auth::new(settings.clone()),
            settings,
            operation: Mutex::new(()),
            context: Context::new(home),
            store: ConfigStore::new(data.join("backups")),
            preferences: Preferences::new(data.join("preferences.json")),
            installer_cache: data.join("installers"),
        }
    }
    pub async fn snapshot(&self) -> Snapshot {
        let account = self.auth.account().await;
        let recovery_error = if let Ok(_operation) = self.operation.try_lock() {
            self.store.recover().err().map(|e| e.to_string())
        } else {
            None
        };
        let mut apps = Vec::new();
        for adapter in self.registry.all().filter(|a| a.desktop().is_some()) {
            let spec = adapter.desktop().unwrap();
            let detected = desktop_app::detect(spec, &self.context.home).await;
            let detection_error = detected.as_ref().err().map(|e| e.to_string());
            let installed = detected.ok().flatten();
            let endpoint = self
                .settings
                .endpoints
                .get(adapter.manifest().id)
                .map(|(url, _)| url.as_str());
            let mode = adapter.current_mode(&self.context, endpoint);
            let credential_account =
                if mode.as_ref().ok().copied().flatten() == Some(Mode::Tokenone) {
                    account
                        .as_ref()
                        .map(|account| {
                            self.preferences.credential_account(
                                adapter.manifest().id,
                                self.settings.issuer.as_deref().unwrap_or_default(),
                                &account.id,
                            )
                        })
                        .transpose()
                } else {
                    Ok(None)
                };
            let restart = self.preferences.needs_restart(adapter.manifest().id);
            let backup = adapter
                .targets(&self.context)
                .map(|t| t.iter().any(|(id, _)| self.store.can_restore(id)))
                .unwrap_or(false);
            apps.push(AppStatus {
                manifest: adapter.manifest(),
                installed: installed.is_some(),
                tokenone_supported: installed.as_ref().is_none_or(|a| a.gateway_supported),
                restart_required: restart.as_ref().is_ok_and(|v| *v),
                version: installed.and_then(|a| a.version),
                mode: mode.as_ref().ok().copied().flatten(),
                credential_account: credential_account.as_ref().ok().copied().flatten(),
                can_restore: backup,
                supported: ensure_supported(adapter).is_ok(),
                error: recovery_error
                    .clone()
                    .or(detection_error)
                    .or_else(|| credential_account.err().map(|e| e.to_string()))
                    .or_else(|| restart.err().map(|e| e.to_string()))
                    .or_else(|| mode.err().map(|e| e.to_string())),
            });
        }
        Snapshot {
            apps,
            account,
            login_ready: self.settings.login_ready(),
            tokenone_ready: self.settings.tokenone_ready(),
            tokenone_portal_ready: self.settings.tokenone_portal_url().is_ok(),
            desktop: true,
        }
    }

    pub async fn defaults(
        &self,
        open_browser: impl Fn(&str) -> Result<()>,
        emit: impl Fn(Progress),
    ) -> Result<Vec<DefaultFailure>> {
        if self.auth.account().await.is_none() || !self.settings.tokenone_ready() {
            return Ok(Vec::new());
        }
        let mut failures = Vec::new();
        for adapter in self.registry.all().filter(|a| a.desktop().is_some()) {
            let id = adapter.manifest().id;
            let has_backup = adapter
                .targets(&self.context)?
                .iter()
                .any(|(key, _)| self.store.can_restore(key));
            if !self.preferences.wants_default(id, has_backup)?
                || ensure_supported(adapter).is_err()
            {
                continue;
            }
            let endpoint = self.settings.endpoints.get(id).map(|(url, _)| url.as_str());
            if adapter.current_mode(&self.context, endpoint)? == Some(Mode::Tokenone) {
                // A pre-existing key may belong to someone else. Only an explicit
                // reconnect can replace it and associate it with the current user.
                continue;
            }
            // Only already-installed applications are prepared. Login never installs
            // unrelated apps or overwrites a remembered subscription/restored choice.
            if !desktop_app::detect(adapter.desktop().unwrap(), &self.context.home)
                .await?
                .is_some_and(|a| a.gateway_supported)
            {
                continue;
            }
            if let Err(error) = self.enable(id, |u| open_browser(u), &emit).await {
                let cancelled = error.to_string().contains("已取消操作");
                failures.push(DefaultFailure {
                    app_id: id.into(),
                    message: error.to_string(),
                    action: error.recovery_action(),
                });
                if cancelled {
                    break;
                }
            }
        }
        Ok(failures)
    }

    pub async fn open(&self, app: &str) -> Result<OpenOutcome> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| message("另一项操作正在进行，请稍后再试"))?;
        let adapter = self.registry.get(app)?;
        ensure_supported(adapter)?;
        desktop_app::open(
            adapter
                .desktop()
                .ok_or_else(|| message("此工具没有桌面入口"))?,
            &self.context.home,
            self.preferences.needs_restart(app)?,
        )
        .await
    }

    // Only the explicit confirmation action invokes this command. Never kill a
    // process; an app that refuses a normal quit must be handled by the user.
    pub async fn reopen(&self, app: &str, emit: impl Fn(Progress)) -> Result<()> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| message("另一项操作正在进行，请稍后再试"))?;
        let adapter = self.registry.get(app)?;
        ensure_supported(adapter)?;
        emit(Progress {
            app_id: app.into(),
            stage: "reopening",
            message: "正在重新打开应用".into(),
        });
        desktop_app::reopen(
            adapter
                .desktop()
                .ok_or_else(|| message("此工具没有桌面入口"))?,
            &self.context.home,
        )
        .await?;
        self.preferences.opened(app)
    }

    pub async fn install(
        &self,
        app: &str,
        _open_browser: impl FnOnce(&str) -> Result<()>,
        emit: impl Fn(Progress),
    ) -> Result<bool> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| message("另一项操作正在进行，请稍后再试"))?;
        let adapter = self.registry.get(app)?;
        ensure_supported(adapter)?;
        let spec = adapter
            .desktop()
            .ok_or_else(|| message("此工具没有桌面安装入口"))?;
        if desktop_app::detect(spec, &self.context.home)
            .await?
            .is_some()
        {
            return Ok(true);
        }
        let progress = |stage, text: String| {
            emit(Progress {
                app_id: app.into(),
                stage,
                message: text,
            })
        };
        progress("detecting", "正在选择适合当前系统和芯片的安装包".into());
        let target = crate::installers::native_target().await?;
        let downloaded = crate::installers::download(
            &self.settings,
            app,
            &self.installer_cache,
            &target,
            &progress,
        )
        .await?;
        progress("installing", "正在验证官方签名并安装".into());
        if target.platform == "macos" {
            desktop_app::install_mac(
                spec,
                &self.context.home,
                &downloaded.path,
                &downloaded.manifest,
                &target,
            )
            .await?;
        } else {
            crate::installers::install_windows(spec, &downloaded, &target).await?;
        }
        let installed = desktop_app::detect(spec, &self.context.home)
            .await?
            .is_some();
        if !installed {
            return Err(actionable(
                "安装尚未完成。若系统要求重启，请重启后重新检测",
                RecoveryAction::Retry,
            ));
        }
        downloaded.cleanup();
        Ok(true)
    }

    pub async fn enable(
        &self,
        app: &str,
        open_browser: impl FnOnce(&str) -> Result<()>,
        emit: impl Fn(Progress),
    ) -> Result<()> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| message("另一项操作正在进行，请稍后再试"))?;
        let adapter = self.registry.get(app)?;
        ensure_supported(adapter)?;
        adapter.check_conflicts(Mode::Tokenone)?;
        let progress = |stage, text: &str| {
            emit(Progress {
                app_id: app.into(),
                stage,
                message: text.into(),
            })
        };
        progress("detecting", "正在检测桌面应用");
        if let Some(spec) = adapter.desktop() {
            let installed = desktop_app::detect(spec, &self.context.home)
                .await?
                .ok_or_else(|| message("请先安装桌面应用"))?;
            if !installed.gateway_supported {
                return Err(message(
                    "当前桌面版本未检测到 Codex 网关支持，请升级官方桌面应用",
                ));
            }
        }
        progress("credentials", "正在连接 TokenOne");
        let c = self
            .auth
            .credential(app, open_browser, |text| progress("authorization", text))
            .await?;
        let account =
            self.auth.account().await.ok_or_else(|| {
                actionable("请先登录 IDONE，再连接 TokenOne", RecoveryAction::Login)
            })?;
        progress("configuring", "正在备份并保存接入配置");
        self.store.recover()?;
        let preference_checkpoint = self.preferences.checkpoint()?;
        let batch = self.store.begin(
            app,
            adapter
                .changes(&self.context, Mode::Tokenone, Some(&c))
                .await?,
        )?;
        progress("verifying", "正在验证连接");
        if let Err(e) = self.verify(adapter, &c).await.and_then(|_| {
            self.preferences.remember_tokenone(
                app,
                self.settings.issuer.as_deref().unwrap_or_default(),
                &account.id,
            )
        }) {
            self.store.revert(batch)?;
            let text = format!("{e}；已撤销本次配置修改");
            return Err(match e.recovery_action() {
                Some(action) => actionable(text, action),
                None => message(text),
            });
        }
        if let Err(e) = self.store.commit(batch) {
            self.store.recover()?;
            self.preferences.rollback(preference_checkpoint)?;
            return Err(e);
        }
        progress("done", "已切换到 TokenOne，请重新打开应用");
        Ok(())
    }

    pub async fn official(&self, app: &str, emit: impl Fn(Progress)) -> Result<()> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| message("另一项操作正在进行，请稍后再试"))?;
        let adapter = self.registry.get(app)?;
        ensure_supported(adapter)?;
        if !adapter.manifest().supports_official {
            return Err(message("该应用不提供官方订阅方式"));
        }
        adapter.check_conflicts(Mode::Official)?;
        emit(Progress {
            app_id: app.into(),
            stage: "configuring",
            message: "正在切回官方订阅".into(),
        });
        self.store.recover()?;
        let preference_checkpoint = self.preferences.checkpoint()?;
        let batch = self.store.begin(
            app,
            adapter.changes(&self.context, Mode::Official, None).await?,
        )?;
        let verified = (|| {
            if adapter.current_mode(&self.context, None)? != Some(Mode::Official) {
                return Err(message("官方接入配置未生效"));
            }
            self.preferences.remember(app, Some(Mode::Official))
        })();
        if let Err(e) = verified {
            self.store.revert(batch)?;
            return Err(e);
        }
        if let Err(e) = self.store.commit(batch) {
            self.store.recover()?;
            self.preferences.rollback(preference_checkpoint)?;
            return Err(e);
        }
        Ok(())
    }

    pub async fn restore(&self, app: &str) -> Result<()> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| message("另一项操作正在进行，请稍后再试"))?;
        let adapter = self.registry.get(app)?;
        self.store.recover()?;
        let changes = self
            .store
            .restore_changes(adapter.targets(&self.context)?)?;
        let preference_checkpoint = self.preferences.checkpoint()?;
        let batch = self.store.begin_restore(app, changes)?;
        if let Err(e) = self.preferences.remember(app, None) {
            self.store.revert(batch)?;
            return Err(e);
        }
        if let Err(e) = self.store.finish_restore(batch) {
            self.store.recover()?;
            self.preferences.rollback(preference_checkpoint)?;
            return Err(e);
        }
        Ok(())
    }

    async fn verify(&self, adapter: &dyn AppAdapter, c: &Credential) -> Result<()> {
        if adapter.current_mode(&self.context, Some(&c.base_url))? != Some(Mode::Tokenone) {
            return Err(message("接入配置未生效"));
        }
        let (url, body) = adapter.verify_request(c);
        let mut request = http_client()?.post(url).json(&body);
        for (header, value) in adapter.request_headers(&c.key) {
            request = request.header(header, value);
        }
        let response = request.send().await.map_err(|_| {
            actionable("无法连接 TokenOne，请检查网络后重试", RecoveryAction::Retry)
        })?;
        match response.status().as_u16() {
            200..=299 => {}
            401 | 403 => {
                return Err(actionable(
                    "TokenOne 调用权限不足，请检查账户或重新连接",
                    RecoveryAction::Portal,
                ))
            }
            429 => {
                return Err(actionable(
                    "TokenOne 请求受限，请稍后重试；若持续失败请检查账户额度",
                    RecoveryAction::Retry,
                ))
            }
            _ => {
                return Err(actionable(
                    "TokenOne 暂时无法处理请求，请稍后重试",
                    RecoveryAction::Retry,
                ))
            }
        }
        let value: Value = response
            .json()
            .await
            .map_err(|_| message("TokenOne 响应格式与应用不兼容"))?;
        if !adapter.valid_response(&value) {
            return Err(message("TokenOne 未返回有效的模型响应"));
        }
        Ok(())
    }
}
