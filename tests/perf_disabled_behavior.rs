//! Without `RUSTIDIAN_PERF` measuring is off: nothing is opened and nothing is written.

use rustidian::{perf, vault::Vault};
use std::fs;
use tempfile::tempdir;

#[test]
fn measuring_is_off_by_default_and_leaves_no_trace() {
    if std::env::var_os("RUSTIDIAN_PERF").is_some() {
        return; // The person running the tests turned it on.
    }
    assert!(!perf::enabled());
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.md"), "x").unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    vault.query("x").unwrap();
    perf::record("quadro_lento", std::time::Duration::from_millis(99), None);
    assert!(!perf::enabled());
    // Once off, it stays off for the whole process.
    assert!(!perf::enable_at(&dir.path().join("depois.jsonl")));
    assert!(!dir.path().join("depois.jsonl").exists());
}
