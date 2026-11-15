//! Native app discovery and launching. No CLI or config-directory heuristics.
use crate::error::{message, Result};
use crate::models::OpenOutcome;
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::process::Command;

#[derive(Clone, Copy)]
pub struct DesktopSpec {
    pub name: &'static str,
    pub bundle_id: &'static str,
    pub windows_package: &'static str,
    pub windows_exe: &'static str,
}

pub const CODEX: DesktopSpec = DesktopSpec {
    name: "Codex",
    bundle_id: "com.openai.codex",
    windows_package: "OpenAI.Codex",
    windows_exe: "Codex.exe",
};
pub const CLAUDE: DesktopSpec = DesktopSpec {
    name: "Claude",
    bundle_id: "com.anthropic.claudefordesktop",
    windows_package: "Claude",
    windows_exe: "Claude.exe",
};

pub struct Installation {
    pub target: PathBuf,
    pub version: Option<String>,
    pub packaged: bool,
    pub gateway_supported: bool,
}

// Parse XML or binary plist without starting the app. Legacy ChatGPT bundles
// can be opened but only a bundled Codex engine enables gateway configuration.
pub fn mac_installation(spec: DesktopSpec, app: &Path) -> Option<Installation> {
    let value = plist::Value::from_file(app.join("Contents/Info.plist")).ok()?;
    let d = value.as_dictionary()?;
    let identity = d.get("CFBundleIdentifier")?.as_string()?;
    if identity != spec.bundle_id && !(spec.name == "Codex" && identity == "com.openai.chat") {
        return None;
    }
    let gateway_supported =
        identity == spec.bundle_id || has_codex_engine(&app.join("Contents/Resources"));
    let executable = d.get("CFBundleExecutable")?.as_string()?;
    if Path::new(executable).components().count() != 1
        || !app.join("Contents/MacOS").join(executable).is_file()
    {
        return None;
    }
    Some(Installation {
        target: app.into(),
        version: d
            .get("CFBundleShortVersionString")
            .and_then(plist::Value::as_string)
            .map(str::to_string),
        packaged: false,
        gateway_supported,
    })
}

fn has_codex_engine(resources: &Path) -> bool {
    [
        "codex-cli/CodexCLI.app/Contents/MacOS/codex",
        "codex",
        "codex/codex",
        "bin/codex",
        "app.asar.unpacked/node_modules/@openai/codex/bin/codex.js",
    ]
    .iter()
    .any(|p| resources.join(p).is_file())
}

/// Read the existing login through the official engine, including OS-native
/// credential backends. `login status` does not log in, refresh or log out.
pub async fn codex_has_account(home: &Path, config_home: &Path) -> Result<bool> {
    let app = detect(CODEX, home)
        .await?
        .ok_or_else(|| message("无法定位官方应用，未修改配置"))?;
    let engine = codex_engine(&app).await?;
    read_codex_account_status(Command::new(engine), config_home, Duration::from_secs(15)).await
}

async fn codex_engine(app: &Installation) -> Result<PathBuf> {
    if cfg!(target_os = "macos") {
        let resources = app.target.join("Contents/Resources");
        for name in [
            "codex-cli/CodexCLI.app/Contents/MacOS/codex",
            "codex",
            "codex/codex",
            "bin/codex",
        ] {
            let path = resources.join(name);
            if path.is_file() {
                return Ok(path);
            }
        }
    } else if cfg!(windows) {
        let root = if app.packaged {
            let mut command = Command::new("powershell.exe");
            command.args(["-NoProfile", "-NonInteractive", "-Command",
                "$ErrorActionPreference='Stop'; [Console]::OutputEncoding=New-Object System.Text.UTF8Encoding $false; $family=$env:LAUNCHONE_APP_TARGET.Split('!')[0]; $p=@(Get-AppxPackage | Where-Object {$_.PackageFamilyName -eq $family}); if($p.Count -ne 1){exit 1}; [Console]::Write($p[0].InstallLocation)"])
                .env("LAUNCHONE_APP_TARGET", &app.target).kill_on_drop(true);
            #[cfg(windows)]
            command.creation_flags(0x08000000);
            let output = tokio::time::timeout(Duration::from_secs(15), command.output())
                .await
                .map_err(|_| message("读取官方应用位置超时，未修改配置"))??;
            if !output.status.success() {
                return Err(message("无法定位官方应用，未修改配置"));
            }
            PathBuf::from(String::from_utf8_lossy(&output.stdout).trim())
        } else {
            app.target
                .parent()
                .ok_or_else(|| message("无法定位官方应用"))?
                .into()
        };
        if root.is_absolute() {
            let mut engines = Vec::new();
            find_codex_engines(&root, 6, &mut engines)?;
            if engines.len() == 1 {
                return Ok(engines.remove(0));
            }
        }
    }
    Err(message(
        "无法定位官方应用的登录组件，请更新官方应用后重试；未修改配置",
    ))
}

fn find_codex_engines(root: &Path, depth: usize, engines: &mut Vec<PathBuf>) -> Result<()> {
    if depth == 0 {
        return Ok(());
    }
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        // Stay inside the detected installation; do not follow links/junctions.
        if entry.path().symlink_metadata()?.is_symlink() {
            continue;
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if entry.path().symlink_metadata()?.file_attributes() & 0x400 != 0 {
                continue;
            }
        }
        if kind.is_file()
            && entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("codex.exe")
        {
            engines.push(entry.path());
        } else if kind.is_dir() {
            find_codex_engines(&entry.path(), depth - 1, engines)?;
        }
    }
    Ok(())
}

async fn read_codex_account_status(
    mut command: Command,
    config_home: &Path,
    timeout: Duration,
) -> Result<bool> {
    command
        .args(["login", "status"])
        .env("CODEX_HOME", config_home)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let output = tokio::time::timeout(timeout, command.output())
        .await
        .map_err(|_| message("读取官方登录状态超时，请允许系统凭据访问后重试；未修改配置"))?
        .map_err(|_| message("无法读取官方登录状态，未修改配置"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let lines: Vec<_> = stdout
        .lines()
        .chain(stderr.lines())
        .map(str::trim)
        .collect();
    if output.status.success()
        && lines.iter().any(|s| {
            *s == "Logged in using ChatGPT"
                || s.starts_with("API key configured - ")
                || s.starts_with("Logged in using an API key")
        })
    {
        return Ok(true);
    }
    if output.status.code() == Some(1) && lines == ["Not logged in"] {
        return Ok(false);
    }
    // Never include raw engine output: it can contain credential details.
    Err(message(
        "无法确认现有官方登录状态，请检查系统凭据访问后重试；未修改配置",
    ))
}

pub async fn detect(spec: DesktopSpec, home: &Path) -> Result<Option<Installation>> {
    if cfg!(target_os = "macos") {
        let mut fallback = None;
        for dir in [PathBuf::from("/Applications"), home.join("Applications")] {
            for name in if spec.name == "Codex" {
                vec!["Codex", "ChatGPT"]
            } else {
                vec![spec.name]
            } {
                if let Some(app) = mac_installation(spec, &dir.join(format!("{name}.app"))) {
                    if app.gateway_supported {
                        return Ok(Some(app));
                    }
                    fallback.get_or_insert(app);
                }
            }
        }
        // Find moved/renamed bundles by their exact Spotlight bundle identity.
        let query = if spec.name == "Codex" {
            "kMDItemCFBundleIdentifier == 'com.openai.codex' || kMDItemCFBundleIdentifier == 'com.openai.chat'".into()
        } else {
            format!("kMDItemCFBundleIdentifier == '{}'", spec.bundle_id)
        };
        let output = Command::new("/usr/bin/mdfind")
            .args(["-0", &query])
            .kill_on_drop(true)
            .output();
        if let Ok(Ok(output)) = tokio::time::timeout(Duration::from_secs(5), output).await {
            if output.status.success() {
                for path in output.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()) {
                    let path = PathBuf::from(String::from_utf8_lossy(path).as_ref());
                    if path.starts_with("/Volumes") || path.starts_with(std::env::temp_dir()) {
                        continue;
                    }
                    if let Some(app) = mac_installation(spec, &path) {
                        if app.gateway_supported {
                            return Ok(Some(app));
                        }
                        fallback.get_or_insert(app);
                    }
                }
            }
        }
        return Ok(fallback);
    }
    if cfg!(windows) {
        return windows_detect(spec).await;
    }
    Ok(None)
}

#[derive(Deserialize)]
struct WindowsApp {
    target: String,
    version: Option<String>,
    packaged: bool,
    gateway_supported: bool,
}
async fn windows_detect(spec: DesktopSpec) -> Result<Option<Installation>> {
    let script = windows_detection_script(spec);
    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let output = tokio::time::timeout(Duration::from_secs(15), command.output())
        .await
        .map_err(|_| message("桌面应用检测超时，请重新检测"))??;
    if !output.status.success() {
        return Err(message("无法读取桌面安装信息，请重新检测"));
    }
    if output.stdout.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    let d: WindowsApp = serde_json::from_slice(&output.stdout)?;
    Ok(Some(Installation {
        target: d.target.into(),
        version: d.version,
        packaged: d.packaged,
        gateway_supported: d.gateway_supported,
    }))
}

fn windows_detection_script(spec: DesktopSpec) -> String {
    // All interpolated values are developer-owned constants, never renderer input.
    format!(
        r#"
$ErrorActionPreference='Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false
$p=Get-AppxPackage | Where-Object {{ ($_.Name -eq '{package}' -or ($_.Name -eq 'OpenAI.ChatGPT-Desktop' -and '{name}' -eq 'Codex') -or ($_.Name -eq 'Anthropic.Claude' -and '{name}' -eq 'Claude')) -and $_.Publisher -match $(if('{name}' -eq 'Codex'){{'OpenAI'}}else{{'Anthropic'}}) }} | Sort-Object @{{Expression={{$_.Name -eq '{package}'}};Descending=$true}} | Select-Object -First 1
if($p) {{
 $m=$p | Get-AppxPackageManifest
 $a=$m.Package.Applications.Application | Where-Object {{ $_.Executable -like '*{exe}' -or ($_.Executable -like '*ChatGPT.exe' -and '{name}' -eq 'Codex') }} | Select-Object -First 1
 if($a) {{ @{{target=($p.PackageFamilyName+'!'+$a.Id);version=$p.Version.ToString();packaged=$true;gateway_supported=('{name}' -eq 'Claude' -or $p.Name -eq '{package}')}} | ConvertTo-Json -Compress; exit }}
}}
$paths=@("$env:LOCALAPPDATA\Programs\{name}\{exe}","$env:LOCALAPPDATA\{name}\{exe}","$env:ProgramFiles\{name}\{exe}")
$roots=@('HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*','HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*','HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*')
foreach($r in $roots) {{ Get-ItemProperty $r -ErrorAction SilentlyContinue | Where-Object {{$_.DisplayName -eq '{name}' -and $_.InstallLocation}} | ForEach-Object {{$paths+=Join-Path $_.InstallLocation '{exe}'}} }}
foreach($path in $paths) {{
 if(Test-Path -LiteralPath $path -PathType Leaf) {{
  $sig=Get-AuthenticodeSignature -LiteralPath $path
  $vendor=if('{name}' -eq 'Codex'){{'OpenAI'}}else{{'Anthropic'}}
  if($sig.Status -eq 'Valid' -and $sig.SignerCertificate.Subject -match $vendor) {{
   @{{target=(Resolve-Path -LiteralPath $path).Path;version=(Get-Item -LiteralPath $path).VersionInfo.ProductVersion;packaged=$false;gateway_supported=$true}} | ConvertTo-Json -Compress; exit
  }}
 }}
}}
"#,
        package = spec.windows_package,
        name = spec.name,
        exe = spec.windows_exe
    )
}

pub async fn open(spec: DesktopSpec, home: &Path, restart_required: bool) -> Result<OpenOutcome> {
    let app = detect(spec, home)
        .await?
        .ok_or_else(|| message("尚未检测到桌面应用，请先安装或重新检测"))?;
    open_installation(app, restart_required).await
}

async fn open_installation(app: Installation, restart_required: bool) -> Result<OpenOutcome> {
    // Query at the time of the click, including when no configuration changed.
    // The renderer must confirm before the separate reopen command can quit it.
    if is_running(&app).await? {
        return Ok(OpenOutcome::AlreadyRunning);
    }
    if restart_required {
        return Ok(OpenOutcome::RestartRequired);
    }
    launch_installation(app).await?;
    Ok(OpenOutcome::Opened)
}

async fn launch(spec: DesktopSpec, home: &Path) -> Result<()> {
    let app = detect(spec, home)
        .await?
        .ok_or_else(|| message("尚未检测到桌面应用，请先安装或重新检测"))?;
    launch_installation(app).await
}

async fn launch_installation(app: Installation) -> Result<()> {
    let mut command = if cfg!(target_os = "macos") {
        let mut c = Command::new("/usr/bin/open");
        c.arg("-a").arg(app.target);
        c
    } else if cfg!(windows) && app.packaged {
        let mut c = Command::new("explorer.exe");
        c.arg(format!("shell:AppsFolder\\{}", app.target.display()));
        c
    } else {
        Command::new(app.target)
    };
    if cfg!(target_os = "macos") {
        let output =
            tokio::time::timeout(Duration::from_secs(10), command.kill_on_drop(true).output())
                .await
                .map_err(|_| message("打开应用等待超时"))??;
        if !output.status.success() {
            return Err(message("无法打开桌面应用，请重新检测后重试"));
        }
    } else {
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|_| message("无法打开桌面应用，请重新检测后重试"))?;
    }
    Ok(())
}

const MAC_RUNNING_SCRIPT: &str = r#"
on run argv
    set targetPath to item 1 of argv
    return application targetPath is running
end run
"#;

async fn is_running(app: &Installation) -> Result<bool> {
    let mut command = if cfg!(target_os = "macos") {
        let mut c = Command::new("/usr/bin/osascript");
        c.args(["-e", MAC_RUNNING_SCRIPT]).arg(&app.target);
        c
    } else if cfg!(windows) {
        let mut c = Command::new("powershell.exe");
        c.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &windows_running_script(app),
        ]);
        #[cfg(windows)]
        c.creation_flags(0x08000000);
        c
    } else {
        return Err(message("当前平台暂不支持桌面应用"));
    };
    let output = tokio::time::timeout(Duration::from_secs(10), command.kill_on_drop(true).output())
        .await
        .map_err(|_| message("检测运行状态超时，请重试"))??;
    if output.status.success() {
        match String::from_utf8_lossy(&output.stdout).trim() {
            "true" => return Ok(true),
            "false" => return Ok(false),
            _ => {}
        }
    }
    Err(message("无法确认应用是否正在运行，请重新检测后重试"))
}

const MAC_REOPEN_SCRIPT: &str = r#"
on run argv
    set targetPath to item 1 of argv
    if application targetPath is running then
        tell application targetPath to quit
        repeat 80 times
            if not (application targetPath is running) then return
            delay 0.25
        end repeat
        error "Application is still running"
    end if
end run
"#;

fn windows_process_script(app: &Installation) -> String {
    // The target was obtained from verified native installation metadata. Quote
    // it as a PowerShell literal, never as shell command text or a process name.
    let target = app.target.to_string_lossy().replace('\'', "''");
    let root = if app.packaged {
        format!(
            r#"
$family=('{target}' -split '!')[0]
$package=Get-AppxPackage | Where-Object {{$_.PackageFamilyName -eq $family}} | Select-Object -First 1
if(!$package -or !$package.InstallLocation) {{ throw 'Package unavailable' }}
$root=$package.InstallLocation.TrimEnd('\')+'\'
"#
        )
    } else {
        format!("$root='{target}'")
    };
    format!(
        r#"
$ErrorActionPreference='Stop'
{root}
function Find-TargetProcesses {{
 Get-Process | Where-Object {{
  try {{ $path=$_.Path; $path -and $(if({packaged}){{ $path.StartsWith($root,[StringComparison]::OrdinalIgnoreCase) }}else{{ $path -eq $root }}) }} catch {{ $false }}
 }}
}}
"#,
        packaged = if app.packaged { "$true" } else { "$false" }
    )
}

fn windows_quit_script(app: &Installation) -> String {
    format!(
        r#"
{}
$processes=@(Find-TargetProcesses)
foreach($process in $processes) {{
 if($process.MainWindowHandle -ne 0) {{ [void]$process.CloseMainWindow() }}
}}
$deadline=[DateTime]::UtcNow.AddSeconds(20)
while(@(Find-TargetProcesses).Count -gt 0) {{
 if([DateTime]::UtcNow -ge $deadline) {{ throw 'Application is still running' }}
 Start-Sleep -Milliseconds 250
}}
"#,
        windows_process_script(app)
    )
}

fn windows_running_script(app: &Installation) -> String {
    format!(
        "{}\nif(@(Find-TargetProcesses).Count -gt 0) {{ 'true' }} else {{ 'false' }}",
        windows_process_script(app)
    )
}

pub async fn reopen(spec: DesktopSpec, home: &Path) -> Result<()> {
    let app = detect(spec, home)
        .await?
        .ok_or_else(|| message("尚未检测到桌面应用，请先安装或重新检测"))?;
    let mut command = if cfg!(target_os = "macos") {
        let mut c = Command::new("/usr/bin/osascript");
        c.args(["-e", MAC_REOPEN_SCRIPT]).arg(&app.target);
        c
    } else if cfg!(windows) {
        let mut c = Command::new("powershell.exe");
        c.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &windows_quit_script(&app),
        ]);
        #[cfg(windows)]
        c.creation_flags(0x08000000);
        c
    } else {
        return Err(message("当前平台暂不支持桌面应用"));
    };
    // A timeout only stops our helper, never the target application or its tasks.
    let result =
        tokio::time::timeout(Duration::from_secs(30), command.kill_on_drop(true).output()).await;
    match result {
        Ok(Ok(output)) if output.status.success() => launch(spec, home).await,
        _ => Err(message(
            "应用尚未正常退出。请保存工作并完全退出应用后，再点击打开；未强制结束进程",
        )),
    }
}

pub async fn install_mac(
    spec: DesktopSpec,
    home: &Path,
    image: &Path,
    manifest: &crate::installers::InstallerManifest,
    target: &crate::installers::NativeTarget,
) -> Result<()> {
    let temp = tempfile::tempdir()?;
    let mount = temp.path().join("mounted");
    std::fs::create_dir(&mount)?;
    let attach = Command::new("/usr/bin/hdiutil")
        .args(["attach", "-readonly", "-nobrowse", "-quiet", "-mountpoint"])
        .arg(&mount)
        .arg(&image)
        .kill_on_drop(true)
        .output();
    let attached = tokio::time::timeout(Duration::from_secs(90), attach)
        .await
        .map_err(|_| message("安装包挂载超时"))??;
    if !attached.status.success() {
        return Err(message("无法打开官方安装包"));
    }
    let result = install_mounted(spec, home, &mount, manifest, target).await;
    let _ = tokio::time::timeout(
        Duration::from_secs(15),
        Command::new("/usr/bin/hdiutil")
            .arg("detach")
            .arg(&mount)
            .kill_on_drop(true)
            .output(),
    )
    .await;
    result
}
async fn install_mounted(
    spec: DesktopSpec,
    home: &Path,
    mount: &Path,
    manifest: &crate::installers::InstallerManifest,
    target: &crate::installers::NativeTarget,
) -> Result<()> {
    let mut app = mount.join(format!("{}.app", spec.name));
    if !app.exists() && spec.name == "Codex" {
        app = mount.join("ChatGPT.app");
    }
    if mac_installation(spec, &app).is_none() {
        return Err(message("安装包中的应用标识不匹配"));
    }
    let info = plist::Value::from_file(app.join("Contents/Info.plist"))
        .map_err(|_| message("应用信息无效"))?;
    let metadata = info
        .as_dictionary()
        .ok_or_else(|| message("应用信息无效"))?;
    let value = |key: &str| metadata.get(key).and_then(plist::Value::as_string);
    if value("CFBundleShortVersionString") != Some(manifest.version.as_str())
        || value("CFBundleVersion") != Some(manifest.build.as_str())
    {
        return Err(message("安装包内版本与清单不符，未安装"));
    }
    let native_arch = if target.arch == "aarch64" {
        "arm64"
    } else {
        "x86_64"
    };
    let per_arch_minimum = metadata
        .get("LSMinimumSystemVersionByArchitecture")
        .and_then(plist::Value::as_dictionary)
        .and_then(|versions| versions.get(native_arch))
        .and_then(plist::Value::as_string);
    for minimum in [value("LSMinimumSystemVersion"), per_arch_minimum]
        .into_iter()
        .flatten()
    {
        if crate::installers::version_parts(&target.os_version)?
            < crate::installers::version_parts(minimum)?
            || crate::installers::version_parts(&manifest.min_os_version)?
                < crate::installers::version_parts(minimum)?
        {
            return Err(message("此安装包不支持当前系统版本，未安装"));
        }
    }
    let executable = info
        .as_dictionary()
        .and_then(|d| d.get("CFBundleExecutable"))
        .and_then(plist::Value::as_string)
        .ok_or_else(|| message("应用入口无效"))?;
    let architectures = crate::macho::architectures(&app.join("Contents/MacOS").join(executable))?;
    if !architectures.contains(&target.arch) {
        return Err(message("官方桌面安装包不支持当前芯片，未安装"));
    }
    for (command, args) in [
        ("/usr/bin/codesign", vec!["--verify", "--deep", "--strict"]),
        ("/usr/sbin/spctl", vec!["--assess", "--type", "execute"]),
    ] {
        let output = tokio::time::timeout(
            Duration::from_secs(60),
            Command::new(command)
                .args(args)
                .arg(&app)
                .kill_on_drop(true)
                .output(),
        )
        .await
        .map_err(|_| message("官方应用签名检查超时"))??;
        if !output.status.success() {
            return Err(message("官方应用签名或系统安全检查未通过，未安装"));
        }
    }
    let signature = Command::new("/usr/bin/codesign")
        .args(["-d", "--verbose=2"])
        .arg(&app)
        .output()
        .await?;
    // Pin the Apple developer team, rather than searching an arbitrary field
    // in codesign output for the company's display name.
    let team = if spec.name == "Codex" {
        "2DC432GLL2"
    } else {
        "Q6L2SF6YDW"
    };
    if !signature.status.success()
        || !String::from_utf8_lossy(&signature.stderr)
            .lines()
            .any(|line| line == format!("TeamIdentifier={team}"))
    {
        return Err(message("官方安装包的发布者不匹配"));
    }
    let applications = home.join("Applications");
    std::fs::create_dir_all(&applications)?;
    let target = applications.join(format!("{}.app", spec.name));
    if target.exists() {
        return Err(message(
            "安装位置已存在文件，请重新检测或检查 Applications 目录",
        ));
    }
    // Stage the copy then rename so an interrupted copy is never detected as installed.
    let staging = tempfile::Builder::new()
        .prefix(".launchone-install-")
        .tempdir_in(&applications)?;
    let copied = staging.path().join(format!("{}.app", spec.name));
    let result = Command::new("/usr/bin/ditto")
        .arg(&app)
        .arg(&copied)
        .output()
        .await?;
    if !result.status.success() {
        return Err(message("无法复制桌面应用，请检查目录权限"));
    }
    std::fs::rename(copied, target)?;
    Ok(())
}

#[cfg(test)]
mod account_tests {
    use super::*;

    #[test]
    fn bundled_engine_search_stays_inside_installation() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("resources/codex-cli/bin");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("codex.exe"), "fixture").unwrap();
        std::fs::write(root.join("other.exe"), "fixture").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(temp.path(), root.join("loop")).unwrap();
        let mut engines = Vec::new();
        find_codex_engines(temp.path(), 6, &mut engines).unwrap();
        assert_eq!(engines, [root.join("codex.exe")]);
    }

    #[tokio::test]
    async fn native_account_probe_handles_login_absence_errors_and_timeout() {
        let temp = tempfile::tempdir().unwrap();
        for (response, code, expected) in [
            ("Logged in using ChatGPT", 0, Some(true)),
            ("API key configured - fixture-masked", 0, Some(true)),
            ("Not logged in", 1, Some(false)),
            ("Error checking login status: fixture-secret", 1, None),
            ("Not logged in", 0, None),
            ("Unknown output fixture-secret", 0, None),
        ] {
            let mut command = fixture_command(temp.path(), false);
            command
                .env("LAUNCHONE_FIXTURE_RESPONSE", response)
                .env("LAUNCHONE_FIXTURE_EXIT", code.to_string());
            let result =
                read_codex_account_status(command, temp.path(), Duration::from_secs(10)).await;
            match expected {
                Some(expected) => assert_eq!(result.unwrap(), expected),
                None => assert!(!result.unwrap_err().to_string().contains("fixture-secret")),
            }
        }
        let result = read_codex_account_status(
            fixture_command(temp.path(), true),
            temp.path(),
            Duration::from_millis(100),
        )
        .await;
        assert!(result.unwrap_err().to_string().contains("超时"));
    }

    fn fixture_command(root: &Path, wait: bool) -> Command {
        let script = root.join(if cfg!(windows) {
            "fixture.ps1"
        } else {
            "fixture.sh"
        });
        if cfg!(windows) {
            std::fs::write(&script, if wait { "Start-Sleep -Seconds 3" } else {
                "param($first,$second)\nif($first -ne 'login' -or $second -ne 'status' -or -not $env:CODEX_HOME){exit 9}\n[Console]::Error.WriteLine($env:LAUNCHONE_FIXTURE_RESPONSE)\nexit ([int]$env:LAUNCHONE_FIXTURE_EXIT)"
            }).unwrap();
            let mut command = Command::new("powershell.exe");
            command
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                ])
                .arg(script);
            command
        } else {
            std::fs::write(&script, if wait { "exec sleep 3" } else {
                "[ \"$1\" = login ] && [ \"$2\" = status ] && [ -n \"$CODEX_HOME\" ] || exit 9\nprintf '%s\\n' \"$LAUNCHONE_FIXTURE_RESPONSE\" >&2\nexit \"$LAUNCHONE_FIXTURE_EXIT\""
            }).unwrap();
            let mut command = Command::new("/bin/sh");
            command.arg(script);
            command
        }
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn running_probe_checks_verified_target_and_never_closes_it() {
        for packaged in [false, true] {
            for running in [false, true] {
                let app = Installation {
                    target: if packaged {
                        "Fixture!App".into()
                    } else {
                        r"D:\Apps\it's fixture\Claude.exe".into()
                    },
                    version: None,
                    packaged,
                    gateway_supported: true,
                };
                let script = format!(
                    r#"
function Get-AppxPackage {{ [pscustomobject]@{{PackageFamilyName='Fixture';InstallLocation="D:\Apps\it's fixture"}} }}
function Get-Process {{
 $paths=@('D:\Other\Claude.exe','D:\Apps\it''s fixture-other\Claude.exe')
 if($env:LAUNCHONE_RUNNING_FIXTURE -eq 'true') {{ $paths+="D:\Apps\it's fixture\Claude.exe" }}
 foreach($path in $paths) {{
  $process=[pscustomobject]@{{Path=$path;MainWindowHandle=42}}
  $process | Add-Member -MemberType ScriptMethod -Name CloseMainWindow -Value {{throw 'Probe attempted to close an app'}}
  $process
 }}
}}
{}"#,
                    windows_running_script(&app)
                );
                let output = Command::new("powershell.exe")
                    .args(["-NoProfile", "-NonInteractive", "-Command", &script])
                    .env("LAUNCHONE_RUNNING_FIXTURE", running.to_string())
                    .output()
                    .await
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert_eq!(
                    String::from_utf8_lossy(&output.stdout).trim(),
                    running.to_string()
                );
            }
        }
    }

    #[tokio::test]
    async fn quit_script_closes_only_the_verified_app_with_no_forced_termination() {
        for packaged in [false, true] {
            let path = r"D:\Apps\it's fixture\Claude.exe";
            let app = Installation {
                target: if packaged {
                    "Fixture!App".into()
                } else {
                    path.into()
                },
                version: None,
                packaged,
                gateway_supported: true,
            };
            let script = format!(
                r#"
$script:closed=$false
function Get-AppxPackage {{ [pscustomobject]@{{PackageFamilyName='Fixture';InstallLocation="D:\Apps\it's fixture"}} }}
function Get-Process {{
 if(!$script:closed) {{
  $app=[pscustomobject]@{{Path=$env:LAUNCHONE_QUIT_FIXTURE;MainWindowHandle=42}}
  $app | Add-Member -MemberType ScriptMethod -Name CloseMainWindow -Value {{$script:closed=$true;return $true}}
  $app
 }}
 $other=[pscustomobject]@{{Path='D:\Other\Claude.exe';MainWindowHandle=43}}
 $other | Add-Member -MemberType ScriptMethod -Name CloseMainWindow -Value {{throw 'Unrelated app was touched'}}
 $other
}}
{}
if(!$script:closed) {{ throw 'Target app was not closed' }}
"#,
                windows_quit_script(&app)
            );
            let output = Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-Command", &script])
                .env("LAUNCHONE_QUIT_FIXTURE", path)
                .output()
                .await
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    #[tokio::test]
    async fn native_detection_script_handles_official_package_identities_in_isolation() {
        // Execute the real PowerShell script with isolated command fixtures. Never
        // inspect installed apps, registry entries, user configs or credentials.
        for (spec, packages, expected, gateway) in [
            (CLAUDE, vec!["Claude"], "Claude", true),
            (CLAUDE, vec!["Anthropic.Claude"], "Anthropic.Claude", true),
            (CODEX, vec!["OpenAI.Codex"], "OpenAI.Codex", true),
            (
                CODEX,
                vec!["OpenAI.ChatGPT-Desktop"],
                "OpenAI.ChatGPT-Desktop",
                false,
            ),
            (
                CODEX,
                vec!["OpenAI.ChatGPT-Desktop", "OpenAI.Codex"],
                "OpenAI.Codex",
                true,
            ),
        ] {
            let fixtures: Vec<_> = packages.into_iter().map(|name| json!({
                "Name": name,
                "Publisher": if spec.name == "Codex" { "CN=OpenAI" } else { "CN=Anthropic" },
                "PackageFamilyName": name,
                "Version": "1.2.3",
            })).collect();
            let script = format!(
                r#"
function Get-AppxPackage {{ foreach($item in ($env:LAUNCHONE_PACKAGE_FIXTURE | ConvertFrom-Json)) {{ $item }} }}
function Get-AppxPackageManifest {{
 param([Parameter(ValueFromPipeline=$true)] $Package)
 process {{ [pscustomobject]@{{ Package=@{{ Applications=@{{ Application=@{{ Executable=$env:LAUNCHONE_EXE_FIXTURE; Id='App' }} }} }} }} }}
}}
function Get-ItemProperty {{ throw 'Fixture unexpectedly reached registry fallback' }}
function Test-Path {{ throw 'Fixture unexpectedly reached filesystem fallback' }}
{}
"#,
                windows_detection_script(spec)
            );
            let output = Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-Command", &script])
                .env(
                    "LAUNCHONE_PACKAGE_FIXTURE",
                    serde_json::to_string(&fixtures).unwrap(),
                )
                .env("LAUNCHONE_EXE_FIXTURE", spec.windows_exe)
                .output()
                .await
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let app: WindowsApp = serde_json::from_slice(&output.stdout)
                .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&output.stdout)));
            assert_eq!(app.target, format!("{expected}!App"));
            assert_eq!(app.version.as_deref(), Some("1.2.3"));
            assert!(app.packaged);
            assert_eq!(app.gateway_supported, gateway);
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod mac_tests {
    use super::*;

    #[tokio::test]
    async fn running_fixture_requires_confirmation_even_without_config_changes() {
        // A temporary native app is the only process this test starts or stops.
        // No official app, user configuration or keychain is accessed.
        let temp = tempfile::tempdir().unwrap();
        let bundle = temp.path().join("Running fixture.app");
        let contents = bundle.join("Contents");
        std::fs::create_dir_all(contents.join("MacOS")).unwrap();
        let mut info = plist::Dictionary::new();
        info.insert(
            "CFBundleIdentifier".into(),
            format!("test.launchone.fixture.{}", std::process::id()).into(),
        );
        info.insert("CFBundleExecutable".into(), "fixture".into());
        info.insert("CFBundlePackageType".into(), "APPL".into());
        plist::Value::Dictionary(info)
            .to_file_xml(contents.join("Info.plist"))
            .unwrap();
        let source = temp.path().join("fixture.swift");
        std::fs::write(&source, "import AppKit\nlet app = NSApplication.shared\napp.setActivationPolicy(.prohibited)\napp.run()\n").unwrap();
        let binary = contents.join("MacOS/fixture");
        let compiled = Command::new("/usr/bin/swiftc")
            .arg(source)
            .arg("-o")
            .arg(&binary)
            .output()
            .await
            .unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let installation = || Installation {
            target: bundle.clone(),
            version: None,
            packaged: false,
            gateway_supported: true,
        };
        assert!(!is_running(&installation()).await.unwrap());
        assert_eq!(
            open_installation(installation(), true).await.unwrap(),
            OpenOutcome::RestartRequired
        );
        assert!(!is_running(&installation()).await.unwrap());
        let mut child = Command::new(binary).kill_on_drop(true).spawn().unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while !is_running(&installation()).await.unwrap() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "Fixture did not register as running"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        for pending in [false, true] {
            assert_eq!(
                open_installation(installation(), pending).await.unwrap(),
                OpenOutcome::AlreadyRunning
            );
            assert!(is_running(&installation()).await.unwrap());
        }
        child.kill().await.unwrap();
        child.wait().await.unwrap();
        assert!(!is_running(&installation()).await.unwrap());
    }

    #[tokio::test]
    async fn quit_script_compiles_without_accessing_any_target_app() {
        let temp = tempfile::tempdir().unwrap();
        let output = Command::new("/usr/bin/osacompile")
            .arg("-o")
            .arg(temp.path().join("quit.scpt"))
            .args(["-e", MAC_REOPEN_SCRIPT])
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
