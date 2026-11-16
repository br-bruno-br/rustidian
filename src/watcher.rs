use notify::{
    Config, ErrorKind, Event, EventHandler, EventKind, RecommendedWatcher, RecursiveMode, Result,
    Watcher,
    event::{CreateKind, ModifyKind},
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

/// Native watches on a vault's included folders. Folder changes update them in
/// place: registering a large vault a second time can exhaust the system's
/// watch limit (27 Oct: one vault held 44,104 of 65,536).
pub struct Watch {
    root: PathBuf,
    watcher: RecommendedWatcher,
    folders: BTreeSet<PathBuf>,
    /// Paths created or moved in since the last update; a folder recreated
    /// under a watched name needs a new watch.
    arrived: Arc<Mutex<BTreeSet<PathBuf>>>,
}

impl Watch {
    pub fn new(root: &Path, mut handler: impl EventHandler) -> Result<Self> {
        let arrived = Arc::<Mutex<BTreeSet<PathBuf>>>::default();
        let record = arrived.clone();
        let watcher = RecommendedWatcher::new(
            move |event: Result<Event>| {
                if let Ok(event) = &event
                    && matches!(
                        event.kind,
                        EventKind::Create(CreateKind::Folder | CreateKind::Any)
                            | EventKind::Modify(ModifyKind::Name(_))
                    )
                {
                    record
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .extend(event.paths.iter().cloned());
                }
                // A handler that panics (its receiver gone at shutdown, say) must not
                // unwind into the native watcher's thread: that aborts the whole process.
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    handler.handle_event(event);
                }));
            },
            Config::default().with_follow_symlinks(false),
        )?;
        Ok(Self {
            root: root.to_owned(),
            watcher,
            folders: BTreeSet::new(),
            arrived,
        })
    }

    /// Watch `folders` (relative to the root, which is always watched) and
    /// release folders that left the vault. Returns whether any folder gained a watch.
    pub fn update(&mut self, folders: &[PathBuf], cancelled: impl Fn() -> bool) -> Result<bool> {
        let wanted = std::iter::once(PathBuf::new())
            .chain(folders.iter().cloned())
            .filter(|path| !crate::vault::is_ignored_path(path))
            .collect::<BTreeSet<_>>();
        let arrived =
            std::mem::take(&mut *self.arrived.lock().unwrap_or_else(PoisonError::into_inner));
        for gone in self.folders.difference(&wanted) {
            // The system already dropped the watch on a deleted folder.
            let _ = self.watcher.unwatch(&self.root.join(gone));
        }
        let added = wanted
            .iter()
            .filter(|path| !self.folders.contains(*path) || arrived.contains(&self.root.join(path)))
            .cloned()
            .collect::<Vec<_>>();
        self.folders = wanted;
        for (index, path) in added.iter().enumerate() {
            let result = if cancelled() {
                Err(notify::Error::generic("Opening cancelled."))
            } else {
                self.watcher
                    .watch(&self.root.join(path), RecursiveMode::NonRecursive)
            };
            match result {
                // A folder deleted since indexing needs no watch.
                Err(error)
                    if match &error.kind {
                        ErrorKind::PathNotFound => true,
                        ErrorKind::Io(error) => error.kind() == std::io::ErrorKind::NotFound,
                        _ => false,
                    } => {}
                Err(error) => {
                    // The next update retries the folders not yet watched.
                    for path in &added[index..] {
                        self.folders.remove(path);
                    }
                    return Err(error);
                }
                Ok(()) => {}
            }
        }
        Ok(!added.is_empty())
    }
}

/// A plain sentence for a watch failure; notify's own text lists raw paths.
pub fn describe(error: &notify::Error) -> String {
    let reason = match &error.kind {
        ErrorKind::MaxFilesWatch => {
            return "Algumas pastas não estão sendo observadas quanto a mudanças feitas fora do Rustidian: \
                    o limite de observação de arquivos do sistema foi atingido. Use Atualizar cofre para incluí-las."
                .into();
        }
        ErrorKind::Generic(message) => message.clone(),
        ErrorKind::Io(error) => error.to_string(),
        ErrorKind::PathNotFound => "uma pasta sumiu enquanto era observada".into(),
        _ => "o observador de arquivos parou".into(),
    };
    format!(
        "Alterações feitas fora do Rustidian podem não aparecer: {}.",
        reason.trim_end_matches('.')
    )
}
