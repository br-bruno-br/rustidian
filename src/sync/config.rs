//! The part of the sync setup that is not a secret: server address, device name and whether
//! sync is on. It lives in the app's state folder (outside the vault), in plain JSON. The key
//! and the token are in the system credential store (`secrets`), the passphrase nowhere.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncConfig {
    /// `http://100.x.y.z:8443`
    pub server: String,
    /// Names this device in conflict copies.
    pub device: String,
    /// The key and the token are in the credential store (set by Connect, cleared by Disconnect).
    pub connected: bool,
    /// Sync runs while this is on and the device is connected.
    pub enabled: bool,
}

/// Where this vault's sync files live (both outside the vault).
#[derive(Clone, Debug)]
pub struct SyncFiles {
    pub config: PathBuf,
    /// The `nota-sync` state database (redb): what was synced, the journal.
    pub database: PathBuf,
}

impl SyncFiles {
    /// The app's state folder for this vault, or `None` when the system has no state folder.
    pub fn for_vault(root: &Path) -> Option<Self> {
        Some(Self {
            config: crate::paths::state_file(root, "sync", "json")?,
            database: crate::paths::state_file(root, "sync", "redb")?,
        })
    }
}

impl SyncConfig {
    pub fn load(path: &Path) -> Result<Self, String> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| format!("Cannot read the sync settings: {e}")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("Cannot read the sync settings: {e}")),
        }
    }

    /// Writes through a temporary file, so a crash never leaves half a settings file.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let parent = path.parent().ok_or("Invalid sync settings path")?;
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Cannot save the sync settings: {e}"))?;
        let json = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)
            .map_err(|e| format!("Cannot save the sync settings: {e}"))?;
        temporary
            .write_all(&json)
            .and_then(|()| temporary.as_file().sync_all())
            .map_err(|e| format!("Cannot save the sync settings: {e}"))?;
        temporary
            .persist(path)
            .map_err(|e| format!("Cannot save the sync settings: {}", e.error))?;
        Ok(())
    }
}

/// A name for this computer to start from.
pub fn default_device_name() -> String {
    ["COMPUTERNAME", "HOSTNAME", "HOST"]
        .iter()
        .find_map(|name| std::env::var(name).ok())
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "this-device".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_missing_file_is_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a").join("sync.json");
        assert_eq!(SyncConfig::load(&path).unwrap(), SyncConfig::default());
        let config = SyncConfig {
            server: "http://100.1.2.3:8443".into(),
            device: "pc".into(),
            connected: true,
            enabled: true,
        };
        config.save(&path).unwrap();
        assert_eq!(SyncConfig::load(&path).unwrap(), config);
        std::fs::write(&path, b"not json").unwrap();
        assert!(SyncConfig::load(&path).is_err());
    }

    #[test]
    fn the_settings_file_has_no_secret_field() {
        let json = serde_json::to_string(&SyncConfig::default()).unwrap();
        for forbidden in ["token", "key", "passphrase", "secret"] {
            assert!(!json.contains(forbidden), "{json}");
        }
    }
}
