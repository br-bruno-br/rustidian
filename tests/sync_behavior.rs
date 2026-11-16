//! The app's sync, without a window: a real `nota-servidor` on a local port, and "devices" that
//! are set up and driven through the same functions the app calls (`connect_vault`,
//! `start_for_vault`, `SyncRunner`), with an in-memory credential store instead of the real one.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use nota_servidor::{ConfigServidor, ServidorLocal, tokens};
use rustidian::sync::config::{SyncConfig, SyncFiles};
use rustidian::sync::connect::{
    ConnectError, Credentials, Outcome, connect_vault, disconnect_vault,
};
use rustidian::sync::gate::{self, Gated, SaveGate};
use rustidian::sync::runner::{SyncRunner, Timing, start_for_vault};
use rustidian::sync::secrets::{MemoryStore, SecretStore};
use rustidian::sync::status::{Phase, SyncStatus};
use rustidian::vault::Vault;

const PHRASE: &str = "correct horse battery staple";
const WAIT: Duration = Duration::from_secs(60);

struct Server {
    data: tempfile::TempDir,
    running: Option<ServidorLocal>,
}

impl Server {
    fn start() -> Self {
        let data = tempfile::tempdir().unwrap();
        let running = ServidorLocal::iniciar(ConfigServidor {
            dados: data.path().to_path_buf(),
            escutar: "127.0.0.1:0".parse().unwrap(),
            permitir_todas_interfaces: false,
        })
        .unwrap();
        Self {
            data,
            running: Some(running),
        }
    }

    fn url(&self) -> String {
        format!("http://{}", self.running.as_ref().unwrap().endereco())
    }

    fn stop(&mut self) {
        self.running = None;
    }
}

/// Timings short enough for tests; the periodic cycle is out of the way unless a test wants it.
fn quick() -> Timing {
    Timing {
        interval: Duration::from_secs(3600),
        after_save: Duration::from_millis(200),
        longest_wait: Duration::from_secs(2),
    }
}

/// One installation of the app: its own vault, its own state folder, its own credential store.
struct Device {
    name: String,
    vault: tempfile::TempDir,
    state: tempfile::TempDir,
    files: SyncFiles,
    store: Arc<MemoryStore>,
    token: String,
    runner: Option<SyncRunner>,
}

impl Device {
    fn new(name: &str, server: &Server) -> Self {
        let state = tempfile::tempdir().unwrap();
        let files = SyncFiles {
            config: state.path().join("sync.json"),
            database: state.path().join("sync.redb"),
        };
        Self {
            name: name.into(),
            vault: tempfile::tempdir().unwrap(),
            state,
            files,
            store: Arc::new(MemoryStore::default()),
            token: tokens::novo(server.data.path(), name).unwrap(),
            runner: None,
        }
    }

    fn root(&self) -> &Path {
        self.vault.path()
    }

    fn connect(&self, server: &Server, phrase: &str) -> Result<Outcome, ConnectError> {
        connect_vault(
            &self.files,
            &*self.store,
            self.root(),
            &Credentials {
                server: server.url(),
                device: self.name.clone(),
                token: self.token.clone(),
                passphrase: phrase.into(),
            },
        )
    }

    fn start(&mut self, timing: Timing) {
        let runner = start_for_vault(
            &self.files,
            &*self.store,
            self.root(),
            Arc::clone(gate::global()),
            timing,
            || {},
        )
        .expect("the connected vault starts");
        // The first cycle runs by itself right away.
        runner.wait_for_cycle(0, WAIT).expect("first cycle");
        self.runner = Some(runner);
    }

    fn runner(&self) -> &SyncRunner {
        self.runner.as_ref().unwrap()
    }

    /// A cycle now, and its result.
    fn sync(&self) -> SyncStatus {
        let before = self.runner().status().cycles;
        self.runner().sync_now();
        self.runner().wait_for_cycle(before, WAIT).expect("cycle")
    }

    fn write(&self, relative: &str, text: &str) {
        let path = self.root().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn read(&self, relative: &str) -> Option<String> {
        fs::read_to_string(self.root().join(relative)).ok()
    }

    /// Every file under the vault's `.trash/`, with its text.
    fn trashed(&self) -> Vec<(String, String)> {
        let mut found = Vec::new();
        collect(&self.root().join(".trash"), &mut found);
        found
    }
}

fn collect(dir: &Path, found: &mut Vec<(String, String)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path: PathBuf = entry.path();
        if path.is_dir() {
            collect(&path, found);
        } else {
            found.push((
                entry.file_name().to_string_lossy().into_owned(),
                fs::read_to_string(&path).unwrap_or_default(),
            ));
        }
    }
}

#[test]
fn two_devices_converge_through_the_server_and_a_delete_goes_to_the_vault_trash() {
    let server = Server::start();
    let mut a = Device::new("desk", &server);
    let mut b = Device::new("laptop", &server);
    assert_eq!(a.connect(&server, PHRASE).unwrap(), Outcome::Created);
    assert_eq!(b.connect(&server, PHRASE).unwrap(), Outcome::Joined);
    a.start(quick());
    b.start(quick());

    // create on one side
    a.write("Notes/a.md", "alpha");
    assert_eq!(a.sync().phase, Phase::Synced);
    b.sync();
    assert_eq!(b.read("Notes/a.md").as_deref(), Some("alpha"));

    // edit on the other
    b.write("Notes/a.md", "beta");
    b.sync();
    a.sync();
    assert_eq!(a.read("Notes/a.md").as_deref(), Some("beta"));

    // both create something at once
    a.write("from-a.md", "A");
    b.write("from-b.md", "B");
    a.sync();
    b.sync();
    a.sync();
    assert_eq!(a.read("from-b.md").as_deref(), Some("B"));
    assert_eq!(b.read("from-a.md").as_deref(), Some("A"));

    // delete in the app: it goes to the vault's .trash/, then travels as a deletion
    let mut vault = Vault::open(a.root()).unwrap();
    vault.trash_entry_in_vault(Path::new("Notes/a.md")).unwrap();
    assert!(a.read("Notes/a.md").is_none());
    let kept = a.trashed();
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert!(
        kept[0].0.starts_with("a ") && kept[0].0.ends_with(".md"),
        "{kept:?}"
    );
    assert_eq!(kept[0].1, "beta");
    a.sync();
    let status = b.sync();
    assert!(b.read("Notes/a.md").is_none());
    assert_eq!(b.trashed().len(), 1, "the other device keeps a copy too");
    assert!(status.vault_changed > 0, "the window is told to reload");
    assert_eq!(status.phase, Phase::Synced);
    assert!(status.conflicts.is_empty());

    // an idle cycle changes nothing and tells the window nothing
    let idle = b.sync();
    let again = b.sync();
    assert_eq!(idle.vault_changed, again.vault_changed);

    // the server never saw a note's words
    let mut seen = String::new();
    for entry in fs::read_dir(server.data.path().join("blobs"))
        .unwrap()
        .flatten()
    {
        seen.push_str(&String::from_utf8_lossy(&fs::read(entry.path()).unwrap()));
    }
    for word in ["alpha", "beta", "from-a", "Notes"] {
        assert!(
            !seen.contains(word),
            "the server holds '{word}' in the clear"
        );
    }
}

#[test]
fn a_whole_folder_deleted_in_the_app_goes_to_the_vault_trash() {
    let server = Server::start();
    let mut a = Device::new("desk", &server);
    let mut b = Device::new("laptop", &server);
    a.connect(&server, PHRASE).unwrap();
    b.connect(&server, PHRASE).unwrap();
    a.start(quick());
    b.start(quick());
    a.write("Project/one.md", "1");
    a.write("Project/deep/two.md", "2");
    a.write("keep.md", "k");
    a.sync();
    b.sync();
    assert_eq!(b.read("Project/deep/two.md").as_deref(), Some("2"));

    let mut vault = Vault::open(a.root()).unwrap();
    vault.trash_entry_in_vault(Path::new("Project")).unwrap();
    assert!(!a.root().join("Project").exists());
    let mut texts: Vec<_> = a.trashed().into_iter().map(|(_, text)| text).collect();
    texts.sort();
    assert_eq!(texts, ["1", "2"]);
    a.sync();
    b.sync();
    assert!(!b.root().join("Project/one.md").exists());
    assert_eq!(b.read("keep.md").as_deref(), Some("k"));
    assert_eq!(b.trashed().len(), 2);
    // outside the vault and unknown paths stay refused
    assert!(vault.trash_entry_in_vault(Path::new("../x")).is_err());
    assert!(vault.trash_entry_in_vault(Path::new("Missing")).is_err());
}

#[test]
fn a_wrong_passphrase_is_refused_and_nothing_is_stored() {
    let server = Server::start();
    let a = Device::new("desk", &server);
    let c = Device::new("tablet", &server);
    assert_eq!(a.connect(&server, PHRASE).unwrap(), Outcome::Created);

    let error = c.connect(&server, "not the phrase at all").unwrap_err();
    assert_eq!(error, ConnectError::WrongPassphrase);
    assert!(error.to_string().contains("Wrong passphrase"));
    assert!(c.store.is_empty(), "no key or token was stored");
    assert!(!c.files.config.exists(), "no settings were written");
    assert!(
        start_for_vault(
            &c.files,
            &*c.store,
            c.root(),
            Arc::new(SaveGate::default()),
            quick(),
            || {}
        )
        .is_err()
    );

    // a token the server does not know is told apart from a wrong phrase
    let mut d = Device::new("phone", &server);
    d.token = "nst_not_a_real_token".into();
    assert_eq!(
        d.connect(&server, PHRASE).unwrap_err(),
        ConnectError::Unauthorized
    );
    // a first device with a short phrase is told to use a longer one, and sends nothing
    let fresh = Server::start();
    let e = Device::new("desk", &fresh);
    assert!(matches!(
        e.connect(&fresh, "short"),
        Err(ConnectError::Passphrase(_))
    ));
    assert!(!fresh.data.path().join("cofre.chave").exists());
}

#[test]
fn secrets_live_only_in_the_credential_store_and_disconnect_clears_them() {
    let server = Server::start();
    let mut a = Device::new("desk", &server);
    a.connect(&server, PHRASE).unwrap();
    assert_eq!(a.store.len(), 2, "the key and the token");

    // no file of this device (settings, state, vault) holds the token or the phrase
    a.start(quick());
    a.write("n.md", "hello");
    a.sync();
    let settings = SyncConfig::load(&a.files.config).unwrap();
    assert!(settings.connected && settings.enabled);
    assert_eq!(settings.device, "desk");

    // the runner is told to stop first, as the app does (it also frees the state file, which
    // Windows keeps locked while the database is open)
    a.runner.take().unwrap().stop(Duration::from_secs(10));
    for dir in [a.state.path(), a.root()] {
        let mut files = Vec::new();
        walk(dir, &mut files);
        for file in files {
            let bytes = fs::read(&file).unwrap();
            for secret in [a.token.as_bytes(), PHRASE.as_bytes()] {
                assert!(
                    !bytes.windows(secret.len()).any(|w| w == secret),
                    "a secret is in {}",
                    file.display()
                );
            }
        }
    }
    disconnect_vault(&a.files, &*a.store, a.root()).unwrap();
    assert!(
        a.store.is_empty(),
        "Disconnect deletes the key and the token"
    );
    let settings = SyncConfig::load(&a.files.config).unwrap();
    assert!(!settings.connected && !settings.enabled);
    assert_eq!(settings.server, server.url(), "the address is kept");
    assert!(!a.files.database.exists());
    assert!(
        start_for_vault(
            &a.files,
            &*a.store,
            a.root(),
            Arc::new(SaveGate::default()),
            quick(),
            || {}
        )
        .is_err()
    );
    assert_eq!(a.read("n.md").as_deref(), Some("hello"), "the notes stay");
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, found);
        } else {
            found.push(path);
        }
    }
}

#[test]
fn offline_keeps_the_app_working_and_the_notes_are_synced_when_the_server_returns() {
    let mut server = Server::start();
    let mut a = Device::new("desk", &server);
    a.connect(&server, PHRASE).unwrap();
    a.start(quick());
    a.write("kept.md", "before");
    assert_eq!(a.sync().phase, Phase::Synced);

    server.stop();
    a.write("kept.md", "written while offline");
    a.write("new.md", "new while offline");
    let status = a.sync();
    assert_eq!(status.phase, Phase::Offline);
    assert_eq!(status.label().text, "Offline");
    assert!(status.error.is_none());
    assert_eq!(a.read("kept.md").as_deref(), Some("written while offline"));
    assert_eq!(a.read("new.md").as_deref(), Some("new while offline"));
}

#[test]
fn a_cycle_that_deletes_most_of_the_server_stops_until_allowed_once() {
    let server = Server::start();
    let mut a = Device::new("desk", &server);
    let mut b = Device::new("laptop", &server);
    a.connect(&server, PHRASE).unwrap();
    b.connect(&server, PHRASE).unwrap();
    a.start(quick());
    b.start(quick());
    for n in 0..25 {
        a.write(&format!("many/n{n:02}.md"), &format!("note {n}"));
    }
    a.sync();
    b.sync();
    assert_eq!(b.read("many/n07.md").as_deref(), Some("note 7"));

    // the vault is emptied from outside (an unmounted drive, say)
    fs::remove_dir_all(a.root().join("many")).unwrap();
    let held = a.sync();
    assert_eq!(held.phase, Phase::Failed);
    assert_eq!(held.mass_deletion, Some((25, 25)));
    assert_eq!(held.label().text, "Sync error");
    assert!(
        held.mass_deletion_text()
            .unwrap()
            .contains("nothing was sent")
    );
    b.sync();
    assert_eq!(
        b.read("many/n07.md").as_deref(),
        Some("note 7"),
        "the server still has everything"
    );

    // "Allow this deletion once": one cycle goes through, and the guard is back after it
    let before = a.runner().status().cycles;
    a.runner().allow_mass_deletion_once();
    let allowed = a.runner().wait_for_cycle(before, WAIT).unwrap();
    assert_eq!(allowed.phase, Phase::Synced);
    assert!(allowed.mass_deletion.is_none());
    b.sync();
    assert!(b.read("many/n07.md").is_none());
    assert_eq!(
        b.trashed().len(),
        25,
        "the other device keeps every note in .trash"
    );
}

#[test]
fn files_the_scan_leaves_out_are_listed_in_the_status() {
    let server = Server::start();
    let mut a = Device::new("desk", &server);
    a.connect(&server, PHRASE).unwrap();
    a.start(quick());
    let big = fs::File::create(a.root().join("huge.bin")).unwrap();
    big.set_len(51 * 1024 * 1024).unwrap();
    drop(big);
    a.write("fine.md", "ok");
    let status = a.sync();
    assert_eq!(status.phase, Phase::Synced);
    assert!(
        status.refused.iter().any(|r| r.contains("huge.bin")),
        "{:?}",
        status.refused
    );
}

#[test]
fn conflict_copies_are_counted_and_listed() {
    let server = Server::start();
    let mut a = Device::new("desk", &server);
    let mut b = Device::new("laptop", &server);
    a.connect(&server, PHRASE).unwrap();
    b.connect(&server, PHRASE).unwrap();
    a.start(quick());
    b.start(quick());
    a.write("shared.md", "start");
    a.sync();
    b.sync();
    a.write("shared.md", "edit on desk");
    b.write("shared.md", "edit on laptop");
    a.sync();
    let status = b.sync();
    assert_eq!(status.phase, Phase::Synced);
    assert_eq!(status.conflicts.len(), 1, "{:?}", status.conflicts);
    assert!(status.conflicts[0].starts_with("shared (conflito laptop "));
    assert_eq!(status.label().text, "1 conflict");
    // nothing was lost: the winner is the server's, the copy keeps the other text
    assert_eq!(b.read("shared.md").as_deref(), Some("edit on desk"));
    assert_eq!(
        b.read(&status.conflicts[0]).as_deref(),
        Some("edit on laptop")
    );
}

#[test]
fn a_save_triggers_one_debounced_cycle_and_the_timer_runs_cycles_by_itself() {
    let server = Server::start();
    let mut a = Device::new("desk", &server);
    let mut b = Device::new("laptop", &server);
    a.connect(&server, PHRASE).unwrap();
    b.connect(&server, PHRASE).unwrap();
    a.start(quick());
    b.start(quick());

    a.write("typed.md", "draft");
    let before = a.runner().status().cycles;
    for _ in 0..5 {
        a.runner().notify_saved();
        std::thread::sleep(Duration::from_millis(20));
    }
    let after = a.runner().wait_for_cycle(before, WAIT).expect("a cycle");
    assert_eq!(after.cycles, before + 1);
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(
        a.runner().status().cycles,
        before + 1,
        "five saves in a row made one cycle"
    );
    b.sync();
    assert_eq!(b.read("typed.md").as_deref(), Some("draft"));

    // the timer: a short interval cycles without being asked
    let mut c = Device::new("tablet", &server);
    c.connect(&server, PHRASE).unwrap();
    c.start(Timing {
        interval: Duration::from_millis(300),
        ..quick()
    });
    let start = c.runner().status().cycles;
    c.runner()
        .wait_for_cycle(start + 2, Duration::from_secs(20))
        .expect("the timer kept cycling");
}

/// While the cycle does disk work, a save waits; while it waits for the server, a save does not.
#[test]
fn a_save_waits_for_the_cycles_disk_work_but_not_for_its_network_calls() {
    use nota_sync::memoria::Chamada;
    use nota_sync::{Opcoes, RemotoMemoria, Sincronizador};

    fn setup(
        remote: RemotoMemoria,
        gate: &Arc<SaveGate>,
    ) -> (
        tempfile::TempDir,
        tempfile::TempDir,
        Sincronizador<Gated<RemotoMemoria>>,
    ) {
        let vault = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        fs::write(vault.path().join("note.md"), "text").unwrap();
        let sync = Sincronizador::novo(
            Opcoes {
                cofre: vault.path().to_path_buf(),
                estado: state.path().join("s.redb"),
                aparelho: "desk".into(),
            },
            nota_cripto::ChaveCofre::nova(),
            Gated::new(remote, Arc::clone(gate)),
        )
        .unwrap();
        (vault, state, sync)
    }

    // 1. Held inside an effect (disk work): the saver waits.
    {
        let gate = Arc::new(SaveGate::default());
        let (_vault, _state, mut sync) = setup(RemotoMemoria::novo(), &gate);
        let (in_effect, in_effect_seen) = mpsc::channel::<()>();
        let (go, go_seen) = mpsc::channel::<()>();
        sync.definir_gancho_efeito(Some(Box::new(move |_, _| {
            let _ = in_effect.send(());
            let _ = go_seen.recv();
        })));
        let cycle = std::thread::spawn(move || {
            sync.remoto().enter();
            let result = sync.ciclo();
            sync.remoto().leave();
            result.is_ok()
        });
        in_effect_seen
            .recv_timeout(WAIT)
            .expect("the cycle reached its first effect");
        assert!(gate.is_held(), "the cycle holds the gate during disk work");
        let (saved, saved_seen) = mpsc::channel::<()>();
        let saver_gate = Arc::clone(&gate);
        let saver = std::thread::spawn(move || {
            let _hold = saver_gate.hold();
            let _ = saved.send(());
        });
        assert!(
            saved_seen.recv_timeout(Duration::from_millis(400)).is_err(),
            "the save went through while the cycle was writing"
        );
        go.send(()).unwrap();
        saved_seen
            .recv_timeout(WAIT)
            .expect("the save runs once the cycle lets go");
        saver.join().unwrap();
        assert!(cycle.join().unwrap());
        assert!(!gate.is_held());
    }

    // 2. Stuck inside a server call (a slow or dead server): the saver is not held back.
    {
        let gate = Arc::new(SaveGate::default());
        let (in_call, in_call_seen) = mpsc::channel::<()>();
        let (go, go_seen) = mpsc::channel::<()>();
        let remote = RemotoMemoria::novo().com_gancho(move |call| {
            if call == Chamada::EnviarBlob {
                let _ = in_call.send(());
                let _ = go_seen.recv();
            }
        });
        let (_vault, _state, mut sync) = setup(remote, &gate);
        let cycle = std::thread::spawn(move || {
            sync.remoto().enter();
            let result = sync.ciclo();
            sync.remoto().leave();
            result.is_ok()
        });
        in_call_seen
            .recv_timeout(WAIT)
            .expect("the cycle reached the upload");
        let (saved, saved_seen) = mpsc::channel::<()>();
        let saver_gate = Arc::clone(&gate);
        std::thread::spawn(move || {
            let _hold = saver_gate.hold();
            let _ = saved.send(());
        });
        saved_seen
            .recv_timeout(Duration::from_secs(5))
            .expect("a save must not wait for the network");
        go.send(()).unwrap();
        assert!(cycle.join().unwrap());
        assert!(!gate.is_held(), "the gate is free after the cycle");
    }
}

/// The editor's own write path takes the gate: `PendingSave::write` waits for a held gate.
#[test]
fn the_editors_save_waits_while_the_gate_is_held() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Home.md"), "# Home\n").unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    let mut document = vault.open_document(Path::new("Home.md")).unwrap();
    document.text = "# Home\nedited\n".into();
    let pending = vault.prepare_save(&document).unwrap();

    let hold = gate::global().hold();
    let (done, done_seen) = mpsc::channel::<Result<(), String>>();
    let writer = std::thread::spawn(move || {
        let _ = done.send(pending.write());
    });
    assert!(
        done_seen.recv_timeout(Duration::from_millis(400)).is_err(),
        "the editor wrote while a sync cycle held the vault"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("Home.md")).unwrap(),
        "# Home\n",
        "nothing was written yet"
    );
    drop(hold);
    done_seen.recv_timeout(WAIT).unwrap().unwrap();
    writer.join().unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("Home.md")).unwrap(),
        "# Home\nedited\n"
    );
}

#[test]
fn the_credential_store_is_a_trait_the_tests_replace() {
    // MemoryStore stands in for the Windows Credential Manager: same contract.
    let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::default());
    store.set("a", b"1").unwrap();
    assert_eq!(store.get("a").unwrap().as_deref(), Some(&b"1"[..]));
    store.delete("a").unwrap();
    assert_eq!(store.get("a").unwrap(), None);
}
