//! H1: o quadro como lugar de organizar notas. O cartão de nota mostra e edita a nota de verdade
//! (um `Document` por nota), recolhe, rola, recebe notas do explorador e da biblioteca, e abre a
//! nota inteira no outro painel.

use super::canvas::{click, double_click, drag, move_to, on_disk, parsed, rect_of, state, tab_id};
use super::*;
use rustidian::canvas::{self, Canvas};
use std::time::SystemTime;

const QUADRO: &str = r#"{
	"nodes":[
		{"id":"home","type":"file","x":0,"y":0,"width":420,"height":300,"file":"Home.md"},
		{"id":"alpha","type":"file","x":500,"y":0,"width":420,"height":300,"file":"Alpha.md","extra":{"a":1}},
		{"id":"fantasma","type":"file","x":0,"y":400,"width":420,"height":240,"file":"Fantasma.md"}
	],
	"edges":[],
	"metadata":{"version":"1.0-1.0"}
}"#;

fn session_with_board(board: &str) -> Session {
    let mut session = Session::open_with(&[("Quadro.canvas", board.as_bytes())]);
    session
        .app_mut()
        .open_note(std::path::Path::new("Quadro.canvas"));
    session.settle();
    session
}

fn board_session() -> Session {
    session_with_board(QUADRO)
}

/// O corpo do cartão: abaixo do cabeçalho, onde o duplo clique entra em edição.
fn body_of(session: &Session, id: &str) -> egui::Pos2 {
    let rect = rect_of(session, id);
    egui::pos2(rect.center().x, rect.min.y + rect.height() * 0.6)
}

fn double_click_body(session: &mut Session, id: &str) {
    let position = body_of(session, id);
    double_click(session, position);
}

fn text_of(session: &Session, name: &str) -> Option<String> {
    session
        .app()
        .tabs
        .iter()
        .chain(session.app().card_tabs.iter())
        .find(|tab| tab.document.path.ends_with(name))
        .map(|tab| tab.document.text.clone())
}

fn disk(session: &Session, name: &str) -> String {
    std::fs::read_to_string(session.path(name)).expect("nota no disco")
}

fn modified(session: &Session, name: &str) -> SystemTime {
    std::fs::metadata(session.path(name))
        .and_then(|meta| meta.modified())
        .expect("data de modificação")
}

fn step_for(session: &mut Session, millis: u64) {
    let until = Instant::now() + Duration::from_millis(millis);
    while Instant::now() < until {
        session.harness.step();
        std::thread::sleep(Duration::from_millis(15));
    }
}

fn wheel(session: &mut Session, delta: f32, modifiers: egui::Modifiers) {
    session.harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, delta),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Unknown,
        modifiers,
    });
    session.settle();
}

const CTRL_ALT: egui::Modifiers = egui::Modifiers {
    alt: true,
    ctrl: true,
    shift: false,
    mac_cmd: false,
    command: true,
};

#[test]
fn o_cartao_mostra_a_nota_sem_abrir_aba_nem_sujar_nada() {
    let session = board_session();
    // Só o quadro está aberto; abrir um quadro com notas não cria aba nem cartão aberto.
    assert_eq!(session.app().tabs.len(), 1);
    assert!(session.app().card_tabs.is_empty());
    assert!(!session.app().tabs[0].document.dirty());
    // O cartão da Home desenhou o texto e sabe o quanto rola.
    let (scroll, max) = state(&session)
        .card_scroll("home")
        .expect("o cartão de nota foi desenhado");
    assert_eq!(scroll, 0.0);
    assert!(max >= 0.0);
    // O arquivo do quadro não mudou.
    assert_eq!(
        std::fs::read_to_string(session.path("Quadro.canvas")).unwrap(),
        QUADRO
    );
}

#[test]
fn duplo_clique_edita_pela_aba_aberta_e_nada_vira_dois_documentos() {
    let mut session = board_session();
    // A Home aberta numa aba (a do quadro continua existindo).
    session.app_mut().open_note_at(Path::new("Home.md"), true);
    session.settle();
    let board = session
        .app()
        .tabs
        .iter()
        .position(|tab| tab.document.path.ends_with("Quadro.canvas"))
        .expect("aba do quadro");
    session.app_mut().active = board;
    session.settle();
    assert_eq!(session.app().tabs.len(), 2);

    double_click_body(&mut session, "home");
    assert_eq!(state(&session).editing_note(), Some("home"));
    session.type_text("do cartão");
    // A aba da Home já tem o texto, e nenhum cartão aberto foi criado: um Document só.
    let home = text_of(&session, "Home.md").unwrap();
    assert!(home.ends_with("do cartão"), "{home:?}");
    assert!(session.app().card_tabs.is_empty());
    assert_eq!(session.app().tabs.len(), 2);
    let home_tab = session
        .app()
        .tabs
        .iter()
        .find(|tab| tab.document.path.ends_with("Home.md"))
        .unwrap();
    assert!(home_tab.document.dirty());

    // O caminho de volta: o que muda na aba aparece no que o cartão edita em seguida.
    if let Some(tab) = session
        .app_mut()
        .tabs
        .iter_mut()
        .find(|tab| tab.document.path.ends_with("Home.md"))
    {
        tab.document.text.push_str(" + da aba");
        tab.edited();
    }
    session.settle();
    session.type_text("!");
    let home = text_of(&session, "Home.md").unwrap();
    assert!(
        home.contains("do cartão!") && home.ends_with(" + da aba"),
        "o cartão edita o mesmo texto da aba: {home:?}"
    );
    // O quadro em si continua sem alteração.
    assert_eq!(
        std::fs::read_to_string(session.path("Quadro.canvas")).unwrap(),
        QUADRO
    );
}

#[test]
fn sem_aba_o_cartao_abre_um_documento_proprio_e_o_autosave_salva_uma_vez() {
    let mut session = board_session();
    let before = disk(&session, "Alpha.md");
    double_click_body(&mut session, "alpha");
    assert_eq!(state(&session).editing_note(), Some("alpha"));
    session.type_text("\nlinha nova");
    // Sem aba para a Alpha: o app guarda um cartão aberto (sem aba na tela).
    assert_eq!(session.app().tabs.len(), 1);
    assert_eq!(session.app().card_tabs.len(), 1);
    assert_eq!(disk(&session, "Alpha.md"), before, "ainda não salvou");
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    assert_eq!(state(&session).editing_note(), None);
    // O autosave grava e o cartão aberto some quando não há mais o que salvar.
    session.wait_for(|app| app.card_tabs.is_empty());
    let saved = disk(&session, "Alpha.md");
    assert_eq!(saved, format!("{before}\nlinha nova"));
    // Salvou uma vez só: mais um tempo de quadros e o arquivo nem é tocado de novo.
    let stamp = modified(&session, "Alpha.md");
    step_for(&mut session, 1200);
    assert_eq!(modified(&session, "Alpha.md"), stamp);
    assert_eq!(disk(&session, "Alpha.md"), saved);
    assert!(session.app().card_tabs.is_empty());
    assert_eq!(session.app().tabs.len(), 1);
}

#[test]
fn entrar_e_sair_da_edicao_sem_digitar_nao_grava_a_nota() {
    let mut session = board_session();
    let stamp = modified(&session, "Alpha.md");
    let before = disk(&session, "Alpha.md");
    double_click_body(&mut session, "alpha");
    assert_eq!(state(&session).editing_note(), Some("alpha"));
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    assert_eq!(state(&session).editing_note(), None);
    assert!(
        session
            .app()
            .card_tabs
            .iter()
            .all(|tab| !tab.document.dirty())
    );
    step_for(&mut session, 1200);
    assert_eq!(modified(&session, "Alpha.md"), stamp);
    assert_eq!(disk(&session, "Alpha.md"), before);
    // Sem nada para salvar, o cartão aberto também foi embora.
    assert!(session.app().card_tabs.is_empty());
}

#[test]
fn digitar_e_desfazer_ate_o_texto_original_tambem_nao_grava() {
    let mut session = board_session();
    let stamp = modified(&session, "Alpha.md");
    double_click_body(&mut session, "alpha");
    session.type_text("x");
    session.key(egui::Modifiers::NONE, egui::Key::Backspace);
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    step_for(&mut session, 1200);
    assert_eq!(modified(&session, "Alpha.md"), stamp);
}

#[test]
fn ctrl_s_salva_o_cartao_na_hora() {
    let mut session = board_session();
    double_click_body(&mut session, "alpha");
    session.type_text(" salvo");
    session.key(CTRL, egui::Key::S);
    let saved = disk(&session, "Alpha.md");
    assert!(saved.ends_with(" salvo"), "{saved:?}");
    assert!(
        session
            .app()
            .card_tabs
            .iter()
            .all(|tab| !tab.document.dirty())
    );
}

#[test]
fn abrir_a_nota_numa_aba_com_o_cartao_editando_entrega_o_texto_sem_perder() {
    let mut session = board_session();
    double_click_body(&mut session, "alpha");
    session.type_text(" do cartão");
    assert_eq!(session.app().card_tabs.len(), 1);
    // A nota agora também numa aba: o texto do cartão passa para a aba e o cartão fecha.
    session.app_mut().open_note_at(Path::new("Alpha.md"), true);
    session.settle();
    session.settle();
    assert!(session.app().card_tabs.is_empty());
    let tab = session
        .app()
        .tabs
        .iter()
        .find(|tab| tab.document.path.ends_with("Alpha.md"))
        .expect("aba da Alpha");
    assert!(tab.document.text.ends_with(" do cartão"));
    // Quem salva agora é a aba; a nota chega ao disco uma vez, com o texto.
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    session.wait_for(|app| {
        std::fs::read_to_string(app.vault.as_ref().unwrap().root.join("Alpha.md"))
            .is_ok_and(|text| text.ends_with(" do cartão"))
    });
}

#[test]
fn nota_que_nao_existe_mostra_aviso_e_nao_quebra_nem_perde_o_cartao() {
    let mut session = board_session();
    assert!(
        state(&session).canvas().node("fantasma").is_some(),
        "o cartão continua no quadro"
    );
    // O duplo clique no corpo de uma nota que sumiu não entra em edição nem abre aba.
    double_click_body(&mut session, "fantasma");
    assert_eq!(state(&session).editing_note(), None);
    assert_eq!(session.app().tabs.len(), 1);
    assert!(session.app().card_tabs.is_empty());
    assert!(session.app().error.is_none());
    // Apagada fora do app com o quadro aberto: segue desenhando e o quadro não muda.
    std::fs::remove_file(session.path("Home.md")).unwrap();
    session.settle();
    step_for(&mut session, 300);
    assert!(state(&session).canvas().node("home").is_some());
    assert_eq!(
        std::fs::read_to_string(session.path("Quadro.canvas")).unwrap(),
        QUADRO
    );
}

#[test]
fn recolher_guarda_no_proprio_no_e_expandir_devolve() {
    let mut session = board_session();
    let height = parsed(&session).node("alpha").unwrap().height;
    let (collapse, _) = state(&session).note_buttons("alpha").unwrap();
    click(&mut session, collapse.center());
    let node = parsed(&session).node("alpha").cloned().unwrap();
    assert!(node.is_collapsed());
    assert_eq!(node.height, canvas::NOTE_HEADER);
    // O campo extra que já existia no nó segue ali, e o arquivo (depois de salvar) tem os campos
    // nossos como campos opcionais do JSON Canvas.
    assert_eq!(node.extra["extra"]["a"], 1);
    session.key(CTRL, egui::Key::S);
    let saved = on_disk(&session, "Quadro.canvas");
    let alpha = &saved["nodes"][1];
    assert_eq!(alpha["rustidianCollapsed"], true);
    assert_eq!(alpha["rustidianHeight"], height as i64);
    assert_eq!(saved["metadata"]["version"], "1.0-1.0");

    // Recolhido, o botão está no mesmo canto e expande.
    let (collapse, _) = state(&session).note_buttons("alpha").unwrap();
    click(&mut session, collapse.center());
    let node = parsed(&session).node("alpha").cloned().unwrap();
    assert!(!node.is_collapsed());
    assert_eq!(node.height, height);
    session.key(CTRL, egui::Key::S);
    let saved = Canvas::parse(&disk(&session, "Quadro.canvas")).unwrap();
    assert!(!saved.to_json().contains("rustidian"));
    assert_eq!(saved.node("alpha").unwrap().extra["extra"]["a"], 1);
}

#[test]
fn a_roda_sobre_um_cartao_que_nao_cabe_rola_o_cartao_e_o_fundo_continua_dando_zoom() {
    let long: String = (1..=60).map(|n| format!("linha {n}\n")).collect();
    let mut session = Session::open_with(&[
        ("Longa.md", long.as_bytes()),
        ("Quadro.canvas", QUADRO.as_bytes()),
    ]);
    let board = QUADRO.replace("\"file\":\"Alpha.md\"", "\"file\":\"Longa.md\"");
    std::fs::write(session.path("Quadro.canvas"), board).unwrap();
    session.settle();
    session
        .app_mut()
        .open_note(std::path::Path::new("Quadro.canvas"));
    session.settle();
    let (scroll, max) = state(&session)
        .card_scroll("alpha")
        .expect("cartão desenhado");
    assert_eq!(scroll, 0.0);
    assert!(max > 50.0, "a nota longa não cabe no cartão: {max}");
    let zoom = state(&session).zoom();
    let on_card = body_of(&session, "alpha");
    move_to(&mut session, on_card);
    wheel(&mut session, -60.0, egui::Modifiers::NONE);
    let (scrolled, _) = state(&session).card_scroll("alpha").unwrap();
    assert!(scrolled > 0.0, "rolou o cartão: {scrolled}");
    assert_eq!(state(&session).zoom(), zoom, "rolar o cartão não dá zoom");
    // Fora de qualquer cartão, a roda continua dando zoom.
    move_to(&mut session, egui::pos2(700.0, 700.0));
    wheel(&mut session, 120.0, egui::Modifiers::NONE);
    assert!(state(&session).zoom() > zoom);
}

#[test]
fn o_botao_e_o_ctrl_alt_clique_abrem_a_nota_no_outro_painel_e_o_quadro_fica() {
    let mut session = board_session();
    let board_id = tab_id(&session);
    assert!(!session.app().panes.is_split());
    // O botão do cabeçalho: cria a divisão à direita com a Alpha, o quadro fica onde estava.
    let (_, aside) = state(&session).note_buttons("alpha").unwrap();
    click(&mut session, aside.center());
    session.settle();
    session.settle();
    let app = session.app();
    assert!(app.panes.is_split(), "a divisão foi criada");
    let focused = app.panes.focused_active().expect("aba ativa do painel");
    let note = app.tabs.iter().find(|tab| tab.id() == focused).unwrap();
    assert!(note.document.path.ends_with("Alpha.md"));
    assert!(app.panes.tabs_of(0).contains(&board_id));
    assert_eq!(app.panes.active_of(0), Some(board_id));
    assert_eq!(
        app.panes.orientation(),
        rustidian::panes::Orientation::Side,
        "à direita"
    );

    // Ctrl+Alt+clique no corpo do cartão da Home: ela aparece no outro painel (a mesma aba se
    // já estiver aberta), sem sair do quadro.
    let target = body_of_on(&session, board_id, "home");
    move_to(&mut session, target);
    for pressed in [true, false] {
        session
            .harness
            .input_mut()
            .events
            .push(egui::Event::PointerButton {
                pos: target,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: CTRL_ALT,
            });
    }
    session.harness.step();
    session.settle();
    session.settle();
    let app = session.app();
    let focused = app.panes.focused_active().unwrap();
    let note = app.tabs.iter().find(|tab| tab.id() == focused).unwrap();
    assert!(
        note.document.path.ends_with("Home.md"),
        "{:?}",
        note.document.path
    );
    assert_eq!(app.panes.active_of(0), Some(board_id));
    assert_eq!(
        app.tabs
            .iter()
            .filter(|tab| tab.document.path.ends_with("Home.md"))
            .count(),
        1,
        "uma aba só para a nota"
    );
}

fn body_of_on(session: &Session, board_id: u64, id: &str) -> egui::Pos2 {
    let rect = state_of_board(session, board_id)
        .node_screen_rect(id)
        .expect("cartão");
    egui::pos2(rect.center().x, rect.min.y + rect.height() * 0.6)
}

fn state_of_board(session: &Session, board_id: u64) -> &crate::canvas_ui::CanvasState {
    session
        .app()
        .canvases
        .state(board_id)
        .expect("o quadro foi desenhado")
}

#[test]
fn soltar_uma_nota_do_explorador_no_quadro_cria_o_cartao_onde_soltou() {
    let mut session = board_session();
    let before = parsed(&session).nodes.len();
    // A linha da Second no explorador (sem aba aberta com esse nome).
    let row = session
        .harness
        .get_all_by_label("Second")
        .next()
        .expect("linha da Second no explorador")
        .rect();
    let from = row.center();
    let to = egui::pos2(900.0, 560.0);
    drag(&mut session, from, to);
    session.settle();
    let board = parsed(&session);
    assert_eq!(board.nodes.len(), before + 1, "um cartão novo");
    let node = board.nodes.last().unwrap();
    assert_eq!(node.file.as_deref(), Some("Second.md"));
    assert!(node.is_note_card());
    // O centro do cartão caiu onde o ponteiro foi solto.
    let screen = state(&session).node_screen_rect(&node.id).unwrap();
    assert!(
        (screen.center() - to).length() < 6.0,
        "cartão em {:?}, soltou em {to:?}",
        screen.center()
    );
    // Soltar fora do explorador não moveu a nota de pasta.
    assert!(session.path("Second.md").exists());
    assert!(session.app().tabs[0].document.dirty());
}

#[test]
fn soltar_do_explorador_fora_do_quadro_nao_cria_cartao() {
    let mut session = board_session();
    let before = parsed(&session).nodes.len();
    let row = session
        .harness
        .get_all_by_label("Second")
        .next()
        .unwrap()
        .rect();
    // Solta em cima da barra de abas, fora da área do quadro.
    drag(&mut session, row.center(), egui::pos2(700.0, 30.0));
    session.settle();
    assert_eq!(parsed(&session).nodes.len(), before);
}

#[test]
fn a_biblioteca_lista_filtra_marca_e_poe_no_quadro_pelo_botao() {
    let mut session = board_session();
    assert!(state(&session).library().is_none());
    // Abre pela paleta de comandos.
    session.command("Canvas: Toggle note library");
    session.settle();
    let library = state(&session).library().expect("biblioteca aberta");
    let names: Vec<&str> = library
        .rows
        .iter()
        .map(|(path, _, _, _)| path.as_str())
        .collect();
    for note in ["Home.md", "Second.md", "Alpha.md"] {
        assert!(names.contains(&note), "{note} não está em {names:?}");
    }
    let marked: Vec<&str> = library
        .rows
        .iter()
        .filter(|(_, in_board, _, _)| *in_board)
        .map(|(path, _, _, _)| path.as_str())
        .collect();
    assert!(marked.contains(&"Home.md") && marked.contains(&"Alpha.md"));
    assert!(!marked.contains(&"Second.md"));

    // Busca: só a Second, sem maiúsculas.
    session.type_text("SECO");
    session.settle();
    let library = state(&session).library().unwrap();
    assert_eq!(library.rows.len(), 1, "{:?}", library.rows);
    assert_eq!(library.rows[0].0, "Second.md");
    let plus = library.rows[0].3;

    // O botão + põe a nota no quadro, e a biblioteca passa a marcá-la.
    let before = parsed(&session).nodes.len();
    click(&mut session, plus.center());
    session.settle();
    let board = parsed(&session);
    assert_eq!(board.nodes.len(), before + 1);
    assert!(board.has_file("Second.md"));
    let library = state(&session).library().unwrap();
    assert_eq!(library.rows[0].0, "Second.md");
    assert!(library.rows[0].1, "agora marcada como já no quadro");

    // Fecha pelo mesmo comando.
    session.command("Canvas: Toggle note library");
    session.settle();
    assert!(state(&session).library().is_none());
}

#[test]
fn arrastar_da_biblioteca_para_o_quadro_cria_o_cartao_onde_soltou() {
    let mut session = board_session();
    session.command("Canvas: Toggle note library");
    session.settle();
    let library = state(&session).library().unwrap();
    let row = library
        .rows
        .iter()
        .find(|(path, _, _, _)| path == "Second.md")
        .expect("linha da Second")
        .2;
    let to = egui::pos2(900.0, 600.0);
    let before = parsed(&session).nodes.len();
    drag(&mut session, row.left_center() + egui::vec2(30.0, 0.0), to);
    session.settle();
    let board = parsed(&session);
    assert_eq!(board.nodes.len(), before + 1);
    let node = board.nodes.last().unwrap();
    assert_eq!(node.file.as_deref(), Some("Second.md"));
    let screen = state(&session).node_screen_rect(&node.id).unwrap();
    assert!((screen.center() - to).length() < 6.0);
    // O quadro continua um JSON Canvas válido com os campos de antes.
    session.key(CTRL, egui::Key::S);
    let saved = on_disk(&session, "Quadro.canvas");
    assert_eq!(saved["metadata"]["version"], "1.0-1.0");
    assert_eq!(saved["nodes"][1]["extra"]["a"], 1);
}

#[test]
fn soltar_na_propria_biblioteca_nao_cria_cartao() {
    let mut session = board_session();
    session.command("Canvas: Toggle note library");
    session.settle();
    let library = state(&session).library().unwrap();
    let row = library
        .rows
        .iter()
        .find(|(path, _, _, _)| path == "Second.md")
        .unwrap()
        .2;
    let before = parsed(&session).nodes.len();
    let inside = row.center() + egui::vec2(0.0, 60.0);
    drag(
        &mut session,
        row.left_center() + egui::vec2(30.0, 0.0),
        inside,
    );
    session.settle();
    assert_eq!(parsed(&session).nodes.len(), before);
}

#[test]
fn a_mesma_nota_em_dois_quadros_continua_um_arquivo_so() {
    let mut session = board_session();
    let second = r#"{"nodes":[{"id":"h","type":"file","x":0,"y":0,"width":300,"height":200,"file":"Home.md"}],"edges":[]}"#;
    std::fs::write(session.path("Outro.canvas"), second).unwrap();
    session.settle();
    session
        .app_mut()
        .open_note_at(Path::new("Outro.canvas"), true);
    session.settle();
    // Edita a Home pelo segundo quadro: um cartão aberto só, no mesmo arquivo.
    let target = body_of(&session, "h");
    double_click(&mut session, target);
    session.type_text(" pelo outro quadro");
    assert_eq!(session.app().card_tabs.len(), 1);
    session.key(CTRL, egui::Key::S);
    assert!(disk(&session, "Home.md").ends_with(" pelo outro quadro"));
    assert_eq!(
        std::fs::read_dir(session.path(""))
            .unwrap()
            .filter(|entry| entry
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("Home"))
            .count(),
        1
    );
}

#[test]
fn clicar_fora_termina_a_edicao_do_cartao() {
    let mut session = board_session();
    double_click_body(&mut session, "alpha");
    assert_eq!(state(&session).editing_note(), Some("alpha"));
    // Um clique no fundo do quadro, fora de qualquer cartão.
    click(&mut session, egui::pos2(1000.0, 700.0));
    session.settle();
    assert_eq!(state(&session).editing_note(), None);
}
