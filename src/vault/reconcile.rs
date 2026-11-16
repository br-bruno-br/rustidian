//! What a full re-index of an open vault does with the notes it did not find.
//!
//! A scan can come up short: a sync tool deletes a folder and recreates it a moment later, an
//! editor saves through a temporary file, a directory cannot be read for an instant. Dropping
//! every note the scan missed would make such a note vanish from the tabs, the graph and the
//! backlinks, and come back a second later. So a note the scan missed leaves the index only when
//! the file is confirmed gone twice in a row (a `stat` at each scan), and a pair "one note gone,
//! one new note with the same text" is a rename only when it is the only such pair.

use super::{Note, Vault};
use std::{
    collections::{BTreeSet, HashMap},
    fs,
    path::PathBuf,
};

#[derive(Debug, Default, PartialEq)]
pub struct Reconciled {
    /// Notes that moved: the old path as the previous index spelled it, the new one as this
    /// scan did. Only a note and a new note with the same, non-blank text, each alone with that
    /// text, are paired; anything ambiguous stays a deletion plus a creation.
    pub renamed: Vec<(PathBuf, PathBuf)>,
    /// Some note is only doubtfully gone: it is kept for now and a new scan should confirm it.
    pub recheck: bool,
    /// The notes of this vault differ from what the scan found by itself.
    pub changed: bool,
}

impl Vault {
    /// Reconcile this fresh scan with the `previous` index of the same vault. `doubtful` holds
    /// the notes the last scan already missed; it is replaced by those this scan missed.
    pub fn reconcile(&mut self, previous: &Vault, doubtful: &mut BTreeSet<PathBuf>) -> Reconciled {
        let mut result = Reconciled::default();
        if previous.root != self.root {
            doubtful.clear();
            return result;
        }
        let missed: Vec<&Note> = previous
            .notes
            .iter()
            .filter(|note| {
                !self.paths.contains_key(&note.path) && !self.oversized.contains(&note.path)
            })
            .collect();
        let mut gone = Vec::new();
        let mut present = Vec::new();
        for note in missed {
            match fs::symlink_metadata(self.root.join(&note.path)) {
                Ok(metadata) if metadata.is_file() => present.push(note),
                Ok(_) => gone.push(note),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => gone.push(note),
                // Not being able to look is not evidence that the file is gone.
                Err(_) => present.push(note),
            }
        }
        // One note gone and one new note with the same text, each alone with it, is a rename.
        let created: Vec<&Note> = self
            .notes
            .iter()
            .filter(|note| !previous.paths.contains_key(&note.path))
            .collect();
        let mut by_text: HashMap<&str, (Vec<&Note>, Vec<&Note>)> = HashMap::new();
        for note in &gone {
            if !note.text.trim().is_empty() {
                by_text.entry(&note.text).or_default().0.push(note);
            }
        }
        for note in &created {
            if !note.text.trim().is_empty()
                && let Some(entry) = by_text.get_mut(note.text.as_str())
            {
                entry.1.push(note);
            }
        }
        let mut renamed_from = BTreeSet::new();
        for (old, new) in by_text.values() {
            if let ([old], [new]) = (old.as_slice(), new.as_slice()) {
                renamed_from.insert(old.path.clone());
                result.renamed.push((old.path.clone(), new.path.clone()));
            }
        }
        result.renamed.sort();
        let mut keep: Vec<Note> = present.into_iter().cloned().collect();
        let mut still_doubtful = BTreeSet::new();
        for note in gone {
            if renamed_from.contains(&note.path) {
                continue;
            }
            // The first time a file is missed it stays; the second time it is gone.
            if !doubtful.contains(&note.path) {
                still_doubtful.insert(note.path.clone());
                keep.push(note.clone());
                result.recheck = true;
            }
        }
        *doubtful = still_doubtful;
        if !keep.is_empty() {
            self.notes.extend(keep);
            self.notes
                .sort_by_key(|note| note.path.to_string_lossy().to_lowercase());
            self.rebuild_lookup();
            result.changed = true;
        }
        result
    }
}
