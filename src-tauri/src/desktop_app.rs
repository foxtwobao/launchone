//! Native app discovery and launching. No CLI or config-directory heuristics.
use crate::error::{message, Result};
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
    pub mac_download: &'static str,
    pub windows_download: &'static str,
}

pub const CODEX: DesktopSpec = DesktopSpec {
    name: "Codex",
    bundle_id: "com.openai.codex",
    windows_package: "OpenAI.Codex",
    windows_exe: "Codex.exe",
    mac_download: "https://persistent.oaistatic.com/codex-app-prod/Codex.dmg",
    windows_download: "https://get.microsoft.com/installer/download/9PLM9XGG6VKS",
};
pub const CLAUDE: DesktopSpec = DesktopSpec {
    name: "Claude",
    bundle_id: "com.anthropic.claudefordesktop",
    windows_package: "Claude",
    windows_exe: "Claude.exe",
    mac_download: "https://downloads.claude.ai/releases/darwin/universal/latest/Claude.dmg",
    windows_download: "https://claude.ai/download",
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
        "codex",
        "codex/codex",
        "bin/codex",
        "app.asar.unpacked/node_modules/@openai/codex/bin/codex.js",
    ]
    .iter()
    .any(|p| resources.join(p).is_file())
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

pub async fn launch(spec: DesktopSpec, home: &Path) -> Result<()> {
    let app = detect(spec, home)
        .await?
        .ok_or_else(|| message("尚未检测到桌面应用，请先安装或重新检测"))?;
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

pub fn download_url(spec: DesktopSpec) -> &'static str {
    if cfg!(target_os = "macos") {
        spec.mac_download
    } else {
        spec.windows_download
    }
}

pub async fn install_mac(spec: DesktopSpec, home: &Path) -> Result<()> {
    let temp = tempfile::tempdir()?;
    let image = temp.path().join("installer.dmg");
    // Follow only HTTPS download redirects. Signature and app identity are checked
    // after download; nothing fetched is executed as a shell script.
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(5))
        .timeout(Duration::from_secs(600))
        .https_only(true)
        .build()
        .map_err(|_| message("无法初始化下载"))?;
    let mut response = client
        .get(spec.mac_download)
        .send()
        .await
        .map_err(|_| message("无法下载官方桌面安装包，请检查网络"))?
        .error_for_status()
        .map_err(|_| message("官方桌面安装包暂不可用"))?;
    let mut file = std::fs::File::create(&image)?;
    let mut size = 0usize;
    use std::io::Write;
    while let Some(bytes) = response
        .chunk()
        .await
        .map_err(|_| message("安装包下载中断，请重试"))?
    {
        size += bytes.len();
        if size > 1024 * 1024 * 1024 {
            return Err(message("安装包大小异常，已停止安装"));
        }
        file.write_all(&bytes)?;
    }
    file.sync_all()?;
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
    let result = install_mounted(spec, home, &mount).await;
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
async fn install_mounted(spec: DesktopSpec, home: &Path, mount: &Path) -> Result<()> {
    let mut app = mount.join(format!("{}.app", spec.name));
    if !app.exists() && spec.name == "Codex" {
        app = mount.join("ChatGPT.app");
    }
    if mac_installation(spec, &app).is_none() {
        return Err(message("安装包中的应用标识不匹配"));
    }
    let info = plist::Value::from_file(app.join("Contents/Info.plist"))
        .map_err(|_| message("应用信息无效"))?;
    let executable = info
        .as_dictionary()
        .and_then(|d| d.get("CFBundleExecutable"))
        .and_then(plist::Value::as_string)
        .ok_or_else(|| message("应用入口无效"))?;
    let arch = if std::env::consts::ARCH == "aarch64" {
        "arm64"
    } else {
        "x86_64"
    };
    let architecture = Command::new("/usr/bin/lipo")
        .arg(app.join("Contents/MacOS").join(executable))
        .args(["-verify_arch", arch])
        .output()
        .await?;
    if !architecture.status.success() {
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
    let vendor = if spec.name == "Codex" {
        "OpenAI"
    } else {
        "Anthropic"
    };
    if !String::from_utf8_lossy(&signature.stderr).contains(vendor) {
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

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use serde_json::json;

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
