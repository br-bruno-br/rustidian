//! The sync thread: runs `Sincronizador::ciclo` every 60 s, shortly after a save, and on demand,
//! and keeps a `SyncStatus` for the window to read.
//!
//! The thread owns the `Sincronizador` (and with it the redb state). The window talks to it by
//! messages and reads the status; it never waits for it, so a dead server cannot freeze the UI.
//! While a cycle runs, the editor's writes wait on the `SaveGate` only during the cycle's disk
//! work (see `gate`).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::panic::AssertUnwindSafe;
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use nota_cripto::ChaveCofre;
use nota_sync::remoto::{ErroRemoto, Remoto};
use nota_sync::{Config, ErroSync, Opcoes, RemotoHttp, Sincronizador};

use super::config::{SyncConfig, SyncFiles};
use super::connect::load_secrets;
use super::gate::{Gated, SaveGate};
use super::secrets::SecretStore;
use super::status::{Phase, SyncStatus};

/// When the thread runs a cycle.
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    /// Between cycles when nothing else asks for one.
    pub interval: Duration,
    /// After a save, this long without another save before the cycle runs (debounce).
    pub after_save: Duration,
    /// A steady stream of saves cannot push the cycle later than this after the first save.
    pub longest_wait: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(60),
            after_save: Duration::from_secs(5),
            longest_wait: Duration::from_secs(30),
        }
    }
}

enum Command {
    SyncNow,
    Saved,
    AllowMassDeletionOnce,
    Stop,
}

struct Shared {
    status: Mutex<SyncStatus>,
    changed: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, SyncStatus> {
        self.status.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn update(&self, wake: &dyn Fn(), change: impl FnOnce(&mut SyncStatus)) {
        change(&mut self.lock());
        self.changed.notify_all();
        wake();
    }
}

pub struct SyncRunner {
    commands: mpsc::Sender<Command>,
    shared: Arc<Shared>,
    finished: mpsc::Receiver<()>,
    thread: Option<JoinHandle<()>>,
}

impl SyncRunner {
    /// Starts the thread. `remote` is wrapped so the editor's saves are held back only while the
    /// cycle does disk work. `wake` is called whenever the status changes (the window repaints).
    pub fn start<R: Remoto + Send + 'static>(
        options: Opcoes,
        key: ChaveCofre,
        remote: R,
        gate: Arc<SaveGate>,
        timing: Timing,
        wake: impl Fn() + Send + 'static,
    ) -> Self {
        let (commands, receiver) = mpsc::channel();
        let (finish, finished) = mpsc::channel();
        let shared = Arc::new(Shared {
            status: Mutex::new(SyncStatus::default()),
            changed: Condvar::new(),
        });
        let worker_shared = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("rustidian-sync".into())
            .spawn(move || {
                run(
                    options,
                    key,
                    Gated::new(remote, gate),
                    timing,
                    &receiver,
                    &worker_shared,
                    &wake,
                );
                let _ = finish.send(());
            });
        let thread = match thread {
            Ok(handle) => Some(handle),
            Err(error) => {
                shared.lock().phase = Phase::Failed;
                shared.lock().error = Some(format!("Não foi possível iniciar a sincronização: {error}"));
                None
            }
        };
        Self {
            commands,
            shared,
            finished,
            thread,
        }
    }

    pub fn status(&self) -> SyncStatus {
        self.shared.lock().clone()
    }

    /// Runs a cycle now (the "Sync now" button).
    pub fn sync_now(&self) {
        let _ = self.commands.send(Command::SyncNow);
    }

    /// A note was saved: a cycle runs `Timing::after_save` later.
    pub fn notify_saved(&self) {
        let _ = self.commands.send(Command::Saved);
    }

    /// The "Allow this deletion once" button: the next cycle may delete many files.
    pub fn allow_mass_deletion_once(&self) {
        let _ = self.commands.send(Command::AllowMassDeletionOnce);
    }

    /// Waits until a cycle beyond number `after` has finished (or `timeout`). For tests.
    pub fn wait_for_cycle(&self, after: u64, timeout: Duration) -> Option<SyncStatus> {
        let deadline = Instant::now() + timeout;
        let mut status = self.shared.lock();
        loop {
            if status.cycles > after && status.phase != Phase::Syncing {
                return Some(status.clone());
            }
            let left = deadline.checked_duration_since(Instant::now())?;
            status = self
                .shared
                .changed
                .wait_timeout(status, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Asks the thread to stop and waits up to `grace` for it. A cycle stuck on a dead server
    /// is not waited for (the thread ends on its own when the call times out).
    pub fn stop(mut self, grace: Duration) {
        let _ = self.commands.send(Command::Stop);
        if self.finished.recv_timeout(grace).is_ok()
            && let Some(thread) = self.thread.take()
        {
            let _ = thread.join();
        }
    }
}

impl Drop for SyncRunner {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
    }
}

/// The thread's body.
fn run<R: Remoto>(
    options: Opcoes,
    key: ChaveCofre,
    remote: Gated<R>,
    timing: Timing,
    commands: &mpsc::Receiver<Command>,
    shared: &Shared,
    wake: &dyn Fn(),
) {
    let vault = options.cofre.clone();
    let mut sync = match Sincronizador::novo(options, key, remote) {
        Ok(sync) => sync,
        Err(error) => {
            shared.update(wake, |s| {
                s.phase = Phase::Failed;
                s.error = Some(format!("Não foi possível iniciar a sincronização: {error}"));
            });
            return;
        }
    };
    shared.update(wake, |s| s.conflicts = scan_vault(&vault).conflicts);

    let mut next_periodic = Instant::now();
    // (when the debounced cycle is due, the latest it may be pushed to)
    let mut debounce: Option<(Instant, Instant)> = None;
    loop {
        let due = debounce.map_or(next_periodic, |(at, _)| at.min(next_periodic));
        let mut allow_mass = false;
        let run_now = match commands.recv_timeout(due.saturating_duration_since(Instant::now())) {
            Ok(Command::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            Ok(Command::SyncNow) => true,
            Ok(Command::AllowMassDeletionOnce) => {
                allow_mass = true;
                true
            }
            Ok(Command::Saved) => {
                let now = Instant::now();
                debounce = Some(match debounce {
                    None => (now + timing.after_save, now + timing.longest_wait),
                    Some((_, latest)) => ((now + timing.after_save).min(latest), latest),
                });
                false
            }
            Err(RecvTimeoutError::Timeout) => true,
        };
        if !run_now {
            continue;
        }
        let crashed = cycle(&mut sync, &vault, shared, wake, allow_mass);
        next_periodic = Instant::now() + timing.interval;
        debounce = None;
        if crashed {
            return;
        }
    }
}

/// One cycle. Returns `true` if the engine panicked (the thread then stops).
fn cycle<R: Remoto>(
    sync: &mut Sincronizador<Gated<R>>,
    vault: &Path,
    shared: &Shared,
    wake: &dyn Fn(),
    allow_mass_deletion: bool,
) -> bool {
    shared.update(wake, |s| s.phase = Phase::Syncing);
    let before = scan_vault(vault);
    sync.permitir_exclusao_em_massa(allow_mass_deletion);
    sync.remoto().enter();
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| sync.ciclo()));
    sync.remoto().leave();
    sync.permitir_exclusao_em_massa(false);
    let after = scan_vault(vault);
    let vault_changed = after.fingerprint != before.fingerprint;
    let now = chrono::Local::now().naive_local();
    let crashed = result.is_err();
    shared.update(wake, |s| {
        s.cycles += 1;
        s.conflicts = after.conflicts;
        if vault_changed {
            s.vault_changed += 1;
        }
        s.offline_reason = None;
        match result {
            Ok(Ok(report)) => {
                s.phase = Phase::Synced;
                s.last_sync = Some(now);
                s.error = None;
                s.mass_deletion = None;
                s.refused = report.recusados;
            }
            Ok(Err(ErroSync::Remoto(ErroRemoto::Rede(reason)))) => {
                s.phase = Phase::Offline;
                s.offline_reason = Some(reason);
            }
            Ok(Err(ErroSync::Remoto(ErroRemoto::NaoAutorizado))) => {
                s.phase = Phase::Failed;
                s.mass_deletion = None;
                s.error = Some(
                    "O servidor recusou o token deste dispositivo (401). Desconecte e conecte de novo com um token válido."
                        .into(),
                );
            }
            Ok(Err(ErroSync::ExclusaoEmMassa { lapides, total })) => {
                s.phase = Phase::Failed;
                s.mass_deletion = Some((lapides, total));
                s.error = Some(
                    ErroSync::ExclusaoEmMassa { lapides, total }.to_string(),
                );
            }
            Ok(Err(error)) => {
                s.phase = Phase::Failed;
                s.mass_deletion = None;
                s.error = Some(error.to_string());
            }
            Err(_) => {
                s.phase = Phase::Failed;
                s.error = Some("O mecanismo de sincronização parou de forma inesperada; reinicie o Rustidian.".into());
            }
        }
    });
    crashed
}

/// What a cheap walk of the vault finds: the conflict copies and a fingerprint of the files
/// (names, sizes, modification times; nothing is read), to tell whether a cycle changed them.
struct Scan {
    conflicts: Vec<String>,
    fingerprint: u64,
}

fn scan_vault(root: &Path) -> Scan {
    let mut scan = Scan {
        conflicts: Vec::new(),
        fingerprint: 0,
    };
    walk(root, "", &mut scan);
    scan.conflicts.sort();
    scan
}

fn walk(root: &Path, relative: &str, scan: &mut Scan) {
    let dir = if relative.is_empty() {
        root.to_path_buf()
    } else {
        root.join(relative)
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        // Dot folders (`.trash`, `.obsidian`, `.git`) and the engine's temporaries.
        if name.starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = if relative.is_empty() {
            name.clone()
        } else {
            format!("{relative}/{name}")
        };
        if kind.is_dir() {
            walk(root, &path, scan);
        } else if kind.is_file() {
            if is_conflict_copy(&name) {
                scan.conflicts.push(path.clone());
            }
            let mut hasher = DefaultHasher::new();
            path.hash(&mut hasher);
            if let Ok(metadata) = entry.metadata() {
                metadata.len().hash(&mut hasher);
                metadata.modified().ok().hash(&mut hasher);
            }
            scan.fingerprint = scan.fingerprint.wrapping_add(hasher.finish());
        }
    }
}

/// `note (conflito <device> <date>).md`, as `nota-sync` names the copies it keeps.
pub fn is_conflict_copy(file_name: &str) -> bool {
    // `nota-sync` may add " 2", " 3" after the parenthesis when the name was taken.
    file_name
        .find(" (conflito ")
        .is_some_and(|at| file_name[at..].contains(')'))
}

/// Starts sync for a connected vault the way the app does: settings from `files`, the key and
/// the token from `store`, an HTTP client for the server.
pub fn start_for_vault(
    files: &SyncFiles,
    store: &dyn SecretStore,
    vault: &Path,
    gate: Arc<SaveGate>,
    timing: Timing,
    wake: impl Fn() + Send + 'static,
) -> Result<SyncRunner, String> {
    let config = SyncConfig::load(&files.config)?;
    if !config.connected {
        return Err("Este cofre não está conectado a um servidor de sincronização.".into());
    }
    let (key, token) = load_secrets(store, vault)?;
    let remote = RemotoHttp::novo(&Config {
        servidor: config.server.clone(),
        token,
        aparelho: config.device.clone(),
    })?;
    if let Some(parent) = files.database.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Não foi possível criar a pasta de estado da sincronização: {e}"))?;
    }
    Ok(SyncRunner::start(
        Opcoes {
            cofre: vault.to_path_buf(),
            estado: files.database.clone(),
            aparelho: config.device,
        },
        key,
        remote,
        gate,
        timing,
        wake,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflict_copies_are_recognised_by_their_name() {
        assert!(is_conflict_copy("Note (conflito pc 2026-11-15 0905).md"));
        assert!(is_conflict_copy("Note (conflito pc 2026-11-15 0905) 2.md"));
        assert!(is_conflict_copy("a (conflito telefone 2026-01-01 1010)"));
        assert!(!is_conflict_copy("Plain note.md"));
        assert!(!is_conflict_copy("Talk about (conflito).md"));
        assert!(!is_conflict_copy("Note (conflict).md"));
    }

    #[test]
    fn the_scan_lists_conflicts_and_skips_dot_folders() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a/b")).unwrap();
        std::fs::create_dir_all(dir.path().join(".trash")).unwrap();
        std::fs::write(
            dir.path().join("a/b/x (conflito pc 2026-11-15 0905).md"),
            "1",
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".trash/y (conflito pc 2026-11-15 0905).md"),
            "1",
        )
        .unwrap();
        std::fs::write(dir.path().join("plain.md"), "1").unwrap();
        let first = scan_vault(dir.path());
        assert_eq!(first.conflicts, ["a/b/x (conflito pc 2026-11-15 0905).md"]);
        std::fs::write(dir.path().join("plain.md"), "12").unwrap();
        assert_ne!(scan_vault(dir.path()).fingerprint, first.fingerprint);
    }
}
