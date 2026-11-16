//! The index photo is a cache: opening with it must give what opening without it gives, a note
//! is read again whenever its size or time differ from the photo's, and a photo that is missing,
//! cut, altered, of another version or of another vault is ignored without an error.

use rustidian::{index_photo::IndexPhoto, vault::Vault};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use tempfile::tempdir;

fn write(root: &Path, name: &str, text: &str) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, text).unwrap();
    age(&path);
}

/// An hour old, so the photo trusts the file; a file touched just now is not trusted.
fn age(path: &Path) {
    set_time(path, SystemTime::now() - Duration::from_secs(3600));
}

fn set_time(path: &Path, time: SystemTime) {
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(time)
        .unwrap();
}

fn modified(path: &Path) -> SystemTime {
    fs::metadata(path).unwrap().modified().unwrap()
}

fn fill(root: &Path) {
    for number in 0..24 {
        let folder = ["", "Dir/", "Dir/Sub/", "Épsilon/"][number % 4];
        let alias = if number % 3 == 0 {
            format!("---\naliases: [Apelido {number}]\ntags: [t{number}]\n---\n")
        } else {
            String::new()
        };
        write(
            root,
            &format!("{folder}Nota {number:02}.md"),
            &format!(
                "{alias}# Nota {number}\nLink [[Nota {:02}]] e [[Nota {:02}]]. ação\n",
                (number + 1) % 24,
                (number + 5) % 24
            ),
        );
    }
    write(root, "figura.png", "not really an image");
    write(root, "texto.txt", "texto simples");
    write(root, ".oculto/Nota escondida.md", "invisível");
}

/// What a vault answers, as comparable text.
fn answers(vault: &Vault) -> Vec<String> {
    let mut lines = vec![
        format!("attachments {:?}", vault.attachments),
        format!("folders {:?}", vault.folders),
        format!("warnings {:?}", vault.warnings),
    ];
    for note in &vault.notes {
        lines.push(format!(
            "{} {:?} {:?} {:?}",
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
    }
    lines
}

fn open(root: &Path, photo: &Path) -> Vault {
    Vault::open_with_photo(root, IndexPhoto::At(photo.to_owned())).unwrap()
}

fn open_plain(root: &Path) -> Vault {
    Vault::open_with_photo(root, IndexPhoto::Off).unwrap()
}

#[track_caller]
fn as_if_without_photo(vault: &Vault) {
    assert_eq!(answers(vault), answers(&open_plain(&vault.root)));
}

#[test]
fn the_first_open_writes_the_photo_and_the_second_reads_no_note() {
    let folder = tempdir().unwrap();
    let photos = tempdir().unwrap();
    let photo = photos.path().join("indice.photo");
    fill(folder.path());
    let first = open(folder.path(), &photo);
    assert!(!first.photo.found && first.photo.written);
    assert_eq!((first.photo.reused, first.photo.read), (0, 24));
    as_if_without_photo(&first);
    let second = open(folder.path(), &photo);
    assert!(second.photo.found && !second.photo.written);
    assert_eq!((second.photo.reused, second.photo.read, second.photo.gone), (24, 0, 0));
    as_if_without_photo(&second);
    assert_eq!(answers(&first), answers(&second));
    // Without a photo nothing is written and nothing is reused.
    let plain = open_plain(folder.path());
    assert_eq!((plain.photo.reused, plain.photo.read), (0, 24));
    assert!(!plain.photo.written);
}

#[test]
fn a_note_with_the_recorded_size_and_time_is_reused_without_reading() {
    let folder = tempdir().unwrap();
    let photos = tempdir().unwrap();
    let photo = photos.path().join("indice.photo");
    fill(folder.path());
    open(folder.path(), &photo);
    let target = folder.path().join("Dir/Nota 01.md");
    let (time, original) = (modified(&target), fs::read_to_string(&target).unwrap());
    // Same size and time: the photo's text is used, which shows the file was not read.
    let same_size = original.replace("ação", "AÇÃO");
    assert_eq!(same_size.len(), original.len());
    fs::write(&target, &same_size).unwrap();
    set_time(&target, time);
    let reused = open(folder.path(), &photo);
    assert_eq!(reused.photo.reused, 24);
    let note = reused.notes.iter().find(|n| n.path.ends_with("Nota 01.md")).unwrap();
    assert_eq!(note.text, original);
    // A different size or time is read.
    fs::write(&target, format!("{same_size}mais\n")).unwrap();
    set_time(&target, time);
    let by_size = open(folder.path(), &photo);
    assert_eq!((by_size.photo.reused, by_size.photo.read), (23, 1));
    as_if_without_photo(&by_size);
    fs::write(&target, &same_size).unwrap();
    set_time(&target, time + Duration::from_secs(60));
    let by_time = open(folder.path(), &photo);
    assert_eq!((by_time.photo.reused, by_time.photo.read), (23, 1));
    assert!(by_time.notes.iter().any(|n| n.text == same_size));
    as_if_without_photo(&by_time);
}

#[test]
fn changed_new_and_removed_notes_match_opening_without_the_photo() {
    let folder = tempdir().unwrap();
    let photos = tempdir().unwrap();
    let photo = photos.path().join("indice.photo");
    fill(folder.path());
    open(folder.path(), &photo);
    write(folder.path(), "Dir/Nota 01.md", "[[Nota 02]] mudou\n");
    write(folder.path(), "Nota 02.md", "[[Nova]]\n");
    write(folder.path(), "Nova.md", "---\naliases: [Recém]\n---\n[[Nota 00]]\n");
    write(folder.path(), "Dir/Nova dentro.md", "dentro\n");
    fs::remove_file(folder.path().join("Nota 04.md")).unwrap();
    fs::remove_file(folder.path().join("Dir/Sub/Nota 06.md")).unwrap();
    let after = open(folder.path(), &photo);
    assert!(after.photo.found);
    assert_eq!(after.photo.gone, 2);
    assert_eq!(after.photo.read, 4);
    assert_eq!(after.photo.reused, 24 - 2 - 1);
    as_if_without_photo(&after);
    // Opening changed the photo, so the next open reads nothing.
    assert!(after.photo.written);
    let next = open(folder.path(), &photo);
    assert_eq!((next.photo.read, next.photo.gone), (0, 0));
    assert_eq!(next.photo.reused, next.notes.len());
    assert_eq!(answers(&after), answers(&next));
}

#[test]
fn notes_touched_just_before_the_photo_are_read_again_next_time() {
    let folder = tempdir().unwrap();
    let photos = tempdir().unwrap();
    let photo = photos.path().join("indice.photo");
    fill(folder.path());
    fs::write(folder.path().join("Recente.md"), "agora\n").unwrap();
    let first = open(folder.path(), &photo);
    assert!(first.photo.written);
    let second = open(folder.path(), &photo);
    assert_eq!((second.photo.reused, second.photo.read), (24, 1));
    as_if_without_photo(&second);
}

#[test]
fn a_missing_or_damaged_photo_opens_normally_and_is_replaced() {
    let folder = tempdir().unwrap();
    let photos = tempdir().unwrap();
    let photo = photos.path().join("indice.photo");
    fill(folder.path());
    let expected = answers(&open_plain(folder.path()));
    // Missing.
    let missing = open(folder.path(), &photo);
    assert!(!missing.photo.found);
    assert_eq!(answers(&missing), expected);
    let good = fs::read(&photo).unwrap();
    let damages: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("garbage", b"isto nao e uma foto do indice".to_vec()),
        ("cut at the start", good[..10].to_vec()),
        ("cut in the middle", good[..good.len() / 2].to_vec()),
        ("cut at the end", good[..good.len() - 1].to_vec()),
        ("bytes after the end", [good.clone(), vec![0]].concat()),
        ("another layout", {
            let mut other = good.clone();
            other[8] ^= 0xff;
            other
        }),
        ("a changed byte in the middle", {
            let mut other = good.clone();
            let middle = other.len() / 2;
            other[middle] ^= 0x01;
            other
        }),
        ("a changed closing hash", {
            let mut other = good.clone();
            let last = other.len() - 1;
            other[last] ^= 0x01;
            other
        }),
        ("an enormous count", {
            // The entry count follows the magic, layout, version and root.
            let mut other = good.clone();
            let at = 8 + 4 + 8 + env!("CARGO_PKG_VERSION").len() + 8
                + folder.path().canonicalize().unwrap().to_str().unwrap().len();
            other[at..at + 8].copy_from_slice(&u64::MAX.to_le_bytes());
            other
        }),
    ];
    for (name, bytes) in damages {
        fs::write(&photo, bytes).unwrap();
        let vault = open(folder.path(), &photo);
        assert!(!vault.photo.found, "{name}");
        assert_eq!(answers(&vault), expected, "{name}");
        // The damaged photo is replaced by a good one.
        assert!(vault.photo.written, "{name}");
        assert!(open(folder.path(), &photo).photo.found, "{name}");
    }
    // A photo that is a folder cannot be written, and the open still works.
    let blocked = photos.path().join("pasta.photo");
    fs::create_dir(&blocked).unwrap();
    let vault = open(folder.path(), &blocked);
    assert!(!vault.photo.found && !vault.photo.written);
    assert_eq!(answers(&vault), expected);
}

#[test]
fn a_photo_of_another_vault_is_ignored() {
    let first = tempdir().unwrap();
    let second = tempdir().unwrap();
    let photos = tempdir().unwrap();
    let photo = photos.path().join("indice.photo");
    fill(first.path());
    write(second.path(), "Outra.md", "outra nota\n");
    open(first.path(), &photo);
    let other = open(second.path(), &photo);
    assert!(!other.photo.found);
    assert_eq!(other.notes.len(), 1);
    assert_eq!(answers(&other), answers(&open_plain(second.path())));
}

#[test]
fn the_apps_own_photo_is_only_for_big_vaults_and_never_changes_the_result() {
    // Small vault, app's own location: opens the same as without a photo.
    let folder = tempdir().unwrap();
    fill(folder.path());
    let vault = Vault::open(folder.path()).unwrap();
    assert!(!vault.photo.written);
    assert_eq!(answers(&vault), answers(&open_plain(folder.path())));
    let photo: PathBuf = folder.path().join("indice.photo");
    assert!(!photo.exists());
}
