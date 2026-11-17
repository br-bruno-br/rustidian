//! Pedido R12: nota Markdown acima do teto de tamanho fica fora do índice, com um aviso só,
//! e sem ser lida inteira.

use rustidian::vault::{MAX_NOTE_BYTES, Vault};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::tempdir;

fn write(root: &Path, name: &str, text: &str) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn paths(vault: &Vault) -> Vec<String> {
    vault
        .notes
        .iter()
        .map(|note| note.path.to_string_lossy().replace('\\', "/"))
        .collect()
}

#[test]
fn oversized_note_stays_out_of_the_index_with_one_warning() {
    let dir = tempdir().unwrap();
    write(dir.path(), "pequena.md", "oi");
    // Não é UTF-8 válido: se o arquivo fosse lido, o aviso seria de erro de leitura.
    fs::write(
        dir.path().join("enorme.md"),
        vec![0xff; MAX_NOTE_BYTES as usize + 1],
    )
    .unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(
        dir.path().join("sub/grande.md"),
        vec![b'a'; MAX_NOTE_BYTES as usize + 10],
    )
    .unwrap();

    let vault = Vault::open(dir.path()).unwrap();
    assert_eq!(paths(&vault), ["pequena.md"]);
    assert_eq!(vault.warnings.len(), 1, "um aviso só: {:?}", vault.warnings);
    let warning = &vault.warnings[0];
    assert!(warning.contains("2 notas maiores que 8 MB"), "{warning}");
    assert!(
        warning.contains("enorme.md") && warning.contains("grande.md"),
        "{warning}"
    );
    assert!(!warning.contains("pequena.md"));
}

#[test]
fn a_note_exactly_at_the_ceiling_is_indexed() {
    let dir = tempdir().unwrap();
    write(
        dir.path(),
        "limite.md",
        &"a".repeat(MAX_NOTE_BYTES as usize),
    );
    let vault = Vault::open(dir.path()).unwrap();
    assert_eq!(paths(&vault), ["limite.md"]);
    assert!(vault.warnings.is_empty(), "{:?}", vault.warnings);
}

#[test]
fn a_note_that_grows_past_the_ceiling_leaves_through_the_full_index() {
    let dir = tempdir().unwrap();
    write(dir.path(), "cresce.md", "pequena");
    let mut vault = Vault::open(dir.path()).unwrap();
    write(
        dir.path(),
        "cresce.md",
        &"x".repeat(MAX_NOTE_BYTES as usize + 1),
    );
    // O caminho rápido recusa; quem decide é a indexação completa.
    assert_eq!(vault.refresh_changed(&[PathBuf::from("cresce.md")]), None);
    assert_eq!(vault.notes[0].text, "pequena");
    let reopened = Vault::open(dir.path()).unwrap();
    assert!(reopened.notes.is_empty());
    assert_eq!(reopened.warnings.len(), 1);
}

#[test]
fn many_oversized_notes_name_only_a_few() {
    let dir = tempdir().unwrap();
    for number in 0..8 {
        fs::write(
            dir.path().join(format!("n{number}.md")),
            vec![b'a'; MAX_NOTE_BYTES as usize + 1],
        )
        .unwrap();
    }
    let vault = Vault::open(dir.path()).unwrap();
    assert_eq!(vault.warnings.len(), 1);
    assert!(vault.warnings[0].contains("8 notas") && vault.warnings[0].contains("e mais 3"));
}
