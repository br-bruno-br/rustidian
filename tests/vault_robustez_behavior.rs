//! Pedido R12: o índice não refaz trabalho por gravação do próprio app nem por arquivo
//! temporário de editor, e não perde nota por uma varredura curta.

use rustidian::vault::{self, Vault};
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
