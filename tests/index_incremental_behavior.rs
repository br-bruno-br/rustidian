//! Saving or changing one note updates only that note's entries in the index. The result must be
//! what a full index of the same folder gives: the notes, their aliases and tags, how links
//! resolve and the backlinks of every note, in the same order.

use rustidian::vault::Vault;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::tempdir;

const TARGETS: &[&str] = &[
    "Alpha", "Beta", "Gamma", "Delta", "Épsilon", "Dir/Zeta", "Zeta", "Beta.md", "Nada", "",
];

fn write(root: &Path, name: &str, text: &str) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// Everything the index answers for `vault`, as comparable values.
fn answers(vault: &Vault) -> Vec<String> {
    let mut lines = Vec::new();
    for note in &vault.notes {
        lines.push(format!(
            "nota {} aliases={:?} tags={:?} texto={:?}",
            note.path.display(),
            note.aliases,
            note.tags,
            note.text
        ));
        let backlinks: Vec<_> = vault
            .backlinks(&note.path)
            .into_iter()
            .map(|from| from.path.clone())
            .collect();
        lines.push(format!("  backlinks {backlinks:?}"));
        for target in TARGETS {
            lines.push(format!(
                "  resolve {target:?} = {:?}",
                vault.resolve(&note.path, target)
            ));
        }
    }
    lines
}

#[track_caller]
fn same_as_full_index(vault: &Vault, step: &str) {
    let full = Vault::open(&vault.root).unwrap();
    let (incremental, expected) = (answers(vault), answers(&full));
    assert_eq!(incremental, expected, "after {step}");
}

fn edit(vault: &mut Vault, name: &str, text: &str) {
    let mut document = vault.read(Path::new(name)).unwrap();
    document.text = text.into();
    vault.save(&mut document).unwrap();
}

fn vault_with_notes(root: &Path) -> Vault {
    write(root, "Alpha.md", "# Alpha\nVai para [[Beta]] e [[Gamma]].\n");
    write(
        root,
        "Beta.md",
        "---\naliases: [Segundo]\ntags: [um]\n---\n[[Alpha]]\n",
    );
    write(root, "Gamma.md", "Sem links.\n");
    write(root, "Delta.md", "[[Delta]] e [[Alpha]]\n");
    write(root, "Épsilon.md", "[Texto](Beta.md) e ![[Gamma]]\n");
    write(root, "Dir/Zeta.md", "[[Zeta]] [[Beta]]\n");
    Vault::open(root).unwrap()
}

fn sequence(vault: &mut Vault) {
    // Add a link.
    edit(vault, "Gamma.md", "Agora [[Alpha]] e [[Beta]].\n");
    same_as_full_index(vault, "adding links");
    // Remove a link.
    edit(vault, "Alpha.md", "# Alpha\nSó [[Gamma]].\n");
    same_as_full_index(vault, "removing a link");
    // A note that links to itself, by title and by path.
    edit(vault, "Gamma.md", "[[Gamma]] [[Gamma.md]] [[Alpha]]\n");
    same_as_full_index(vault, "a self link");
    // Change an alias and a tag.
    edit(
        vault,
        "Beta.md",
        "---\naliases: [Terceiro, Outro]\ntags: [dois, três]\n---\n[[Alpha]] [[Delta]]\n",
    );
    same_as_full_index(vault, "changing aliases and tags");
    // Links to something that does not exist, and to a note by a path.
    edit(vault, "Delta.md", "[[Nada]] [[Dir/Zeta]] [[Épsilon]]\n");
    same_as_full_index(vault, "a missing target and a path link");
    // Drop every link, then the same text again (nothing to move).
    edit(vault, "Épsilon.md", "nada\n");
    same_as_full_index(vault, "dropping every link");
    edit(vault, "Épsilon.md", "nada\n");
    same_as_full_index(vault, "saving the same text");
    // Back to many links, in an order unlike the note order.
    edit(
        vault,
        "Dir/Zeta.md",
        "[[Delta]] [[Gamma]] [[Beta]] [[Alpha]] [[Épsilon]]\n",
    );
    same_as_full_index(vault, "linking every note");
}

#[test]
fn saving_a_note_updates_computed_backlinks_like_a_full_index() {
    let folder = tempdir().unwrap();
    let mut vault = vault_with_notes(folder.path());
    // Computed before the edits: the incremental path is the one under test.
    assert_eq!(vault.backlinks(Path::new("Alpha.md")).len(), 2);
    sequence(&mut vault);
}

#[test]
fn saving_a_note_before_any_backlinks_exist_matches_a_full_index() {
    let folder = tempdir().unwrap();
    let mut vault = vault_with_notes(folder.path());
    edit(&mut vault, "Gamma.md", "Agora [[Alpha]].\n");
    edit(&mut vault, "Alpha.md", "[[Gamma]]\n");
    // The first backlinks request comes after the edits.
    same_as_full_index(&vault, "edits before the first backlinks");
    sequence(&mut vault);
}

#[test]
fn only_the_saved_note_changes_and_the_order_of_notes_stays() {
    let folder = tempdir().unwrap();
    let mut vault = vault_with_notes(folder.path());
    let before: Vec<PathBuf> = vault.notes.iter().map(|note| note.path.clone()).collect();
    let others: Vec<String> = vault
        .notes
        .iter()
        .filter(|note| note.path != Path::new("Gamma.md"))
        .map(|note| note.text.clone())
        .collect();
    edit(&mut vault, "Gamma.md", "[[Alpha]]\n");
    let after: Vec<PathBuf> = vault.notes.iter().map(|note| note.path.clone()).collect();
    assert_eq!(before, after);
    let untouched: Vec<String> = vault
        .notes
        .iter()
        .filter(|note| note.path != Path::new("Gamma.md"))
        .map(|note| note.text.clone())
        .collect();
    assert_eq!(others, untouched);
}

#[test]
fn creating_a_note_still_matches_a_full_index() {
    let folder = tempdir().unwrap();
    let mut vault = vault_with_notes(folder.path());
    assert_eq!(vault.backlinks(Path::new("Alpha.md")).len(), 2);
    let mut document = rustidian::vault::Document::new(PathBuf::from("Novo.md"));
    document.text = "[[Alpha]] [[Beta]]\n".into();
    vault.save(&mut document).unwrap();
    same_as_full_index(&vault, "creating a note");
    edit(&mut vault, "Novo.md", "[[Gamma]]\n");
    same_as_full_index(&vault, "editing the new note");
}
