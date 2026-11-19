//! The window side of sync: the Sync page in Settings, the status in the status bar, and the
//! details window (conflicts, left-out files, errors). The work itself is in `rustidian::sync`.

use super::settings_ui::{group, row, toggle};
use super::*;
use rustidian::sync::{
    config::{SyncConfig, SyncFiles, default_device_name},
    connect::{ConnectError, Credentials, Outcome, connect_vault, disconnect_vault},
    gate,
    runner::{SyncRunner, Timing, start_for_vault},
    secrets::{KeyringStore, SecretStore},
    status::{Phase, SyncStatus, Tone},
};

#[derive(Default)]
struct Form {
    server: String,
    device: String,
    token: String,
    passphrase: String,
}

/// Everything the window keeps about sync for the open vault.
pub(crate) struct SyncPanel {
    root: Option<PathBuf>,
    files: Option<SyncFiles>,
    config: SyncConfig,
    store: Arc<dyn SecretStore>,
    runner: Option<SyncRunner>,
    status: SyncStatus,
    /// Why sync could not start, when it could not.
    start_error: Option<String>,
    form: Form,
    connecting: Option<Receiver<Result<Outcome, ConnectError>>>,
    /// The result of the last Connect or Disconnect: (it worked, what to tell the user).
    notice: Option<(bool, String)>,
    details_open: bool,
    /// The last `SyncStatus::vault_changed` the window acted on.
    reloaded: u64,
}

impl Default for SyncPanel {
    fn default() -> Self {
        Self {
            root: None,
            files: None,
            config: SyncConfig::default(),
            store: Arc::new(KeyringStore),
            runner: None,
            status: SyncStatus::default(),
            start_error: None,
            form: Form::default(),
            connecting: None,
            notice: None,
            details_open: false,
            reloaded: 0,
        }
    }
}

impl SyncPanel {
    /// Sync is running for this vault: deleting goes to the vault's own `.trash/`.
    pub(crate) fn active(&self) -> bool {
        self.runner.is_some()
    }

    /// A note was saved (or a file moved or deleted): a cycle runs a few seconds from now.
    pub(crate) fn notify_saved(&self) {
        if let Some(runner) = &self.runner {
            runner.notify_saved();
        }
    }

    fn stop(&mut self) {
        if let Some(runner) = self.runner.take() {
            runner.stop(Duration::from_millis(1500));
        }
        self.status = SyncStatus::default();
        self.reloaded = 0;
    }

    fn start(&mut self, ctx: &egui::Context) {
        self.stop();
        self.start_error = None;
        let (Some(root), Some(files)) = (self.root.clone(), self.files.clone()) else {
            return;
        };
        let wake = ctx.clone();
        match start_for_vault(
            &files,
            &*self.store,
            &root,
            Arc::clone(gate::global()),
            Timing::default(),
            move || wake.request_repaint(),
        ) {
            Ok(runner) => self.runner = Some(runner),
            Err(error) => self.start_error = Some(error),
        }
    }

    fn load_settings(&mut self) {
        self.config = self
            .files
            .as_ref()
            .and_then(|files| SyncConfig::load(&files.config).ok())
            .unwrap_or_default();
        self.form = Form {
            server: self.config.server.clone(),
            device: if self.config.device.is_empty() {
                default_device_name()
            } else {
                self.config.device.clone()
            },
            ..Form::default()
        };
    }
}

impl NotesApp {
    /// Once per frame: follow the open vault, and pick up what the sync thread reports.
    pub(crate) fn sync_tick(&mut self, ctx: &egui::Context) {
        let root = self.vault.as_ref().map(|vault| vault.root.clone());
        if self.sync.root != root {
            self.sync.stop();
            self.sync.notice = None;
            self.sync.files = root.as_deref().and_then(SyncFiles::for_vault);
            self.sync.root = root;
            self.sync.load_settings();
            if self.sync.config.connected && self.sync.config.enabled {
                self.sync.start(ctx);
                if let Some(error) = self.sync.start_error.clone() {
                    self.error = Some(error);
                }
            }
        }
        if let Some(runner) = &self.sync.runner {
            let status = runner.status();
            // O histórico das notas marca como "sync" o que muda no disco durante um ciclo.
            if status.phase == Phase::Syncing || status.vault_changed != self.sync.reloaded {
                self.historico_ui.sync_ate = Some(Instant::now() + Duration::from_secs(20));
            }
            if status.vault_changed != self.sync.reloaded {
                // A cycle wrote to the vault: reload through the same path an outside edit
                // takes. Tabs with unsaved text keep their buffer.
                self.sync.reloaded = status.vault_changed;
                self.refresh_required = true;
                self.refresh_due = Some(Instant::now() + Duration::from_millis(150));
            }
            self.sync.status = status;
        }
        let outcome =
            self.sync
                .connecting
                .as_ref()
                .and_then(|receiver| match receiver.try_recv() {
                    Ok(result) => Some(result),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => Some(Err(ConnectError::Local(
                        "A tentativa de conexão parou de forma inesperada.".into(),
                    ))),
                });
        if let Some(outcome) = outcome {
            self.sync.connecting = None;
            self.sync_connected(ctx, outcome);
        }
    }

    fn sync_connected(&mut self, ctx: &egui::Context, outcome: Result<Outcome, ConnectError>) {
        match outcome {
            Ok(outcome) => {
                self.sync.form.token.clear();
                self.sync.form.passphrase.clear();
                self.sync.load_settings();
                self.sync.start(ctx);
                self.sync.notice = Some((
                    self.sync.start_error.is_none(),
                    match (outcome, &self.sync.start_error) {
                        (_, Some(error)) => error.clone(),
                        (Outcome::Created, None) => {
                            "Conectado. Este dispositivo criou a chave do cofre; os outros dispositivos entram com a mesma frase secreta.".into()
                        }
                        (Outcome::Joined, None) => "Conectado. Este dispositivo entrou no cofre existente.".into(),
                    },
                ));
            }
            Err(error) => {
                // The passphrase is never kept; the token stays so a retry needs only the phrase.
                self.sync.form.passphrase.clear();
                self.sync.notice = Some((false, error.to_string()));
            }
        }
        ctx.request_repaint();
    }

    fn sync_connect(&mut self, ctx: &egui::Context) {
        let (Some(root), Some(files)) = (self.sync.root.clone(), self.sync.files.clone()) else {
            self.sync.notice = Some((false, "Abra um cofre primeiro.".into()));
            return;
        };
        let credentials = Credentials {
            server: self.sync.form.server.clone(),
            device: self.sync.form.device.clone(),
            token: self.sync.form.token.clone(),
            passphrase: std::mem::take(&mut self.sync.form.passphrase),
        };
        let store = Arc::clone(&self.sync.store);
        let (sender, receiver) = mpsc::channel();
        let wake = ctx.clone();
        // Argon2id and the network: off the UI thread.
        let spawned = std::thread::Builder::new()
            .name("rustidian-sync-connect".into())
            .spawn(move || {
                let _ = sender.send(connect_vault(&files, &*store, &root, &credentials));
                wake.request_repaint();
            });
        match spawned {
            Ok(_) => {
                self.sync.connecting = Some(receiver);
                self.sync.notice = None;
            }
            Err(error) => {
                self.sync.notice = Some((
                    false,
                    format!("Não foi possível iniciar a conexão: {error}"),
                ));
            }
        }
    }

    fn sync_disconnect(&mut self) {
        let (Some(root), Some(files)) = (self.sync.root.clone(), self.sync.files.clone()) else {
            return;
        };
        self.sync.stop();
        self.sync.notice = Some(match disconnect_vault(&files, &*self.sync.store, &root) {
            Ok(()) => (
                true,
                "Desconectado. A chave e o token foram apagados do cofre de credenciais do sistema; suas notas não foram mexidas.".into(),
            ),
            Err(error) => (false, format!("Não foi possível desconectar: {error}")),
        });
        self.sync.load_settings();
    }

    /// The Sync page of Settings.
    pub(super) fn sync_settings(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.sync.root.is_none() {
            ui.weak("Abra um cofre para configurar a sincronização.");
            return;
        }
        enum Action {
            Connect,
            Disconnect,
            SyncNow,
            Details,
            Toggle,
        }
        let mut action = None;
        let connecting = self.sync.connecting.is_some();
        let connected = self.sync.config.connected;
        let panel = &mut self.sync;
        group(ui, |ui| {
            if !connected {
                row(
                    ui,
                    "Endereço do servidor",
                    "O nota-servidor, por exemplo http://100.x.y.z:8443 (o endereço dele no Tailscale).",
                    |ui| {
                        let response = ui.add(
                            TextEdit::singleline(&mut panel.form.server)
                                .id_salt("sync-server")
                                .hint_text("http://100.x.y.z:8443")
                                .desired_width(ui.available_width()),
                        );
                        crate::ui_probe::record(ui.ctx(), "sync-server", response.rect);
                        false
                    },
                );
                ui.separator();
                row(
                    ui,
                    "Nome do dispositivo",
                    "Dá nome a este dispositivo nas cópias guardadas quando dois dispositivos editam a mesma nota.",
                    |ui| {
                        let response = ui.add(
                            TextEdit::singleline(&mut panel.form.device)
                                .id_salt("sync-device")
                                .desired_width(ui.available_width()),
                        );
                        crate::ui_probe::record(ui.ctx(), "sync-device", response.rect);
                        false
                    },
                );
                ui.separator();
                row(
                    ui,
                    "Token",
                    "O token deste dispositivo, de `nota-servidor token novo`. Fica no cofre de credenciais do sistema, nunca em um arquivo.",
                    |ui| {
                        let response = ui.add(
                            TextEdit::singleline(&mut panel.form.token)
                                .id_salt("sync-token")
                                .password(true)
                                .desired_width(ui.available_width()),
                        );
                        crate::ui_probe::record(ui.ctx(), "sync-token", response.rect);
                        false
                    },
                );
                ui.separator();
                row(
                    ui,
                    "Frase secreta",
                    "O primeiro dispositivo cria a chave do cofre com ela; os outros precisam digitar a mesma. Usada uma vez e não guardada: guarde-a no seu gerenciador de senhas.",
                    |ui| {
                        let response = ui.add(
                            TextEdit::singleline(&mut panel.form.passphrase)
                                .id_salt("sync-passphrase")
                                .password(true)
                                .desired_width(ui.available_width()),
                        );
                        crate::ui_probe::record(ui.ctx(), "sync-passphrase", response.rect);
                        if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            action = Some(Action::Connect);
                        }
                        false
                    },
                );
                ui.separator();
                row(
                    ui,
                    "Conectar",
                    if connecting {
                        "Conectando…"
                    } else {
                        "Confere o servidor, prepara a chave e começa a sincronizar este cofre."
                    },
                    |ui| {
                        let response = ui.add_enabled(!connecting, egui::Button::new("Conectar"));
                        crate::ui_probe::record(ui.ctx(), "sync-connect", response.rect);
                        if response.clicked() {
                            action = Some(Action::Connect);
                        }
                        false
                    },
                );
            } else {
                row(
                    ui,
                    "Servidor",
                    &format!("Este dispositivo é “{}”.", panel.config.device),
                    |ui| {
                        ui.label(&panel.config.server);
                        false
                    },
                );
                ui.separator();
                if toggle(
                    ui,
                    "Sincronizar este cofre",
                    "Desligado: nada é enviado nem recebido. A chave e o token continuam no cofre de credenciais.",
                    "sync-enabled",
                    &mut panel.config.enabled,
                ) {
                    action = Some(Action::Toggle);
                }
                ui.separator();
                row(ui, "Status", &panel.status.label().text, |ui| {
                    let response = ui.button("Detalhes");
                    crate::ui_probe::record(ui.ctx(), "sync-details", response.rect);
                    if response.clicked() {
                        action = Some(Action::Details);
                    }
                    let enabled = panel.runner.is_some();
                    let response = ui.add_enabled(enabled, egui::Button::new("Sincronizar agora"));
                    crate::ui_probe::record(ui.ctx(), "sync-now", response.rect);
                    if response.clicked() {
                        action = Some(Action::SyncNow);
                    }
                    false
                });
                ui.separator();
                row(
                    ui,
                    "Desconectar",
                    "Apaga a chave e o token deste dispositivo do cofre de credenciais do sistema. Suas notas e a cópia do servidor permanecem.",
                    |ui| {
                        let response = ui.button("Desconectar");
                        crate::ui_probe::record(ui.ctx(), "sync-disconnect", response.rect);
                        if response.clicked() {
                            action = Some(Action::Disconnect);
                        }
                        false
                    },
                );
            }
        });
        if let Some((ok, message)) = &self.sync.notice {
            ui.add_space(theme::space::M);
            let color = if *ok {
                ui.visuals().text_color()
            } else {
                ui.visuals().error_fg_color
            };
            let response =
                ui.add(egui::Label::new(RichText::new(message).color(color)).selectable(true));
            crate::ui_probe::record(ctx, "sync-notice", response.rect);
        }
        if let Some(error) = &self.sync.start_error {
            ui.add_space(theme::space::M);
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        match action {
            Some(Action::Connect) if !connecting => self.sync_connect(ctx),
            Some(Action::Disconnect) => self.sync_disconnect(),
            Some(Action::SyncNow) => {
                if let Some(runner) = &self.sync.runner {
                    runner.sync_now();
                }
            }
            Some(Action::Details) => {
                // The settings dialog is modal and would cover the details window.
                self.dialog = None;
                self.sync.details_open = true;
            }
            Some(Action::Toggle) => {
                if let Some(files) = self.sync.files.clone()
                    && let Err(error) = self.sync.config.save(&files.config)
                {
                    self.error = Some(error);
                }
                if self.sync.config.enabled {
                    self.sync.start(ctx);
                } else {
                    self.sync.stop();
                }
            }
            _ => {}
        }
        if connecting {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }

    /// The sync state at the right end of the status bar; a click opens the details.
    pub(super) fn sync_status_label(&mut self, ui: &mut egui::Ui) {
        let (text, tone) = if self.sync.runner.is_some() {
            let label = self.sync.status.label();
            (label.text, label.tone)
        } else if self.sync.start_error.is_some() {
            ("Erro de sincronização".to_owned(), Tone::Bad)
        } else {
            return;
        };
        let color = match tone {
            Tone::Good | Tone::Busy => ui.visuals().weak_text_color(),
            Tone::Attention => ui.visuals().warn_fg_color,
            Tone::Bad => ui.visuals().error_fg_color,
        };
        let response = ui
            .add(
                egui::Label::new(RichText::new(text).size(11.0).color(color))
                    .sense(egui::Sense::click()),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Detalhes da sincronização");
        crate::ui_probe::record(ui.ctx(), "sync-status", response.rect);
        if response.clicked() {
            self.sync.details_open = true;
        }
    }

    /// The details window: what is wrong (or fine), the conflict copies and what was left out.
    pub(super) fn sync_window(&mut self, ctx: &egui::Context) {
        if !self.sync.details_open {
            return;
        }
        enum Action {
            SyncNow,
            AllowDeletion,
            Open(PathBuf),
            Settings,
        }
        let status = self.sync.status.clone();
        let start_error = self.sync.start_error.clone();
        let running = self.sync.runner.is_some();
        let mut open = true;
        let mut action = None;
        egui::Window::new("Sincronização")
            .id(egui::Id::unique("sync-details-window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(420.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    if let Some(error) = &start_error {
                        ui.colored_label(ui.visuals().error_fg_color, "A sincronização não pôde começar");
                        ui.add(egui::Label::new(error).selectable(true));
                        return;
                    }
                    let label = status.label();
                    ui.strong(&label.text);
                    match status.phase {
                        Phase::Offline => {
                            ui.weak("Não foi possível acessar o servidor. O Rustidian continua com a cópia deste dispositivo e tenta de novo em um minuto.");
                            if let Some(reason) = &status.offline_reason {
                                ui.add(egui::Label::new(RichText::new(reason).weak()).selectable(true));
                            }
                        }
                        Phase::Failed => {
                            if let Some(text) = status.mass_deletion_text() {
                                ui.add(egui::Label::new(text).selectable(true));
                                if ui.button("Permitir esta exclusão uma vez").clicked() {
                                    action = Some(Action::AllowDeletion);
                                }
                            } else if let Some(error) = &status.error {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(error).color(ui.visuals().error_fg_color),
                                    )
                                    .selectable(true),
                                );
                            }
                        }
                        Phase::Synced => {
                            if let Some(time) = status.last_sync {
                                ui.weak(format!("Última sincronização {}", time.format("%Y-%m-%d %H:%M")));
                            }
                        }
                        Phase::Starting | Phase::Syncing => {}
                    }
                    ui.horizontal(|ui| {
                        if ui.add_enabled(running, egui::Button::new("Sincronizar agora")).clicked() {
                            action = Some(Action::SyncNow);
                        }
                        if ui.button("Configurações").clicked() {
                            action = Some(Action::Settings);
                        }
                    });
                    if !status.conflicts.is_empty() {
                        ui.add_space(theme::space::M);
                        ui.strong(format!("Conflitos ({})", status.conflicts.len()));
                        ui.weak("Dois dispositivos editaram a mesma nota. A versão do servidor ficou com o nome; a sua foi salva como cópia. Una as duas à mão e depois apague a cópia.");
                        for name in &status.conflicts {
                            if ui.link(name).clicked() {
                                action = Some(Action::Open(PathBuf::from(name)));
                            }
                        }
                    }
                    if !status.refused.is_empty() {
                        ui.add_space(theme::space::M);
                        ui.strong(format!("Ignorados ({})", status.refused.len()));
                        ui.weak("Estes não foram sincronizados, e o motivo.");
                        for line in &status.refused {
                            ui.add(egui::Label::new(RichText::new(line).size(12.0)).selectable(true));
                        }
                    }
                });
            });
        if !open {
            self.sync.details_open = false;
        }
        match action {
            Some(Action::SyncNow) => {
                if let Some(runner) = &self.sync.runner {
                    runner.sync_now();
                }
            }
            Some(Action::AllowDeletion) => {
                if let Some(runner) = &self.sync.runner {
                    runner.allow_mass_deletion_once();
                }
            }
            Some(Action::Open(path)) => {
                self.sync.details_open = false;
                self.open_note(&path);
            }
            Some(Action::Settings) => {
                self.sync.details_open = false;
                self.settings_page = settings_ui::Page::Sync;
                self.ask(Dialog::Settings);
            }
            None => {}
        }
    }

    /// Deleting in the app: into the vault's `.trash/` when sync is on, else the system Trash.
    pub(crate) fn sync_trash_in_vault(&self) -> bool {
        self.sync.active()
    }
}
