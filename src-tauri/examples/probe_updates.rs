//! Exercises the real updater's TLS, comparisons, download and verification; never installs.
use std::{error::Error, fs, path::PathBuf, time::Duration};
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri_plugin_updater::UpdaterExt;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: probe_updates CONFIG_JSON DOWNLOAD_DIRECTORY".into());
    }
    let config: serde_json::Value = serde_json::from_slice(&fs::read(&args[0])?)?;
    let origin = config["LAUNCHONE_SERVICE_URL"]
        .as_str()
        .ok_or("missing service URL")?;
    let public_key = config["LAUNCHONE_UPDATE_PUBLIC_KEY"]
        .as_str()
        .ok_or("missing public key")?;
    let output = PathBuf::from(&args[1]);
    fs::create_dir_all(&output)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let mut downloaded = 0;
    for (target, arch) in [
        ("darwin", "aarch64"),
        ("darwin", "x86_64"),
        ("windows", "x86_64"),
    ] {
        for (current, expected) in [
            ("0.2.3", true),
            (env!("CARGO_PKG_VERSION"), false),
            ("99.0.0", false),
        ] {
            let mut context = mock_context(noop_assets());
            context.package_info_mut().version = current.parse()?;
            context.config_mut().plugins.0.insert(
                "updater".into(),
                serde_json::json!({
                    "pubkey": public_key, "requireSignedVersion": true,
                }),
            );
            let app = mock_builder()
                .plugin(tauri_plugin_updater::Builder::new().build())
                .build(context)?;
            let updater = app
                .updater_builder()
                .timeout(Duration::from_secs(120))
                .endpoints(vec![format!(
                    "{origin}/v1/updates/{target}/{arch}/{{{{current_version}}}}"
                )
                .parse()?])?
                .build()?;
            let update = runtime.block_on(updater.check())?;
            if update.is_some() != expected {
                return Err(format!("unexpected check result: {target}/{arch}/{current}").into());
            }
            if let Some(mut update) = update {
                let mut bytes = runtime.block_on(update.download(|_, _| {}, || {}))?;
                fs::write(output.join(format!("{target}-{arch}.bin")), &bytes)?;
                fs::write(
                    output.join(format!("{target}-{arch}.sig")),
                    &update.signature,
                )?;
                let real_version = update.version.clone();
                update.version = "99.0.0".into();
                if update.verify(&bytes).is_ok() {
                    return Err("forged version was accepted".into());
                }
                update.version = real_version;
                bytes[0] ^= 1;
                if update.verify(&bytes).is_ok() {
                    return Err("tampered payload was accepted".into());
                }
                downloaded += 1;
                println!("{target}/{arch}: HTTPS download, signature, version binding and tamper rejection passed");
            } else {
                println!("{target}/{arch}: installed {current}, no update (correct)");
            }
        }
    }
    if downloaded != 3 {
        return Err("not all platform downloads were verified".into());
    }
    Ok(())
}
