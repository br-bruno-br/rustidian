//! Where the vault key (K) and the device token live: the operating system's credential store
//! (Windows Credential Manager, Secret Service on Linux), through the `keyring` crate. Never a
//! file, and never the passphrase: that one is typed once and dropped.
//!
//! The store is a trait so tests use `MemoryStore` and never touch the real credential store.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, PoisonError};

use nota_sync::protocolo::sha256_hex;

/// The name this app files its entries under in the credential store.
pub const SERVICE: &str = "rustidian-sync";

pub trait SecretStore: Send + Sync {
    /// `None` when there is no such entry.
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>, String>;
    fn set(&self, account: &str, value: &[u8]) -> Result<(), String>;
    /// Deleting something that is not there is not an error.
    fn delete(&self, account: &str) -> Result<(), String>;
}

/// The key of a vault's entry: a short stable id of its folder, then what it holds.
pub fn account(vault: &Path, what: Secret) -> String {
    let id = sha256_hex(vault.to_string_lossy().as_bytes());
    format!("{}:{}", &id[..16], what.name())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Secret {
    /// The vault key K (32 bytes).
    Key,
    /// This device's bearer token.
    Token,
}

impl Secret {
    fn name(self) -> &'static str {
        match self {
            Secret::Key => "key",
            Secret::Token => "token",
        }
    }
}

/// The real thing. The error texts come from the credential store and never hold a secret.
pub struct KeyringStore;

fn entry(account: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, account).map_err(|e| format!("system credential store: {e}"))
}

impl SecretStore for KeyringStore {
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>, String> {
        match entry(account)?.get_secret() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("system credential store: {e}")),
        }
    }

    fn set(&self, account: &str, value: &[u8]) -> Result<(), String> {
        entry(account)?
            .set_secret(value)
            .map_err(|e| format!("system credential store: {e}"))
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        match entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("system credential store: {e}")),
        }
    }
}

/// For tests: the same contract, in memory.
#[derive(Default)]
pub struct MemoryStore {
    entries: Mutex<HashMap<String, Vec<u8>>>,
}

impl MemoryStore {
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Vec<u8>>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl SecretStore for MemoryStore {
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>, String> {
        Ok(self.lock().get(account).cloned())
    }

    fn set(&self, account: &str, value: &[u8]) -> Result<(), String> {
        self.lock().insert(account.to_owned(), value.to_vec());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        self.lock().remove(account);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accounts_differ_by_vault_and_by_secret() {
        let a = account(Path::new("C:/a"), Secret::Key);
        assert_ne!(a, account(Path::new("C:/b"), Secret::Key));
        assert_ne!(a, account(Path::new("C:/a"), Secret::Token));
        assert_eq!(a, account(Path::new("C:/a"), Secret::Key));
        assert!(!a.contains("C:/a"));
    }

    #[test]
    fn memory_store_follows_the_contract() {
        let store = MemoryStore::default();
        assert_eq!(store.get("x").unwrap(), None);
        store.set("x", b"1").unwrap();
        assert_eq!(store.get("x").unwrap().as_deref(), Some(&b"1"[..]));
        store.delete("x").unwrap();
        store.delete("x").unwrap();
        assert!(store.is_empty());
    }
}
