//! Sync of the open vault with a `nota-servidor`, on top of the `nota-sync` engine.
//!
//! - [`gate`]: the editor's writes and the sync cycle take turns on the vault's files;
//! - [`secrets`]: the vault key and the device token, in the system credential store;
//! - [`config`]: the settings that are not secrets;
//! - [`connect`]: first device (creates the key) and the others (open it with the passphrase);
//! - [`runner`]: the background thread and the status it publishes ([`status`]).
//!
//! Nothing here draws a window: the window code (`src/sync_ui.rs`) only calls these, and the
//! tests drive the same functions without a GUI.

pub mod config;
pub mod connect;
pub mod gate;
pub mod runner;
pub mod secrets;
pub mod status;
