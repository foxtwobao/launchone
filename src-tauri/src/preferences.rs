use crate::config::atomic_write;
use crate::error::{message, Result};
use crate::models::Mode;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Default, Serialize, Deserialize)]
struct Choices {
    // A present null entry means the user explicitly restored their original config.
    apps: BTreeMap<String, Option<Mode>>,
}

pub struct Preferences(PathBuf);
impl Preferences {
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }
    fn read(&self) -> Result<Choices> {
        match std::fs::read(&self.0) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Choices::default()),
            Err(e) => Err(e.into()),
        }
    }
    pub fn wants_default(&self, app: &str, has_backup: bool) -> Result<bool> {
        Ok(!has_backup && !self.read()?.apps.contains_key(app))
    }
    pub fn remember(&self, app: &str, mode: Option<Mode>) -> Result<()> {
        if !app.bytes().all(|c| c.is_ascii_lowercase() || c == b'-') {
            return Err(message("无法识别的应用"));
        }
        let mut choices = self.read()?;
        choices.apps.insert(app.into(), mode);
        atomic_write(&self.0, &serde_json::to_vec(&choices)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn subscriptions_restore_and_legacy_backups_prevent_default_override() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preferences.json");
        let p = Preferences::new(path.clone());
        assert!(p.wants_default("codex", false).unwrap());
        assert!(!p.wants_default("codex", true).unwrap());
        p.remember("codex", Some(Mode::Official)).unwrap();
        assert!(!Preferences::new(path.clone())
            .wants_default("codex", false)
            .unwrap());
        p.remember("claude-desktop", None).unwrap();
        assert!(!Preferences::new(path.clone())
            .wants_default("claude-desktop", false)
            .unwrap());
        std::fs::write(path, "broken").unwrap();
        assert!(p.wants_default("codex", false).is_err());
    }
}
