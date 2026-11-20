//! H2 (seção como cartão, quadro dentro de quadro) e H3 (organizar, mapa mental, etiquetas e
//! filtro) do quadro, na GUI sem janela. O que importa é o texto da nota (um `Document` só) e o
//! arquivo do quadro: JSON Canvas com os campos que o app não conhece preservados.

use super::canvas::{button, click, double_click, move_to, parsed, rect_of, state};
use super::*;
use rustidian::canvas::{Canvas, Node};

const GUIA: &str = "# Guia\n\nabertura\n\n## Resumo\nlinha do resumo\n\n## Detalhe\nfino\n";

const SHIFT: egui::Modifiers = egui::Modifiers::SHIFT;

fn session_with(files: &[(&str, &str)]) -> Session {
    let bytes: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(name, text)| (*name, text.as_bytes()))
        .collect();
    let mut session = Session::open_with(&bytes);
    session
        .app_mut()
        .open_note(std::path::Path::new("Quadro.canvas"));
    session.settle();
    session
}

fn wait_active(session: &mut Session, name: &'static str) {
    session.wait_for(move |app| {
        app.tabs
            .get(app.active)
            .is_some_and(|tab| tab.document.path == Path::new(name))
    });
    session.settle();
}

fn note_text(session: &Session, name: &str) -> Option<String> {
    session
        .app()
        .tabs
        .iter()
        .chain(session.app().card_tabs.iter())
        .find(|tab| tab.document.path.ends_with(name))
        .map(|tab| tab.document.text.clone())
}

fn disk(session: &Session, name: &str) -> String {
    std::fs::read_to_string(session.path(name)).expect("arquivo no disco")
}

fn body_of(session: &Session, id: &str) -> egui::Pos2 {
    let rect = rect_of(session, id);
    egui::pos2(rect.center().x, rect.min.y + rect.height() * 0.6)
}

fn secondary_click(session: &mut Session, pos: egui::Pos2) {
    move_to(session, pos);
    for pressed in [true, false] {
        session.harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        session.harness.step();
    }
    session.settle();
}

fn shown(session: &Session, key: &str) -> String {
    state(session)
        .shown(key)
        .unwrap_or_else(|| panic!("o desenho não registrou {key}"))
}

fn dirty(session: &Session) -> bool {
    session.app().tabs[session.app().active].document.dirty()
}

/// Ctrl+Z com o ponteiro sobre o quadro (o atalho só vale ali).
fn undo(session: &mut Session) {
    let area = state(session).area_for_tests();
    move_to(session, area.center());
    session.key(CTRL, egui::Key::Z);
}

// ---------------------------------------------------------------------------------------------
// H2a: seção como cartão
// ---------------------------------------------------------------------------------------------

const QUADRO_SECOES: &str = r##"{
	"nodes":[
		{"id":"s","type":"file","x":0,"y":0,"width":420,"height":260,"file":"Guia.md","subpath":"#Resumo","gerado":{"a":1}},
		{"id":"w","type":"file","x":520,"y":0,"width":420,"height":260,"file":"Guia.md"}
	],
	"edges":[],
	"metadata":{"version":"1.0-1.0"}
}"##;

fn secoes() -> Session {
    session_with(&[("Guia.md", GUIA), ("Quadro.canvas", QUADRO_SECOES)])
}

#[test]
fn o_cartao_de_secao_mostra_so_o_trecho_e_o_titulo_diz_nota_e_secao() {
    let session = secoes();
    let body = shown(&session, "body:s");
    assert!(body.contains("linha do resumo"), "{body:?}");
    assert!(
        !body.contains("abertura") && !body.contains("fino"),
        "só a seção Resumo: {body:?}"
    );
    assert_eq!(shown(&session, "title:s"), "Guia › Resumo");
    // O cartão da nota inteira, na mesma nota, mostra tudo.
    let whole = shown(&session, "body:w");
    assert!(
        whole.contains("abertura") && whole.contains("fino"),
        "{whole:?}"
    );
    assert_eq!(shown(&session, "title:w"), "Guia");
    // Só abrir e olhar não suja o quadro nem cria cartão aberto da nota.
    assert!(!dirty(&session));
    assert!(session.app().card_tabs.is_empty());
    assert_eq!(disk(&session, "Quadro.canvas"), QUADRO_SECOES);
}

#[test]
fn editar_a_secao_escreve_na_nota_inteira_e_nao_cria_copia() {
    let mut session = secoes();
    let at = body_of(&session, "s");
    double_click(&mut session, at);
    assert_eq!(state(&session).editing_note(), Some("s"));
    session.type_text(" mais");
    // Um Document só: o trecho foi trocado dentro da nota inteira, no mesmo lugar.
    let expected = "# Guia\n\nabertura\n\n## Resumo\nlinha do resumo mais\n\n## Detalhe\nfino\n";
    assert_eq!(note_text(&session, "Guia.md").as_deref(), Some(expected));
    assert_eq!(
        session.app().card_tabs.len(),
        1,
        "um cartão aberto, sem cópia"
    );
    // O quadro em si não mudou (o subpath continua valendo) e o outro cartão vê o texto novo.
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    assert!(!dirty(&session));
    assert_eq!(
        parsed(&session).node("s").unwrap().subpath.as_deref(),
        Some("#Resumo")
    );
    assert!(shown(&session, "body:w").contains("resumo mais"));
    assert!(shown(&session, "body:s").contains("resumo mais"));
    assert!(!shown(&session, "body:s").contains("fino"));
}

#[test]
fn renomear_o_cabecalho_no_cartao_leva_o_subpath_junto() {
    let mut session = secoes();
    let at = body_of(&session, "s");
    double_click(&mut session, at);
    session.key(CTRL, egui::Key::A);
    session.type_text("## Visão geral\ncorpo novo");
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    let expected = "# Guia\n\nabertura\n\n## Visão geral\ncorpo novo\n\n## Detalhe\nfino\n";
    assert_eq!(note_text(&session, "Guia.md").as_deref(), Some(expected));
    let board = parsed(&session);
    assert_eq!(
        board.node("s").unwrap().subpath.as_deref(),
        Some("#Visão geral")
    );
    // Os campos que o app não conhece seguem no cartão.
    assert_eq!(board.node("s").unwrap().extra["gerado"]["a"], 1);
    assert_eq!(shown(&session, "title:s"), "Guia › Visão geral");
    assert!(shown(&session, "body:s").contains("corpo novo"));
}

#[test]
fn secao_sumida_avisa_e_mostrar_a_nota_inteira_nao_perde_texto() {
    let board = QUADRO_SECOES.replace("#Resumo", "#Inexistente");
    let mut session = session_with(&[("Guia.md", GUIA), ("Quadro.canvas", &board)]);
    assert_eq!(shown(&session, "body:s"), "SECAO-SUMIDA");
    // Com a seção sumida o duplo clique não entra em edição (nada a escrever).
    let at = body_of(&session, "s");
    double_click(&mut session, at);
    assert!(!state(&session).is_editing());
    let button = state(&session)
        .fix_button("s")
        .expect("botão do aviso")
        .center();
    click(&mut session, button);
    assert_eq!(parsed(&session).node("s").unwrap().subpath, None);
    let body = shown(&session, "body:s");
    assert!(
        body.contains("abertura") && body.contains("fino"),
        "{body:?}"
    );
    // A nota nunca foi tocada.
    assert_eq!(disk(&session, "Guia.md"), GUIA);
}

#[test]
fn escolher_a_secao_pelo_menu_do_cartao_troca_o_subpath() {
    let mut session = secoes();
    let at = body_of(&session, "w");
    secondary_click(&mut session, at);
    session
        .harness
        .get_by_label("Mostrar só uma seção…")
        .click();
    session.settle();
    assert!(state(&session).picker_open());
    session.harness.get_by_label_contains("Detalhe").click();
    session.settle();
    assert!(!state(&session).picker_open());
    assert_eq!(
        parsed(&session).node("w").unwrap().subpath.as_deref(),
        Some("#Detalhe")
    );
    assert_eq!(shown(&session, "title:w"), "Guia › Detalhe");
    let body = shown(&session, "body:w");
    assert!(
        body.contains("fino") && !body.contains("abertura"),
        "{body:?}"
    );
    // "Mostrar a nota inteira" desfaz.
    let at = body_of(&session, "w");
    secondary_click(&mut session, at);
    session
        .harness
        .get_by_label("Mostrar a nota inteira")
        .click();
    session.settle();
    assert_eq!(parsed(&session).node("w").unwrap().subpath, None);
}

#[test]
fn escolher_a_secao_em_cartao_novo_deixa_o_cartao_de_origem_como_esta() {
    let mut session = secoes();
    let before = parsed(&session).nodes.len();
    let at = body_of(&session, "w");
    secondary_click(&mut session, at);
    session
        .harness
        .get_by_label("Mostrar só uma seção…")
        .click();
    session.settle();
    session.harness.get_by_label("Em um cartão novo").click();
    session.settle();
    session.harness.get_by_label_contains("Resumo").click();
    session.settle();
    let board = parsed(&session);
    assert_eq!(board.nodes.len(), before + 1);
    assert_eq!(
        board.node("w").unwrap().subpath,
        None,
        "o original não mudou"
    );
    let created: &Node = board.nodes.last().unwrap();
    assert_eq!(created.file.as_deref(), Some("Guia.md"));
    assert_eq!(created.subpath.as_deref(), Some("#Resumo"));
    assert_eq!(
        state(&session).selected(),
        std::slice::from_ref(&created.id)
    );
}

#[test]
fn o_painel_de_links_conta_o_cartao_de_secao_e_mostra_nota_seta_secao() {
    let mut session = secoes();
    session.open_note("Guia");
    session.app_mut().show_right = true;
    session.app_mut().right_tab = 1;
    session.settle();
    assert!(
        session.has_label("Nos quadros (2)"),
        "os dois cartões contam"
    );
    assert!(session.has_label("Quadro › Resumo"), "linha da seção");
}

// ---------------------------------------------------------------------------------------------
// H2b: quadro dentro de quadro
// ---------------------------------------------------------------------------------------------

const QUADRO_DENTRO: &str = r##"{
	"nodes":[
		{"id":"sub","type":"file","x":0,"y":0,"width":520,"height":340,"file":"Filho.canvas"},
		{"id":"eu","type":"file","x":600,"y":0,"width":320,"height":200,"file":"Quadro.canvas"}
	],
	"edges":[]
}"##;

const FILHO: &str = r##"{
	"nodes":[
		{"id":"t","type":"text","x":0,"y":0,"width":240,"height":80,"text":"# Ideia boa\nresto"},
		{"id":"n","type":"file","x":320,"y":0,"width":240,"height":120,"file":"Alpha.md","color":"4"},
		{"id":"volta","type":"file","x":0,"y":200,"width":240,"height":120,"file":"Quadro.canvas"}
	],
	"edges":[{"id":"e","fromNode":"t","toNode":"n"}]
}"##;

fn dentro() -> Session {
    session_with(&[("Quadro.canvas", QUADRO_DENTRO), ("Filho.canvas", FILHO)])
}

#[test]
fn o_cartao_de_quadro_mostra_a_miniatura_e_o_ciclo_para_na_profundidade_um() {
    let session = dentro();
    // A miniatura lista os cartões do quadro menor, cada um com o título.
    assert_eq!(shown(&session, "mini:sub"), "Ideia boa|Alpha|Quadro");
    // Um cartão que aponta para o próprio quadro aberto não desenha miniatura.
    assert_eq!(shown(&session, "mini:eu"), "CICLO");
    // Nada foi gravado nem aberto por olhar.
    assert!(!dirty(&session));
    assert_eq!(disk(&session, "Quadro.canvas"), QUADRO_DENTRO);
    assert_eq!(session.app().tabs.len(), 1);
}

#[test]
fn quadro_menor_que_sumiu_ou_esta_vazio_diz_o_que_houve() {
    let board = r##"{"nodes":[
        {"id":"a","type":"file","x":0,"y":0,"width":300,"height":200,"file":"Nao existe.canvas"},
        {"id":"b","type":"file","x":400,"y":0,"width":300,"height":200,"file":"Vazio.canvas"},
        {"id":"c","type":"file","x":800,"y":0,"width":300,"height":200,"file":"Ruim.canvas"}],"edges":[]}"##;
    let session = session_with(&[
        ("Quadro.canvas", board),
        ("Vazio.canvas", r#"{"nodes":[],"edges":[]}"#),
        ("Ruim.canvas", "{ não é json"),
    ]);
    assert_eq!(shown(&session, "mini:a"), "Quadro não encontrado no cofre");
    assert_eq!(shown(&session, "mini:b"), "Quadro vazio");
    assert_eq!(shown(&session, "mini:c"), "Quadro inválido");
}

#[test]
fn duplo_clique_abre_o_quadro_menor_e_alt_seta_volta() {
    let mut session = dentro();
    let at = body_of(&session, "sub");
    double_click(&mut session, at);
    wait_active(&mut session, "Filho.canvas");
    assert!(crate::canvas_ui::is_canvas_tab(
        &session.app().tabs[session.app().active]
    ));
    // Alt+Seta volta ao quadro de onde veio.
    session.key(egui::Modifiers::ALT, egui::Key::ArrowLeft);
    wait_active(&mut session, "Quadro.canvas");
    assert_eq!(disk(&session, "Quadro.canvas"), QUADRO_DENTRO);
}

#[test]
fn a_miniatura_acompanha_o_quadro_menor_aberto_e_editado() {
    let mut session = dentro();
    assert_eq!(shown(&session, "mini:sub"), "Ideia boa|Alpha|Quadro");
    // Muda o quadro menor no disco: a miniatura relê em até um segundo.
    let novo = FILHO.replace("# Ideia boa", "# Outra ideia");
    std::fs::write(session.path("Filho.canvas"), novo).unwrap();
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        session.settle();
        if shown(&session, "mini:sub").starts_with("Outra ideia") {
            break;
        }
        assert!(Instant::now() < until, "a miniatura não se atualizou");
        std::thread::sleep(Duration::from_millis(50));
    }
}

// ---------------------------------------------------------------------------------------------
// H3a: seleção, alinhar, distribuir e grade
// ---------------------------------------------------------------------------------------------

const QUADRO_SOLTO: &str = r##"{
	"nodes":[
		{"id":"a","type":"text","x":0,"y":0,"width":200,"height":100,"text":"A"},
		{"id":"b","type":"text","x":300,"y":60,"width":200,"height":100,"text":"B"},
		{"id":"c","type":"text","x":900,"y":20,"width":200,"height":100,"text":"C"},
		{"id":"d","type":"text","x":150,"y":400,"width":200,"height":100,"text":"D"}
	],
	"edges":[],
	"metadata":{"version":"1.0-1.0"}
}"##;

fn soltos() -> Session {
    session_with(&[("Quadro.canvas", QUADRO_SOLTO)])
}

/// Shift+arrastar do ponto `from` até `to` (em pontos do quadro), em passos pequenos.
fn marquee(session: &mut Session, from: (f64, f64), to: (f64, f64)) {
    let (from, to) = (state(session).screen_of(from), state(session).screen_of(to));
    session.harness.event(egui::Event::ModifiersChanged(SHIFT));
    move_to(session, from);
    button(session, from, true, SHIFT);
    for step in 1..=6 {
        move_to(session, from + (to - from) * (step as f32 / 6.0));
    }
    button(session, to, false, SHIFT);
    session
        .harness
        .event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    session.settle();
}

fn select_all(session: &mut Session) {
    let area = state(session).area_for_tests();
    move_to(session, area.center());
    session.key(CTRL, egui::Key::A);
}

#[test]
fn shift_arrastar_no_fundo_faz_o_retangulo_de_selecao_e_soma_ao_que_ja_estava() {
    let mut session = soltos();
    // O retângulo pega a e b (toca neles) e não c nem d.
    marquee(&mut session, (-15.0, -15.0), (520.0, 150.0));
    let mut picked = state(&session).selected().to_vec();
    picked.sort();
    assert_eq!(picked, ["a", "b"]);
    // Um segundo retângulo com Shift soma o d ao que já estava selecionado.
    marquee(&mut session, (100.0, 340.0), (420.0, 560.0));
    let mut picked = state(&session).selected().to_vec();
    picked.sort();
    assert_eq!(picked, ["a", "b", "d"]);
    // Não é gesto de arquivo: nada foi gravado.
    assert!(!dirty(&session));
    assert_eq!(disk(&session, "Quadro.canvas"), QUADRO_SOLTO);
}

#[test]
fn cada_comando_de_organizar_e_um_passo_de_desfazer() {
    let mut session = soltos();
    let original = parsed(&session);
    select_all(&mut session);
    assert_eq!(state(&session).selected().len(), 4);

    for (command, check) in [
        ("Quadro: Alinhar à esquerda", 0),
        ("Quadro: Alinhar pelo topo", 1),
        ("Quadro: Distribuir na horizontal", 2),
        ("Quadro: Espaçamento igual", 3),
        ("Quadro: Arrumar em grade", 4),
    ] {
        session.command(command);
        session.settle();
        let after = parsed(&session);
        assert_ne!(after, original, "{command} não mexeu em nada");
        let rect = |id: &str| after.node(id).unwrap().rect();
        match check {
            0 => assert!(["a", "b", "c", "d"].iter().all(|id| rect(id).x == 0.0)),
            1 => assert!(["a", "b", "c", "d"].iter().all(|id| rect(id).y == 0.0)),
            2 => {
                // As pontas (a e c, os de x menor e maior) ficam; os vãos entre todos são iguais.
                assert_eq!(rect("a").x, 0.0);
                assert_eq!(rect("c").x + rect("c").w, 1100.0);
                let mut xs: Vec<(f64, f64)> = ["a", "b", "c", "d"]
                    .iter()
                    .map(|id| (rect(id).x, rect(id).w))
                    .collect();
                xs.sort_by(|p, q| p.0.total_cmp(&q.0));
                let gaps: Vec<f64> = xs.windows(2).map(|w| w[1].0 - (w[0].0 + w[0].1)).collect();
                assert!(
                    gaps.iter().all(|gap| (gap - gaps[0]).abs() <= 1.0),
                    "{gaps:?}"
                );
            }
            3 => {
                let mut xs: Vec<(f64, f64)> = ["a", "b", "c", "d"]
                    .iter()
                    .map(|id| (rect(id).x, rect(id).w))
                    .collect();
                xs.sort_by(|p, q| p.0.total_cmp(&q.0));
                let gaps: Vec<f64> = xs.windows(2).map(|w| w[1].0 - (w[0].0 + w[0].1)).collect();
                assert!(gaps.iter().all(|gap| *gap == 40.0), "{gaps:?}");
            }
            _ => {
                // 4 cartões: duas colunas e duas fileiras, sem ninguém encostar.
                let mut columns: Vec<i64> = ["a", "b", "c", "d"]
                    .iter()
                    .map(|id| rect(id).x as i64)
                    .collect();
                columns.sort_unstable();
                columns.dedup();
                assert_eq!(columns.len(), 2, "{columns:?}");
                for (index, first) in after.nodes.iter().enumerate() {
                    for second in &after.nodes[index + 1..] {
                        assert!(!first.rect().intersects(&second.rect()));
                    }
                }
            }
        }
        // Um desfazer devolve o quadro inteiro ao estado de antes do comando.
        undo(&mut session);
        assert_eq!(parsed(&session), original, "{command}: um desfazer só");
        select_all(&mut session);
    }
    // Campos desconhecidos do arquivo seguem ali depois de tudo isso.
    session.command("Quadro: Alinhar à direita");
    session.key(CTRL, egui::Key::S);
    let saved: serde_json::Value = serde_json::from_str(&disk(&session, "Quadro.canvas")).unwrap();
    assert_eq!(saved["metadata"]["version"], "1.0-1.0");
}

#[test]
fn organizar_sem_cartoes_suficientes_avisa_e_nao_grava() {
    let mut session = soltos();
    let at = rect_of(&session, "a").center();
    click(&mut session, at);
    assert_eq!(state(&session).selected(), ["a"]);
    session.command("Quadro: Alinhar à esquerda");
    session.settle();
    assert!(
        state(&session)
            .notice()
            .is_some_and(|text| text.contains("dois ou mais")),
        "{:?}",
        state(&session).notice()
    );
    assert!(!dirty(&session));
    assert_eq!(
        parsed(&session).nodes,
        Canvas::parse(QUADRO_SOLTO).unwrap().nodes
    );
}

#[test]
fn os_comandos_de_organizar_so_aparecem_na_paleta_de_um_quadro() {
    let mut session = Session::open();
    session.open_note("Home");
    session.key(CTRL, egui::Key::P);
    session.type_text("alinhar à esquerda");
    session.settle();
    assert!(!session.has_label("Quadro: Alinhar à esquerda"));
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    // Num quadro, sim.
    let mut board = soltos();
    board.key(CTRL, egui::Key::P);
    board.type_text("alinhar à esquerda");
    board.settle();
    assert!(board.has_label("Quadro: Alinhar à esquerda"));
}

// ---------------------------------------------------------------------------------------------
// H3b: mapa mental
// ---------------------------------------------------------------------------------------------

const QUADRO_ARVORE: &str = r##"{
	"nodes":[
		{"id":"r","type":"text","x":0,"y":0,"width":200,"height":80,"text":"Raiz"},
		{"id":"a","type":"text","x":900,"y":500,"width":200,"height":80,"text":"A"},
		{"id":"b","type":"text","x":40,"y":450,"width":200,"height":80,"text":"B"},
		{"id":"a1","type":"text","x":20,"y":700,"width":200,"height":120,"text":"A1"},
		{"id":"a2","type":"text","x":60,"y":900,"width":200,"height":120,"text":"A2"},
		{"id":"solto","type":"text","x":2000,"y":2000,"width":200,"height":80,"text":"solto"}
	],
	"edges":[
		{"id":"e0","fromNode":"r","toNode":"a"},
		{"id":"e1","fromNode":"b","toNode":"r","peso":3},
		{"id":"e2","fromNode":"a","toNode":"a1"},
		{"id":"e3","fromNode":"a","toNode":"a2"}
	]
}"##;

#[test]
fn mapa_mental_arruma_a_arvore_a_partir_do_cartao_e_um_desfazer_volta_tudo() {
    let mut session = session_with(&[("Quadro.canvas", QUADRO_ARVORE)]);
    let original = parsed(&session);
    let at = rect_of(&session, "r").center();
    click(&mut session, at);
    assert_eq!(state(&session).selected(), ["r"]);
    session.command("Quadro: Mapa mental a partir do cartão selecionado");
    session.settle();
    let tree = parsed(&session);
    let rect = |id: &str| tree.node(id).unwrap().rect();
    // A raiz não sai do lugar; filhos à direita; netos à direita dos pais.
    assert_eq!(rect("r"), original.node("r").unwrap().rect());
    for child in ["a", "b"] {
        assert!(rect(child).x >= rect("r").x + rect("r").w, "{child}");
    }
    for grandchild in ["a1", "a2"] {
        assert!(
            rect(grandchild).x >= rect("a").x + rect("a").w,
            "{grandchild}"
        );
    }
    let ids = ["r", "a", "b", "a1", "a2"];
    for (index, first) in ids.iter().enumerate() {
        for second in &ids[index + 1..] {
            assert!(!rect(first).intersects(&rect(second)), "{first} x {second}");
        }
    }
    // O cartão solto e os campos desconhecidos das setas ficam como estavam.
    assert_eq!(rect("solto"), original.node("solto").unwrap().rect());
    assert_eq!(tree.edge("e1").unwrap().extra["peso"], 3);
    // Um desfazer só devolve tudo.
    undo(&mut session);
    assert_eq!(parsed(&session), original);
}

#[test]
fn mapa_mental_de_cartao_sem_ligacao_avisa() {
    let mut session = session_with(&[("Quadro.canvas", QUADRO_ARVORE)]);
    let at = rect_of(&session, "solto").center();
    click(&mut session, at);
    session.command("Quadro: Mapa mental a partir do cartão selecionado");
    session.settle();
    assert!(
        state(&session)
            .notice()
            .is_some_and(|text| text.contains("não tem ligações"))
    );
    assert!(!dirty(&session));
}

// ---------------------------------------------------------------------------------------------
// H3c: etiquetas e filtro
// ---------------------------------------------------------------------------------------------

const QUADRO_FILTRO: &str = r##"{
	"nodes":[
		{"id":"a","type":"text","x":0,"y":0,"width":220,"height":100,"text":"A","color":"1","rustidianTags":["ideia"]},
		{"id":"b","type":"text","x":300,"y":0,"width":220,"height":100,"text":"B","color":"4","rustidianTags":["Ideia","urgente"]},
		{"id":"c","type":"text","x":600,"y":0,"width":220,"height":100,"text":"C","color":"1"},
		{"id":"d","type":"text","x":900,"y":0,"width":220,"height":100,"text":"D"}
	],
	"edges":[],
	"metadata":{"version":"1.0-1.0"}
}"##;

#[test]
fn o_filtro_esmaece_o_que_nao_casa_e_nao_mexe_no_arquivo() {
    let mut session = session_with(&[("Quadro.canvas", QUADRO_FILTRO)]);
    assert_eq!(shown(&session, "veil:a"), "0", "sem filtro nada esmaece");
    session.command("Quadro: Mostrar ou esconder o filtro");
    session.settle();
    assert!(state(&session).filter_open());
    // A faixa lista as etiquetas do quadro; escolher uma deixa em destaque só quem a tem.
    session.harness.get_by_label("#ideia").click();
    session.settle();
    assert_eq!(state(&session).filter().tag.as_deref(), Some("ideia"));
    for (id, veil) in [("a", "0"), ("b", "0"), ("c", "1"), ("d", "1")] {
        assert_eq!(shown(&session, &format!("veil:{id}")), veil, "{id}");
    }
    // O mesmo clique de novo tira o filtro.
    session.harness.get_by_label("#ideia").click();
    session.settle();
    assert!(!state(&session).filter().is_active());
    assert_eq!(shown(&session, "veil:d"), "0");
    // Filtrar nunca grava: o quadro e o arquivo seguem iguais.
    assert!(!dirty(&session));
    assert_eq!(disk(&session, "Quadro.canvas"), QUADRO_FILTRO);
}

#[test]
fn cor_e_etiqueta_juntas_e_limpar_o_filtro_pela_paleta() {
    let mut session = session_with(&[("Quadro.canvas", QUADRO_FILTRO)]);
    session.command("Quadro: Mostrar ou esconder o filtro");
    session.settle();
    state_mut(&mut session).set_filter(Some("1"), Some("ideia"));
    session.settle();
    // Cor 1 e etiqueta ideia: só o a.
    for (id, veil) in [("a", "0"), ("b", "1"), ("c", "1"), ("d", "1")] {
        assert_eq!(shown(&session, &format!("veil:{id}")), veil, "{id}");
    }
    session.command("Quadro: Limpar o filtro");
    session.settle();
    assert!(!state(&session).filter().is_active());
    assert_eq!(shown(&session, "veil:d"), "0");
}

fn state_mut(session: &mut Session) -> &mut crate::canvas_ui::CanvasState {
    let id = super::canvas::tab_id(session);
    session
        .app_mut()
        .canvases
        .state_mut_for_tests(id)
        .expect("o quadro foi desenhado")
}

#[test]
fn etiquetar_pela_paleta_grava_o_campo_nosso_e_um_desfazer_tira() {
    let mut session = session_with(&[("Quadro.canvas", QUADRO_FILTRO)]);
    let at = rect_of(&session, "d").center();
    click(&mut session, at);
    session.command("Quadro: Etiquetar o cartão selecionado");
    session.settle();
    assert!(state(&session).tag_editor_open());
    session.type_text("Projeto, #urgente  projeto");
    session.harness.get_by_label("Salvar").click();
    session.settle();
    assert!(!state(&session).tag_editor_open());
    let board = parsed(&session);
    assert_eq!(board.node("d").unwrap().tags(), ["Projeto", "urgente"]);
    // O campo vai para o arquivo como lista de textos e o resto continua igual.
    session.key(CTRL, egui::Key::S);
    let saved: serde_json::Value = serde_json::from_str(&disk(&session, "Quadro.canvas")).unwrap();
    assert_eq!(
        saved["nodes"][3]["rustidianTags"],
        serde_json::json!(["Projeto", "urgente"])
    );
    assert_eq!(
        saved["nodes"][1]["rustidianTags"],
        serde_json::json!(["Ideia", "urgente"])
    );
    assert_eq!(saved["metadata"]["version"], "1.0-1.0");
    // Um desfazer tira as etiquetas.
    undo(&mut session);
    assert!(parsed(&session).node("d").unwrap().tags().is_empty());
    // A faixa do filtro passa a oferecer a etiqueta nova assim que ela existe.
    session.key(CTRL, egui::Key::Y);
    session.command("Quadro: Mostrar ou esconder o filtro");
    session.settle();
    assert!(session.has_label("#Projeto"));
}
