//! Anonymous mirror manifests and resumable, content-addressed installer downloads.
use crate::config::{atomic_write, private_directory};
use crate::error::{actionable, message, RecoveryAction, Result};
use crate::settings::{http_client, validate_service_url, Settings};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

const MAX_INSTALLER_SIZE: u64 = 5 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallerManifest {
    pub app: String,
    pub platform: String,
    pub arch: String,
    pub supported_archs: Vec<String>,
    pub min_os_version: String,
    pub version: String,
    pub build: String,
    pub format: String,
    pub size: u64,
    pub sha256: String,
    pub url: String,
}

#[derive(Debug)]
pub struct NativeTarget {
    pub platform: String,
    pub arch: String,
    pub os_version: String,
}

pub async fn native_target() -> Result<NativeTarget> {
    if cfg!(target_os = "macos") {
        // sysctl reports hardware capability, including when this process runs in Rosetta.
        let arm = output("/usr/sbin/sysctl", &["-n", "hw.optional.arm64"]).await;
        let arch = match arm.as_deref() {
            Ok("1") => "aarch64",
            Ok("0") => "x86_64",
            _ => match output("/usr/bin/uname", &["-m"]).await?.as_str() {
                "arm64" => "aarch64",
                "x86_64"
                    if output("/usr/sbin/sysctl", &["-n", "machdep.cpu.brand_string"])
                        .await?
                        .contains("Intel") =>
                {
                    "x86_64"
                }
                _ => return Err(message("无法识别当前 Mac 的芯片架构")),
            },
        };
        return Ok(NativeTarget {
            platform: "macos".into(),
            arch: arch.into(),
            os_version: output("/usr/bin/sw_vers", &["-productVersion"]).await?,
        });
    }
    if cfg!(windows) {
        // WMI identifies the machine, not the architecture of this PowerShell process.
        let value = output("powershell.exe", &["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference='Stop'; $cpu=Get-CimInstance Win32_Processor | Select-Object -First 1; $os=Get-CimInstance Win32_OperatingSystem; @{arch=[int]$cpu.Architecture;version=$os.Version} | ConvertTo-Json -Compress"]).await?;
        let value: serde_json::Value = serde_json::from_str(&value)?;
        let arch = match value["arch"].as_u64() {
            Some(9) => "x86_64",
            Some(12) => "aarch64",
            _ => return Err(message("当前 Windows 芯片暂不支持安装")),
        };
        return Ok(NativeTarget {
            platform: "windows".into(),
            arch: arch.into(),
            os_version: value["version"]
                .as_str()
                .ok_or_else(|| message("无法识别系统版本"))?
                .into(),
        });
    }
    Err(message("当前系统暂不支持安装桌面应用"))
}

async fn output(program: &str, args: &[&str]) -> Result<String> {
    let mut command = Command::new(program);
    command.args(args).kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let value = tokio::time::timeout(Duration::from_secs(20), command.output())
        .await
        .map_err(|_| message("识别系统信息超时，请重试"))??;
    if !value.status.success() {
        return Err(message("无法识别当前系统信息，请重试"));
    }
    Ok(String::from_utf8_lossy(&value.stdout).trim().into())
}

pub(crate) fn version_parts(value: &str) -> Result<[u64; 4]> {
    let mut result = [0; 4];
    let parts: Vec<_> = value.split('.').collect();
    if parts.is_empty() || parts.len() > 4 {
        return Err(message("安装包系统要求无效"));
    }
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return Err(message("安装包系统要求无效"));
        }
        result[i] = part.parse().map_err(|_| message("安装包系统要求无效"))?;
    }
    Ok(result)
}

impl InstallerManifest {
    pub fn validate(&self, app: &str, target: &NativeTarget) -> Result<()> {
        if self.app != app
            || self.platform != target.platform
            || self.arch != target.arch
            || !self.supported_archs.contains(&target.arch)
            || self
                .supported_archs
                .iter()
                .any(|a| !["aarch64", "x86_64"].contains(&a.as_str()))
            || self.size == 0
            || self.size > MAX_INSTALLER_SIZE
            || self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || self.version.is_empty()
            || self.version.len() > 128
            || self.build.is_empty()
            || self.build.len() > 128
            || !matches!(
                (self.platform.as_str(), self.format.as_str()),
                ("macos", "dmg") | ("windows", "msix")
            )
        {
            return Err(message("安装包清单无效或与当前系统、芯片不匹配"));
        }
        validate_service_url(&self.url)?;
        if version_parts(&target.os_version)? < version_parts(&self.min_os_version)? {
            return Err(message(format!(
                "此版本需要 {} {} 或更新系统，请升级系统后重试",
                if target.platform == "macos" {
                    "macOS"
                } else {
                    "Windows"
                },
                self.min_os_version
            )));
        }
        Ok(())
    }
}

async fn manifest(
    settings: &Settings,
    app: &str,
    target: &NativeTarget,
) -> Result<InstallerManifest> {
    if !["codex", "claude-desktop"].contains(&app) {
        return Err(message("此应用暂不支持镜像安装"));
    }
    let base = settings
        .service_url
        .as_deref()
        .ok_or_else(|| message("安装包服务尚未配置"))?;
    validate_service_url(base)?;
    let mut response = http_client()?
        .get(format!(
            "{}/v1/installers/{}/{}/{}",
            base.trim_end_matches('/'),
            app,
            target.platform,
            target.arch
        ))
        .send()
        .await
        .map_err(|_| {
            actionable(
                "无法连接安装包服务，请检查网络后重试",
                RecoveryAction::Retry,
            )
        })?;
    match response.status().as_u16() {
        200 => (),
        404 => return Err(message("此应用尚无适合当前系统和芯片的安装包")),
        503 => {
            return Err(actionable(
                "安装包正在准备中，请稍后重试",
                RecoveryAction::Retry,
            ))
        }
        _ => {
            return Err(actionable(
                "安装包服务暂不可用，请稍后重试",
                RecoveryAction::Retry,
            ))
        }
    }
    let mut data = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| message("无法读取安装包清单"))?
    {
        if data.len() + chunk.len() > 64 * 1024 {
            return Err(message("安装包清单大小异常"));
        }
        data.extend_from_slice(&chunk);
    }
    let result: InstallerManifest =
        serde_json::from_slice(&data).map_err(|_| message("安装包清单格式无效"))?;
    result.validate(app, target)?;
    Ok(result)
}

pub struct DownloadedInstaller {
    pub manifest: InstallerManifest,
    pub path: PathBuf,
    task_directory: PathBuf,
    _lock: File,
}

impl DownloadedInstaller {
    pub fn cleanup(self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(self.task_directory.join("task.json"));
    }
}

fn cache_file(path: &Path) -> Result<File> {
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink() || !m.is_file()) {
        return Err(message("安装包缓存位置无效，请清理后重试"));
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

pub async fn download(
    settings: &Settings,
    app: &str,
    cache: &Path,
    target: &NativeTarget,
    emit: impl Fn(&'static str, String),
) -> Result<DownloadedInstaller> {
    let service = settings
        .service_url
        .as_deref()
        .ok_or_else(|| message("安装包服务尚未配置"))?;
    validate_service_url(service)?;
    let service_binding = format!(
        "{:x}",
        Sha256::digest(service.trim_end_matches('/').as_bytes())
    );
    let directory = cache
        .join(service_binding)
        .join(format!("{app}-{}-{}", target.platform, target.arch));
    private_directory(&directory)?;
    let lock = cache_file(&directory.join("download.lock"))?;
    lock.try_lock_exclusive()
        .map_err(|_| message("此安装包已在另一窗口下载，请稍后重试"))?;
    let task = directory.join("task.json");
    // Only clean an inactive task after holding its lock. Never remove another
    // process's partial file, even if a very slow download lasts many days.
    if task
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age > Duration::from_secs(7 * 24 * 60 * 60))
    {
        for entry in std::fs::read_dir(&directory)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let content_file = name.split_once('.').is_some_and(|(hash, suffix)| {
                hash.len() == 64
                    && hash
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    && ["part", "dmg", "msix"].contains(&suffix)
            });
            if name == "task.json" || content_file {
                std::fs::remove_file(entry.path())?;
            }
        }
    }
    let saved = std::fs::read(&task)
        .ok()
        .and_then(|data| serde_json::from_slice::<InstallerManifest>(&data).ok());
    let mut current = match saved {
        Some(value) if value.validate(app, target).is_ok() => value,
        _ => manifest(settings, app, target).await?,
    };
    // At most one refresh for a removed file or migrated host. A newer manifest
    // must never be appended to the partial bytes of a previous content hash.
    for attempt in 0..2 {
        current.validate(app, target)?;
        atomic_write(&task, &serde_json::to_vec(&current)?)?;
        let path = directory.join(format!("{}.part", current.sha256));
        let complete = directory.join(format!("{}.{}", current.sha256, current.format));
        if complete.is_file() && hash_matches(&complete, &current.sha256, current.size)? {
            return Ok(DownloadedInstaller {
                manifest: current,
                path: complete,
                task_directory: directory,
                _lock: lock,
            });
        }
        // Windows rename does not replace an existing destination. A corrupted
        // completed cache must be removed before recreating this immutable file.
        if std::fs::symlink_metadata(&complete).is_ok() {
            std::fs::remove_file(&complete)?;
        }
        emit("downloading", "正在下载安装包，可在中断后继续".into());
        match transfer(&current, &path, &emit).await? {
            Transfer::Complete => {
                emit("verifying", "正在校验安装包完整性".into());
                if !hash_matches(&path, &current.sha256, current.size)? {
                    let _ = std::fs::remove_file(&path);
                    return Err(actionable(
                        "安装包完整性检查未通过，请重新下载",
                        RecoveryAction::Retry,
                    ));
                }
                std::fs::rename(&path, &complete)?;
                return Ok(DownloadedInstaller {
                    manifest: current,
                    path: complete,
                    task_directory: directory,
                    _lock: lock,
                });
            }
            Transfer::Refresh if attempt == 0 => {
                let next = manifest(settings, app, target).await?;
                if current.sha256 != next.sha256 || current.size != next.size {
                    let _ = std::fs::remove_file(&path);
                }
                current = next;
            }
            Transfer::Refresh => {
                return Err(actionable(
                    "安装包地址暂不可用，请稍后重试",
                    RecoveryAction::Retry,
                ))
            }
        }
    }
    unreachable!()
}

enum Transfer {
    Complete,
    Refresh,
}

async fn transfer(
    value: &InstallerManifest,
    path: &Path,
    emit: &impl Fn(&'static str, String),
) -> Result<Transfer> {
    let mut file = cache_file(path)?;
    let mut offset = file.metadata()?.len();
    if offset > value.size {
        file.set_len(0)?;
        offset = 0;
    }
    if offset == value.size {
        return Ok(Transfer::Complete);
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5
                || validate_service_url(attempt.url().as_str()).is_err()
            {
                attempt.error("invalid download redirect")
            } else {
                attempt.follow()
            }
        }))
        .timeout(Duration::from_secs(60 * 60))
        .https_only(!cfg!(debug_assertions))
        .build()
        .map_err(|_| message("无法初始化下载"))?;
    // This anonymous client has no default Authorization headers or cookies.
    let mut request = client.get(&value.url).header("Accept-Encoding", "identity");
    let expected_etag = format!("\"{}\"", value.sha256);
    if offset > 0 {
        request = request
            .header("Range", format!("bytes={offset}-"))
            .header("If-Range", &expected_etag);
    }
    let mut response = match request.send().await {
        Ok(response) => response,
        Err(_) => return Ok(Transfer::Refresh),
    };
    match response.status().as_u16() {
        404 | 410 => return Ok(Transfer::Refresh),
        200 => {
            file.set_len(0)?;
            offset = 0;
        }
        206 => {
            let expected = format!("bytes {}-{}/{}", offset, value.size - 1, value.size);
            if offset == 0
                || response
                    .headers()
                    .get("content-range")
                    .and_then(|s| s.to_str().ok())
                    != Some(expected.as_str())
                || response.headers().get("etag").and_then(|s| s.to_str().ok())
                    != Some(expected_etag.as_str())
            {
                file.set_len(0)?;
                return Err(actionable(
                    "续传响应无效，请重新下载",
                    RecoveryAction::Retry,
                ));
            }
        }
        416 => {
            file.set_len(0)?;
            return Err(actionable(
                "下载记录已过期，请重新下载",
                RecoveryAction::Retry,
            ));
        }
        _ => {
            return Err(actionable(
                "下载安装包失败，请稍后重试",
                RecoveryAction::Retry,
            ))
        }
    }
    if response
        .content_length()
        .is_some_and(|length| length != value.size - offset)
    {
        return Err(message("安装包大小与清单不符，已停止下载"));
    }
    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::Start(offset))?;
    let mut last_percent = 101;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| actionable("下载中断，点击重试可继续下载", RecoveryAction::Retry))?
    {
        if offset + chunk.len() as u64 > value.size {
            file.set_len(0)?;
            return Err(message("安装包大小异常，已停止下载"));
        }
        file.write_all(&chunk)?;
        offset += chunk.len() as u64;
        let percent = offset * 100 / value.size;
        if percent != last_percent {
            emit("downloading", format!("正在下载安装包 {percent}%"));
            last_percent = percent;
        }
    }
    file.sync_all()?;
    if offset != value.size {
        return Err(actionable(
            "下载未完成，点击重试可继续下载",
            RecoveryAction::Retry,
        ));
    }
    Ok(Transfer::Complete)
}

fn hash_matches(path: &Path, expected: &str, expected_size: u64) -> Result<bool> {
    let mut file = File::open(path)?;
    if file.metadata()?.len() != expected_size {
        return Ok(false);
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()) == expected)
}

/// Windows verifies the MSIX before requesting native elevation, then verifies
/// it again in the elevated process before package registration.
pub async fn install_windows(
    spec: crate::desktop_app::DesktopSpec,
    installer: &DownloadedInstaller,
    target: &NativeTarget,
) -> Result<()> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let quote = |value: &str| format!("'{}'", value.replace('\'', "''"));
    let definitions = format!("$ErrorActionPreference='Stop';$PackagePath={};$ExpectedHash={};$ExpectedName={};$ExpectedVersion={};$ExpectedArch={};$OsVersion={};$Publisher={};\n",
        quote(&installer.path.to_string_lossy()), quote(&installer.manifest.sha256), quote(spec.windows_package),
        quote(&installer.manifest.version), quote(if target.arch == "aarch64" { "arm64" } else { "x64" }), quote(&target.os_version),
        quote(if spec.name == "Codex" { "OpenAI" } else { "Anthropic PBC" }));
    let verify = r#"
if ((Get-FileHash -LiteralPath $PackagePath -Algorithm SHA256).Hash.ToLowerInvariant() -ne $ExpectedHash) { throw 'Hash mismatch' }
$sig=Get-AuthenticodeSignature -LiteralPath $PackagePath
if($sig.Status -ne 'Valid' -or $null -eq $sig.SignerCertificate) { throw 'Package signature invalid' }
$signer=$sig.SignerCertificate.GetNameInfo([System.Security.Cryptography.X509Certificates.X509NameType]::SimpleName,$false)
$allowed=if($Publisher -eq 'OpenAI') { @('OpenAI','OpenAI, L.L.C.','OpenAI, Inc.') } else { @('Anthropic PBC','Anthropic, PBC') }
if($signer -notin $allowed) { throw 'Publisher mismatch' }
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip=[System.IO.Compression.ZipFile]::OpenRead($PackagePath)
try {
 $entry=$zip.GetEntry('AppxManifest.xml')
 if($null -eq $entry) { throw 'Missing manifest' }
 if($entry.Length -gt 1048576) { throw 'Manifest too large' }
 $reader=New-Object System.IO.StreamReader($entry.Open())
 try { $xml=New-Object System.Xml.XmlDocument; $xml.XmlResolver=$null; $xml.LoadXml($reader.ReadToEnd()) } finally { $reader.Dispose() }
 $identity=$xml.Package.Identity
 if($identity.Name -ne $ExpectedName -or $identity.Version -ne $ExpectedVersion -or $identity.ProcessorArchitecture -ne $ExpectedArch) { throw 'Identity or architecture mismatch' }
 if($identity.Publisher -ne $sig.SignerCertificate.Subject) { throw 'Signed publisher mismatch' }
 foreach($family in $xml.Package.Dependencies.TargetDeviceFamily) {
  if([version]$OsVersion -lt [version]$family.MinVersion) { throw 'Operating system too old' }
 }
} finally { $zip.Dispose() }
"#;
    // A different administrator's credentials would register this per-user
    // package for that administrator. Require UAC elevation of the same user.
    let elevated = format!("{definitions}{verify}\nif([System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value -ne $ExpectedUserSid) {{ exit 3 }}\ntry {{ Add-AppxPackage -Path $PackagePath -ErrorAction Stop; exit 0 }} catch {{ exit 1 }}");
    let elevated_literal = quote(&elevated);
    let script = format!("{definitions}{verify}\n$ExpectedUserSid=[System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value; $elevated='$ExpectedUserSid=' + \"'\" + $ExpectedUserSid + \"';\" + {elevated_literal}; $encoded=[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($elevated));\ntry {{ $p=Start-Process -FilePath 'powershell.exe' -Verb RunAs -ArgumentList @('-NoProfile','-NonInteractive','-EncodedCommand',$encoded) -Wait -PassThru; if($p.ExitCode -ne 0) {{ exit 1 }} }} catch {{ exit 2 }}");
    let encoded = STANDARD.encode(
        script
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let installed = tokio::time::timeout(Duration::from_secs(15 * 60), command.output())
        .await
        .map_err(|_| {
            message("Windows 安装等待超时，请检查系统安装窗口，完成后重新检测；不要重复启动安装")
        })??;
    if !installed.status.success() {
        return Err(actionable(
            "Windows 安装未完成或授权已取消。请确认管理员权限和系统依赖后重试",
            RecoveryAction::Retry,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        extract::State,
        http::{HeaderMap, Response},
        routing::get,
        Router,
    };
    use std::future::IntoFuture;
    use std::sync::{Arc, Mutex};

    fn target() -> NativeTarget {
        NativeTarget {
            platform: "macos".into(),
            arch: "aarch64".into(),
            os_version: "14.0".into(),
        }
    }
    fn fixture(url: String) -> InstallerManifest {
        InstallerManifest {
            app: "codex".into(),
            platform: "macos".into(),
            arch: "aarch64".into(),
            supported_archs: vec!["aarch64".into(), "x86_64".into()],
            min_os_version: "13.0".into(),
            version: "1.0".into(),
            build: "100".into(),
            format: "dmg".into(),
            size: 10,
            sha256: format!("{:x}", Sha256::digest(b"0123456789")),
            url,
        }
    }

    #[test]
    fn compatibility_rejects_wrong_architecture_old_system_and_invalid_urls() {
        let mut value = fixture("https://download.example.test/package.dmg".into());
        value.validate("codex", &target()).unwrap();
        assert!(value.validate("claude-desktop", &target()).is_err());
        value.arch = "x86_64".into();
        assert!(value.validate("codex", &target()).is_err());
        value.arch = "aarch64".into();
        value.min_os_version = "14.1".into();
        assert!(value.validate("codex", &target()).is_err());
        value.min_os_version = "13.0".into();
        value.url = "https://user:secret@example.test/file".into();
        assert!(value.validate("codex", &target()).is_err());
        assert!(version_parts("10.0.22000").unwrap() > version_parts("10.0.19045").unwrap());
        assert_eq!(
            version_parts("14").unwrap(),
            version_parts("14.0.0").unwrap()
        );
    }

    #[tokio::test]
    async fn resumes_partial_download_and_rejects_mismatched_ranges() {
        #[derive(Clone)]
        struct TestState {
            seen: Arc<Mutex<Vec<HeaderMap>>>,
            etag: String,
        }
        async fn serve(State(state): State<TestState>, headers: HeaderMap) -> Response<Body> {
            state.seen.lock().unwrap().push(headers.clone());
            Response::builder()
                .status(206)
                .header("Content-Range", "bytes 4-9/10")
                .header("ETag", &state.etag)
                .body(Body::from("456789"))
                .unwrap()
        }
        let expected = fixture("https://example.test/file".into());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(
            axum::serve(
                listener,
                Router::new()
                    .route("/file", get(serve))
                    .with_state(TestState {
                        seen: seen.clone(),
                        etag: format!("\"{}\"", expected.sha256),
                    }),
            )
            .into_future(),
        );
        let mut value = fixture(format!("http://{addr}/file"));
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("download.part");
        std::fs::write(&path, "0123").unwrap();
        assert!(matches!(
            transfer(&value, &path, &|_, _| {}).await.unwrap(),
            Transfer::Complete
        ));
        assert!(hash_matches(&path, &value.sha256, value.size).unwrap());
        let request = seen.lock().unwrap()[0].clone();
        assert_eq!(request["Range"], "bytes=4-");
        assert_eq!(request["If-Range"], format!("\"{}\"", value.sha256));
        assert!(!request.contains_key("Authorization"));
        std::fs::write(&path, "012").unwrap();
        assert!(transfer(&value, &path, &|_, _| {}).await.is_err());
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
        std::fs::write(&path, "0123").unwrap();
        value.sha256 = "a".repeat(64);
        assert!(transfer(&value, &path, &|_, _| {}).await.is_err());
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
        server.abort();
    }

    #[tokio::test]
    async fn ignored_range_restarts_and_tampered_full_file_fails_hash() {
        async fn serve() -> &'static str {
            "0123456789"
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(
            axum::serve(listener, Router::new().route("/file", get(serve))).into_future(),
        );
        let value = fixture(format!("http://{addr}/file"));
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("download.part");
        std::fs::write(&path, "0123").unwrap();
        assert!(matches!(
            transfer(&value, &path, &|_, _| {}).await.unwrap(),
            Transfer::Complete
        ));
        assert_eq!(std::fs::read(&path).unwrap(), b"0123456789");
        assert!(hash_matches(&path, &value.sha256, value.size).unwrap());
        std::fs::write(&path, "012345678x").unwrap();
        assert!(!hash_matches(&path, &value.sha256, value.size).unwrap());
        server.abort();
    }

    #[derive(Clone)]
    struct MirrorFixture {
        state: Arc<Mutex<MirrorState>>,
    }

    struct MirrorState {
        latest: InstallerManifest,
        files: std::collections::HashMap<String, Vec<u8>>,
        manifests_read: usize,
        requests: Vec<(String, HeaderMap)>,
    }

    async fn mirror_fixture() -> (Settings, MirrorFixture, tokio::task::JoinHandle<()>) {
        use axum::{extract::Path as RoutePath, Json};
        async fn latest(State(fixture): State<MirrorFixture>) -> Json<InstallerManifest> {
            let mut state = fixture.state.lock().unwrap();
            state.manifests_read += 1;
            Json(state.latest.clone())
        }
        async fn file(
            State(fixture): State<MirrorFixture>,
            RoutePath(name): RoutePath<String>,
            headers: HeaderMap,
        ) -> Response<Body> {
            let mut state = fixture.state.lock().unwrap();
            state.requests.push((name.clone(), headers.clone()));
            let Some(bytes) = state.files.get(&name) else {
                return Response::builder().status(404).body(Body::empty()).unwrap();
            };
            let etag = format!("\"{:x}\"", Sha256::digest(bytes));
            if let Some(offset) = headers
                .get("Range")
                .and_then(|h| h.to_str().ok())
                .and_then(|h| h.strip_prefix("bytes="))
                .and_then(|h| h.strip_suffix('-'))
                .and_then(|h| h.parse::<usize>().ok())
            {
                if offset >= bytes.len() {
                    return Response::builder().status(416).body(Body::empty()).unwrap();
                }
                return Response::builder()
                    .status(206)
                    .header("ETag", etag)
                    .header(
                        "Content-Range",
                        format!("bytes {}-{}/{}", offset, bytes.len() - 1, bytes.len()),
                    )
                    .body(Body::from(bytes[offset..].to_vec()))
                    .unwrap();
            }
            Response::builder()
                .status(200)
                .header("ETag", etag)
                .body(Body::from(bytes.clone()))
                .unwrap()
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let fixture = MirrorFixture {
            state: Arc::new(Mutex::new(MirrorState {
                latest: fixture(format!("{origin}/old")),
                files: [("old".into(), b"0123456789".to_vec())]
                    .into_iter()
                    .collect(),
                manifests_read: 0,
                requests: Vec::new(),
            })),
        };
        let app = Router::new()
            .route("/v1/installers/{app}/{platform}/{arch}", get(latest))
            .route("/{name}", get(file))
            .with_state(fixture.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let settings = Settings {
            service_url: Some(origin),
            ..Settings::default()
        };
        (settings, fixture, server)
    }

    fn publish_new(fixture: &MirrorFixture) -> InstallerManifest {
        let mut state = fixture.state.lock().unwrap();
        let mut latest = state.latest.clone();
        latest.url = latest.url.replace("/old", "/new");
        latest.version = "2.0".into();
        latest.build = "200".into();
        latest.sha256 = format!("{:x}", Sha256::digest(b"abcdefghij"));
        state.files.insert("new".into(), b"abcdefghij".to_vec());
        state.latest = latest.clone();
        latest
    }

    /// Persist the exact state of an interrupted transfer using a real task
    /// snapshot created by download(), then release its cross-process lock.
    fn interrupt(installer: DownloadedInstaller, bytes: &[u8]) -> PathBuf {
        let partial = installer.path.with_extension("part");
        std::fs::write(&partial, bytes).unwrap();
        std::fs::remove_file(&installer.path).unwrap();
        drop(installer);
        partial
    }

    #[tokio::test]
    async fn restarted_download_keeps_saved_manifest_when_latest_changes() {
        let (settings, fixture, server) = mirror_fixture().await;
        let cache = tempfile::tempdir().unwrap();
        let first = download(&settings, "codex", cache.path(), &target(), |_, _| {})
            .await
            .unwrap();
        let old_sha = first.manifest.sha256.clone();
        let partial = interrupt(first, b"0123");
        publish_new(&fixture);
        let resumed = download(&settings, "codex", cache.path(), &target(), |_, _| {})
            .await
            .unwrap();
        assert_eq!(resumed.manifest.sha256, old_sha);
        assert_eq!(std::fs::read(&resumed.path).unwrap(), b"0123456789");
        assert!(!partial.exists());
        let state = fixture.state.lock().unwrap();
        assert_eq!(
            state.manifests_read, 1,
            "a changed latest manifest must not replace an in-progress task"
        );
        assert_eq!(state.requests[1].0, "old");
        assert_eq!(state.requests[1].1["Range"], "bytes=4-");
        drop(state);
        resumed.cleanup();
        server.abort();
    }

    #[tokio::test]
    async fn removed_file_refreshes_manifest_and_never_appends_old_hash_bytes() {
        let (settings, fixture, server) = mirror_fixture().await;
        let cache = tempfile::tempdir().unwrap();
        let first = download(&settings, "codex", cache.path(), &target(), |_, _| {})
            .await
            .unwrap();
        let partial = interrupt(first, b"0123");
        let latest = publish_new(&fixture);
        fixture.state.lock().unwrap().files.remove("old");
        let refreshed = download(&settings, "codex", cache.path(), &target(), |_, _| {})
            .await
            .unwrap();
        assert_eq!(refreshed.manifest.sha256, latest.sha256);
        assert_eq!(std::fs::read(&refreshed.path).unwrap(), b"abcdefghij");
        assert!(!partial.exists());
        let state = fixture.state.lock().unwrap();
        assert_eq!(state.manifests_read, 2);
        assert_eq!(state.requests[1].0, "old");
        assert_eq!(state.requests[1].1["Range"], "bytes=4-");
        assert_eq!(state.requests[2].0, "new");
        assert!(!state.requests[2].1.contains_key("Range"));
        drop(state);
        refreshed.cleanup();
        server.abort();
    }

    #[tokio::test]
    async fn service_binding_isolates_lab_and_production_download_tasks() {
        let (lab, lab_fixture, lab_server) = mirror_fixture().await;
        let (production, prod_fixture, prod_server) = mirror_fixture().await;
        publish_new(&prod_fixture);
        let cache = tempfile::tempdir().unwrap();
        let lab_installer = download(&lab, "codex", cache.path(), &target(), |_, _| {})
            .await
            .unwrap();
        let lab_path = lab_installer.path.clone();
        drop(lab_installer);
        let prod_installer = download(&production, "codex", cache.path(), &target(), |_, _| {})
            .await
            .unwrap();
        assert_ne!(prod_installer.path.parent(), lab_path.parent());
        assert_eq!(std::fs::read(&prod_installer.path).unwrap(), b"abcdefghij");
        assert_eq!(std::fs::read(&lab_path).unwrap(), b"0123456789");
        assert_eq!(lab_fixture.state.lock().unwrap().manifests_read, 1);
        assert_eq!(prod_fixture.state.lock().unwrap().manifests_read, 1);
        assert_eq!(lab_fixture.state.lock().unwrap().requests.len(), 1);
        prod_installer.cleanup();
        lab_server.abort();
        prod_server.abort();
    }

    #[tokio::test]
    async fn corrupted_completed_cache_is_removed_and_downloaded_again() {
        let (settings, fixture, server) = mirror_fixture().await;
        let cache = tempfile::tempdir().unwrap();
        let first = download(&settings, "codex", cache.path(), &target(), |_, _| {})
            .await
            .unwrap();
        std::fs::write(&first.path, "xxxxxxxxxx").unwrap();
        drop(first);
        let repaired = download(&settings, "codex", cache.path(), &target(), |_, _| {})
            .await
            .unwrap();
        assert_eq!(std::fs::read(&repaired.path).unwrap(), b"0123456789");
        assert_eq!(fixture.state.lock().unwrap().requests.len(), 2);
        repaired.cleanup();
        server.abort();
    }
}
