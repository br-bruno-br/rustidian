//! H2b: trocar entre nota e quadro sem perder nada. O atalho Ctrl+Shift+B cicla Nota, Quadro e
//! Lista (cria o quadro par de uma nota se faltar); a paleta vai direto a um modo.
//! "Transformar em nota" troca um cartão de texto por um cartão vivo de uma nota nova, e renomear
//! uma nota pelo app leva os cartões dos quadros junto.

use super::canvas::{move_to, parsed, rect_of, state, tab_id};
use super::*;
use egui_kittest::kittest::NodeT;
use rustidian::canvas::{Canvas, NodeKind};

const NONE: egui::Modifiers = egui::Modifiers::NONE;

fn toggle(session: &mut Session) {
    session.key(CTRL | egui::Modifiers::SHIFT, egui::Key::B);
}

fn to_note(session: &mut Session) {
    session.command("ir para a nota");
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

    // Volta à nota par pela paleta e de novo ao quadro: o quadro que já existe é aberto, não refeito.
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
    assert!(session.has_label("De volta para Home."));
    // Ler a Lista não muda o arquivo nem suja a aba.
    assert!(!session.app().tabs[0].document.dirty());

    // Da Lista, o mesmo atalho volta à nota par (Home.md existe) e a Lista some.
    toggle(&mut session);
    wait_active(&mut session, "Home.md");
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

    // O título abre a nota; o trecho do item mostra o cartão no quadro.
    session
        .harness
        .get_by_label_contains("palavra rara")
        .click();
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
    // Dois renomeares seguidos: o segundo ainda pode ver "Home.md mudou fora do aplicativo" (o
    // primeiro reescreveu o link dela e o índice do cofre se atualiza em outra thread), então
    // tenta de novo até o arquivo existir (máximo ~10 s).
    let rename = |session: &mut Session, from: &str, to: &str| {
        for _ in 0..100 {
            session
                .app_mut()
                .ask_file(Dialog::RenameNote, PathBuf::from(from));
            session.app_mut().dialog_input = to.to_owned();
            session.key(NONE, egui::Key::Enter);
            session.settle();
            if session.path(to).exists() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("o renomear não aconteceu: {:?}", session.app().error);
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

/// O rótulo do modo no rodapé (a etiqueta "board-mode" do `ui_probe` não é lida aqui: o texto na
/// tela é que importa).
fn footer_mode(session: &Session) -> Option<BoardMode> {
    let found: Vec<BoardMode> = [BoardMode::Note, BoardMode::Board, BoardMode::List]
        .into_iter()
        .filter(|mode| {
            session
                .harness
                .query_all_by_label(mode.label())
                .any(|node| node.rect().min.y > 800.0 - 28.0)
        })
        .collect();
    assert!(
        found.len() <= 1,
        "o rodapé mostra mais de um modo: {found:?}"
    );
    found.first().copied()
}

#[test]
fn o_rodape_mostra_o_modo_atual_e_a_ajuda_da_lista_some_ao_sair_dela() {
    let mut session = Session::open();
    session.app_mut().show_left = false;
    session.open_note("Home");
    session.settle();
    assert_eq!(footer_mode(&session), Some(BoardMode::Note));

    toggle(&mut session);
    wait_active(&mut session, "Home.canvas");
    assert_eq!(footer_mode(&session), Some(BoardMode::Board));

    toggle(&mut session);
    assert!(session.app().tabs[0].board_list);
    assert_eq!(footer_mode(&session), Some(BoardMode::List));
    assert!(!session.has_label("Ctrl+Shift+B volta ao quadro"));

    // Da Lista de volta à nota (o caso do print 05): nenhuma mensagem da Lista sobrevive.
    toggle(&mut session);
    wait_active(&mut session, "Home.md");
    assert_eq!(footer_mode(&session), Some(BoardMode::Note));
    assert!(
        !session.has_label("Lista do quadro"),
        "o rodapé não pode falar da Lista numa nota"
    );
    session.open_note("Second");
    session.settle();
    assert_eq!(footer_mode(&session), Some(BoardMode::Note));
    assert!(!session.has_label("Lista do quadro"));
}

#[test]
fn a_mensagem_de_status_sai_quando_a_aba_ou_o_modo_muda() {
    let mut session = Session::open();
    session.app_mut().show_left = false;
    session.open_note("Home");
    // Uma mensagem velha, escrita em outro momento (num quadro de antes, digamos).
    session.app_mut().status = "Mensagem velha".into();
    session.settle();
    assert!(
        session.has_label("Mensagem velha"),
        "enquanto nada muda, ela fica"
    );

    // Trocar de nota limpa.
    session.open_note("Second");
    session.settle();
    assert!(!session.has_label("Mensagem velha"));

    // A mensagem do próprio gesto (quadro criado a partir da nota) fica...
    toggle(&mut session);
    wait_active(&mut session, "Second.canvas");
    assert!(
        session.app().status.contains("criado a partir da nota"),
        "{}",
        session.app().status
    );
    session.settle();
    assert!(session.has_label("criado a partir da nota"));
    // ...até o modo mudar de novo.
    toggle(&mut session);
    assert!(session.app().tabs[0].board_list);
    assert!(session.app().status.is_empty(), "{}", session.app().status);
}

fn mode(session: &Session) -> Option<BoardMode> {
    session.app().board_mode()
}

#[test]
fn o_atalho_cicla_nota_quadro_lista_nota_na_mesma_aba() {
    let mut session = Session::open();
    session.open_note("Home");
    assert_eq!(mode(&session), Some(BoardMode::Note));

    toggle(&mut session);
    wait_active(&mut session, "Home.canvas");
    assert_eq!(mode(&session), Some(BoardMode::Board));
    let board = read(&session, "Home.canvas");

    toggle(&mut session);
    assert_eq!(mode(&session), Some(BoardMode::List));

    toggle(&mut session);
    wait_active(&mut session, "Home.md");
    assert_eq!(mode(&session), Some(BoardMode::Note));
    assert_eq!(session.active_text(), HOME);

    // Uma volta inteira, sempre na mesma aba, e o quadro não é refeito.
    toggle(&mut session);
    wait_active(&mut session, "Home.canvas");
    assert_eq!(read(&session, "Home.canvas"), board);
    assert_eq!(session.app().tabs.len(), 1);
}

#[test]
fn sem_nota_par_a_lista_volta_ao_quadro() {
    let mut session = Session::open_with(&[("Ideias.canvas", IDEIAS.as_bytes())]);
    session.app_mut().open_note(Path::new("Ideias.canvas"));
    session.settle();
    assert_eq!(mode(&session), Some(BoardMode::Board));
    toggle(&mut session);
    assert_eq!(mode(&session), Some(BoardMode::List));
    toggle(&mut session);
    assert_eq!(
        mode(&session),
        Some(BoardMode::Board),
        "sem Ideias.md, volta ao quadro"
    );
    assert_eq!(active_path(&session), Path::new("Ideias.canvas"));
    assert!(
        !session.path("Ideias.md").exists(),
        "nenhuma nota nasce por ora"
    );
}

#[test]
fn ctrl_alt_b_nao_e_mais_atalho() {
    let mut session = Session::open();
    session.open_note("Home");
    toggle(&mut session);
    wait_active(&mut session, "Home.canvas");
    to_note_old_shortcut(&mut session);
    session.settle();
    assert_eq!(
        active_path(&session),
        Path::new("Home.canvas"),
        "Ctrl+Alt+B não faz nada"
    );
    assert_eq!(mode(&session), Some(BoardMode::Board));
}

fn to_note_old_shortcut(session: &mut Session) {
    session.key(CTRL | egui::Modifiers::ALT, egui::Key::B);
}

#[test]
fn a_paleta_vai_direto_a_nota_ao_quadro_e_a_lista() {
    let mut session = Session::open();
    session.open_note("Alpha");

    // Da nota direto à Lista: cria o quadro e já abre na Lista.
    session.command("ir para a lista");
    wait_active(&mut session, "Alpha.canvas");
    assert_eq!(mode(&session), Some(BoardMode::List));
    assert!(session.path("Alpha.canvas").exists());

    session.command("ir para o quadro");
    assert_eq!(mode(&session), Some(BoardMode::Board));
    session.command("ir para a lista");
    assert_eq!(mode(&session), Some(BoardMode::List));
    session.command("ir para a nota");
    wait_active(&mut session, "Alpha.md");
    assert_eq!(mode(&session), Some(BoardMode::Note));
    session.command("ir para o quadro");
    wait_active(&mut session, "Alpha.canvas");
    assert_eq!(mode(&session), Some(BoardMode::Board));
    assert_eq!(session.app().tabs.len(), 1);
}

#[test]
fn a_paleta_nao_oferece_ir_para_o_modo_em_que_ja_esta() {
    let mut session = Session::open();
    session.open_note("Alpha");
    let names = |session: &mut Session| -> Vec<String> {
        session.key(CTRL, egui::Key::P);
        session.type_text("ir para");
        let found = ["Ir para a nota", "Ir para o quadro", "Ir para a lista"]
            .into_iter()
            .filter(|name| session.has_label(name))
            .map(String::from)
            .collect();
        session.key(NONE, egui::Key::Escape);
        found
    };
    assert_eq!(names(&mut session), ["Ir para o quadro", "Ir para a lista"]);
    toggle(&mut session);
    wait_active(&mut session, "Alpha.canvas");
    assert_eq!(names(&mut session), ["Ir para a nota", "Ir para a lista"]);
}

/// O segmento "Nota", "Quadro" ou "Lista" do seletor do cabeçalho (a barra de ferramentas do
/// quadro e o rodapé também têm textos iguais, por isso se filtra pela faixa do cabeçalho).
pub(super) fn segment<'a>(session: &'a Session, label: &'a str) -> egui_kittest::Node<'a> {
    session
        .harness
        .query_all_by_label(label)
        .find(|node| {
            let rect = node.rect();
            rect.min.y > 60.0 && rect.max.y < 130.0
        })
        .unwrap_or_else(|| panic!("o seletor não tem o segmento {label}"))
}

pub(super) fn click_segment(session: &mut Session, label: &str) {
    segment(session, label).click();
    session.settle();
    session.settle();
}

#[test]
fn o_seletor_do_cabecalho_marca_o_modo_e_tem_tamanho_de_toque() {
    let mut session = Session::open();
    session.open_note("Home");
    session.settle();
    for label in ["Nota", "Quadro", "Lista"] {
        let rect = segment(&session, label).rect();
        assert!(
            rect.height() >= 32.0 && rect.width() >= 48.0,
            "{label}: {rect:?} é pequeno para o toque"
        );
    }
    let selected = |session: &Session| -> Vec<&'static str> {
        ["Nota", "Quadro", "Lista"]
            .into_iter()
            .filter(|label| {
                segment(session, label).accesskit_node().toggled()
                    == Some(egui::accesskit::Toggled::True)
            })
            .collect()
    };
    assert_eq!(selected(&session), ["Nota"]);
    click_segment(&mut session, "Quadro");
    wait_active(&mut session, "Home.canvas");
    assert_eq!(selected(&session), ["Quadro"]);
    click_segment(&mut session, "Lista");
    assert_eq!(selected(&session), ["Lista"]);
    // O ícone de leitura/edição da nota continua lá, separado do seletor.
    click_segment(&mut session, "Nota");
    wait_active(&mut session, "Home.md");
    assert_eq!(selected(&session), ["Nota"]);
}

#[test]
fn o_seletor_leva_da_nota_ao_quadro_a_lista_e_de_volta_criando_o_quadro_se_faltar() {
    let mut session = Session::open();
    session.app_mut().show_left = false;
    session.open_note("Alpha");
    assert!(!session.path("Alpha.canvas").exists());

    // "Lista" direto da nota cria o quadro e abre a Lista.
    click_segment(&mut session, "Lista");
    wait_active(&mut session, "Alpha.canvas");
    assert_eq!(mode(&session), Some(BoardMode::List));
    assert!(session.path("Alpha.canvas").exists());
    assert_eq!(session.app().tabs.len(), 1);

    click_segment(&mut session, "Quadro");
    assert_eq!(mode(&session), Some(BoardMode::Board));
    click_segment(&mut session, "Nota");
    wait_active(&mut session, "Alpha.md");
    assert_eq!(mode(&session), Some(BoardMode::Note));
    assert_eq!(session.active_text(), ALPHA);

    // Clicar no segmento do modo atual não faz nada.
    click_segment(&mut session, "Nota");
    assert_eq!(active_path(&session), Path::new("Alpha.md"));
    click_segment(&mut session, "Quadro");
    wait_active(&mut session, "Alpha.canvas");
    assert_eq!(mode(&session), Some(BoardMode::Board));
}

fn open_ideias() -> Session {
    let mut session = Session::open_with(&[("Ideias.canvas", IDEIAS.as_bytes())]);
    session.app_mut().show_left = false;
    session.app_mut().open_note(Path::new("Ideias.canvas"));
    session.settle();
    session
}

#[test]
fn nota_num_quadro_sem_nota_par_pergunta_dentro_do_app_e_cancelar_nao_cria_nada() {
    let mut session = open_ideias();
    click_segment(&mut session, "Nota");
    // Nada mudou até responder; a pergunta é do app (Modal), com Criar e Cancelar de toque.
    assert_eq!(active_path(&session), Path::new("Ideias.canvas"));
    assert!(!session.path("Ideias.md").exists());
    assert!(session.has_label("Criar a nota Ideias?"));
    for label in ["Criar", "Cancelar"] {
        let rect = session.harness.get_by_label(label).rect();
        assert!(rect.height() >= 32.0, "{label}: {rect:?}");
    }

    session.harness.get_by_label("Cancelar").click();
    session.settle();
    session.settle();
    assert!(!session.has_label("Criar a nota Ideias?"));
    assert!(!session.path("Ideias.md").exists());
    assert_eq!(active_path(&session), Path::new("Ideias.canvas"));
    assert_eq!(mode(&session), Some(BoardMode::Board));

    // Esc também cancela.
    click_segment(&mut session, "Nota");
    assert!(session.has_label("Criar a nota Ideias?"));
    session.key(NONE, egui::Key::Escape);
    assert!(!session.has_label("Criar a nota Ideias?"));
    assert!(!session.path("Ideias.md").exists());
}

#[test]
fn criar_a_nota_par_gera_x_md_com_titulo_abre_no_modo_nota_e_o_seletor_passa_a_funcionar() {
    let mut session = open_ideias();
    let board_before = read(&session, "Ideias.canvas");
    click_segment(&mut session, "Nota");
    session.harness.get_by_label("Criar").click();
    session.settle();
    session.settle();
    wait_active(&mut session, "Ideias.md");

    assert_eq!(
        read(&session, "Ideias.md"),
        "# Ideias

"
    );
    assert_eq!(
        read(&session, "Ideias.canvas"),
        board_before,
        "o quadro não muda"
    );
    assert_eq!(mode(&session), Some(BoardMode::Note));
    assert_eq!(session.app().tabs.len(), 1);
    // A mensagem nasceu neste modo e fica.
    assert!(
        session.has_label("Nota Ideias.md criada."),
        "{}",
        session.app().status
    );

    // Agora o par existe: Quadro e Nota vão e voltam sem pergunta.
    click_segment(&mut session, "Quadro");
    wait_active(&mut session, "Ideias.canvas");
    click_segment(&mut session, "Nota");
    wait_active(&mut session, "Ideias.md");
    assert!(!session.has_label("Criar a nota"));
}

#[test]
fn o_atalho_e_a_paleta_tambem_perguntam_antes_de_criar_a_nota_par() {
    let mut session = open_ideias();
    to_note(&mut session);
    assert!(session.has_label("Criar a nota Ideias?"));
    assert!(!session.path("Ideias.md").exists());
    // Enter confirma.
    session.key(NONE, egui::Key::Enter);
    wait_active(&mut session, "Ideias.md");
    assert!(session.path("Ideias.md").exists());
}

#[test]
fn o_cartao_de_texto_selecionado_tem_o_botao_transformar_em_nota_a_vista() {
    let mut session = open_ideias();
    // Sem seleção, sem botão.
    assert!(!session.has_label("Transformar em nota"));
    let card = rect_of(&session, "t");
    super::canvas::click(&mut session, card.center());
    assert_eq!(state(&session).selected(), ["t"]);
    let button = session.harness.get_by_label("Transformar em nota").rect();
    assert!(button.height() >= 28.0, "{button:?}");
    // Perto do cartão: logo abaixo (ou acima, se não coubesse).
    assert!(
        (button.min.y - card.max.y).abs() < 60.0 || (card.min.y - button.max.y).abs() < 60.0,
        "{button:?} longe do cartão {card:?}"
    );

    // Selecionar o outro cartão de texto leva o botão para ele.
    let other = rect_of(&session, "o").center();
    super::canvas::click(&mut session, other);
    assert_eq!(state(&session).selected(), ["o"]);
    let moved = session.harness.get_by_label("Transformar em nota").rect();
    assert!(moved.min.x > button.min.x, "{moved:?} não seguiu o cartão");

    // O botão faz o mesmo que o item do menu.
    session.harness.get_by_label("Transformar em nota").click();
    session.settle();
    session.settle();
    assert_eq!(read(&session, "outro.md"), "outro");
    let node = parsed(&session);
    let node = node.node("o").unwrap();
    assert_eq!(node.node_kind(), NodeKind::File);
    assert_eq!(node.file.as_deref(), Some("outro.md"));
    // Virou cartão de nota: o botão some.
    assert!(!session.has_label("Transformar em nota"));
}

#[test]
fn a_barra_de_ferramentas_do_quadro_nao_tem_mais_o_botao_lista() {
    let mut session = Session::open();
    session.app_mut().show_left = false;
    session.open_note("Home");
    toggle(&mut session);
    wait_active(&mut session, "Home.canvas");
    // A barra continua com as ferramentas de criar cartão...
    for label in ["Texto", "Link", "Grupo"] {
        assert!(session.has_label(label), "falta o botão {label}");
    }
    // ...e o único "Lista" fora do rodapé é o segmento do cabeçalho.
    let outside_header_and_footer = session
        .harness
        .query_all_by_label("Lista")
        .filter(|node| node.rect().min.y > 130.0 && node.rect().min.y < 800.0 - 28.0)
        .count();
    assert_eq!(outside_header_and_footer, 0);
    // O seletor segue levando à Lista.
    click_segment(&mut session, "Lista");
    assert_eq!(mode(&session), Some(BoardMode::List));
}

#[test]
fn na_lista_o_titulo_abre_a_nota_e_o_exportar_e_um_botao_no_topo() {
    let mut session = Session::open();
    session.app_mut().show_left = false;
    session.open_note("Home");
    toggle(&mut session);
    wait_active(&mut session, "Home.canvas");
    toggle(&mut session);
    assert_eq!(mode(&session), Some(BoardMode::List));

    // Nada de "abrir nota" solto, nem "Voltar ao quadro" (o seletor cobre).
    assert!(!session.has_label("abrir nota"));
    assert!(!session.has_label("Voltar ao quadro"));
    // "Exportar como nota (cópia)" é um botão de tamanho de toque, longe do título.
    let title = session.harness.get_by_label("Lista do quadro").rect();
    let export = session
        .harness
        .get_by_label("Exportar como nota (cópia)")
        .rect();
    assert!(export.height() >= 32.0, "{export:?}");
    assert!(
        export.min.x > title.max.x + 40.0 && (export.center().y - title.center().y).abs() < 20.0,
        "o botão {export:?} deve ficar separado do título {title:?}, na mesma faixa do topo"
    );

    // Clicar no título do item abre a nota (na mesma aba, com o quadro atrás no histórico).
    session.harness.get_by_label("Second").click();
    session.settle();
    session.settle();
    wait_active(&mut session, "Second.md");
    assert_eq!(session.active_text(), SECOND);
}

/// Rótulos que mostram o texto dado (dentro da faixa vertical `y_range`).
fn labels_in<'a>(
    session: &'a Session,
    text: &'a str,
    y_range: std::ops::Range<f32>,
) -> Vec<egui::Rect> {
    session
        .harness
        .query_all_by_label_contains(text)
        .map(|node| node.rect())
        .filter(|rect| y_range.contains(&rect.min.y))
        .collect()
}

#[test]
fn o_quadro_aparece_sem_ponto_canvas_na_aba_e_no_cabecalho() {
    let mut session = Session::open();
    session.open_note("Home");
    toggle(&mut session);
    wait_active(&mut session, "Home.canvas");
    session.settle();
    session.settle();
    // O nome na aba, no título da janela e no cabeçalho é "Home"; o arquivo segue Home.canvas.
    assert_eq!(session.app().tabs[0].title(), "Home");
    assert_eq!(
        session.app().window_title,
        format!("Home — {} — Rustidian", vault_name(&session))
    );
    assert!(session.path("Home.canvas").exists());
    // Faixa das abas e do cabeçalho (acima de 130 px): nenhum texto com `.canvas`.
    assert!(
        labels_in(&session, ".canvas", 0.0..130.0).is_empty(),
        "a aba ou o cabeçalho ainda mostram .canvas"
    );
    // O rótulo acessível da aba é o nome sem a extensão.
    assert!(
        !labels_in(&session, "Home", 30.0..130.0).is_empty(),
        "a aba não aparece com o nome Home"
    );
}

fn vault_name(session: &Session) -> String {
    session
        .app()
        .vault
        .as_ref()
        .map(rustidian::vault::Vault::name)
        .unwrap_or_default()
}

/// As linhas do explorador (coluna da esquerda), de cima para baixo: texto e posição.
fn explorer_rows(session: &Session) -> Vec<(String, egui::Rect)> {
    let mut rows: Vec<(String, egui::Rect)> = session
        .harness
        .query_all_by_label_contains("")
        .filter_map(|node| {
            let rect = node.rect();
            let label = node.accesskit_node().label()?;
            (rect.min.x > 50.0
                && rect.max.x < 340.0
                && rect.width() > 150.0
                && rect.min.y > 140.0
                && (28.0..40.0).contains(&rect.height()))
            .then(|| (label.to_string(), rect))
        })
        .collect();
    rows.sort_by(|a, b| a.1.min.y.total_cmp(&b.1.min.y));
    rows.dedup_by(|a, b| a.1.min.y == b.1.min.y);
    rows
}

#[test]
fn na_lista_de_arquivos_o_quadro_par_vira_um_item_recuado_sob_a_nota() {
    let mut session = Session::open_with(&[
        ("Home.canvas", br#"{"nodes":[],"edges":[]}"#),
        ("Ideias.canvas", IDEIAS.as_bytes()),
    ]);
    session.settle();
    session.settle();
    let rows = explorer_rows(&session);
    let names: Vec<&str> = rows.iter().map(|(name, _)| name.as_str()).collect();
    // Home.canvas some como linha solta e aparece como "Quadro" logo abaixo de "Home".
    assert!(!names.contains(&"Home.canvas"), "{names:?}");
    let home = names.iter().position(|name| *name == "Home").expect("Home");
    assert_eq!(names[home + 1], "Quadro", "{names:?}");
    assert!(
        rows[home + 1].1.height() >= 28.0,
        "alvo de toque pequeno: {:?}",
        rows[home + 1].1
    );
    // Um quadro sem nota par continua como hoje: linha solta com o nome do arquivo.
    assert!(names.contains(&"Ideias.canvas"), "{names:?}");
    assert_eq!(names.iter().filter(|name| **name == "Quadro").count(), 1);
}

#[test]
fn clicar_no_quadro_recuado_abre_o_quadro_e_o_arquivo_segue_no_disco() {
    let mut session = Session::open_with(&[("Home.canvas", br#"{"nodes":[],"edges":[]}"#)]);
    session.settle();
    session.settle();
    let rows = explorer_rows(&session);
    let (_, rect) = rows
        .iter()
        .find(|(name, _)| name == "Quadro")
        .expect("item do quadro");
    move_to(&mut session, rect.center());
    for pressed in [true, false] {
        session.harness.event(egui::Event::PointerButton {
            pos: rect.center(),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: NONE,
        });
        session.harness.step();
    }
    session.settle();
    wait_active(&mut session, "Home.canvas");
    assert!(session.path("Home.canvas").exists());
}

#[test]
fn o_quadro_recuado_acompanha_a_nota_na_ordem_inversa() {
    let mut session = Session::open_with(&[("Home.canvas", br#"{"nodes":[],"edges":[]}"#)]);
    session.app_mut().sort_descending = true;
    session.settle();
    session.settle();
    let rows = explorer_rows(&session);
    let names: Vec<&str> = rows.iter().map(|(name, _)| name.as_str()).collect();
    let home = names.iter().position(|name| *name == "Home").expect("Home");
    assert_eq!(names[home + 1], "Quadro", "{names:?}");
    // Z a A: Second vem antes de Home.
    assert!(
        names.iter().position(|name| *name == "Second") < Some(home),
        "{names:?}"
    );
}

#[test]
fn o_menu_do_quadro_recuado_renomeia_o_arquivo_do_quadro() {
    let mut session = Session::open_with(&[("Home.canvas", br#"{"nodes":[],"edges":[]}"#)]);
    session.settle();
    session.settle();
    let rect = explorer_rows(&session)
        .into_iter()
        .find(|(name, _)| name == "Quadro")
        .expect("item do quadro")
        .1;
    secondary_click(&mut session, rect.center());
    session.harness.get_by_label("Renomear…").click();
    session.settle();
    session.settle();
    assert!(session.app().dialog == Some(Dialog::RenameNote));
    assert_eq!(
        session.app().dialog_path.as_deref(),
        Some(Path::new("Home.canvas"))
    );
}
