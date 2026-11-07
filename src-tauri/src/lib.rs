pub mod adapters;
pub mod auth;
pub mod config;
pub mod error;
pub mod models;
pub mod process;
pub mod service;
pub mod settings;
mod vendor;

#[cfg(feature = "desktop")]
mod desktop {
    use crate::error::{message, Result};
    use crate::models::{Account, Progress, Snapshot};
    use crate::service::Runtime;
    use crate::settings::Settings;
    use std::sync::Arc;
    use tauri::{Emitter, Manager, State};
    use tauri_plugin_opener::OpenerExt;

    #[tauri::command]
    async fn get_snapshot(runtime: State<'_, Arc<Runtime>>) -> Result<Snapshot> {
        Ok(runtime.snapshot().await)
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

    pub fn run() {
        tauri::Builder::default()
            .plugin(tauri_plugin_opener::init())
            .setup(|app| {
                let home = dirs::home_dir().ok_or("无法确定用户目录")?;
                let data = app.path().app_local_data_dir()?;
                app.manage(Arc::new(Runtime::new(
                    home,
                    data,
                    Settings::from_environment(),
                )));
                Ok(())
            })
            .invoke_handler(tauri::generate_handler![
                get_snapshot,
                login,
                cancel_login,
                logout,
                enable_app,
                use_official,
                restore_app
            ])
            .run(tauri::generate_context!())
            .expect("无法启动 LaunchOne 桌面窗口");
    }
}

#[cfg(feature = "desktop")]
pub use desktop::run;
