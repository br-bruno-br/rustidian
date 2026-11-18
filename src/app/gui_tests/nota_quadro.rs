//! H2b: trocar entre nota e quadro sem perder nada. O atalho Ctrl+Shift+B cria (e abre) o quadro
//! par de uma nota, e num quadro alterna entre o desenho e a Lista; Ctrl+Alt+B volta à nota.
//! "Transformar em nota" troca um cartão de texto por um cartão vivo de uma nota nova, e renomear
//! uma nota pelo app leva os cartões dos quadros junto.

use super::canvas::{move_to, parsed, rect_of, state, tab_id};
use super::*;
use rustidian::canvas::{Canvas, NodeKind};

const NONE: egui::Modifiers = egui::Modifiers::NONE;

fn toggle(session: &mut Session) {
    session.key(CTRL | egui::Modifiers::SHIFT, egui::Key::B);
}

fn to_note(session: &mut Session) {
    session.key(CTRL | egui::Modifiers::ALT, egui::Key::B);
}

fn active_path(session: &Session) -> PathBuf {
    session.app().tabs[session.app().active]
        .document
        .path
        .clone()
}

fn wait_active(session: &mut Session, name: &'static str) {
    session.wait_for(move |app| {
        app.tabs
            .get(app.active)
            .is_some_and(|tab| tab.document.path == Path::new(name))
    });
    session.settle();
}

fn read(session: &Session, name: &str) -> String {
    std::fs::read_to_string(session.path(name)).expect("arquivo no disco")
}

fn secondary_click(session: &mut Session, pos: egui::Pos2) {
    move_to(session, pos);
    for pressed in [true, false] {
        session.harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: NONE,
        });
        session.harness.step();
    }
    session.settle();
}

#[test]
fn o_atalho_na_nota_cria_o_quadro_par_sem_copiar_texto_nem_tocar_na_nota() {
    let mut session = Session::open();
    session.open_note("Home");
    let before = std::fs::read(session.path("Home.md")).unwrap();
    assert!(!session.path("Home.canvas").exists());

    toggle(&mut session);
    wait_active(&mut session, "Home.canvas");

    let text = read(&session, "Home.canvas");
    let board = Canvas::parse(&text).expect("quadro válido");
    let files: Vec<String> = board
        .nodes
        .iter()
        .map(|node| node.file.clone().expect("só cartões de arquivo"))
        .collect();
    // No centro a própria nota; em volta, uma por link, na ordem em que aparecem na nota.
    assert_eq!(files, ["Home.md", "Second.md", "Alpha.md"]);
    assert_eq!(board.edges.len(), 2);
    assert!(
        board
            .nodes
            .iter()
            .all(|node| node.node_kind() == NodeKind::File)
    );
    // Nenhum texto da nota foi copiado para o quadro, e a nota ficou como estava.
    assert!(!text.contains("Veja") && !text.contains("zebra"), "{text}");
    assert_eq!(std::fs::read(session.path("Home.md")).unwrap(), before);
    // Abriu na mesma aba, desenhado (não como código-fonte), e o atalho não virou negrito.
    assert_eq!(session.app().tabs.len(), 1);
    assert!(crate::canvas_ui::is_canvas_tab(&session.app().tabs[0]));
    assert!(!session.app().tabs[0].document.dirty());
    // O quadro foi desenhado com o cartão vivo da nota.
    assert!(rect_of(&session, &board.nodes[0].id).is_positive());
}

#[test]
fn o_atalho_pela_paleta_faz_o_mesmo_e_a_nota_e_o_quadro_vao_e_voltam_sem_refazer_nada() {
    let mut session = Session::open();
    session.open_note("Alpha");
    session.command("alternar nota e quadro");
    wait_active(&mut session, "Alpha.canvas");
    let first = read(&session, "Alpha.canvas");

    // Volta à nota par e de novo ao quadro: o quadro que já existe é aberto, não refeito.
    to_note(&mut session);
    wait_active(&mut session, "Alpha.md");
    assert_eq!(session.active_text(), ALPHA);
    toggle(&mut session);
    wait_active(&mut session, "Alpha.canvas");
    assert_eq!(read(&session, "Alpha.canvas"), first);
    assert_eq!(session.app().tabs.len(), 1);
}

#[test]
fn um_quadro_par_que_ja_existe_e_usado_como_esta() {
    let mut session = Session::open_with(&[(
        "Second.canvas",
        br#"{"nodes":[{"id":"x","type":"text","x":0,"y":0,"width":200,"height":80,"text":"meu quadro"}],"edges":[]}"#,
    )]);
    let original = read(&session, "Second.canvas");
    session.open_note("Second");
    toggle(&mut session);
    wait_active(&mut session, "Second.canvas");
    assert_eq!(read(&session, "Second.canvas"), original);
    assert_eq!(parsed(&session).nodes.len(), 1);
}

#[test]
fn no_quadro_o_atalho_alterna_a_lista_na_mesma_aba_e_guarda_o_modo() {
    let mut session = Session::open();
    session.open_note("Home");
    toggle(&mut session);
    wait_active(&mut session, "Home.canvas");
    assert!(!session.app().tabs[0].board_list);

    toggle(&mut session);
    assert!(session.app().tabs[0].board_list, "o atalho abre a Lista");
    assert_eq!(session.app().tabs.len(), 1, "é a mesma aba");
    assert!(session.has_label("Lista do quadro"));
    // A Lista mostra as notas do quadro como itens, com o trecho do começo de cada uma.
    assert!(session.has_label("Second"));
    assert!(session.has_label("De volta para [[Home]]."));
    // Ler a Lista não muda o arquivo nem suja a aba.
    assert!(!session.app().tabs[0].document.dirty());

    toggle(&mut session);
    assert!(!session.app().tabs[0].board_list, "de novo volta ao quadro");
    assert!(!session.has_label("Lista do quadro"));
}

#[test]
fn clicar_num_item_da_lista_volta_ao_quadro_focando_o_cartao() {
    let mut session = Session::open();
    session.app_mut().show_left = false;
    session.open_note("Home");
    toggle(&mut session);
    wait_active(&mut session, "Home.canvas");
    let alpha = parsed(&session)
        .nodes
        .iter()
        .find(|node| node.file.as_deref() == Some("Alpha.md"))
        .map(|node| node.id.clone())
        .expect("cartão da Alpha");
    toggle(&mut session);
    assert!(session.app().tabs[0].board_list);

    session.harness.get_by_label("Alpha").click();
    session.settle();
    session.settle();
    assert!(!session.app().tabs[0].board_list, "voltou ao quadro");
    assert_eq!(state(&session).selected(), std::slice::from_ref(&alpha));
    let rect = rect_of(&session, &alpha);
    let area = state(&session).area_for_tests();
    let off = rect.center() - area.center();
    assert!(
        off.x.abs() < 5.0 && off.y.abs() < 50.0,
        "o cartão {rect:?} deve estar no meio da área {area:?}"
    );
}

const IDEIAS: &str = r#"{
	"nodes":[
		{"id":"t","type":"text","x":100,"y":50,"width":260,"height":120,"color":"3","text":"Ideia solta\nsegunda linha"},
		{"id":"o","type":"text","x":520,"y":50,"width":200,"height":80,"text":"outro"}
	],
	"edges":[{"id":"e","fromNode":"t","toNode":"o","label":"leva a"}]
}"#;

#[test]
fn transformar_em_nota_cria_a_nota_troca_o_cartao_no_lugar_e_o_desfazer_devolve_o_texto() {
    let mut session = Session::open_with(&[("Ideias.canvas", IDEIAS.as_bytes())]);
    session.app_mut().show_left = false;
    session.app_mut().open_note(Path::new("Ideias.canvas"));
    session.settle();
    let center = rect_of(&session, "t").center();
    secondary_click(&mut session, center);
    session.harness.get_by_label("Transformar em nota").click();
    session.settle();
    session.settle();

    // A nota nasceu com o texto do cartão, byte a byte, na pasta do quadro.
    assert_eq!(
        read(&session, "Ideia solta.md"),
        "Ideia solta\nsegunda linha"
    );
    // O cartão virou um cartão vivo da nota: mesmo id, posição, tamanho, cor e ligação.
    let board = parsed(&session);
    let node = board.node("t").expect("o cartão continua");
    assert_eq!(node.node_kind(), NodeKind::File);
    assert_eq!(node.file.as_deref(), Some("Ideia solta.md"));
    assert_eq!(node.text, None);
    assert_eq!(
        (node.x, node.y, node.width, node.height),
        (100.0, 50.0, 260.0, 120.0)
    );
    assert_eq!(node.color.as_deref(), Some("3"));
    assert_eq!(board.edges[0].from_node, "t");
    assert!(session.app().tabs[0].document.dirty());

    // Desfazer (Ctrl+Z com o ponteiro no quadro) devolve o cartão de texto; a nota fica.
    let over = state(&session).area_for_tests().center();
    move_to(&mut session, over);
    session.key(CTRL, egui::Key::Z);
    let board = parsed(&session);
    let node = board.node("t").unwrap();
    assert_eq!(node.node_kind(), NodeKind::Text);
    assert_eq!(node.text.as_deref(), Some("Ideia solta\nsegunda linha"));
    assert_eq!(
        read(&session, "Ideia solta.md"),
        "Ideia solta\nsegunda linha"
    );
    let _ = tab_id(&session);
}

#[test]
fn renomear_a_nota_pelo_app_leva_o_campo_file_dos_quadros_junto() {
    const PLANO: &str = r#"{"nodes":[
		{"id":"a","type":"file","x":0,"y":0,"width":300,"height":200,"file":"Alpha.md"},
		{"id":"s","type":"file","x":400,"y":0,"width":300,"height":200,"file":"Second.md"},
		{"id":"h","type":"file","x":0,"y":300,"width":300,"height":200,"file":"Home.md"}
	],"edges":[]}"#;
    let mut session = Session::open_with(&[("Plano.canvas", PLANO.as_bytes())]);
    let rename = |session: &mut Session, from: &str, to: &str| {
        session
            .app_mut()
            .ask_file(Dialog::RenameNote, PathBuf::from(from));
        session.app_mut().dialog_input = to.to_owned();
        session.key(NONE, egui::Key::Enter);
        session.settle();
    };

    // Quadro fechado: o arquivo do quadro no disco é reescrito.
    rename(&mut session, "Alpha.md", "Zeta.md");
    assert!(session.path("Zeta.md").exists());
    let board = Canvas::parse(&read(&session, "Plano.canvas")).unwrap();
    assert_eq!(board.node("a").unwrap().file.as_deref(), Some("Zeta.md"));
    assert_eq!(board.node("s").unwrap().file.as_deref(), Some("Second.md"));

    // Quadro aberto numa aba: a aba acompanha, pelo caminho normal (editada, salva depois).
    session.app_mut().open_note(Path::new("Plano.canvas"));
    session.settle();
    rename(&mut session, "Second.md", "Segunda.md");
    let board = parsed(&session);
    assert_eq!(board.node("s").unwrap().file.as_deref(), Some("Segunda.md"));
    assert_eq!(board.node("a").unwrap().file.as_deref(), Some("Zeta.md"));
    session.key(CTRL, egui::Key::S);
    let saved = Canvas::parse(&read(&session, "Plano.canvas")).unwrap();
    assert_eq!(saved.node("s").unwrap().file.as_deref(), Some("Segunda.md"));
    assert_eq!(saved.node("h").unwrap().file.as_deref(), Some("Home.md"));
}

#[test]
fn exportar_a_lista_gera_uma_nota_nova_que_nunca_sobrescreve() {
    let mut session = Session::open();
    session.app_mut().show_left = false;
    session.open_note("Home");
    toggle(&mut session);
    wait_active(&mut session, "Home.canvas");
    let board_text = read(&session, "Home.canvas");
    toggle(&mut session);
    session
        .harness
        .get_by_label("Exportar como nota (cópia)")
        .click();
    session.settle();
    session.settle();

    let copy = read(&session, "Home (lista).md");
    assert!(copy.starts_with("# Home\n"), "{copy}");
    assert!(copy.contains("Cópia da visão em lista"), "{copy}");
    assert!(copy.contains("- [[Second]]"), "{copy}");
    assert!(copy.contains("- [[Alpha]]"), "{copy}");
    // O quadro e as notas não mudaram.
    assert_eq!(read(&session, "Home.canvas"), board_text);
    assert_eq!(read(&session, "Home.md"), HOME);
    // A cópia abriu numa aba nova; exportar de novo não toca na primeira.
    assert_eq!(active_path(&session), Path::new("Home (lista).md"));
    session.app_mut().open_note(Path::new("Home.canvas"));
    let active = session.app().active;
    session.app_mut().tabs[active].board_list = true;
    session.settle();
    session
        .harness
        .get_by_label("Exportar como nota (cópia)")
        .click();
    session.settle();
    session.settle();
    assert_eq!(read(&session, "Home (lista).md"), copy);
    assert!(session.path("Home (lista) 1.md").exists());
}
