//! H2a: a seção "Nos quadros (N)" do painel de links diz em quais quadros a nota aparece, e
//! clicar numa linha abre o quadro com o cartão centralizado e selecionado.

use super::*;
use rustidian::tab::View;

const PLANO: &str = r##"{
	"nodes":[
		{"id":"g1","type":"group","x":-40,"y":-60,"width":700,"height":340,"label":"Fase 1"},
		{"id":"n1","type":"file","x":0,"y":0,"width":240,"height":160,"file":"Alpha.md"},
		{"id":"n2","type":"file","x":1500,"y":900,"width":240,"height":160,"file":"Second.md"}
	],
	"edges":[]
}"##;

const OUTRO: &str = r##"{"nodes":[{"id":"x","type":"file","x":5,"y":5,"width":100,"height":80,"file":"./Alpha.md"}],"edges":[]}"##;

fn painel_de_alpha() -> Session {
    let mut session = Session::open_with(&[
        ("Plano.canvas", PLANO.as_bytes()),
        ("Pasta/Outro.canvas", OUTRO.as_bytes()),
    ]);
    session.open_note("Alpha");
    session.app_mut().show_right = true;
    session.app_mut().right_tab = 1;
    session.settle();
    session
}

#[test]
fn secao_nos_quadros_conta_os_quadros_e_mostra_o_grupo() {
    let session = painel_de_alpha();
    assert!(session.has_label("Nos quadros (2)"), "contagem da seção");
    assert!(session.has_label("Plano · Fase 1"), "quadro com o grupo");
    assert!(session.has_label("Outro"), "quadro sem grupo");
}

#[test]
fn clicar_na_linha_abre_o_quadro_e_centraliza_o_cartao() {
    let mut session = painel_de_alpha();
    session.harness.get_by_label("Plano · Fase 1").click();
    session.settle();
    session.settle();
    let app = session.app();
    let tab = &app.tabs[app.active];
    assert_eq!(tab.document.path, Path::new("Plano.canvas"));
    assert_ne!(tab.view, View::Source);
    let state = app
        .canvases
        .state(tab.id())
        .expect("o quadro foi desenhado");
    assert_eq!(state.selected(), ["n1"]);
    let rect = state.node_screen_rect("n1").expect("cartão no quadro");
    let area = state.area_for_tests();
    // O ajuste inicial deixaria o cartão (no canto do quadro) longe do meio; o foco o traz.
    let off = rect.center() - area.center();
    assert!(
        off.x.abs() < 5.0 && off.y.abs() < 50.0,
        "o cartão {rect:?} deve estar no meio da área {area:?}"
    );
}
