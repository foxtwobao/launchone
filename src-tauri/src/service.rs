use crate::adapters::{ensure_supported, AppAdapter, Context, Registry};
use crate::auth::Auth;
use crate::config::{read_value, ConfigStore};
use crate::error::{message, Result};
use crate::models::{AppStatus, Credential, Mode, Progress, Snapshot};
use crate::process;
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
        }
    }

    pub async fn snapshot(&self) -> Snapshot {
        let mut apps = Vec::new();
        for adapter in self.registry.all() {
            let installed = process::detect(adapter, &self.context.home).await;
            let endpoint = self
                .settings
                .endpoints
                .get(adapter.manifest().id)
                .map(|(u, _)| u.as_str());
            let config = adapter
                .path(&self.context)
                .and_then(|p| read_value(&p, adapter.format()));
            let mode = config.as_ref().ok().and_then(|v| adapter.mode(v, endpoint));
            apps.push(AppStatus {
                manifest: adapter.manifest(),
                installed: installed.is_some(),
                version: installed.map(|(_, v)| v),
                mode,
                can_restore: self.store.can_restore(adapter.manifest().id),
                supported: ensure_supported(adapter).is_ok(),
                error: config.err().map(|e| e.to_string()),
            });
        }
        Snapshot {
            apps,
            account: self.auth.account().await,
            login_ready: self.settings.login_ready(),
            tokenone_ready: self.settings.tokenone_ready(),
            desktop: true,
        }
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
        progress("credentials", "正在连接 TokenOne");
        // Resolve credentials before invoking an installer; an expired account must
        // not leave an unexpected partial installation.
        let credential = self
            .auth
            .credential(app, open_browser, |text| progress("authorization", text))
            .await?;
        progress("detecting", "正在检测工具");
        self.ensure_installed(adapter, &progress).await?;
        progress("configuring", "正在保存接入配置");
        let path = adapter.path(&self.context)?;
        let receipt = self.store.apply(
            app,
            &path,
            adapter.format(),
            adapter.fields(Mode::Tokenone, Some(&credential))?,
        )?;
        progress("verifying", "正在验证连接");
        let verified = self.verify(adapter, &credential).await;
        if let Err(error) = verified {
            self.store.rollback(app, &path, adapter.format(), receipt)?;
            return Err(message(format!("{error}；已撤销本次配置修改")));
        }
        progress("done", "已连接 TokenOne");
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
            return Err(message("该工具不提供官方登录"));
        }
        adapter.check_conflicts(Mode::Official)?;
        let progress = |stage, text: &str| {
            emit(Progress {
                app_id: app.into(),
                stage,
                message: text.into(),
            })
        };
        progress("detecting", "正在检测工具");
        let executable = self.ensure_installed(adapter, &progress).await?;
        progress("configuring", "正在切换为官方登录");
        let path = adapter.path(&self.context)?;
        let receipt = self.store.apply(
            app,
            &path,
            adapter.format(),
            adapter.fields(Mode::Official, None)?,
        )?;
        progress("official_login", "请在浏览器中完成官方登录");
        if let Err(error) = process::official_login(adapter, &executable).await {
            self.store.rollback(app, &path, adapter.format(), receipt)?;
            return Err(message(format!("{error}；已撤销本次配置修改")));
        }
        progress("done", "已切换为官方登录，请重新打开工具");
        Ok(())
    }

    pub async fn restore(&self, app: &str) -> Result<()> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| message("另一项操作正在进行，请稍后再试"))?;
        let adapter = self.registry.get(app)?;
        self.store.restore(app, &adapter.path(&self.context)?)
    }

    async fn ensure_installed(
        &self,
        adapter: &dyn AppAdapter,
        emit: &impl Fn(&'static str, &str),
    ) -> Result<PathBuf> {
        if let Some((path, _)) = process::detect(adapter, &self.context.home).await {
            return Ok(path);
        }
        emit("installing", "正在通过官方渠道安装");
        process::install(adapter).await?;
        process::detect(adapter, &self.context.home)
            .await
            .map(|(path, _)| path)
            .ok_or_else(|| message("安装程序已结束，但尚未检测到可用工具，请重试或检查安装结果"))
    }

    async fn verify(&self, adapter: &dyn AppAdapter, c: &Credential) -> Result<()> {
        // Re-read the config so a successful gateway response alone cannot make a
        // concurrent or ineffective file update look successful.
        let value = read_value(&adapter.path(&self.context)?, adapter.format())?;
        if adapter.mode(&value, Some(&c.base_url)) != Some(Mode::Tokenone) {
            return Err(message("接入配置未生效"));
        }
        let (url, body) = adapter.verify_request(c);
        let mut request = http_client()?.post(url).json(&body);
        for (header, value) in adapter.request_headers(&c.key) {
            request = request.header(header, value);
        }
        let response = request
            .send()
            .await
            .map_err(|_| message("无法连接 TokenOne，请检查网络"))?;
        match response.status().as_u16() {
            200..=299 => {}
            401 | 403 => return Err(message("TokenOne 调用权限不足，请重新登录或检查账户")),
            429 => return Err(message("TokenOne 额度不足或请求受限，请稍后重试")),
            _ => return Err(message("TokenOne 暂时无法处理请求，请稍后重试")),
        }
        let value: Value = response
            .json()
            .await
            .map_err(|_| message("TokenOne 响应格式与工具不兼容"))?;
        if !adapter.valid_response(&value) {
            return Err(message("TokenOne 未返回有效的模型响应"));
        }
        Ok(())
    }
}
