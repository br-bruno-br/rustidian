//! R14: quadros `.canvas` na GUI sem janela. O quadro é aberto numa aba, arrastado e editado com
//! eventos de ponteiro e teclado do egui, e o que importa é o arquivo no disco: JSON Canvas
//! válido, com os campos que o app não conhece preservados (o Obsidian abre o mesmo arquivo).

use super::*;
use rustidian::canvas::{self, Canvas};
use rustidian::tab::View;
use serde_json::Value;

/// Um quadro escrito à mão pela especificação 1.0, com campos que o app não conhece.
const PLANO: &str = r##"{
	"nodes":[
		{"id":"g1","type":"group","x":-40,"y":-60,"width":700,"height":340,"label":"Fase 1","color":"4","origem":"plugin"},
		{"id":"n1","type":"text","x":0,"y":0,"width":260,"height":80,"text":"# Ideia\nUma **boa** ideia","color":"1","extra":{"a":[1,{"b":null}]}},
		{"id":"n2","type":"file","x":380,"y":0,"width":240,"height":160,"file":"Home.md"},
		{"id":"n3","type":"link","x":0,"y":400,"width":300,"height":120,"url":"https://jsoncanvas.org"}
	],
	"edges":[
		{"id":"e1","fromNode":"n1","fromSide":"right","toNode":"n2","toSide":"left","label":"liga","peso":3}
	],
	"metadata":{"version":"1.0-1.0"}
}"##;

fn session_with_plan() -> Session {
    let mut session = Session::open_with(&[("Plano.canvas", PLANO.as_bytes())]);
    session
        .app_mut()
        .open_note(std::path::Path::new("Plano.canvas"));
    session.settle();
    session
}

pub(super) fn tab_id(session: &Session) -> u64 {
    session.app().tabs[session.app().active].id()
}

pub(super) fn state(session: &Session) -> &crate::canvas_ui::CanvasState {
    session
        .app()
        .canvases
        .state(tab_id(session))
        .expect("o quadro foi desenhado")
}

pub(super) fn rect_of(session: &Session, id: &str) -> egui::Rect {
    state(session)
        .node_screen_rect(id)
        .unwrap_or_else(|| panic!("nó {id} não está no quadro"))
}

pub(super) fn parsed(session: &Session) -> Canvas {
    Canvas::parse(&session.active_text()).expect("o texto da aba é um quadro válido")
}

pub(super) fn on_disk(session: &Session, name: &str) -> Value {
    serde_json::from_slice(&std::fs::read(session.path(name)).expect("arquivo gravado"))
        .expect("JSON no disco")
}

pub(super) fn move_to(session: &mut Session, pos: egui::Pos2) {
    session.harness.event(egui::Event::PointerMoved(pos));
    session.harness.step();
}

pub(super) fn button(
    session: &mut Session,
    pos: egui::Pos2,
    pressed: bool,
    modifiers: egui::Modifiers,
) {
    session.harness.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers,
    });
    session.harness.step();
}

/// Um clique no ponto (movimento, apertar e soltar em quadros seguidos).
pub(super) fn click(session: &mut Session, pos: egui::Pos2) {
    move_to(session, pos);
    button(session, pos, true, egui::Modifiers::NONE);
    button(session, pos, false, egui::Modifiers::NONE);
    session.settle();
}

/// Duas vezes seguidas no mesmo quadro: o egui conta o duplo clique.
pub(super) fn double_click(session: &mut Session, pos: egui::Pos2) {
    move_to(session, pos);
    // O harness dá um quadro (0,25 s) a cada evento enfileirado, e o duplo clique do egui vale
    // até 0,3 s: os quatro eventos vão direto para o mesmo quadro.
    for pressed in [true, false, true, false] {
        session
            .harness
            .input_mut()
            .events
            .push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
    }
    session.harness.step();
    session.settle();
}

/// Arrasta de `from` até `to` em passos pequenos.
pub(super) fn drag(session: &mut Session, from: egui::Pos2, to: egui::Pos2) {
    move_to(session, from);
    button(session, from, true, egui::Modifiers::NONE);
    for step in 1..=6 {
        let pos = from + (to - from) * (step as f32 / 6.0);
        move_to(session, pos);
    }
    button(session, to, false, egui::Modifiers::NONE);
    session.settle();
}

#[test]
fn quadro_abre_desenhado_com_os_nos_e_nao_mexe_no_arquivo() {
    let session = session_with_plan();
    let tab = &session.app().tabs[session.app().active];
    assert_eq!(
        tab.view,
        View::Read,
        "um quadro abre desenhado, não como JSON"
    );
    assert!(!tab.document.dirty(), "só abrir não suja o documento");
    let canvas = state(&session).canvas();
    assert_eq!(canvas.nodes.len(), 4);
    assert_eq!(canvas.edges.len(), 1);
    assert!(state(&session).error().is_none());
    // O quadro inteiro coube na tela depois de abrir ("Ajustar" automático).
    let area = state(&session).node_screen_rect("n3").unwrap();
    assert!(area.is_positive() && area.center().x > 0.0 && area.center().y > 0.0);
    // O arquivo continua byte a byte o que era.
    assert_eq!(
        std::fs::read_to_string(session.path("Plano.canvas")).unwrap(),
        PLANO
    );
}

#[test]
fn arrastar_um_cartao_move_salva_e_preserva_os_campos_desconhecidos() {
    let mut session = session_with_plan();
    let zoom = state(&session).zoom();
    let before = parsed(&session).node("n1").unwrap().rect();
    let rect = rect_of(&session, "n1");
    let from = rect.center();
    drag(&mut session, from, from + egui::vec2(100.0, 50.0));
    let after = parsed(&session).node("n1").unwrap().rect();
    let moved_x = after.x - before.x;
    let moved_y = after.y - before.y;
    assert!(
        (moved_x - (100.0 / f64::from(zoom))).abs() <= 1.5
            && (moved_y - (50.0 / f64::from(zoom))).abs() <= 1.5,
        "moveu ({moved_x}, {moved_y}) com zoom {zoom}"
    );
    assert_eq!(
        (after.w, after.h),
        (before.w, before.h),
        "mover não muda o tamanho"
    );
    assert!(session.app().tabs[session.app().active].document.dirty());

    session.key(CTRL, egui::Key::S);
    let saved = on_disk(&session, "Plano.canvas");
    // Só o x e o y de n1 mudam; todo o resto do arquivo é o mesmo JSON de antes.
    let mut expected: Value = serde_json::from_str(PLANO).unwrap();
    expected["nodes"][1]["x"] = saved["nodes"][1]["x"].clone();
    expected["nodes"][1]["y"] = saved["nodes"][1]["y"].clone();
    assert_eq!(saved, expected);
    assert_eq!(saved["nodes"][0]["origem"], "plugin");
    assert_eq!(saved["edges"][0]["peso"], 3);
    assert_eq!(saved["metadata"]["version"], "1.0-1.0");
    assert_eq!(saved["nodes"][1]["extra"]["a"][1]["b"], Value::Null);
}

#[test]
fn mover_o_grupo_leva_os_cartoes_de_dentro() {
    let mut session = session_with_plan();
    let before = parsed(&session);
    // A faixa do rótulo do grupo (o miolo vazio do grupo não responde ao clique).
    let group = rect_of(&session, "g1");
    let handle = egui::pos2(group.min.x + group.width() * 0.5, group.min.y + 6.0);
    drag(&mut session, handle, handle + egui::vec2(40.0, 40.0));
    let after = parsed(&session);
    for id in ["g1", "n1", "n2"] {
        let (a, b) = (before.node(id).unwrap(), after.node(id).unwrap());
        assert!(
            b.x > a.x && b.y > a.y,
            "{id} devia ter andado junto com o grupo"
        );
    }
    let (a, b) = (before.node("n3").unwrap(), after.node("n3").unwrap());
    assert_eq!(
        (a.x, a.y),
        (b.x, b.y),
        "n3 está fora do grupo e fica parado"
    );
}

#[test]
fn redimensionar_pela_alca_do_canto() {
    let mut session = session_with_plan();
    let rect = rect_of(&session, "n1");
    click(&mut session, rect.center());
    assert_eq!(state(&session).selected(), ["n1"]);
    let before = parsed(&session).node("n1").unwrap().rect();
    let corner = rect_of(&session, "n1").right_bottom();
    drag(&mut session, corner, corner + egui::vec2(60.0, 40.0));
    let after = parsed(&session).node("n1").unwrap().rect();
    assert!(
        after.w > before.w + 10.0 && after.h > before.h + 5.0,
        "{before:?} → {after:?}"
    );
    assert_eq!(
        (after.x, after.y),
        (before.x, before.y),
        "a ponta oposta fica parada"
    );
}

#[test]
fn duplo_clique_no_fundo_cria_cartao_de_texto_e_digitar_grava() {
    let mut session = session_with_plan();
    let before = parsed(&session).nodes.len();
    let free = state(&session).screen_of((450.0, 450.0));
    double_click(&mut session, free);
    assert!(
        state(&session).is_editing(),
        "o cartão novo entra em edição"
    );
    session.type_text("Cartão novo");
    // Clicar em outro lugar confirma o texto.
    let elsewhere = state(&session).screen_of((200.0, 330.0));
    click(&mut session, elsewhere);
    let canvas = parsed(&session);
    assert_eq!(canvas.nodes.len(), before + 1);
    let created = canvas
        .nodes
        .iter()
        .find(|node| node.text.as_deref() == Some("Cartão novo"))
        .expect("o cartão novo está no JSON");
    assert_eq!(created.kind, "text");
    assert_eq!(created.id.len(), 16);
    session.key(CTRL, egui::Key::S);
    let saved = on_disk(&session, "Plano.canvas");
    assert_eq!(saved["nodes"].as_array().unwrap().len(), before + 1);
    assert_eq!(saved["metadata"]["version"], "1.0-1.0");
}

#[test]
fn cartao_novo_que_fica_vazio_some_sem_sujar_o_arquivo() {
    let mut session = session_with_plan();
    let free = state(&session).screen_of((450.0, 450.0));
    double_click(&mut session, free);
    assert!(state(&session).is_editing());
    let elsewhere = state(&session).screen_of((200.0, 330.0));
    click(&mut session, elsewhere);
    assert_eq!(parsed(&session).nodes.len(), 4);
    assert!(!session.app().tabs[session.app().active].document.dirty());
}

#[test]
fn ligar_dois_cartoes_pelo_ponto_do_lado() {
    let mut session = session_with_plan();
    let first = rect_of(&session, "n1");
    click(&mut session, first.center());
    // O ponto de ligação do lado de baixo fica 16 px abaixo da borda.
    let dot = egui::pos2(first.center().x, first.max.y + 16.0);
    let target = rect_of(&session, "n3").center();
    drag(&mut session, dot, target);
    let canvas = parsed(&session);
    assert_eq!(canvas.edges.len(), 2);
    let edge = &canvas.edges[1];
    assert_eq!(
        (edge.from_node.as_str(), edge.to_node.as_str()),
        ("n1", "n3")
    );
    assert_eq!(edge.from_side.as_deref(), Some("bottom"));
    assert!(edge.to_side.is_some());
    assert!(
        edge.to_arrow() && !edge.from_arrow(),
        "seta no destino por padrão"
    );
}

#[test]
fn apagar_o_cartao_leva_as_arestas_e_desfazer_traz_de_volta() {
    let mut session = session_with_plan();
    let original = session.active_text();
    let target = rect_of(&session, "n2").center();
    click(&mut session, target);
    session.key(egui::Modifiers::NONE, egui::Key::Delete);
    let canvas = parsed(&session);
    assert!(canvas.node("n2").is_none());
    assert!(
        canvas.edges.is_empty(),
        "a aresta que tocava em n2 sai junto"
    );
    session.key(CTRL, egui::Key::S);
    assert_eq!(
        on_disk(&session, "Plano.canvas")["nodes"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    // Ctrl+Z devolve o texto de antes, byte a byte.
    session.key(CTRL, egui::Key::Z);
    assert_eq!(session.active_text(), original);
}

#[test]
fn clicar_no_titulo_do_cartao_de_nota_abre_a_nota() {
    let mut session = session_with_plan();
    let note = rect_of(&session, "n2");
    // O título fica na faixa de cima do cartão.
    click(&mut session, egui::pos2(note.center().x, note.min.y + 8.0));
    session.wait_for(|app| {
        app.tabs
            .get(app.active)
            .is_some_and(|tab| tab.document.path.ends_with("Home.md"))
    });
    assert_eq!(session.active_text(), HOME);
}

#[test]
fn roda_do_mouse_muda_o_zoom() {
    let mut session = session_with_plan();
    let before = state(&session).zoom();
    let pos = rect_of(&session, "n1").center();
    move_to(&mut session, pos);
    session.harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, 120.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Unknown,
        modifiers: egui::Modifiers::NONE,
    });
    session.settle();
    assert!(
        state(&session).zoom() > before,
        "zoom {before} → {}",
        state(&session).zoom()
    );
}

#[test]
fn novo_quadro_pela_paleta_cria_o_arquivo_vazio_e_abre_desenhado() {
    let mut session = Session::open();
    session.command("New canvas");
    session.wait_for(|app| {
        app.tabs
            .get(app.active)
            .is_some_and(|tab| tab.document.path.ends_with("Quadro.canvas"))
    });
    let tab = &session.app().tabs[session.app().active];
    assert_eq!(tab.view, View::Read);
    let on_disk = std::fs::read_to_string(session.path("Quadro.canvas")).unwrap();
    assert_eq!(on_disk, canvas::EMPTY);
    assert!(Canvas::parse(&on_disk).unwrap().nodes.is_empty());
    // O explorador mostra o arquivo novo.
    assert!(session.has_label("Quadro.canvas") || session.has_label("Quadro"));
    // Outro quadro não sobrescreve o primeiro.
    session.command("New canvas");
    session.wait_for(|app| {
        app.tabs
            .get(app.active)
            .is_some_and(|tab| tab.document.path.ends_with("Quadro 1.canvas"))
    });
}

#[test]
fn json_invalido_mostra_o_aviso_e_nunca_sobrescreve_o_arquivo() {
    let broken = b"{ isto nao e json";
    let mut session = Session::open_with(&[("Quebrado.canvas", broken)]);
    session
        .app_mut()
        .open_note(std::path::Path::new("Quebrado.canvas"));
    session.settle();
    assert!(session.has_label("Não foi possível abrir este quadro"));
    assert!(state(&session).error().is_some());
    // As teclas de edição não fazem nada num quadro inválido.
    session.key(CTRL, egui::Key::A);
    session.key(egui::Modifiers::NONE, egui::Key::Delete);
    assert_eq!(session.active_text(), "{ isto nao e json");
    assert_eq!(
        std::fs::read(session.path("Quebrado.canvas")).unwrap(),
        broken
    );
    // O botão abre o código-fonte.
    session
        .harness
        .get_by_label("Abrir como código-fonte")
        .click();
    session.settle();
    assert_eq!(session.app().tabs[session.app().active].view, View::Source);
}

#[test]
fn autosave_grava_o_quadro_sem_ctrl_s() {
    let mut session = session_with_plan();
    let target = rect_of(&session, "n3").center();
    click(&mut session, target);
    session.key(egui::Modifiers::NONE, egui::Key::Delete);
    for _ in 0..300 {
        let saved = std::fs::read_to_string(session.path("Plano.canvas")).unwrap();
        if !saved.contains("jsoncanvas.org") {
            assert!(Canvas::parse(&saved).is_ok());
            return;
        }
        session.harness.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("o autosave não gravou o quadro em 3 s");
}

#[test]
fn editar_o_codigo_fonte_e_voltar_mostra_o_texto_novo() {
    let mut session = session_with_plan();
    session.app_mut().tabs[0].set_view(View::Source);
    session.settle();
    session.app_mut().tabs[0].document.text = canvas::EMPTY.to_owned();
    session.app_mut().tabs[0].edited();
    session.app_mut().tabs[0].set_view(View::Read);
    session.settle();
    assert!(state(&session).canvas().nodes.is_empty());
}

#[test]
fn duplo_clique_na_aresta_edita_o_rotulo() {
    let mut session = session_with_plan();
    let middle = {
        let canvas = state(&session).canvas();
        let curve = canvas.curve(canvas.edge("e1").unwrap()).unwrap();
        state(&session).screen_of(curve.at(0.5))
    };
    double_click(&mut session, middle);
    assert!(
        state(&session).is_editing(),
        "o rótulo da aresta entra em edição"
    );
    // O campo já vem com o rótulo de antes; Ctrl+A e digitar troca tudo.
    session.key(CTRL, egui::Key::A);
    session.type_text("depende de");
    let elsewhere = state(&session).screen_of((200.0, 330.0));
    click(&mut session, elsewhere);
    let canvas = parsed(&session);
    assert_eq!(
        canvas.edge("e1").unwrap().label.as_deref(),
        Some("depende de")
    );
    assert_eq!(
        canvas.edge("e1").unwrap().extra["peso"],
        3,
        "campos da aresta seguem"
    );
}

#[test]
fn o_botao_ligar_une_os_dois_cartoes_escolhidos_com_shift() {
    let mut session = session_with_plan();
    let first = rect_of(&session, "n1").center();
    click(&mut session, first);
    let second = rect_of(&session, "n3").center();
    move_to(&mut session, second);
    session
        .harness
        .event(egui::Event::ModifiersChanged(egui::Modifiers::SHIFT));
    session.harness.step();
    button(&mut session, second, true, egui::Modifiers::SHIFT);
    button(&mut session, second, false, egui::Modifiers::SHIFT);
    session
        .harness
        .event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    session.settle();
    assert_eq!(state(&session).selected().len(), 2);
    session.harness.get_by_label("Ligar").click();
    session.settle();
    let canvas = parsed(&session);
    assert_eq!(canvas.edges.len(), 2);
    assert_eq!(canvas.edges[1].from_node, "n1");
    assert_eq!(canvas.edges[1].to_node, "n3");
}

#[test]
#[ignore = "gera um PNG do quadro; precisa de RUSTIDIAN_PRINTS_DIR"]
fn print_do_quadro() {
    let dir = PathBuf::from(
        std::env::var_os("RUSTIDIAN_PRINTS_DIR").expect("defina RUSTIDIAN_PRINTS_DIR"),
    );
    std::fs::create_dir_all(&dir).expect("pasta dos prints");
    let mut session = session_with_plan();
    let selected = rect_of(&session, "n1").center();
    click(&mut session, selected);
    session.settle();
    let image = session.harness.render().expect("renderização com wgpu");
    image.save(dir.join("quadro.png")).expect("gravar o PNG");
}
