//! Sync of the open vault with a `nota-servidor`, on top of the `nota-sync` engine.
//!
//! - [`secrets`]: the vault key and the device token, in the system credential store;
//! - [`config`]: the settings that are not secrets;
//! - [`connect`]: first device (creates the key) and the others (open it with the passphrase).

pub mod config;
pub mod connect;
pub mod secrets;
