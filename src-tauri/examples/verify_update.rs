//! Offline release verification using the same verifier as Tauri's updater.
use base64::{engine::general_purpose::STANDARD, Engine};
use minisign_verify::{PublicKey, Signature};
use std::{error::Error, fs};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 4 {
        return Err("usage: verify_update CONFIG_JSON ARTIFACT SIGNATURE VERSION".into());
    }
    let config: serde_json::Value = serde_json::from_slice(&fs::read(&args[0])?)?;
    let key = config["LAUNCHONE_UPDATE_PUBLIC_KEY"]
        .as_str()
        .ok_or("missing public key")?;
    let key = String::from_utf8(STANDARD.decode(key.trim())?)?;
    let key = PublicKey::decode(&key)?;
    let signature = String::from_utf8(STANDARD.decode(fs::read_to_string(&args[2])?.trim())?)?;
    let signature = Signature::decode(&signature)?;
    let data = fs::read(&args[1])?;
    key.verify(&data, &signature, true)?;
    let version = signature
        .trusted_comment()
        .split('\t')
        .find_map(|s| s.strip_prefix("version:"));
    if version != Some(args[3].as_str()) {
        return Err("signed artifact version does not match manifest version".into());
    }
    let mut tampered = data;
    if tampered.is_empty() {
        return Err("empty update artifact".into());
    }
    tampered[0] ^= 1;
    if key.verify(&tampered, &signature, true).is_ok() {
        return Err("tampered artifact unexpectedly accepted".into());
    }
    println!(
        "Verified {}: signature, signed version {}, tamper rejection",
        args[1], args[3]
    );
    Ok(())
}
