//! Fluxos de GUI sem janela e sem X11 (R8a): o app inteiro roda dentro do harness do
//! `egui_kittest`, com teclado e texto injetados como eventos do egui. Substitui, um a um, os
//! trechos equivalentes de `tests/gui_smoke.py` (que dirige o app por X11 e só roda no Linux).
//!
//! Cada teste monta um cofre descartável numa pasta temporária; nada toca nas notas nem na
//! configuração de quem roda os testes (o estado por usuário vai para outra pasta temporária).

use super::*;
use egui_kittest::{Harness, kittest::Queryable};
use std::time::Duration;

mod canvas;
mod dividir;
mod edicao;
mod navegacao;
mod onde_aparece;
mod prints;
mod tabela;

/// Ctrl no Windows/Linux: o egui precisa dos dois marcadores para casar `COMMAND` e `CTRL`.
pub(super) const CTRL: egui::Modifiers = egui::Modifiers {
    alt: false,
    ctrl: true,
    shift: false,
    mac_cmd: false,
    command: true,
};

pub(super) const HOME: &str = "# Home\n\nVeja [[Second]] e [[Alpha]].\n";
pub(super) const SECOND: &str = "# Second\n\nDe volta para [[Home]].\n";
pub(super) const ALPHA: &str = "# Alpha\n\nA palavra rara é zebra, escrita só aqui.\n";

/// Manda o estado por usuário (sessão, lista de cofres, preferências) para uma pasta descartável.
/// Roda uma vez por processo, antes do primeiro app; sem isso o teste leria e gravaria o
/// perfil real de quem roda `cargo test`.
pub(super) fn isolate_user_state() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let folder = tempfile::tempdir()
            .expect("pasta temporária do estado")
            .keep();
        for name in [
            "LOCALAPPDATA",
            "APPDATA",
            "XDG_STATE_HOME",
            "XDG_DATA_HOME",
            "XDG_CONFIG_HOME",
        ] {
            // SAFETY: roda uma única vez, antes de qualquer app dos testes de GUI existir; os
            // demais testes do binário não leem estas variáveis.
            unsafe { std::env::set_var(name, folder.join(name)) };
        }
        // SAFETY: igual ao anterior.
        unsafe { std::env::set_var("HOME", &folder) };
    });
}

pub(super) struct Session {
    /// A pasta do cofre fica viva enquanto o teste roda.
    folder: tempfile::TempDir,
    harness: Harness<'static, NotesApp>,
}

impl Session {
    pub(super) fn open() -> Self {
        isolate_user_state();
        let folder = tempfile::tempdir().expect("cofre temporário");
        for (name, text) in [
            ("Home.md", HOME),
            ("Second.md", SECOND),
            ("Alpha.md", ALPHA),
        ] {
            std::fs::write(folder.path().join(name), text).expect("nota do cofre");
        }
        let root = folder.path().to_path_buf();
        let harness = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            // Alguns símbolos da interface (▸) não estão nas fontes do harness; viram quadrado.
            .allow_missing_glyphs()
            // O editor de texto não tem nome acessível (achado do harness; fica para a R8b).
            .with_accessibility_check(false)
            .build_eframe(move |cc| NotesApp::new(cc, &root, Instant::now(), None));
        let mut session = Session { folder, harness };
        session.wait_for(|app| app.vault.is_some() && app.loading.is_none());
        session
    }

    pub(super) fn app(&self) -> &NotesApp {
        self.harness.state()
    }

    pub(super) fn path(&self, name: &str) -> PathBuf {
        self.folder.path().join(name)
    }

    /// Passa quadros até a condição valer (o índice do cofre carrega numa thread própria).
    /// O prazo é largo (20 s) porque a suíte inteira roda em paralelo e uma máquina carregada
    /// atrasa as threads do app; o pânico aponta a linha do teste que esperava.
    #[track_caller]
    pub(super) fn wait_for(&mut self, condition: impl Fn(&NotesApp) -> bool) {
        for _ in 0..2000 {
            self.harness.step();
            if condition(self.harness.state()) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("a condição da GUI não chegou em 20 s");
    }

    pub(super) fn settle(&mut self) {
        for _ in 0..4 {
            self.harness.step();
        }
    }

    pub(super) fn key(&mut self, modifiers: egui::Modifiers, key: egui::Key) {
        self.harness.key_press_modifiers(modifiers, key);
        self.settle();
    }

    pub(super) fn type_text(&mut self, text: &str) {
        self.harness.event(egui::Event::Text(text.to_owned()));
        self.settle();
    }

    /// Abre uma nota como o usuário: Ctrl+O, o nome, Enter.
    pub(super) fn open_note(&mut self, name: &str) {
        self.key(CTRL, egui::Key::O);
        self.type_text(name);
        self.key(egui::Modifiers::NONE, egui::Key::Enter);
        let wanted = format!("{name}.md");
        self.wait_for(|app| {
            app.tabs
                .get(app.active)
                .is_some_and(|tab| tab.document.path.ends_with(&wanted))
        });
    }

    /// Roda um comando pela paleta: Ctrl+P, o nome, Enter.
    pub(super) fn command(&mut self, name: &str) {
        self.key(CTRL, egui::Key::P);
        self.type_text(name);
        self.key(egui::Modifiers::NONE, egui::Key::Enter);
    }

    pub(super) fn active_text(&self) -> String {
        self.app()
            .tabs
            .get(self.app().active)
            .map(|tab| tab.document.text.clone())
            .unwrap_or_default()
    }

    pub(super) fn has_label(&self, text: &str) -> bool {
        self.harness
            .query_all_by_label_contains(text)
            .next()
            .is_some()
    }
}

#[test]
fn abre_o_cofre_e_lista_as_notas_no_explorador() {
    let session = Session::open();
    let vault = session.app().vault.as_ref().expect("cofre aberto");
    assert_eq!(vault.notes.len(), 3, "as três notas do cofre de teste");
    assert!(
        session.app().tabs.len() <= 1,
        "no máximo a nota inicial aberta, não {}",
        session.app().tabs.len()
    );
    // O explorador lista as notas pelo nome, sem a extensão.
    for name in ["Home", "Second", "Alpha"] {
        assert!(session.has_label(name), "o explorador não mostra {name}");
    }
}

#[test]
fn abre_uma_nota_pelo_abrir_rapido() {
    let mut session = Session::open();
    session.open_note("Second");
    assert_eq!(session.app().tabs.len(), 1);
    assert_eq!(session.active_text(), SECOND);
    // Abrir de novo outra nota muda a aba ativa e o texto exibido.
    session.open_note("Alpha");
    assert_eq!(session.active_text(), ALPHA);
}

#[test]
fn edita_e_salva_com_ctrl_s_gravando_os_bytes_exatos() {
    let mut session = Session::open();
    session.open_note("Second");
    session.command("Source editing");
    session.settle();
    // O cursor vai ao fim do texto e o que se digita entra ali.
    session.key(CTRL, egui::Key::End);
    session.type_text("controle");
    assert_eq!(session.active_text(), format!("{SECOND}controle"));
    session.key(CTRL, egui::Key::S);
    let saved = std::fs::read_to_string(session.path("Second.md")).unwrap();
    assert_eq!(
        saved,
        format!("{SECOND}controle"),
        "Ctrl+S grava o que foi digitado"
    );
    // Desfazer e salvar de novo devolve a nota original, byte a byte.
    session.key(CTRL, egui::Key::Z);
    session.key(CTRL, egui::Key::S);
    let restored = std::fs::read_to_string(session.path("Second.md")).unwrap();
    assert_eq!(restored, SECOND);
}

#[test]
fn a_visualizacao_de_leitura_nao_aceita_digitacao() {
    let mut session = Session::open();
    session.open_note("Second");
    session.command("Reading view");
    session.settle();
    session.type_text("ignorado");
    session.key(CTRL, egui::Key::S);
    assert_eq!(
        std::fs::read_to_string(session.path("Second.md")).unwrap(),
        SECOND
    );
    assert_eq!(session.active_text(), SECOND);
}

#[test]
fn busca_nas_notas_pelo_painel_lateral() {
    let mut session = Session::open();
    session.key(CTRL | egui::Modifiers::SHIFT, egui::Key::F);
    assert!(session.app().searching, "Ctrl+Shift+F abre a busca");
    session.type_text("zebra");
    session.settle();
    let found: Vec<String> = session
        .app()
        .search_cache
        .paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    assert_eq!(found, ["Alpha.md"], "só a Alpha tem a palavra rara");
    assert!(session.has_label("1 resultado"));
    // Uma busca sem acerto mostra zero resultados.
    session.key(CTRL, egui::Key::A);
    session.type_text("palavra-que-nao-existe");
    session.settle();
    assert!(session.app().search_cache.paths.is_empty());
    assert!(session.has_label("0 resultados"));
}

#[test]
fn painel_de_links_de_entrada_lista_quem_aponta_para_a_nota() {
    let mut session = Session::open();
    session.open_note("Second");
    session.app_mut().show_right = true;
    session.app_mut().right_tab = 1;
    session.settle();
    // A Home é a única que aponta para a Second: o painel conta uma menção e a lista vem do cofre.
    assert!(
        session.has_label("1 menções com link"),
        "o painel deve contar uma menção"
    );
    assert_eq!(session.sources_linking_to("Second.md"), ["Home.md"]);
    // Quem aponta para a Home é a Second.
    session.open_note("Home");
    session.settle();
    assert!(session.has_label("1 menções com link"));
    assert_eq!(session.sources_linking_to("Home.md"), ["Second.md"]);
}

impl Session {
    /// Os arquivos que o cofre diz apontarem para `note` (o mesmo cálculo do painel).
    pub(super) fn sources_linking_to(&self, note: &str) -> Vec<String> {
        let vault = self.app().vault.as_ref().expect("cofre aberto");
        vault
            .backlink_mentions(Path::new(note), &[])
            .iter()
            .map(|mention| mention.path.to_string_lossy().into_owned())
            .collect()
    }

    pub(super) fn app_mut(&mut self) -> &mut NotesApp {
        self.harness.state_mut()
    }
}
