//! R15: tela dividida em dois painéis, sem janela (egui_kittest). Cobre dividir pela paleta, pelo
//! menu da aba e por Ctrl+Alt+clique num link, a mesma nota nos dois painéis (uma aba só, salva uma
//! vez só, sem perder edição), o foco do teclado, fechar painel e aba, arrastar a divisória e o
//! layout lembrado ao reabrir.

use super::edicao::NONE;
use super::*;
use rustidian::panes::Orientation;

const CTRL_ALT: egui::Modifiers = egui::Modifiers {
    alt: true,
    ctrl: true,
    shift: false,
    mac_cmd: false,
    command: true,
};

impl Session {
    fn pane_paths(&self, pane: usize) -> Vec<String> {
        self.app()
            .panes
            .tabs_of(pane)
            .iter()
            .filter_map(|id| self.app().tabs.iter().find(|tab| tab.id() == *id))
            .map(|tab| tab.document.path.to_string_lossy().into_owned())
            .collect()
    }

    fn active_path(&self) -> String {
        self.app()
            .tabs
            .get(self.app().active)
            .map(|tab| tab.document.path.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// Quantos controles de texto mostram exatamente este texto (o editor tem o texto como valor).
    fn editors_with(&self, text: &str) -> usize {
        self.harness.query_all_by_value(text).count()
    }

    /// Abre o app de novo sobre o mesmo cofre, como depois de fechar e abrir o programa.
    fn reopen(&mut self) {
        let root = self.folder.path().to_path_buf();
        let harness = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .allow_missing_glyphs()
            .with_accessibility_check(false)
            .build_eframe(move |cc| NotesApp::new(cc, &root, Instant::now(), None));
        self.harness = harness;
        self.wait_for(|app| app.vault.is_some() && app.loading.is_none());
        self.settle();
    }

    fn pointer(&mut self, pos: egui::Pos2, pressed: Option<bool>) {
        self.harness.event(egui::Event::PointerMoved(pos));
        if let Some(pressed) = pressed {
            self.harness.event(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: NONE,
            });
        }
        self.harness.step();
    }
}

#[test]
fn dividir_a_direita_pela_paleta_mostra_a_mesma_nota_nos_dois_paineis() {
    let mut session = Session::open();
    session.open_note("Second");
    assert!(!session.app().panes.is_split());
    session.command("Dividir à direita");
    session.settle();
    let app = session.app();
    assert!(app.panes.is_split(), "a tela ficou dividida");
    assert_eq!(app.panes.orientation(), Orientation::Side);
    assert_eq!(app.tabs.len(), 1, "a mesma aba aparece nos dois painéis");
    assert_eq!(app.panes.shown_in(app.tabs[0].id()), 2);
    assert_eq!(app.panes.focus(), 1, "o painel novo fica com o foco");
    assert_eq!(app.pane_rects.len(), 2);
    let (left, right) = (app.pane_rects[0], app.pane_rects[1]);
    assert!(
        left.right() <= right.left(),
        "lado a lado: {left:?} {right:?}"
    );
    assert!((left.top() - right.top()).abs() < 1.0);
}

#[test]
fn dividir_abaixo_empilha_os_paineis() {
    let mut session = Session::open();
    session.open_note("Second");
    session.command("Dividir abaixo");
    session.settle();
    let app = session.app();
    assert_eq!(app.panes.orientation(), Orientation::Stacked);
    let (top, bottom) = (app.pane_rects[0], app.pane_rects[1]);
    assert!(
        top.bottom() <= bottom.top(),
        "em cima e embaixo: {top:?} {bottom:?}"
    );
    assert!((top.left() - bottom.left()).abs() < 1.0);
    // Dividir de novo não cria um terceiro painel.
    session.command("Dividir abaixo");
    session.settle();
    assert_eq!(session.app().panes.len(), 2);
}

#[test]
fn a_mesma_nota_nos_dois_paineis_e_uma_so_e_o_que_se_digita_aparece_nos_dois() {
    let mut session = Session::open();
    session.open_note("Second");
    session.command("Source editing");
    session.command("Dividir à direita");
    session.settle();
    assert_eq!(session.app().tabs.len(), 1);
    let before = session.active_text();
    // Painel de baixo/direita (com o foco): digita no fim.
    session.key(CTRL, egui::Key::End);
    session.type_text("direita");
    let after_right = format!("{before}direita");
    assert_eq!(session.active_text(), after_right);
    // O outro painel mostra o mesmo texto agora.
    assert_eq!(
        session.editors_with(&after_right),
        2,
        "os dois editores mostram a nota editada"
    );
    // Passa o foco ao outro painel e digita lá: nada se perde.
    session.command("Ir para o outro painel");
    session.settle();
    assert_eq!(session.app().panes.focus(), 0);
    session.key(CTRL, egui::Key::End);
    session.type_text("esquerda");
    let both = format!("{before}direitaesquerda");
    assert_eq!(session.active_text(), both);
    assert_eq!(session.editors_with(&both), 2);
    // Um save só, pelo caminho normal: o arquivo tem tudo e a nota não fica suja.
    session.key(CTRL, egui::Key::S);
    assert_eq!(
        std::fs::read_to_string(session.path("Second.md")).unwrap(),
        both
    );
    assert!(!session.app().tabs[0].document.dirty());
    assert_eq!(session.app().tabs.len(), 1, "continua uma aba só");
}

#[test]
fn o_autosave_grava_a_nota_dividida_sem_salvar_duas_vezes_nem_perder_edicao() {
    let mut session = Session::open();
    session.open_note("Second");
    session.command("Source editing");
    session.command("Dividir à direita");
    session.settle();
    session.key(CTRL, egui::Key::End);
    session.type_text("auto");
    let wanted = format!("{SECOND}auto");
    // Sem Ctrl+S: o autosave (600 ms) leva o texto ao disco, uma gravação para a aba única.
    session.wait_for(|app| !app.tabs[0].document.dirty());
    session.app_mut().settle_saves();
    assert_eq!(
        std::fs::read_to_string(session.path("Second.md")).unwrap(),
        wanted
    );
    assert_eq!(session.app().tabs.len(), 1);
}

#[test]
fn abrir_a_nota_no_outro_painel_cria_a_divisao_e_foca_a_nota() {
    let mut session = Session::open();
    session.open_note("Home");
    session
        .app_mut()
        .open_note_in_other_pane(std::path::Path::new("Alpha.md"), Orientation::Side);
    session.settle();
    let app = session.app();
    assert!(app.panes.is_split());
    assert_eq!(app.tabs.len(), 2);
    assert_eq!(session.pane_paths(0), ["Home.md"]);
    assert_eq!(session.pane_paths(1), ["Alpha.md"]);
    assert_eq!(
        session.active_path(),
        "Alpha.md",
        "o foco vai para a nota aberta"
    );
    assert_eq!(session.active_text(), ALPHA);
    // Com o foco no painel da Alpha, abrir a Second "no outro painel" a põe no da Home.
    session
        .app_mut()
        .open_note_in_other_pane(std::path::Path::new("Second.md"), Orientation::Side);
    session.settle();
    assert_eq!(session.app().tabs.len(), 3);
    assert_eq!(session.pane_paths(0), ["Home.md", "Second.md"]);
    assert_eq!(session.pane_paths(1), ["Alpha.md"]);
    assert_eq!(session.app().panes.focus(), 0);
    // Abrir uma nota que já está aberta mostra a MESMA aba no outro painel, sem duplicar.
    session
        .app_mut()
        .open_note_in_other_pane(std::path::Path::new("Second.md"), Orientation::Side);
    session.settle();
    assert_eq!(
        session.app().tabs.len(),
        3,
        "nenhuma aba nova para uma nota já aberta"
    );
    assert_eq!(session.pane_paths(1), ["Alpha.md", "Second.md"]);
    assert_eq!(session.app().panes.shown_in(session.app().tabs[2].id()), 2);
}

#[test]
fn ctrl_alt_enter_sobre_um_link_abre_a_nota_no_outro_painel() {
    let mut session = Session::open();
    session.open_note("Home");
    session.command("Source editing");
    // Cursor dentro de [[Second]], na terceira linha ("Veja [[Second]] e [[Alpha]].").
    session.key(CTRL, egui::Key::Home);
    session.key(NONE, egui::Key::ArrowDown);
    session.key(NONE, egui::Key::ArrowDown);
    for _ in 0..9 {
        session.key(NONE, egui::Key::ArrowRight);
    }
    session.key(CTRL_ALT, egui::Key::Enter);
    session.settle();
    session.settle();
    assert!(session.app().panes.is_split(), "o link abriu uma divisão");
    assert_eq!(session.pane_paths(0), ["Home.md"]);
    assert_eq!(session.pane_paths(1), ["Second.md"]);
    assert_eq!(session.active_path(), "Second.md");
}

#[test]
fn o_gesto_de_dividir_no_clique_de_um_link_manda_a_nota_para_o_outro_painel() {
    // Ctrl+Alt+clique: o app guarda o gesto do quadro e `follow_link` o respeita.
    let mut session = Session::open();
    session.open_note("Home");
    session.app_mut().split_gesture = true;
    session.app_mut().follow_link("Second");
    session.settle();
    assert!(session.app().panes.is_split());
    assert_eq!(session.pane_paths(1), ["Second.md"]);
    // Sem o gesto, o mesmo link abre na própria aba (nada de divisão nova).
    let mut session = Session::open();
    session.open_note("Home");
    session.app_mut().follow_link("Second");
    session.settle();
    assert!(!session.app().panes.is_split());
    assert_eq!(session.active_path(), "Second.md");
}

#[test]
fn o_menu_da_aba_divide_a_tela() {
    let mut session = Session::open();
    session.open_note("Home");
    // Botão direito na aba: o menu tem os dois itens.
    {
        let tab = session
            .harness
            .query_all_by_role(egui::Role::Tab)
            .next()
            .expect("a aba da Home");
        tab.click_secondary();
    }
    session.settle();
    session.click_label("Dividir abaixo");
    session.settle();
    assert!(session.app().panes.is_split());
    assert_eq!(session.app().panes.orientation(), Orientation::Stacked);
}

#[test]
fn o_foco_segue_o_clique_e_a_paleta_age_no_painel_ativo() {
    let mut session = Session::open();
    session.open_note("Home");
    session
        .app_mut()
        .open_note_in_other_pane(std::path::Path::new("Alpha.md"), Orientation::Side);
    session.settle();
    assert_eq!(session.app().panes.focus(), 1);
    // Clique dentro do painel da esquerda: ele vira o painel ativo.
    let left = session.app().pane_rects[0];
    let inside = egui::pos2(left.center().x, left.top() + 200.0);
    session.pointer(inside, None);
    session.pointer(inside, Some(true));
    session.pointer(inside, Some(false));
    session.settle();
    assert_eq!(session.app().panes.focus(), 0, "o clique moveu o foco");
    assert_eq!(session.active_path(), "Home.md");
    // "Fechar aba atual" fecha a aba do painel ativo (a Home), não a do outro.
    session.command("Close current tab");
    session.settle();
    assert_eq!(session.app().tabs.len(), 1);
    assert_eq!(session.active_path(), "Alpha.md");
    assert!(!session.app().panes.is_split(), "o painel vazio fechou");
}

#[test]
fn fechar_a_aba_de_um_painel_que_o_outro_tambem_mostra_so_a_tira_dali() {
    let mut session = Session::open();
    session.open_note("Second");
    session.command("Dividir à direita");
    session.settle();
    assert_eq!(session.app().panes.focus(), 1);
    session.key(CTRL, egui::Key::W);
    session.settle();
    // A nota continua aberta (no painel 0), sem pergunta nem perda; sobra um painel só.
    assert_eq!(session.app().tabs.len(), 1);
    assert!(!session.app().panes.is_split());
    assert_eq!(session.active_path(), "Second.md");
    assert!(session.app().pending.is_none());
}

#[test]
fn fechar_o_painel_junta_as_abas_no_que_sobra_e_nao_fecha_nenhuma_nota() {
    let mut session = Session::open();
    session.open_note("Home");
    session
        .app_mut()
        .open_note_in_other_pane(std::path::Path::new("Alpha.md"), Orientation::Side);
    session.settle();
    session.command("Fechar o painel dividido");
    session.settle();
    assert!(!session.app().panes.is_split());
    assert_eq!(session.app().tabs.len(), 2, "nenhuma nota fechou");
    assert_eq!(session.app().panes.tabs_of(0).len(), 2);
    // Sem divisão, o comando nem aparece na paleta.
    session.key(CTRL, egui::Key::P);
    session.type_text("Fechar o painel");
    session.settle();
    assert!(!session.has_label("Fechar o painel dividido"));
}

#[test]
fn nota_suja_no_painel_nao_se_perde_ao_fechar_o_painel() {
    let mut session = Session::open();
    session.open_note("Home");
    session.command("Source editing");
    session.key(CTRL, egui::Key::End);
    session.type_text("rascunho");
    session
        .app_mut()
        .open_note_in_other_pane(std::path::Path::new("Alpha.md"), Orientation::Stacked);
    session.settle();
    session.command("Fechar o painel dividido");
    session.settle();
    let home = session
        .app()
        .tabs
        .iter()
        .find(|tab| tab.document.path == Path::new("Home.md"))
        .expect("a Home continua aberta");
    assert!(home.document.text.ends_with("rascunho"));
}

#[test]
fn arrastar_a_divisoria_muda_o_tamanho_dos_paineis_e_respeita_o_minimo() {
    let mut session = Session::open();
    session.open_note("Home");
    session.command("Dividir à direita");
    session.settle();
    let first = session.app().pane_rects.clone();
    let width = first[0].width();
    let divider = egui::pos2(first[0].right() + 3.0, first[0].center().y);
    let to = divider + egui::vec2(150.0, 0.0);
    session.pointer(divider, None);
    session.pointer(divider, Some(true));
    for step in 1..=5 {
        session.pointer(divider + (to - divider) * (step as f32 / 5.0), None);
    }
    session.pointer(to, Some(false));
    session.settle();
    let after = session.app().pane_rects.clone();
    assert!(
        after[0].width() > width + 100.0,
        "o painel da esquerda cresceu: {width} -> {}",
        after[0].width()
    );
    assert!(after[1].width() >= rustidian::panes::MIN_PANE - 1.0);
    // Arrastar para muito longe para no mínimo do outro painel.
    let divider = egui::pos2(after[0].right() + 3.0, after[0].center().y);
    let to = divider + egui::vec2(3000.0, 0.0);
    session.pointer(divider, None);
    session.pointer(divider, Some(true));
    for step in 1..=5 {
        session.pointer(divider + (to - divider) * (step as f32 / 5.0), None);
    }
    session.pointer(to, Some(false));
    session.settle();
    let after = session.app().pane_rects.clone();
    assert!(
        after[1].width() >= rustidian::panes::MIN_PANE - 1.0,
        "{after:?}"
    );
}

#[test]
fn o_layout_dividido_volta_ao_reabrir_o_cofre() {
    let mut session = Session::open();
    session.open_note("Home");
    session
        .app_mut()
        .open_note_in_other_pane(std::path::Path::new("Alpha.md"), Orientation::Stacked);
    session.settle();
    session.app_mut().persist_session();
    session.reopen();
    let app = session.app();
    assert!(app.panes.is_split(), "a divisão voltou");
    assert_eq!(app.panes.orientation(), Orientation::Stacked);
    assert_eq!(session.pane_paths(0), ["Home.md"]);
    assert_eq!(session.pane_paths(1), ["Alpha.md"]);
    assert_eq!(session.app().panes.focus(), 1);
    assert_eq!(session.active_path(), "Alpha.md");
    assert_eq!(session.app().pane_rects.len(), 2);
}

#[test]
fn a_mesma_nota_nos_dois_paineis_tambem_volta_ao_reabrir() {
    let mut session = Session::open();
    session.open_note("Second");
    session.command("Dividir à direita");
    session.settle();
    session.app_mut().persist_session();
    session.reopen();
    assert!(session.app().panes.is_split());
    assert_eq!(session.app().tabs.len(), 1, "continua sendo uma aba só");
    assert_eq!(session.app().panes.shown_in(session.app().tabs[0].id()), 2);
    assert_eq!(session.pane_paths(0), ["Second.md"]);
    assert_eq!(session.pane_paths(1), ["Second.md"]);
}

#[test]
fn quadro_canvas_abre_no_outro_painel_ao_lado_de_uma_nota() {
    let plano = r##"{"nodes":[{"id":"n1","type":"text","x":0,"y":0,"width":260,"height":80,"text":"ideia"}],"edges":[]}"##;
    let mut session = Session::open_with(&[("Plano.canvas", plano.as_bytes())]);
    session.open_note("Home");
    session
        .app_mut()
        .open_note_in_other_pane(std::path::Path::new("Plano.canvas"), Orientation::Side);
    session.settle();
    session.settle();
    assert!(session.app().panes.is_split());
    assert_eq!(session.pane_paths(1), ["Plano.canvas"]);
    // O quadro foi desenhado no painel da direita.
    let id = session.app().tabs[session.app().active].id();
    assert!(session.app().canvases.state(id).is_some());
    // E o outro painel continua mostrando a Home.
    assert_eq!(session.pane_paths(0), ["Home.md"]);
}

#[test]
fn abrir_outra_nota_no_painel_de_uma_aba_compartilhada_nao_troca_a_do_outro_painel() {
    let mut session = Session::open();
    session.open_note("Second");
    session.command("Dividir à direita");
    session.settle();
    // No painel focado (direita) abre a Alpha pelo Abrir rápido: o painel da esquerda segue na Second.
    session.open_note("Alpha");
    session.settle();
    assert_eq!(session.active_path(), "Alpha.md");
    assert!(session.pane_paths(0).contains(&"Second.md".to_owned()));
    assert!(session.pane_paths(1).contains(&"Alpha.md".to_owned()));
    let left = session.app().panes.active_of(0).unwrap();
    let second = session
        .app()
        .tabs
        .iter()
        .find(|tab| tab.document.path == Path::new("Second.md"))
        .unwrap();
    assert_eq!(left, second.id(), "o painel da esquerda ficou na Second");
}
