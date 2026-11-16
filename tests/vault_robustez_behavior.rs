//! Pedido R12: o índice não perde nota por uma varredura curta, não refaz trabalho por
//! gravação do próprio app nem por arquivo temporário de editor.

use rustidian::vault::{self, MAX_NOTE_BYTES, Vault};
use std::{
    collections::BTreeSet,
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

fn pair(from: &str, to: &str) -> (PathBuf, PathBuf) {
    (PathBuf::from(from), PathBuf::from(to))
}

// ---- Temporários de editor -------------------------------------------------------------------

#[test]
fn editor_temporaries_are_recognised_and_notes_are_not() {
    for temporary in [
        "nota.md~",
        "nota.md.tmp",
        "nota.md.swp",
        "dir/.nota.md.swp",
        "~$nota.md",
        "4913",
        "nota.md.sb-1f3a9c-xYz",
        "#nota.md#",
        "nota.md.part",
        "NOTA.MD.TMP",
    ] {
        assert!(
            vault::is_editor_temporary(Path::new(temporary)),
            "{temporary}"
        );
    }
    for note in [
        "nota.md",
        "2024.md",
        "dir/4913.md",
        "tmp.md",
        "Temp/nota.md",
        "nota~1.md",
        "#tag.md",
        "img.png",
    ] {
        assert!(!vault::is_editor_temporary(Path::new(note)), "{note}");
    }
}

#[test]
fn external_save_through_a_temporary_updates_the_note_in_place() {
    let dir = tempdir().unwrap();
    write(dir.path(), "nota.md", "antes");
    write(dir.path(), "outra.md", "outra");
    let mut vault = Vault::open(dir.path()).unwrap();
    // O editor grava no temporário, troca pelo arquivo de verdade e o vigia relata os dois.
    write(dir.path(), "nota.md.tmp", "depois");
    fs::rename(dir.path().join("nota.md.tmp"), dir.path().join("nota.md")).unwrap();
    let changed = vault
        .refresh_changed(&[
            PathBuf::from("nota.md.tmp"),
            PathBuf::from("nota.md~"),
            PathBuf::from("nota.md"),
        ])
        .expect("o temporário não pode obrigar a indexar tudo");
    assert_eq!(changed, [PathBuf::from("nota.md")]);
    assert_eq!(vault.note(Path::new("nota.md")).unwrap().text, "depois");
    assert_eq!(vault.note(Path::new("outra.md")).unwrap().text, "outra");
}

// ---- Gravação do próprio app -----------------------------------------------------------------

#[test]
fn what_the_app_saved_is_not_a_change() {
    let dir = tempdir().unwrap();
    write(dir.path(), "nota.md", "antes");
    let mut vault = Vault::open(dir.path()).unwrap();
    let mut document = vault.read(Path::new("nota.md")).unwrap();
    document.text = "gravado pelo app, com acentuação".into();
    vault.save(&mut document).unwrap();
    assert!(vault.holds_known_text(Path::new("nota.md"), None));

    // Outro programa grava, mesmo com o mesmo tamanho: agora é mudança.
    write(dir.path(), "nota.md", "gravado por outro, com acentuação");
    assert!(!vault.holds_known_text(Path::new("nota.md"), None));
    write(dir.path(), "nota.md", "gravado pelo app, com acentuação");
    assert!(vault.holds_known_text(Path::new("nota.md"), None));

    // Arquivo que sumiu, ou que o índice nunca viu, não está "conhecido".
    fs::remove_file(dir.path().join("nota.md")).unwrap();
    assert!(!vault.holds_known_text(Path::new("nota.md"), None));
    write(dir.path(), "nova.md", "x");
    assert!(!vault.holds_known_text(Path::new("nova.md"), None));
    // ... a menos que a aba tenha acabado de gravar esse texto.
    assert!(vault.holds_known_text(Path::new("nova.md"), Some("x")));
    // Temporários e arquivos ignorados nunca pedem trabalho.
    assert!(vault.holds_known_text(Path::new("nova.md.tmp"), None));
    assert!(vault.holds_known_text(Path::new(".obsidian/app.json"), None));
}

// ---- "Sumiu" só vale depois de conferir de novo ----------------------------------------------

/// Uma indexação nova do cofre, reconciliada com a anterior como o app faz.
fn rescan(
    previous: &Vault,
    doubtful: &mut BTreeSet<PathBuf>,
) -> (Vault, rustidian::vault::Reconciled) {
    let mut fresh = Vault::open(&previous.root).unwrap();
    let result = fresh.reconcile(previous, doubtful);
    (fresh, result)
}

#[test]
fn a_sync_that_deletes_and_recreates_never_drops_the_note() {
    let dir = tempdir().unwrap();
    write(dir.path(), "Pasta/nota.md", "conteúdo da nota");
    write(dir.path(), "fixa.md", "fixa");
    let first = Vault::open(dir.path()).unwrap();
    let mut doubtful = BTreeSet::new();

    // O sync apagou a pasta inteira e ainda não a recriou.
    fs::remove_dir_all(dir.path().join("Pasta")).unwrap();
    let (second, result) = rescan(&first, &mut doubtful);
    assert_eq!(paths(&second), ["fixa.md", "Pasta/nota.md"]);
    assert!(result.recheck && result.changed && result.renamed.is_empty());
    assert_eq!(doubtful.len(), 1);

    // Ela volta antes da próxima conferência.
    write(dir.path(), "Pasta/nota.md", "conteúdo da nota");
    let (third, result) = rescan(&second, &mut doubtful);
    assert_eq!(paths(&third), ["fixa.md", "Pasta/nota.md"]);
    assert!(!result.recheck && !result.changed);
    assert!(doubtful.is_empty());
}

#[test]
fn a_note_confirmed_gone_twice_leaves_the_index() {
    let dir = tempdir().unwrap();
    write(dir.path(), "a.md", "um");
    write(dir.path(), "b.md", "dois");
    let first = Vault::open(dir.path()).unwrap();
    let mut doubtful = BTreeSet::new();
    fs::remove_file(dir.path().join("b.md")).unwrap();

    let (second, result) = rescan(&first, &mut doubtful);
    assert_eq!(paths(&second), ["a.md", "b.md"]);
    assert!(result.recheck);

    let (third, result) = rescan(&second, &mut doubtful);
    assert_eq!(paths(&third), ["a.md"]);
    assert!(!result.recheck && !result.changed);
    assert!(doubtful.is_empty());
    // Já tirada, não volta numa terceira.
    let (fourth, _) = rescan(&third, &mut doubtful);
    assert_eq!(paths(&fourth), ["a.md"]);
}

#[test]
fn a_short_scan_keeps_a_note_that_is_still_on_disk() {
    let dir = tempdir().unwrap();
    write(dir.path(), "a.md", "um");
    // Esta varredura é "curta": acontece antes de b.md existir.
    let short = Vault::open(dir.path()).unwrap();
    write(dir.path(), "b.md", "dois");
    let previous = Vault::open(dir.path()).unwrap();
    let mut doubtful = BTreeSet::new();
    let mut scanned = short;
    let result = scanned.reconcile(&previous, &mut doubtful);
    assert_eq!(paths(&scanned), ["a.md", "b.md"]);
    assert!(result.changed);
    // O arquivo existe: não há o que reconferir.
    assert!(!result.recheck && doubtful.is_empty());
}

#[test]
fn a_unique_pair_with_the_same_text_is_a_rename() {
    let dir = tempdir().unwrap();
    write(dir.path(), "velho.md", "texto único do par");
    write(dir.path(), "outra.md", "outra");
    let first = Vault::open(dir.path()).unwrap();
    let mut doubtful = BTreeSet::new();
    fs::rename(dir.path().join("velho.md"), dir.path().join("novo.md")).unwrap();

    let (second, result) = rescan(&first, &mut doubtful);
    assert_eq!(result.renamed, [pair("velho.md", "novo.md")]);
    assert_eq!(paths(&second), ["novo.md", "outra.md"]);
    assert!(!result.recheck && doubtful.is_empty());
}

#[test]
fn an_ambiguous_pair_is_a_deletion_and_a_creation() {
    let dir = tempdir().unwrap();
    write(dir.path(), "a.md", "igual");
    write(dir.path(), "b.md", "igual");
    let first = Vault::open(dir.path()).unwrap();
    let mut doubtful = BTreeSet::new();
    // Dois apagados e um criado com o mesmo texto: não dá para dizer qual virou qual.
    fs::remove_file(dir.path().join("a.md")).unwrap();
    fs::remove_file(dir.path().join("b.md")).unwrap();
    write(dir.path(), "c.md", "igual");
    let (second, result) = rescan(&first, &mut doubtful);
    assert!(result.renamed.is_empty());
    assert_eq!(paths(&second), ["a.md", "b.md", "c.md"]);
    let (third, _) = rescan(&second, &mut doubtful);
    assert_eq!(paths(&third), ["c.md"]);

    // Um apagado e dois criados com o mesmo texto: o mesmo.
    let mut doubtful = BTreeSet::new();
    let before = Vault::open(dir.path()).unwrap();
    fs::remove_file(dir.path().join("c.md")).unwrap();
    write(dir.path(), "e.md", "igual");
    write(dir.path(), "f.md", "igual");
    let (_, result) = rescan(&before, &mut doubtful);
    assert!(result.renamed.is_empty());
}

#[test]
fn empty_notes_are_never_paired() {
    let dir = tempdir().unwrap();
    write(dir.path(), "vazia.md", "");
    write(dir.path(), "branca.md", "  \n");
    let first = Vault::open(dir.path()).unwrap();
    let mut doubtful = BTreeSet::new();
    fs::remove_file(dir.path().join("vazia.md")).unwrap();
    fs::remove_file(dir.path().join("branca.md")).unwrap();
    write(dir.path(), "nova.md", "");
    write(dir.path(), "outra.md", "  \n");
    let (_, result) = rescan(&first, &mut doubtful);
    assert!(result.renamed.is_empty());
    assert!(result.recheck);
}

#[test]
fn a_changed_text_is_not_a_rename() {
    let dir = tempdir().unwrap();
    write(dir.path(), "velho.md", "um texto");
    let first = Vault::open(dir.path()).unwrap();
    let mut doubtful = BTreeSet::new();
    fs::remove_file(dir.path().join("velho.md")).unwrap();
    write(dir.path(), "novo.md", "outro texto");
    let (second, result) = rescan(&first, &mut doubtful);
    assert!(result.renamed.is_empty());
    assert_eq!(paths(&second), ["novo.md", "velho.md"]);
}

#[test]
fn oversized_notes_are_not_adopted_back_from_the_previous_index() {
    let dir = tempdir().unwrap();
    write(dir.path(), "cresce.md", "pequena");
    let first = Vault::open(dir.path()).unwrap();
    write(
        dir.path(),
        "cresce.md",
        &"x".repeat(MAX_NOTE_BYTES as usize + 1),
    );
    let mut doubtful = BTreeSet::new();
    let (second, result) = rescan(&first, &mut doubtful);
    assert!(second.notes.is_empty());
    assert!(!result.recheck && !result.changed);
    assert_eq!(second.warnings.len(), 1);
}

#[test]
fn a_path_spelled_with_the_other_separator_is_the_same_note() {
    let dir = tempdir().unwrap();
    write(dir.path(), "Dir/nota.md", "texto");
    let mut vault = Vault::open(dir.path()).unwrap();
    // O app grava com a barra comum; a varredura devolve o separador do sistema.
    let mut document = rustidian::vault::Document::new(PathBuf::from("Dir/nova.md"));
    document.text = "nova".into();
    vault.save(&mut document).unwrap();
    assert!(vault.holds_known_text(Path::new("Dir/nova.md"), None));
    assert!(vault.holds_known_text(&Path::new("Dir").join("nova.md"), None));

    let mut doubtful = BTreeSet::new();
    let (scanned, result) = rescan(&vault, &mut doubtful);
    assert_eq!(scanned.notes.len(), 2);
    assert_eq!(result, rustidian::vault::Reconciled::default());
}
