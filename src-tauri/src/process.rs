use crate::adapters::AppAdapter;
use crate::error::{message, Result};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

fn candidate_paths(command: &str, home: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let dirs =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect::<Vec<_>>();
    for directory in dirs.into_iter().chain([
        home.join(".local/bin"),
        home.join(".cargo/bin"),
        home.join(".npm-global/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        home.join("AppData/Roaming/npm"),
        home.join("AppData/Local/Microsoft/WinGet/Links"),
    ]) {
        #[cfg(windows)]
        for extension in ["exe", "cmd", "bat"] {
            paths.push(directory.join(format!("{command}.{extension}")));
        }
        #[cfg(not(windows))]
        paths.push(directory.join(command));
    }
    paths
}

pub async fn detect(adapter: &dyn AppAdapter, home: &Path) -> Option<(PathBuf, String)> {
    for path in candidate_paths(adapter.manifest().command, home) {
        if !path.is_file() {
            continue;
        }
        let mut command = make_command(&path);
        command.arg("--version").kill_on_drop(true);
        let Ok(Ok(output)) = tokio::time::timeout(Duration::from_secs(8), command.output()).await
        else {
            continue;
        };
        if output.status.success() {
            let version = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or_default()
                .chars()
                .take(100)
                .collect();
            return Some((path, version));
        }
    }
    None
}

fn make_command(path: &Path) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut command = if matches!(
            path.extension().and_then(|s| s.to_str()),
            Some("cmd" | "bat")
        ) {
            let mut c = Command::new("cmd.exe");
            c.args(["/D", "/C"]).arg(path);
            c
        } else {
            Command::new(path)
        };
        command.as_std_mut().creation_flags(0x08000000);
        command
    }
    #[cfg(not(windows))]
    {
        Command::new(path)
    }
}

pub async fn install(adapter: &dyn AppAdapter) -> Result<()> {
    let script = adapter.install_script(std::env::consts::OS)?;
    let temporary = tempfile::tempdir()?;
    let mut command = if cfg!(windows) {
        let mut c = Command::new("powershell.exe");
        c.args(["-NoProfile", "-NonInteractive", "-Command", script]);
        c
    } else {
        let mut c = Command::new("/bin/sh");
        c.args(["-c", script, "launchone-install"])
            .arg(temporary.path());
        c
    };
    command.kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(600), command.output())
        .await
        .map_err(|_| message("安装等待超时，请检查网络后重试"))??;
    if !output.status.success() {
        return Err(message("官方安装程序未完成，请检查网络和系统权限后重试"));
    }
    Ok(())
}

pub async fn official_login(adapter: &dyn AppAdapter, executable: &Path) -> Result<()> {
    if has_official_session(adapter, executable).await {
        return Ok(());
    }
    let output = tokio::time::timeout(
        Duration::from_secs(180),
        make_command(executable)
            .args(adapter.official_arguments())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| message("官方登录等待超时，请重试"))??;
    if !output.status.success() || !has_official_session(adapter, executable).await {
        return Err(message(
            "尚未检测到有效的官方登录，请重试或在原工具中完成登录",
        ));
    }
    Ok(())
}

async fn has_official_session(adapter: &dyn AppAdapter, executable: &Path) -> bool {
    let Ok(Ok(output)) = tokio::time::timeout(
        Duration::from_secs(10),
        make_command(executable)
            .args(adapter.official_status_arguments())
            .kill_on_drop(true)
            .output(),
    )
    .await
    else {
        return false;
    };
    output.status.success()
        && adapter.official_session(
            &String::from_utf8_lossy(&output.stdout),
            &String::from_utf8_lossy(&output.stderr),
        )
}
