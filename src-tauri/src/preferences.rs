use crate::config::atomic_write;
use crate::error::{message, Result};
use crate::models::{CredentialAccount, Mode};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

#[derive(Default, Serialize, Deserialize)]
struct Choices {
    // A present null entry means the user explicitly restored their original config.
    apps: BTreeMap<String, Option<Mode>>,
    #[serde(default)]
    restart_required: BTreeSet<String>,
    #[serde(default)]
    credential_owners: BTreeMap<String, CredentialOwner>,
}

#[derive(Serialize, Deserialize)]
struct CredentialOwner {
    issuer: String,
    subject: String,
}

pub struct Preferences(PathBuf);
pub struct PreferenceCheckpoint(Option<Vec<u8>>);
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
        self.save_choice(app, mode, None)
    }

    pub fn remember_tokenone(&self, app: &str, issuer: &str, subject: &str) -> Result<()> {
        self.save_choice(
            app,
            Some(Mode::Tokenone),
            Some(CredentialOwner {
                issuer: issuer.into(),
                subject: subject.into(),
            }),
        )
    }

    fn save_choice(
        &self,
        app: &str,
        mode: Option<Mode>,
        owner: Option<CredentialOwner>,
    ) -> Result<()> {
        if !app.bytes().all(|c| c.is_ascii_lowercase() || c == b'-') {
            return Err(message("无法识别的应用"));
        }
        let mut choices = self.read()?;
        choices.apps.insert(app.into(), mode);
        choices.restart_required.insert(app.into());
        if let Some(owner) = owner {
            choices.credential_owners.insert(app.into(), owner);
        } else {
            choices.credential_owners.remove(app);
        }
        atomic_write(&self.0, &serde_json::to_vec(&choices)?)
    }

    pub fn credential_account(
        &self,
        app: &str,
        issuer: &str,
        subject: &str,
    ) -> Result<CredentialAccount> {
        Ok(match self.read()?.credential_owners.get(app) {
            None => CredentialAccount::Unknown,
            Some(owner) if owner.issuer == issuer && owner.subject == subject => {
                CredentialAccount::Current
            }
            Some(_) => CredentialAccount::Other,
        })
    }

    pub fn checkpoint(&self) -> Result<PreferenceCheckpoint> {
        match std::fs::read(&self.0) {
            Ok(bytes) => Ok(PreferenceCheckpoint(Some(bytes))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(PreferenceCheckpoint(None)),
            Err(e) => Err(e.into()),
        }
    }

    pub fn rollback(&self, checkpoint: PreferenceCheckpoint) -> Result<()> {
        if let Some(bytes) = checkpoint.0 {
            atomic_write(&self.0, &bytes)
        } else {
            match std::fs::remove_file(&self.0) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.into()),
            }
        }
    }
    pub fn needs_restart(&self, app: &str) -> Result<bool> {
        Ok(self.read()?.restart_required.contains(app))
    }
    pub fn opened(&self, app: &str) -> Result<()> {
        let mut choices = self.read()?;
        choices.restart_required.remove(app);
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
        assert!(Preferences::new(path.clone())
            .needs_restart("codex")
            .unwrap());
        p.opened("codex").unwrap();
        assert!(!p.needs_restart("codex").unwrap());
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

    #[test]
    fn legacy_choices_have_unknown_owner_and_reconnect_binds_issuer_and_subject() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preferences.json");
        std::fs::write(
            &path,
            r#"{"apps":{"codex":"tokenone"},"restart_required":[]}"#,
        )
        .unwrap();
        let prefs = Preferences::new(path.clone());
        assert_eq!(
            prefs
                .credential_account("codex", "issuer-a", "alice")
                .unwrap(),
            CredentialAccount::Unknown
        );
        assert!(!prefs.wants_default("codex", false).unwrap());
        prefs
            .remember_tokenone("codex", "issuer-a", "alice")
            .unwrap();
        let restored = Preferences::new(path);
        assert_eq!(
            restored
                .credential_account("codex", "issuer-a", "alice")
                .unwrap(),
            CredentialAccount::Current
        );
        assert_eq!(
            restored
                .credential_account("codex", "issuer-a", "bob")
                .unwrap(),
            CredentialAccount::Other
        );
        assert_eq!(
            restored
                .credential_account("codex", "issuer-b", "alice")
                .unwrap(),
            CredentialAccount::Other
        );
        assert_eq!(
            restored
                .credential_account("claude-desktop", "issuer-a", "alice")
                .unwrap(),
            CredentialAccount::Unknown
        );
        restored
            .remember_tokenone("codex", "issuer-a", "bob")
            .unwrap();
        assert_eq!(
            restored
                .credential_account("codex", "issuer-a", "bob")
                .unwrap(),
            CredentialAccount::Current
        );
        restored.remember("codex", Some(Mode::Official)).unwrap();
        assert_eq!(
            restored
                .credential_account("codex", "issuer-a", "bob")
                .unwrap(),
            CredentialAccount::Unknown
        );
        restored
            .remember_tokenone("codex", "issuer-a", "bob")
            .unwrap();
        restored.remember("codex", None).unwrap();
        assert_eq!(
            restored
                .credential_account("codex", "issuer-a", "bob")
                .unwrap(),
            CredentialAccount::Unknown
        );
    }

    #[test]
    fn failed_configuration_can_restore_previous_owner_and_restart_state() {
        let dir = tempfile::tempdir().unwrap();
        let prefs = Preferences::new(dir.path().join("preferences.json"));
        prefs.remember_tokenone("codex", "issuer", "alice").unwrap();
        prefs.opened("codex").unwrap();
        let checkpoint = prefs.checkpoint().unwrap();
        prefs.remember_tokenone("codex", "issuer", "bob").unwrap();
        prefs.rollback(checkpoint).unwrap();
        assert_eq!(
            prefs
                .credential_account("codex", "issuer", "alice")
                .unwrap(),
            CredentialAccount::Current
        );
        assert!(!prefs.needs_restart("codex").unwrap());
        let fresh = Preferences::new(dir.path().join("fresh.json"));
        let checkpoint = fresh.checkpoint().unwrap();
        fresh.remember_tokenone("codex", "issuer", "bob").unwrap();
        fresh.rollback(checkpoint).unwrap();
        assert!(fresh.wants_default("codex", false).unwrap());
        assert_eq!(
            fresh.credential_account("codex", "issuer", "bob").unwrap(),
            CredentialAccount::Unknown
        );
    }
}
