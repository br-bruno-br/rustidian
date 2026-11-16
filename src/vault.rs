use crate::index_photo::{self, IndexPhoto, PhotoReport, Stamp};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};
use tempfile::NamedTempFile;
use walkdir::WalkDir;

pub type Result<T> = std::result::Result<T, String>;

/// The app's own index photo is kept for vaults of at least this many notes.
const PHOTO_MIN_NOTES: usize = 1000;
/// ... and rewritten when more than this many notes, plus one in this many of the vault, were
/// read or dropped on opening.
const PHOTO_STALE_BASE: usize = 50;
const PHOTO_STALE_SHARE: usize = 50;

/// Navigation and watcher refreshes share the same development-clutter policy.
pub fn is_ignored_path(relative: &Path) -> bool {
    relative.components().any(|part| {
        let Component::Normal(name) = part else {
            return false;
        };
        name.as_encoded_bytes().first() == Some(&b'.')
            || matches!(
                name.to_str(),
                Some("node_modules" | "__pycache__" | "target" | "vendor" | "dist" | "build")
            )
    })
}

#[derive(Clone, Debug)]
pub struct Note {
    pub path: PathBuf,
    pub text: String,
    pub aliases: Vec<String>,
    pub tags: Vec<String>,
    folded: std::sync::OnceLock<String>,
}

#[derive(Clone, Debug)]
pub struct LinkedMention {
    pub path: PathBuf,
    pub range: std::ops::Range<usize>,
    pub line: usize,
    pub before: String,
    pub link: String,
    pub after: String,
}

/// Most unlinked mentions listed for one note.
pub const UNLINKED_LIMIT: usize = 200;

impl LinkedMention {
    /// The mention of `text[range]` in the note at `from`, with up to 50 chars before and 70
    /// after it on the same line.
    fn at(from: &Path, text: &str, range: std::ops::Range<usize>) -> Self {
        let line_start = text[..range.start].rfind('\n').map_or(0, |start| start + 1);
        let line_end = text[range.end..]
            .find('\n')
            .map_or(text.len(), |end| range.end + end);
        let prefix = &text[line_start..range.start];
        let suffix = text[range.end..line_end].trim_end_matches('\r');
        let mut before: String = prefix
            .chars()
            .rev()
            .take(50)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if prefix.chars().count() > 50 {
            before.insert(0, '…');
        }
        let mut after: String = suffix.chars().take(70).collect();
        if suffix.chars().count() > 70 {
            after.push('…');
        }
        LinkedMention {
            path: from.to_owned(),
            line: text[..range.start]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1,
            link: text[range.clone()].into(),
            range,
            before,
            after,
        }
    }

    /// Visible link text, while `link` and `range` retain the exact source for navigation.
    pub fn label(&self) -> String {
        let raw = self.link.strip_prefix('!').unwrap_or(&self.link);
        if let Some(wiki) = raw
            .strip_prefix("[[")
            .and_then(|link| link.strip_suffix("]]"))
        {
            return wiki.split_once('|').map_or(wiki, |(_, label)| label).into();
        }
        use pulldown_cmark::{Event, Options, Parser};
        // Reference-style links need their label even when their definition is elsewhere.
        let mut reference = |_link: pulldown_cmark::BrokenLink<'_>| Some(("".into(), "".into()));
        let mut label = String::new();
        for event in Parser::new_with_broken_link_callback(
            &self.link,
            Options::empty(),
            Some(&mut reference),
        ) {
            match event {
                Event::Text(text) | Event::Code(text) => label.push_str(&text),
                Event::SoftBreak | Event::HardBreak => label.push(' '),
                _ => {}
            }
        }
        if label.is_empty() {
            self.link.clone()
        } else {
            label
        }
    }
}

impl Note {
    pub(crate) fn new(path: PathBuf, text: String) -> Self {
        let metadata = crate::metadata::analyze(&text);
        Self {
            path,
            text,
            aliases: metadata.aliases,
            tags: metadata.tags,
            folded: std::sync::OnceLock::new(),
        }
    }
    /// A note whose text, aliases and tags the index photo already holds.
    pub(crate) fn from_photo(
        path: PathBuf,
        text: String,
        aliases: Vec<String>,
        tags: Vec<String>,
    ) -> Self {
        Self {
            path,
            text,
            aliases,
            tags,
            folded: std::sync::OnceLock::new(),
        }
    }
    pub fn folded_text(&self) -> &str {
        self.folded.get_or_init(|| self.text.to_lowercase())
    }
    pub fn title(&self) -> String {
        file_title(&self.path)
    }
}

#[derive(Clone, Debug)]
pub struct Document {
    pub path: PathBuf,
    pub text: String,
    original: Option<String>,
    saved_revision: u64,
    kind: DocumentKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DocumentKind {
    Note,
    Image,
}

impl Document {
    fn next_revision() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }

    /// Changes only when saved bytes are loaded or successfully committed.
    pub fn saved_revision(&self) -> u64 {
        self.saved_revision
    }

    pub fn saved_text(&self) -> Option<&str> {
        self.original.as_deref()
    }

    pub fn blank() -> Self {
        Self {
            path: PathBuf::new(),
            text: String::new(),
            original: Some(String::new()),
            saved_revision: Self::next_revision(),
            kind: DocumentKind::Note,
        }
    }

    pub fn is_blank(&self) -> bool {
        self.path.as_os_str().is_empty()
    }

    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            text: String::new(),
            original: None,
            saved_revision: Self::next_revision(),
            kind: DocumentKind::Note,
        }
    }

    pub fn dirty(&self) -> bool {
        !self.is_image() && self.original.as_deref() != Some(&self.text)
    }
    pub fn is_image(&self) -> bool {
        self.kind == DocumentKind::Image
    }

    pub fn is_plain_text(&self) -> bool {
        !self.is_blank() && !self.is_image() && !is_markdown(&self.path)
    }

    pub fn title(&self) -> String {
        if self.is_blank() {
            "New tab".into()
        } else {
            file_title(&self.path)
        }
    }
}

#[derive(Clone)]
pub struct Vault {
    pub root: PathBuf,
    pub notes: Vec<Note>,
    pub warnings: Vec<String>,
    pub index_time: Duration,
    /// What the index photo did when this vault was opened.
    pub photo: PhotoReport,
    pub attachments: Vec<PathBuf>,
    pub folders: Vec<PathBuf>,
    /// A folder around the default vault (Masih's ~/Desktop around ~/Desktop/o)
    /// holds every project below it: 12,249 notes and 1 GB on 3 Nov. Such a
    /// folder loads only its own files, not its subfolders.
    loose: bool,
    /// Files of other vaults this window holds as tabs, by absolute path: an
    /// agent's file opened beside the note being read. They are read and saved
    /// where they are and never enter the index, the tree or the graph.
    guests: std::collections::BTreeSet<PathBuf>,
    paths: HashMap<PathBuf, usize>,
    names: HashMap<String, Vec<usize>>,
    incoming: std::sync::OnceLock<HashMap<PathBuf, Vec<usize>>>,
}

impl Vault {
    pub fn open(root: &Path) -> Result<Self> {
        Self::open_cancellable(root, || false)
    }

    pub fn open_cancellable(root: &Path, cancelled: impl Fn() -> bool) -> Result<Self> {
        let default = crate::vaults::default_vault().and_then(|vault| vault.canonicalize().ok());
        Self::open_beside(root, default.as_deref(), cancelled)
    }

    /// Open `root` with the index photo at `photo` instead of the app's own.
    pub fn open_with_photo(root: &Path, photo: IndexPhoto) -> Result<Self> {
        let default = crate::vaults::default_vault().and_then(|vault| vault.canonicalize().ok());
        Self::open_beside_with(root, default.as_deref(), || false, photo)
    }

    /// Open `root` where `default` is the default vault.
    pub fn open_beside(
        root: &Path,
        default: Option<&Path>,
        cancelled: impl Fn() -> bool,
    ) -> Result<Self> {
        Self::open_beside_with(root, default, cancelled, IndexPhoto::Auto)
    }

    fn open_beside_with(
        root: &Path,
        default: Option<&Path>,
        cancelled: impl Fn() -> bool,
        photo: IndexPhoto,
    ) -> Result<Self> {
        let root = root
            .canonicalize()
            .map_err(|e| format!("Não foi possível abrir o cofre: {e}"))?;
        if !root.is_dir() {
            return Err("Escolha uma pasta para o seu cofre.".into());
        }
        let mut vault = Self {
            loose: crate::vaults::encloses(&root, default),
            root,
            notes: Vec::new(),
            warnings: Vec::new(),
            index_time: Duration::ZERO,
            photo: PhotoReport::default(),
            attachments: Vec::new(),
            folders: Vec::new(),
            guests: std::collections::BTreeSet::new(),
            paths: HashMap::new(),
            names: HashMap::new(),
            incoming: std::sync::OnceLock::new(),
        };
        vault.refresh_cancellable(cancelled, &photo)?;
        Ok(vault)
    }

    pub fn name(&self) -> String {
        self.root
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    }

    fn refresh_cancellable(
        &mut self,
        cancelled: impl Fn() -> bool,
        photo: &IndexPhoto,
    ) -> Result<()> {
        if !self.root.is_dir() {
            return Err("A pasta do cofre não está mais disponível.".into());
        }
        let start = Instant::now();
        let taken = SystemTime::now();
        let location = match photo {
            IndexPhoto::Auto => crate::paths::state_file(&self.root, "index", "photo"),
            IndexPhoto::Off => None,
            IndexPhoto::At(location) => Some(location.clone()),
        };
        let mut previous = location
            .as_deref()
            .and_then(|location| index_photo::read(location, &self.root));
        let mut report = PhotoReport {
            found: previous.is_some(),
            ..PhotoReport::default()
        };
        let mut notes: Vec<(Note, Option<Stamp>)> = Vec::new();
        let mut attachments = Vec::new();
        let mut folders = Vec::new();
        let mut warnings = Vec::new();
        let entries = WalkDir::new(&self.root)
            .max_depth(if self.loose { 1 } else { usize::MAX })
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| {
                entry.depth() == 0 || !is_ignored_path(Path::new(entry.file_name()))
            });
        for entry in entries {
            if cancelled() {
                return Err("Abertura cancelada.".into());
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    warnings.push(error.to_string());
                    continue;
                }
            };
            if entry.file_type().is_dir() && entry.depth() > 0 && !self.loose {
                folders.push(entry.path().strip_prefix(&self.root).unwrap().into());
            }
            if !entry.file_type().is_file() {
                continue;
            }
            if !is_markdown(entry.path()) {
                if is_text_document(entry.path()) || crate::assets::is_image(entry.path()) {
                    attachments.push(entry.path().strip_prefix(&self.root).unwrap().into());
                }
                continue;
            }
            let relative: PathBuf = entry.path().strip_prefix(&self.root).unwrap().into();
            let stamp = location
                .is_some()
                .then(|| {
                    entry
                        .metadata()
                        .ok()
                        .and_then(|metadata| Stamp::of(&metadata))
                })
                .flatten();
            // A note with the size and time the photo recorded is not read.
            if let Some(stamp) = stamp
                && let Some(old) = previous.as_mut().and_then(|photo| photo.remove(&relative))
                && old.stamp == stamp
            {
                report.reused += 1;
                notes.push((
                    Note::from_photo(relative, old.text, old.aliases, old.tags),
                    Some(stamp),
                ));
                continue;
            }
            match fs::read_to_string(entry.path()) {
                Ok(text) => {
                    report.read += 1;
                    notes.push((Note::new(relative, text), stamp));
                }
                Err(error) => warnings.push(format!("{}: {error}", entry.path().display())),
            }
        }
        report.gone = previous.map_or(0, |photo| photo.len());
        notes.sort_by_key(|(note, _)| note.path.to_string_lossy().to_lowercase());
        let (notes, stamps): (Vec<Note>, Vec<Option<Stamp>>) = notes.into_iter().unzip();
        self.notes = notes;
        self.attachments = attachments;
        folders.sort();
        self.folders = folders;
        self.rebuild_lookup();
        self.warnings = warnings;
        // The photo is rewritten when there is none, or when enough notes differ from it that
        // the next open would read a noticeable number of them again. Small vaults open fast
        // without one, so the app's own folder only gets a photo of a vault big enough.
        if let Some(location) = &location {
            let stale = report.read + report.gone;
            let explicit = matches!(photo, IndexPhoto::At(_));
            let worth = explicit || self.notes.len() >= PHOTO_MIN_NOTES;
            let outdated = if explicit {
                stale > 0
            } else {
                stale > PHOTO_STALE_BASE + self.notes.len() / PHOTO_STALE_SHARE
            };
            if worth && (!report.found || outdated) {
                match index_photo::write(location, &self.root, taken, &self.notes, &stamps) {
                    Ok(()) => report.written = true,
                    Err(error) => eprintln!("Cannot save the index photo: {error}"),
                }
            }
        }
        self.photo = report;
        self.index_time = start.elapsed();
        Ok(())
    }

    pub(crate) fn safe_path(&self, relative: &Path, create_parents: bool) -> Result<PathBuf> {
        if !is_text_document(relative) {
            return Err("Escolha um arquivo Markdown, de texto ou de código-fonte compatível.".into());
        }
        self.safe_file_path(relative, create_parents)
    }

    pub(crate) fn safe_file_path(&self, relative: &Path, create_parents: bool) -> Result<PathBuf> {
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err("As notas devem ficar dentro do cofre. Use um caminho Markdown relativo.".into());
        }
        let mut parent = self.root.clone();
        if let Some(parents) = relative.parent() {
            for part in parents.components() {
                parent.push(part);
                match fs::symlink_metadata(&parent) {
                    Ok(metadata) if metadata.file_type().is_symlink() => {
                        return Err("Pastas vinculadas não podem ser editadas.".into());
                    }
                    Ok(metadata) if !metadata.is_dir() => {
                        return Err("Um caminho pai não é uma pasta.".into());
                    }
                    Ok(_) => {}
                    Err(error)
                        if error.kind() == std::io::ErrorKind::NotFound && !create_parents =>
                    {
                        parent.pop();
                        break;
                    }
                    Err(error)
                        if error.kind() == std::io::ErrorKind::NotFound && create_parents =>
                    {
                        fs::create_dir(&parent)
                            .map_err(|e| format!("Não foi possível criar a pasta: {e}"))?;
                    }
                    Err(error) => return Err(format!("Não foi possível acessar a pasta: {error}")),
                }
            }
        }
        if !parent
            .canonicalize()
            .map_err(|e| e.to_string())?
            .starts_with(&self.root)
        {
            return Err("O caminho está fora deste cofre.".into());
        }
        let absolute = self.root.join(relative);
        if let Ok(metadata) = fs::symlink_metadata(&absolute)
            && metadata.file_type().is_symlink()
        {
            return Err("Arquivos vinculados não podem ser editados.".into());
        }
        Ok(absolute)
    }

    pub fn read(&self, path: &Path) -> Result<Document> {
        let absolute = self.document_path(path, false)?;
        let text = fs::read_to_string(absolute).map_err(|e| format!("Não foi possível ler a nota: {e}"))?;
        if text.contains('\0') {
            return Err("Este arquivo contém dados binários e não pode ser editado como texto.".into());
        }
        Ok(Document {
            path: path.into(),
            original: Some(text.clone()),
            saved_revision: Document::next_revision(),
            text,
            kind: DocumentKind::Note,
        })
    }

    pub fn open_document(&self, path: &Path) -> Result<Document> {
        if is_text_document(path) {
            return self.read(path);
        }
        if !self.attachments.iter().any(|attachment| attachment == path) {
            return Err("Escolha uma nota ou imagem indexada dentro deste cofre.".into());
        }
        let absolute = self.safe_file_path(path, false)?;
        if !absolute.is_file() {
            return Err("Esta imagem não existe mais.".into());
        }
        Ok(Document {
            path: path.to_owned(),
            text: String::new(),
            original: Some(String::new()),
            saved_revision: Document::next_revision(),
            kind: DocumentKind::Image,
        })
    }

    /// Import the current HTML buffer beside its source without replacing either
    /// the source file or an existing Markdown note. Relative links keep their base.
    pub fn import_html(&mut self, source: &Document) -> Result<Document> {
        if !crate::html_import::is_html(&source.path) || source.is_image() {
            return Err("Escolha um arquivo HTML para converter em Markdown.".into());
        }
        self.safe_file_path(&source.path, false)?;
        let text = crate::html_import::convert(&source.text)?;
        let stem = source
            .path
            .file_stem()
            .and_then(|name| name.to_str())
            .ok_or("Nome de arquivo HTML inválido.")?;
        let parent = source.path.parent().unwrap_or(Path::new(""));
        for number in 0..u64::MAX {
            let name = if number == 0 {
                format!("{stem}.md")
            } else {
                format!("{stem} {number}.md")
            };
            let path = parent.join(name);
            let absolute = self.root.join(&path);
            match fs::symlink_metadata(&absolute) {
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
            let mut document = Document::new(path);
            document.text.clone_from(&text);
            match self.save(&mut document) {
                Ok(()) => return Ok(document),
                Err(_) if fs::symlink_metadata(&absolute).is_ok() => continue,
                Err(error) => return Err(error),
            }
        }
        Err("Não foi possível achar um nome de nota Markdown livre.".into())
    }

    /// Create an empty note without asking for a name or replacing an existing file.
    pub fn create_untitled(&mut self) -> Result<Document> {
        self.create_untitled_in(Path::new(""))
    }

    pub fn create_untitled_in(&mut self, folder: &Path) -> Result<Document> {
        let absolute = if folder.as_os_str().is_empty() {
            self.root.clone()
        } else {
            self.folder_path(folder)?
        };
        let metadata = fs::symlink_metadata(&absolute)
            .map_err(|error| format!("Não foi possível criar uma nota nesta pasta: {error}"))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("Escolha uma pasta existente dentro deste cofre.".into());
        }
        for number in 0..u64::MAX {
            let name = if number == 0 {
                "Untitled.md".to_owned()
            } else {
                format!("Untitled {number}.md")
            };
            let path = folder.join(name);
            let absolute = self.root.join(&path);
            match fs::symlink_metadata(&absolute) {
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
            let mut document = Document::new(path);
            match self.save(&mut document) {
                Ok(()) => return Ok(document),
                // Another window may have created this name. save uses noclobber.
                Err(_) if fs::symlink_metadata(&absolute).is_ok() => continue,
                Err(error) => return Err(error),
            }
        }
        Err("Não foi possível achar um nome de nota livre.".into())
    }

    pub fn save(&mut self, document: &mut Document) -> Result<()> {
        if document.is_image() {
            return Err("Imagens não podem ser editadas como Markdown.".into());
        }
        if document.original.is_some() {
            let pending = self.prepare_save(document)?;
            pending.write()?;
            self.commit_save(document, pending.text);
            return Ok(());
        }
        let absolute = self.safe_path(&document.path, true)?;
        // A new note is also a write into the vault: it waits for a sync cycle's disk work.
        let _gate = crate::sync::gate::global().hold();
        if absolute.exists() {
            return Err("Já existe uma nota neste caminho. Escolha outro nome.".into());
        }
        prepare_file(&absolute, document.text.as_bytes(), None)?
            .persist_noclobber(&absolute)
            .map_err(|e| format!("Não foi possível criar a nota: {}", e.error))?;
        self.commit_save(document, document.text.clone());
        Ok(())
    }

    /// The checks and bytes for saving a note that is already on disk. Its
    /// `write` borrows nothing from the vault, so autosave runs it off the UI thread.
    pub fn prepare_save(&self, document: &Document) -> Result<PendingSave> {
        if document.is_image() {
            return Err("Imagens não podem ser editadas como Markdown.".into());
        }
        let original = document
            .original
            .clone()
            .ok_or("Esta nota ainda não está no disco.")?;
        Ok(PendingSave {
            absolute: self.document_path(&document.path, false)?,
            path: document.path.clone(),
            text: document.text.clone(),
            original,
        })
    }

    /// Record that `text`, written by `save` or `PendingSave::write`, is now the
    /// document's saved state.
    pub fn commit_save(&mut self, document: &mut Document, text: String) {
        document.original = Some(text.clone());
        document.saved_revision = Document::next_revision();
        if !self.guests.contains(&document.path) {
            self.index_note(document.path.clone(), text);
        }
    }

    /// Where a document is on disk: inside the vault by its relative path, or
    /// a guest file by its absolute one. Renames, moves and the trash use
    /// `safe_path` and so never touch a guest.
    fn document_path(&self, path: &Path, create_parents: bool) -> Result<PathBuf> {
        if !path.is_absolute() {
            return self.safe_path(path, create_parents);
        }
        if !self.guests.contains(path) {
            return Err("As notas devem ficar dentro do cofre. Use um caminho Markdown relativo.".into());
        }
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                Err("Arquivos vinculados não podem ser editados.".into())
            }
            Ok(metadata) if metadata.is_file() => Ok(path.to_owned()),
            _ => Err("Este arquivo não existe mais.".into()),
        }
    }

    /// Open a file outside this vault as a guest: the document keeps its
    /// absolute path, which is how the window tells it from the vault's notes.
    pub fn open_guest(&mut self, file: &Path) -> Result<Document> {
        let absolute = file
            .canonicalize()
            .map_err(|error| format!("Não foi possível abrir {}: {error}", file.display()))?;
        if absolute.starts_with(&self.root) {
            return Err("Este arquivo pertence ao cofre aberto.".into());
        }
        if !is_text_document(&absolute) {
            return Err("Escolha um arquivo Markdown, de texto ou de código-fonte compatível.".into());
        }
        self.guests.insert(absolute.clone());
        let document = self.read(&absolute);
        if document.is_err() {
            self.guests.remove(&absolute);
        }
        document
    }

    /// Whether `path` is a guest file held by this window.
    pub fn is_guest(&self, path: &Path) -> bool {
        self.guests.contains(path)
    }

    fn index_note(&mut self, path: PathBuf, text: String) {
        let legacy_text = path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("txt"));
        if !is_markdown(&path) && !legacy_text {
            if !self.attachments.contains(&path) {
                self.attachments.push(path);
                self.attachments.sort();
            }
            return;
        }
        self.attachments.retain(|attachment| attachment != &path);
        // A note already in the index keeps its place, its path and its name: only what depends
        // on its text changes, so only that is updated. A new note shifts every index after it
        // and takes the full rebuild (creating, deleting and renaming stay on that path).
        let existing = self
            .paths
            .get(&path)
            .copied()
            .filter(|&index| self.notes.get(index).is_some_and(|note| note.path == path));
        if let Some(index) = existing {
            // The path as the index spells it, which may differ from `path` in its separators.
            let note = Note::new(self.notes[index].path.clone(), text);
            let old = std::mem::replace(&mut self.notes[index], note);
            self.update_incoming(index, &old.text);
            return;
        }
        let note = Note::new(path.clone(), text);
        if let Some(existing) = self.notes.iter_mut().find(|note| note.path == path) {
            *existing = note;
        } else {
            self.notes.push(note);
            self.notes
                .sort_by_key(|note| note.path.to_string_lossy().to_lowercase());
        }
        self.rebuild_lookup();
    }

    /// The notes `text`, as the text of the note at `from`, links to; never `from` itself.
    fn link_targets(&self, from: &Path, text: &str) -> std::collections::HashSet<PathBuf> {
        let mut targets = std::collections::HashSet::new();
        for (_, link) in reference_spans(text) {
            if let Some(target) = self.resolve_destination(from, &link)
                && target != from
            {
                targets.insert(target);
            }
        }
        targets
    }

    /// The note at `index` changed text in place (`old` is the previous one). When the backlinks
    /// are already computed, move this note between the targets it stopped and started linking
    /// to; when they are not, the first request computes them from the current notes. The lists
    /// stay in note order, as the full computation leaves them.
    fn update_incoming(&mut self, index: usize, old: &str) {
        if self.incoming.get().is_none() {
            return;
        }
        let note = &self.notes[index];
        let before = self.link_targets(&note.path, old);
        let after = self.link_targets(&note.path, &note.text);
        let Some(incoming) = self.incoming.get_mut() else {
            return;
        };
        for target in before.difference(&after) {
            if let Some(sources) = incoming.get_mut(target) {
                if let Ok(at) = sources.binary_search(&index) {
                    sources.remove(at);
                }
                if sources.is_empty() {
                    incoming.remove(target);
                }
            }
        }
        for target in after.difference(&before) {
            let sources = incoming.entry(target.clone()).or_default();
            if let Err(at) = sources.binary_search(&index) {
                sources.insert(at, index);
            }
        }
    }

    /// The indexed note at `path`, found without scanning the notes.
    pub fn note(&self, path: &Path) -> Option<&Note> {
        self.notes.get(*self.paths.get(path)?)
    }

    /// Re-read the notes at `paths` (vault-relative) after an edit made outside Rustidian and
    /// update only their entries. Returns the paths whose text changed, or `None` when any path
    /// is not an indexed note that is still a readable file: a new, removed, renamed, unreadable
    /// or non-note path needs the full index, and nothing is changed then.
    pub fn refresh_changed(&mut self, paths: &[PathBuf]) -> Option<Vec<PathBuf>> {
        let mut changed = Vec::new();
        for path in paths {
            let index = *self.paths.get(path)?;
            let absolute = self.root.join(path);
            let metadata = fs::symlink_metadata(&absolute).ok()?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return None;
            }
            let text = fs::read_to_string(&absolute).ok()?;
            if self.notes.get(index)?.text != text {
                changed.push((path.clone(), text));
            }
        }
        let paths = changed.iter().map(|(path, _)| path.clone()).collect();
        for (path, text) in changed {
            self.index_note(path, text);
        }
        Some(paths)
    }

    pub fn duplicate_file(&mut self, from: &Path) -> Result<PathBuf> {
        if is_ignored_path(from) || !(is_text_document(from) || crate::assets::is_image(from)) {
            return Err("Escolha uma nota visível ou uma imagem compatível dentro deste cofre.".into());
        }
        let source = self.safe_file_path(from, false)?;
        let bytes =
            fs::read(&source).map_err(|error| format!("Não foi possível ler o arquivo a copiar: {error}"))?;
        let text = is_text_document(from)
            .then(|| {
                std::str::from_utf8(&bytes)
                    .map(str::to_owned)
                    .map_err(|error| error.to_string())
            })
            .transpose()?;
        let stem = from
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or("Nome de arquivo inválido.")?;
        let extension = from
            .extension()
            .and_then(|value| value.to_str())
            .ok_or("Adicione uma extensão ao nome do arquivo antes de copiá-lo.")?;
        let parent = from.parent().unwrap_or(Path::new(""));
        for counter in 1_u64.. {
            let path = parent.join(format!("{stem} {counter}.{extension}"));
            match fs::symlink_metadata(self.root.join(&path)) {
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
            let absolute = self.safe_file_path(&path, false)?;
            let prepared = prepare_file(&absolute, &bytes, None)?;
            match prepared.persist_noclobber(&absolute) {
                Ok(_) => {
                    if let Some(text) = text {
                        self.index_note(path.clone(), text);
                    } else {
                        self.attachments.push(path.clone());
                        self.attachments.sort();
                    }
                    return Ok(path);
                }
                Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(format!("Não foi possível criar a cópia: {}", error.error)),
            }
        }
        Err("Não foi possível achar um nome de cópia livre.".into())
    }

    pub(crate) fn rebuild_lookup(&mut self) {
        self.paths.clear();
        self.names.clear();
        for (index, note) in self.notes.iter().enumerate() {
            self.paths.insert(note.path.clone(), index);
            if let Some(name) = note.path.file_name().and_then(|name| name.to_str()) {
                self.names.entry(name.into()).or_default().push(index);
            }
        }
        self.incoming.take();
    }

    /// Save a new image without replacing an existing vault entry.
    pub fn import_attachment(&mut self, name: &Path, bytes: &[u8]) -> Result<PathBuf> {
        if name.components().count() != 1
            || !matches!(name.components().next(), Some(Component::Normal(_)))
            || is_ignored_path(name)
            || !crate::assets::is_image(name)
        {
            return Err("Escolha um arquivo de imagem compatível.".into());
        }
        let stem = name
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("Nome de imagem inválido.")?;
        let extension = name
            .extension()
            .and_then(|s| s.to_str())
            .ok_or("Invalid image extension.")?;
        for number in 0.. {
            let relative = if number == 0 {
                name.to_path_buf()
            } else {
                PathBuf::from(format!("{stem} {number}.{extension}"))
            };
            let absolute = self.root.join(&relative);
            match fs::symlink_metadata(&absolute) {
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("Não foi possível acessar o anexo: {error}")),
            }
            let absolute = self.safe_file_path(&relative, false)?;
            let temporary = prepare_file(&absolute, bytes, None)?;
            match temporary.persist_noclobber(&absolute) {
                Ok(_) => {
                    self.index_note(relative.clone(), String::new());
                    return Ok(relative);
                }
                Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(format!("Não foi possível salvar o anexo: {error}")),
            }
        }
        Err("Não foi possível achar um nome de imagem livre.".into())
    }

    pub fn query(&self, query: &str) -> Result<Vec<&Note>> {
        let query = crate::query::Query::parse(query)?;
        Ok(self
            .notes
            .iter()
            .filter(|note| query.matches(note))
            .collect())
    }

    pub fn resolve(&self, from: &Path, target: &str) -> Option<PathBuf> {
        let target = target.split('|').next()?.split('#').next()?.trim();
        if target.is_empty() {
            return Some(from.to_owned());
        }
        self.resolve_path(from, target).or_else(|| {
            crate::paths::decode(target)
                .filter(|decoded| decoded != target)
                .and_then(|decoded| self.resolve_path(from, &decoded))
        })
    }

    fn resolve_path(&self, from: &Path, target: &str) -> Option<PathBuf> {
        let target = if is_text_document(Path::new(target)) {
            PathBuf::from(target)
        } else {
            crate::paths::markdown_name(Path::new(target))
        };
        let exists = |path: &Path| {
            self.paths.contains_key(path)
                || (!is_markdown(path)
                    && is_text_document(path)
                    && self.safe_path(path, false).is_ok_and(|path| path.is_file()))
        };
        let sibling =
            crate::paths::normalise(&from.parent().unwrap_or(Path::new("")).join(&target));
        if let Some(sibling) = sibling
            && exists(&sibling)
        {
            return Some(sibling);
        }
        if let Some(root) = crate::paths::normalise(&target)
            && exists(&root)
        {
            return Some(root);
        }
        if target.components().count() != 1 {
            return None;
        }
        let matching = self.names.get(target.file_name()?.to_str()?)?;
        (matching.len() == 1).then(|| self.notes[matching[0]].path.clone())
    }

    /// Existing attachments and files outside the vault belong to the desktop file handler.
    /// Resolving them must not create notes, change the vault, or follow a symlink escape.
    pub fn external_file_uri(&self, from: &Path, target: &str) -> Option<String> {
        let file = self.external_file(from, target)?;
        Some(crate::paths::file_uri(&file))
    }

    /// A file a link names that the vault does not own: outside the root, or inside it but
    /// not a note or image.
    pub fn external_file(&self, from: &Path, target: &str) -> Option<PathBuf> {
        let target = crate::paths::decode(target.split('|').next()?.split('#').next()?.trim())?;
        let candidate = self
            .root
            .join(from.parent().unwrap_or(Path::new("")))
            .join(target);
        let mut lexical = PathBuf::new();
        for part in candidate.components() {
            match part {
                Component::CurDir => {}
                Component::ParentDir => {
                    if !lexical.pop() {
                        return None;
                    }
                }
                part => lexical.push(part.as_os_str()),
            }
        }
        let file = lexical.canonicalize().ok()?;
        if !file.is_file() {
            return None;
        }
        let root = self
            .root
            .canonicalize()
            .unwrap_or_else(|_| self.root.clone());
        if file.starts_with(&root) && (is_text_document(&file) || crate::assets::is_image(&file)) {
            return None;
        }
        Some(file)
    }

    pub fn open_link(&mut self, from: &Path, target: &str) -> Result<Document> {
        if let Some(path) = self.resolve_destination(from, target) {
            return self.open_document(&path);
        }
        let target = target
            .split('|')
            .next()
            .unwrap_or("")
            .split('#')
            .next()
            .unwrap_or("")
            .trim();
        let mut path = PathBuf::from(target);
        if target.is_empty()
            || !path
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
        {
            return Err("Use o nome de uma nota deste cofre.".into());
        }
        if !is_markdown(&path) {
            let decoded = crate::paths::decode(target).unwrap_or_else(|| target.to_owned());
            if crate::assets::is_image(Path::new(&decoded)) {
                return Err("Esta imagem não existe no cofre.".into());
            }
            if self.root.join(&path).is_file() {
                return Err(
                    "Este link aponta para um anexo, não para uma nota Markdown.".into(),
                );
            }
            let mut name = path.into_os_string();
            name.push(".md");
            path = PathBuf::from(name);
        }
        if self
            .notes
            .iter()
            .filter(|note| note.path.file_name() == path.file_name())
            .count()
            > 1
        {
            return Err("Várias notas têm este nome. Use o caminho completo a partir do cofre.".into());
        }
        let mut document = Document::new(path);
        document.text = format!("# {}\n\n", document.title());
        self.save(&mut document)?;
        Ok(document)
    }

    /// Each actual linked mention, using open drafts instead of their saved copies.
    pub fn backlink_mentions(&self, path: &Path, open: &[(&Path, &str)]) -> Vec<LinkedMention> {
        let mut sources: HashMap<PathBuf, &str> = self
            .backlinks(path)
            .into_iter()
            .map(|note| (note.path.clone(), note.text.as_str()))
            .collect();
        for &(from, text) in open {
            if from != path && !from.as_os_str().is_empty() {
                sources.insert(from.to_owned(), text);
            }
        }
        let mut mentions = Vec::new();
        for (from, text) in sources {
            if from == path {
                continue;
            }
            for (range, target) in reference_spans(text) {
                if self.resolve_destination(&from, &target).as_deref() != Some(path) {
                    continue;
                }
                mentions.push(LinkedMention::at(&from, text, range));
            }
        }
        mentions.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then_with(|| a.range.start.cmp(&b.range.start))
        });
        mentions
    }

    /// Plain-text mentions of `path`'s title or aliases in other notes, outside their links,
    /// using open drafts instead of their saved copies. `link` holds the matched text. At most
    /// `UNLINKED_LIMIT` mentions, so a common name cannot flood the panel.
    pub fn unlinked_mentions(&self, path: &Path, open: &[(&Path, &str)]) -> Vec<LinkedMention> {
        let mut names = vec![file_title(path)];
        if let Some(note) = self
            .paths
            .get(path)
            .and_then(|&index| self.notes.get(index))
        {
            names.extend(note.aliases.iter().cloned());
        }
        let mut sources: std::collections::BTreeMap<PathBuf, &str> = self
            .notes
            .iter()
            .map(|note| (note.path.clone(), note.text.as_str()))
            .collect();
        for &(from, text) in open {
            if !from.as_os_str().is_empty() && sources.contains_key(from) {
                sources.insert(from.to_owned(), text);
            }
        }
        let mut mentions = Vec::new();
        for (from, text) in sources {
            if from == path {
                continue;
            }
            let links: Vec<_> = reference_spans(text)
                .into_iter()
                .map(|(range, _)| range)
                .collect();
            for range in crate::unlinked::spans(text, &names, &links) {
                mentions.push(LinkedMention::at(&from, text, range));
                if mentions.len() >= UNLINKED_LIMIT {
                    return mentions;
                }
            }
        }
        mentions
    }

    /// The wikilink target `from` would use for `path`: its title when that resolves to it,
    /// else its vault path without the Markdown extension.
    pub fn link_target(&self, from: &Path, path: &Path) -> String {
        let title = file_title(path);
        if self.resolve(from, &title).as_deref() == Some(path) {
            return title;
        }
        let relative = if is_markdown(path) {
            path.with_extension("")
        } else {
            path.to_owned()
        };
        relative.to_string_lossy().replace('\\', "/")
    }

    pub fn backlinks(&self, path: &Path) -> Vec<&Note> {
        let incoming = self.incoming.get_or_init(|| {
            let mut incoming: HashMap<PathBuf, Vec<usize>> = HashMap::new();
            for (index, note) in self.notes.iter().enumerate() {
                for target in self.link_targets(&note.path, &note.text) {
                    incoming.entry(target).or_default().push(index);
                }
            }
            incoming
        });
        incoming
            .get(path)
            .into_iter()
            .flatten()
            .map(|&index| &self.notes[index])
            .collect()
    }
}

/// A save of an existing note: `Vault::prepare_save`, `write`, then `Vault::commit_save`.
pub struct PendingSave {
    pub path: PathBuf,
    pub text: String,
    absolute: PathBuf,
    original: String,
}

impl PendingSave {
    /// The saved text the write expects to replace.
    pub fn original(&self) -> &str {
        &self.original
    }

    /// Atomically replaces the note, keeping its permissions, unless it no longer
    /// holds the text it was loaded or last saved with.
    pub fn write(&self) -> Result<()> {
        // The sync cycle and the editor take turns on the vault's files (see `sync::gate`).
        let _gate = crate::sync::gate::global().hold();
        let current = fs::read_to_string(&self.absolute).map_err(|e| {
            format!("A nota foi removida ou não pode ser lida. Suas alterações continuam abertas: {e}")
        })?;
        if current != self.original {
            return Err("Esta nota mudou fora do Rustidian. Salve suas edições em uma nova nota ou mantenha-as aqui enquanto confere a versão do disco. Nada foi sobrescrito.".into());
        }
        let permissions = fs::metadata(&self.absolute)
            .map_err(|e| e.to_string())?
            .permissions();
        let temporary = prepare_file(&self.absolute, self.text.as_bytes(), Some(permissions))?;
        // Recheck after writing the temporary file to catch changes during a long write.
        if fs::read_to_string(&self.absolute).ok().as_ref() != Some(&self.original) {
            return Err(
                "Esta nota mudou fora do Rustidian durante o salvamento. Nada foi sobrescrito.".into(),
            );
        }
        temporary
            .persist(&self.absolute)
            .map_err(|e| format!("Não foi possível concluir o salvamento: {}", e.error))?;
        Ok(())
    }
}

fn prepare_file(
    absolute: &Path,
    bytes: &[u8],
    permissions: Option<fs::Permissions>,
) -> Result<NamedTempFile> {
    let parent = absolute.parent().ok_or("Caminho de nota inválido")?;
    let mut temporary =
        NamedTempFile::new_in(parent).map_err(|e| format!("Não foi possível preparar o salvamento: {e}"))?;
    temporary
        .write_all(bytes)
        .map_err(|e| format!("Não foi possível gravar a nota: {e}"))?;
    if let Some(permissions) = permissions {
        temporary
            .as_file()
            .set_permissions(permissions)
            .map_err(|e| e.to_string())?;
    }
    temporary
        .as_file()
        .sync_all()
        .map_err(|e| format!("Não foi possível gravar a nota no disco: {e}"))?;
    Ok(temporary)
}

pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

pub fn is_text_document(path: &Path) -> bool {
    if let Some(name) = path.file_name().and_then(|name| name.to_str())
        && [
            "makefile",
            "dockerfile",
            "cmakelists.txt",
            "justfile",
            "license",
        ]
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
    {
        return true;
    }
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "md" | "txt"
                    | "rs"
                    | "py"
                    | "pyw"
                    | "js"
                    | "mjs"
                    | "cjs"
                    | "jsx"
                    | "ts"
                    | "tsx"
                    | "c"
                    | "h"
                    | "cpp"
                    | "cc"
                    | "cxx"
                    | "hpp"
                    | "hxx"
                    | "cs"
                    | "java"
                    | "kt"
                    | "kts"
                    | "go"
                    | "rb"
                    | "php"
                    | "swift"
                    | "m"
                    | "mm"
                    | "scala"
                    | "clj"
                    | "cljs"
                    | "ex"
                    | "exs"
                    | "erl"
                    | "hrl"
                    | "hs"
                    | "lua"
                    | "r"
                    | "jl"
                    | "pl"
                    | "pm"
                    | "ps1"
                    | "psm1"
                    | "sh"
                    | "bash"
                    | "zsh"
                    | "fish"
                    | "bat"
                    | "cmd"
                    | "html"
                    | "htm"
                    | "css"
                    | "scss"
                    | "sass"
                    | "less"
                    | "xml"
                    | "xsl"
                    | "xsd"
                    | "json"
                    | "jsonc"
                    | "toml"
                    | "yaml"
                    | "yml"
                    | "ini"
                    | "cfg"
                    | "conf"
                    | "properties"
                    | "env"
                    | "sql"
                    | "graphql"
                    | "gql"
                    | "proto"
                    | "tex"
                    | "bib"
                    | "rst"
                    | "adoc"
                    | "log"
                    | "csv"
                    | "tsv"
                    | "nix"
                    | "zig"
                    | "v"
                    | "sv"
                    | "vhd"
                    | "vhdl"
                    | "svelte"
                    | "vue"
                    | "elm"
                    | "dart"
                    | "ipynb"
            )
        })
}

pub fn file_title(path: &Path) -> String {
    (if is_markdown(path) {
        path.file_stem()
    } else {
        path.file_name()
    })
    .unwrap_or_default()
    .to_string_lossy()
    .into_owned()
}

pub(crate) fn prose_lines(text: &str) -> impl Iterator<Item = &str> {
    let mut fence: Option<(char, usize)> = None;
    text.lines().filter(move |line| {
        let trimmed = line.trim_start();
        let marker = trimmed.chars().next().unwrap_or(' ');
        let count = trimmed.chars().take_while(|c| *c == marker).count();
        if matches!(marker, '`' | '~') && count >= 3 {
            match fence {
                None => fence = Some((marker, count)),
                Some((open, width)) if marker == open && count >= width => fence = None,
                _ => {}
            }
            return false;
        }
        fence.is_none()
    })
}

fn reference_spans(text: &str) -> Vec<(std::ops::Range<usize>, String)> {
    use pulldown_cmark::{Event, Options, Parser, Tag};
    let maths = crate::math::spans(text);
    let mut links = wiki_spans_outside(text, &maths);
    let wiki_ranges: Vec<_> = links.iter().map(|(range, _)| range.clone()).collect();
    let body = crate::metadata::frontmatter(text)
        .ok()
        .flatten()
        .map_or(0, |properties| properties.range.end);
    for (event, range) in Parser::new_ext(
        &text[body..],
        Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_FOOTNOTES,
    )
    .into_offset_iter()
    {
        if let Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) = event {
            let range = body + range.start..body + range.end;
            if maths
                .iter()
                .any(|math| math.range.start < range.end && math.range.end > range.start)
                || wiki_ranges
                    .iter()
                    .any(|wiki| wiki.start < range.end && wiki.end > range.start)
            {
                continue;
            }
            links.push((range, dest_url.into_string()));
        }
    }
    links.sort_by_key(|(range, _)| (range.start, range.end));
    links
}

pub fn wiki_spans(text: &str) -> Vec<(std::ops::Range<usize>, String)> {
    wiki_spans_outside(text, &crate::math::spans(text))
}

/// `wiki_spans` for a caller that already parsed `text`'s math.
pub fn wiki_spans_outside(
    text: &str,
    maths: &[crate::math::Span],
) -> Vec<(std::ops::Range<usize>, String)> {
    let mut links = Vec::new();
    for line in prose_lines(text) {
        let offset = line.as_ptr() as usize - text.as_ptr() as usize;
        let bytes = line.as_bytes();
        let mut cursor = 0;
        let mut code_width = 0;
        while cursor < bytes.len() {
            if bytes[cursor] == b'\\' && code_width == 0 {
                cursor += 2;
                continue;
            }
            if bytes[cursor] == b'`' {
                let width = bytes[cursor..].iter().take_while(|b| **b == b'`').count();
                if code_width == width {
                    code_width = 0;
                } else if code_width == 0 && line[cursor + width..].contains(&"`".repeat(width)) {
                    code_width = width;
                }
                cursor += width;
                continue;
            }
            if code_width == 0
                && bytes[cursor] == b'['
                && line[cursor..].starts_with("[[")
                && let Some(end) = line[cursor + 2..].find("]]")
            {
                let end = cursor + 2 + end;
                let target = line[cursor + 2..end].trim();
                if !target.is_empty()
                    && !maths.iter().any(|math| {
                        math.range.start < offset + end + 2 && math.range.end > offset + cursor
                    })
                {
                    // Markdown tables escape the alias separator: [[Note\|Label]].
                    links.push((
                        offset + cursor..offset + end + 2,
                        target.replace("\\|", "|"),
                    ));
                }
                cursor = end + 2;
                continue;
            }
            cursor += 1;
        }
    }
    links
}

pub fn wiki_links(text: &str) -> Vec<String> {
    wiki_spans(text)
        .into_iter()
        .map(|(_, target)| target)
        .collect()
}
