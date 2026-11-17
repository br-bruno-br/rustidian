//! `RUSTIDIAN_PERF`: one JSON line per measured action, with the vault's size.
//! This file holds the only test that turns measuring on: the switch is read once per process.

use rustidian::{perf, vault::Vault};
use std::{fs, path::Path};
use tempfile::tempdir;

#[test]
fn each_measured_action_writes_one_json_line() {
    let dir = tempdir().unwrap();
    let log = dir.path().join("perf.jsonl");
    assert!(perf::enable_at(&log));
    assert!(perf::enabled());
    // Turned on once: a second switch, from anywhere, is refused.
    assert!(!perf::enable_at(&dir.path().join("outro.jsonl")));

    let vault_dir = dir.path().join("cofre");
    fs::create_dir(&vault_dir).unwrap();
    fs::write(vault_dir.join("a.md"), "um\n").unwrap();
    fs::write(vault_dir.join("b.md"), "dois\n").unwrap();
    let mut vault = Vault::open(&vault_dir).unwrap();
    let mut document = vault.read(Path::new("a.md")).unwrap();
    document.text.push_str("mais");
    vault.save(&mut document).unwrap();
    assert_eq!(vault.query("mais").unwrap().len(), 1);
    perf::record("quadro_lento", std::time::Duration::from_millis(70), None);
    perf::record("com \"aspas\"\n", std::time::Duration::ZERO, Some((1, 2)));

    let lines: Vec<serde_json::Value> = fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("{line}: {e}")))
        .collect();
    let actions: Vec<&str> = lines
        .iter()
        .map(|line| line["acao"].as_str().unwrap())
        .collect();
    for expected in [
        "abrir_cofre",
        "salvar",
        "indexar_nota",
        "buscar",
        "quadro_lento",
    ] {
        assert!(actions.contains(&expected), "{expected} em {actions:?}");
    }
    let opened = &lines[actions.iter().position(|a| *a == "abrir_cofre").unwrap()];
    assert_eq!(opened["notas"], 2);
    assert_eq!(opened["bytes"], 3 + 5);
    assert!(opened["ms"].as_f64().unwrap() >= 0.0);
    assert!(opened["t"].as_u64().unwrap() > 1_700_000_000_000);
    let slow = &lines[actions.iter().position(|a| *a == "quadro_lento").unwrap()];
    assert!((slow["ms"].as_f64().unwrap() - 70.0).abs() < 0.01);
    assert!(slow.get("notas").is_none());
    // Aspas e quebra de linha no nome não quebram o formato.
    assert!(actions.contains(&"com \"aspas\"_"), "{actions:?}");
}
