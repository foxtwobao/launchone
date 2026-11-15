pub mod adapters;
pub mod auth;
#[cfg(any(feature = "desktop", test))]
mod client_config;
pub mod config;
pub mod desktop_app;
pub mod error;
pub mod installers;
pub mod macho;
pub mod models;
pub mod preferences;
pub mod process;
pub mod service;
pub mod settings;
mod vendor;

#[cfg(feature = "desktop")]
mod desktop {
    use crate::error::{message, Result};
    use crate::models::{Account, DefaultFailure, Progress, Snapshot};
    use crate::service::Runtime;
    use crate::settings::Settings;
    use serde::Serialize;
    use std::sync::Arc;
    use tauri::{Emitter, Manager, State};
    use tauri_plugin_opener::OpenerExt;
    use tauri_plugin_updater::UpdaterExt;

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct UpdateInfo {
        version: String,
        notes: Option<String>,
        date: Option<String>,
    }

    fn updater(
        app: &tauri::AppHandle,
        settings: &Settings,
    ) -> Result<tauri_plugin_updater::Updater> {
        let base = settings
            .service_url
            .as_deref()
            .ok_or_else(|| message("更新服务尚未配置"))?;
        let public_key = settings
            .update_public_key
            .as_deref()
            .ok_or_else(|| message("更新服务尚未配置"))?;
        let endpoint = format!(
            "{}/v1/updates/{{{{target}}}}/{{{{arch}}}}/{{{{current_version}}}}",
            base.trim_end_matches('/')
        )
        .parse()
        .map_err(|_| message("更新服务地址无效"))?;
        app.updater_builder()
            .pubkey(public_key)
            .timeout(std::time::Duration::from_secs(30))
            .endpoints(vec![endpoint])
            .map_err(|_| message("更新服务配置无效"))?
            .build()
            .map_err(|_| message("无法初始化更新服务"))
    }

    #[tauri::command]
    async fn check_update(
        app: tauri::AppHandle,
        runtime: State<'_, Arc<Runtime>>,
    ) -> Result<Option<UpdateInfo>> {
        if !runtime
            .settings
            .update_public_key
            .as_deref()
            .is_some_and(|key| !key.trim().is_empty())
        {
            return Err(message("更新服务尚未配置"));
        }
        let update = updater(&app, &runtime.settings)?
            .check()
            .await
            .map_err(|_| message("暂时无法检查更新"))?;
        Ok(update.map(|update| UpdateInfo {
            version: update.version,
            notes: update.body,
            date: update.date.map(|date| date.to_string()),
        }))
    }

    #[tauri::command]
    async fn install_update(app: tauri::AppHandle, runtime: State<'_, Arc<Runtime>>) -> Result<()> {
        let update = updater(&app, &runtime.settings)?
            .check()
            .await
            .map_err(|_| message("暂时无法检查更新"))?
            .ok_or_else(|| message("当前已是最新版本"))?;
        update
            .download_and_install(|_, _| {}, || {})
            .await
            .map_err(|_| message("更新安装失败，请稍后重试"))?;
        app.restart()
    }

    #[tauri::command]
    async fn get_snapshot(runtime: State<'_, Arc<Runtime>>) -> Result<Snapshot> {
        Ok(runtime.snapshot().await)
    }

    #[tauri::command]
    async fn open_tokenone_portal(
        app: tauri::AppHandle,
        runtime: State<'_, Arc<Runtime>>,
    ) -> Result<()> {
        let url = runtime.settings.tokenone_portal_url()?;
        app.opener()
            .open_url(url.as_str(), None::<&str>)
            .map_err(|_| message("无法打开 TokenOne 账户页面，请检查系统浏览器"))
    }

    #[tauri::command]
    async fn login(app: tauri::AppHandle, runtime: State<'_, Arc<Runtime>>) -> Result<Account> {
        let _operation = runtime
            .operation
            .try_lock()
            .map_err(|_| message("另一项操作正在进行，请稍后再试"))?;
        runtime
            .auth
            .login(
                |url| {
                    app.opener()
                        .open_url(url, None::<&str>)
                        .map_err(|_| message("无法打开系统浏览器"))
                },
                |text| {
                    let _ = app.emit(
                        "setup-progress",
                        Progress {
                            app_id: String::new(),
                            stage: "login",
                            message: text.into(),
                        },
                    );
                },
            )
            .await
    }

    #[tauri::command]
    fn cancel_login(runtime: State<'_, Arc<Runtime>>) {
        runtime.auth.cancel();
    }

    #[tauri::command]
    async fn logout(runtime: State<'_, Arc<Runtime>>) -> Result<()> {
        let _operation = runtime
            .operation
            .try_lock()
            .map_err(|_| message("另一项操作正在进行，请稍后再试"))?;
        runtime.auth.logout().await
    }

    #[tauri::command]
    async fn enable_app(
        app: tauri::AppHandle,
        runtime: State<'_, Arc<Runtime>>,
        app_id: String,
    ) -> Result<()> {
        runtime
            .enable(
                &app_id,
                |url| {
                    app.opener()
                        .open_url(url, None::<&str>)
                        .map_err(|_| message("无法打开系统浏览器"))
                },
                |p| {
                    let _ = app.emit("setup-progress", p);
                },
            )
            .await
    }

    #[tauri::command]
    async fn use_official(
        app: tauri::AppHandle,
        runtime: State<'_, Arc<Runtime>>,
        app_id: String,
    ) -> Result<()> {
        runtime
            .official(&app_id, |p| {
                let _ = app.emit("setup-progress", p);
            })
            .await
    }

    #[tauri::command]
    async fn restore_app(runtime: State<'_, Arc<Runtime>>, app_id: String) -> Result<()> {
        runtime.restore(&app_id).await
    }

    #[tauri::command]
    async fn open_app(
        runtime: State<'_, Arc<Runtime>>,
        app_id: String,
    ) -> Result<crate::models::OpenOutcome> {
        runtime.open(&app_id).await
    }

    #[tauri::command]
    async fn reopen_app(
        app: tauri::AppHandle,
        runtime: State<'_, Arc<Runtime>>,
        app_id: String,
    ) -> Result<()> {
        runtime
            .reopen(&app_id, |p| {
                let _ = app.emit("setup-progress", p);
            })
            .await
    }

    #[tauri::command]
    async fn install_app(
        app: tauri::AppHandle,
        runtime: State<'_, Arc<Runtime>>,
        app_id: String,
    ) -> Result<bool> {
        runtime
            .install(
                &app_id,
                |url| {
                    app.opener()
                        .open_url(url, None::<&str>)
                        .map_err(|_| message("无法打开官方下载页面"))
                },
                |p| {
                    let _ = app.emit("setup-progress", p);
                },
            )
            .await
    }

    #[tauri::command]
    async fn prepare_defaults(
        app: tauri::AppHandle,
        runtime: State<'_, Arc<Runtime>>,
    ) -> Result<Vec<DefaultFailure>> {
        runtime
            .defaults(
                |url| {
                    app.opener()
                        .open_url(url, None::<&str>)
                        .map_err(|_| message("无法打开浏览器"))
                },
                |p| {
                    let _ = app.emit("setup-progress", p);
                },
            )
            .await
    }

    pub fn run() {
        tauri::Builder::default()
            .plugin(tauri_plugin_opener::init())
            .plugin(tauri_plugin_updater::Builder::new().build())
            .setup(|app| {
                let home = dirs::home_dir().ok_or("无法确定用户目录")?;
                // Both builds manage the same Codex/Claude files. Share their
                // original backups and config lock, including backups from 0.2.3.
                let data = app.path().local_data_dir()?.join("com.tokenone.launchone");
                // Resolve public config before Auth reads a session: issuer and
                // client ID form part of the saved login's service binding.
                let settings = tauri::async_runtime::block_on(crate::client_config::load(
                    Settings::from_environment(),
                    &data,
                    option_env!("LAUNCHONE_BUILD_ENV").unwrap_or("production"),
                ));
                app.manage(Arc::new(Runtime::new(home, data, settings)));
                Ok(())
            })
            .invoke_handler(tauri::generate_handler![
                get_snapshot,
                open_tokenone_portal,
                login,
                cancel_login,
                logout,
                enable_app,
                use_official,
                restore_app,
                open_app,
                reopen_app,
                install_app,
                prepare_defaults,
                check_update,
                install_update
            ])
            .run(tauri::generate_context!())
            .expect("无法启动 LaunchOne 桌面窗口");
    }
}

#[cfg(feature = "desktop")]
pub use desktop::run;
