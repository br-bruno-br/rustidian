//! R8b-2, lote 1: paleta enfileirada, Ctrl+Q e título embutido de `tests/gui_smoke.py`,
//! migrados para o egui_kittest (ver mod.rs e docs/bruno/R8B2-INVENTARIO.md).
//!
//! O que o smoke em Python conferia por X11 e que aqui vira estado do app: qual diálogo está
//! aberto, se a pergunta de sair está pendente e se a janela pode fechar (`allow_close`), e os
//! bytes da nota no disco. O processo fechando de verdade e o código de saída não entram aqui:
//! ficam como integração (inventário, classe c).

use super::edicao::{NONE, Step};
use super::*;
use egui_kittest::kittest::{NodeT, Queryable};

const QUIT_KEY: egui::Modifiers = CTRL;

fn read(session: &Session, name: &str) -> String {
    std::fs::read_to_string(session.path(name)).expect("nota do cofre")
}

fn dialog_is_commands(session: &Session) -> bool {
    matches!(session.app().dialog, Some(Dialog::Commands))
}

fn dialog_is_quick_open(session: &Session) -> bool {
    matches!(session.app().dialog, Some(Dialog::QuickOpen))
}

/// Clica no título embutido da nota (um rótulo de texto, não a aba nem o explorador).
fn click_title(session: &mut Session, title: &str) {
    let node = session
        .harness
        .query_all_by_label(title)
        .find(|node| node.accesskit_node().role() == egui::Role::Label)
        .unwrap_or_else(|| panic!("o título {title:?} não aparece como rótulo"));
    node.click();
    session.settle();
}

// ---------------------------------------------------------------- palette-queued

#[test]
fn escape_enfileirado_fecha_so_as_configuracoes_e_a_paleta_seguinte_abre() {
    for (shortcut, key) in [(CTRL, egui::Key::P), (CTRL, egui::Key::O)] {
        let mut session = Session::open();
        session.open_note("Home");
        session.command("Open settings");
        assert!(
            matches!(session.app().dialog, Some(Dialog::Settings)),
            "as configurações abrem pela paleta"
        );
        // O Escape chega no mesmo quadro que o atalho: fecha as configurações, não a paleta nova.
        session.queued(&[Step::Key(NONE, egui::Key::Escape), Step::Key(shortcut, key)]);
        let opened = if key == egui::Key::P {
            dialog_is_commands(&session)
        } else {
            dialog_is_quick_open(&session)
        };
        assert!(
            opened,
            "o atalho enfileirado depois do Escape abre o diálogo novo"
        );
        if key == egui::Key::P {
            session.type_text("Toggle right sidebar");
            session.key(NONE, egui::Key::Escape);
        } else {
            session.type_text("Home");
            session.key(NONE, egui::Key::Enter);
            session.wait_for(|app| {
                app.tabs
                    .get(app.active)
                    .is_some_and(|tab| tab.document.path.ends_with("Home.md"))
            });
        }
        assert!(
            session.app().dialog.is_none(),
            "o diálogo novo fecha ao terminar"
        );
    }
}

#[test]
fn texto_enfileirado_sob_um_atalho_fechado_por_escape_nao_entra_na_nota() {
    for (shortcut, key) in [(CTRL, egui::Key::P), (CTRL, egui::Key::O)] {
        for query in [None, Some("Queued query must not enter the note")] {
            let mut session = Session::open();
            session.open_note("Home");
            let mut steps = vec![Step::Key(shortcut, key)];
            if let Some(text) = query {
                steps.push(Step::Text(text));
            }
            steps.push(Step::Key(NONE, egui::Key::Escape));
            session.queued(&steps);
            assert!(
                session.app().dialog.is_none(),
                "Escape depois do atalho precisa fechar o diálogo"
            );
            session.key(CTRL, egui::Key::S);
            assert_eq!(
                read(&session, "Home.md"),
                HOME,
                "o texto do diálogo fechado não pode ir para a nota"
            );
        }
    }
}

fn click_label(session: &mut Session, label: &str) {
    session.harness.get_by_label(label).click();
    session.settle();
}

#[test]
fn comando_enfileirado_com_a_consulta_roda_e_consulta_invalida_nao_faz_nada() {
    let mut session = Session::open();
    session.open_note("Home");
    let before = session.app().show_right;
    session.queued(&[
        Step::Key(CTRL, egui::Key::P),
        Step::Text("Toggle right sidebar"),
        Step::Key(NONE, egui::Key::Enter),
    ]);
    assert_ne!(
        session.app().show_right,
        before,
        "atalho, consulta e Enter no mesmo quadro executam o comando"
    );
    let after = session.app().show_right;
    session.queued(&[
        Step::Key(CTRL, egui::Key::P),
        Step::Text("NoSuchNativeCommandInvalid123456"),
        Step::Key(NONE, egui::Key::Enter),
    ]);
    assert!(
        dialog_is_commands(&session),
        "consulta sem resultado mantém a paleta aberta"
    );
    assert_eq!(session.app().show_right, after, "consulta inválida não age");
    session.key(NONE, egui::Key::Escape);
    session.queued(&[
        Step::Key(CTRL, egui::Key::P),
        Step::Text("Toggle right sidebar"),
        Step::Key(NONE, egui::Key::Enter),
    ]);
    assert_eq!(
        session.app().show_right,
        before,
        "a segunda consulta válida restaura a barra"
    );
    assert_eq!(read(&session, "Home.md"), HOME, "nenhum byte da nota mudou");
}

// ---------------------------------------------------------------- Ctrl+Q (quit)

#[test]
fn ctrl_q_pergunta_sair_e_so_sair_ou_enter_fecham_a_janela() {
    let mut session = Session::open();
    session.open_note("Home");

    // Ctrl+Q sem edição não fecha: abre a pergunta "Sair do Rustidian?".
    session.key(QUIT_KEY, egui::Key::Q);
    assert!(
        matches!(session.app().pending, Some(Pending::ConfirmQuit)),
        "Ctrl+Q pergunta antes de fechar"
    );
    assert!(!session.app().allow_close, "perguntar não fecha a janela");

    // Escape mantém a janela.
    session.key(NONE, egui::Key::Escape);
    assert!(session.app().pending.is_none(), "Escape fecha a pergunta");
    assert!(!session.app().allow_close, "Escape mantém a janela");

    // Cancelar mantém a janela.
    session.key(QUIT_KEY, egui::Key::Q);
    click_label(&mut session, "Cancelar");
    assert!(session.app().pending.is_none(), "Cancelar fecha a pergunta");
    assert!(!session.app().allow_close, "Cancelar mantém a janela");

    // Teclas digitadas sob a pergunta não chegam à nota nem abrem a paleta.
    session.key(QUIT_KEY, egui::Key::Q);
    session.type_text("typed");
    session.key(CTRL, egui::Key::P);
    assert!(
        session.app().dialog.is_none(),
        "Ctrl+P sob a pergunta não abre a paleta"
    );
    assert!(
        matches!(session.app().pending, Some(Pending::ConfirmQuit)),
        "a pergunta continua aberta enquanto as teclas chegam a ela"
    );
    session.key(NONE, egui::Key::Escape);
    assert!(!session.app().allow_close);
    assert_eq!(
        read(&session, "Home.md"),
        HOME,
        "teclas sob a pergunta não mudam a nota"
    );

    // Enter responde Sair: a janela pode fechar.
    session.key(QUIT_KEY, egui::Key::Q);
    session.key(NONE, egui::Key::Enter);
    // Sair sem alterações fecha por ViewportCommand dentro de `window::quit_dialog`; esse
    // fechamento não aparece no estado do app (allow_close é do caminho com alterações). O
    // "Enter fecha" fica coberto por window.rs::quit_dialog_closes_once_on_enter_and_never_on_cancel.
    assert!(session.app().pending.is_none(), "Enter resolve a pergunta");
}

#[test]
fn ctrl_q_com_edicao_nao_salva_pergunta_so_o_aviso_de_alteracoes() {
    let mut session = Session::open();
    session.edit("Home", "Source editing");
    session.type_chars("UNSAVED_QUIT");
    session.key(QUIT_KEY, egui::Key::Q);
    assert!(
        matches!(session.app().pending, Some(Pending::Quit)),
        "com edição não salva, Ctrl+Q mostra só o aviso de alterações"
    );
    assert!(
        !matches!(session.app().pending, Some(Pending::ConfirmQuit)),
        "não pode aparecer a pergunta de sair junto"
    );
    assert!(!session.app().allow_close, "o aviso ainda não deixa fechar");
    assert_eq!(read(&session, "Home.md"), HOME, "nada foi salvo sem pedido");
}

// ---------------------------------------------------------------- title (renomear pelo título)

#[test]
fn clicar_no_titulo_renomeia_no_enter_e_no_sair_cancela_no_escape_e_nao_sobrescreve() {
    let source = "# Body heading stays independent\n\nExact body فارسی.\n";
    let mut session = Session::open_with(&[
        ("Notes/Title edit.md", source.as_bytes()),
        (
            "Notes/Title backlinks.md",
            "[[Title edit]]\n\n[read](Title%20edit.md)\n".as_bytes(),
        ),
        ("Notes/Title conflict.md", b"Do not replace this note.\n"),
    ]);
    session.open_note("Notes/Title edit");
    session.command("Reading view");
    session.settle();

    // Enter confirma: o arquivo muda de nome, o texto é o mesmo e os links apontam para o novo nome.
    click_title(&mut session, "Title edit");
    session.key(CTRL, egui::Key::A);
    session.type_text("Title renamed");
    session.key(NONE, egui::Key::Enter);
    session.settle();
    assert!(
        session.path("Notes/Title renamed.md").exists(),
        "o Enter renomeia"
    );
    assert!(
        !session.path("Notes/Title edit.md").exists(),
        "o nome antigo some"
    );
    assert_eq!(
        std::fs::read_to_string(session.path("Notes/Title renamed.md")).unwrap(),
        source,
        "o rename não mexe no corpo"
    );
    let linked = read(&session, "Notes/Title backlinks.md");
    // Mesmo critério do smoke: o nome novo aparece nos links, o antigo não, e o link em
    // Markdown fica com o nome codificado (o app escreve o caminho completo, Notes/…).
    assert!(
        linked.contains("Title renamed") && linked.contains("Title%20renamed.md"),
        "os links da nota passam a apontar para o novo nome: {linked}"
    );
    assert!(
        !linked.contains("Title edit"),
        "nenhum link antigo sobra: {linked}"
    );

    // Escape cancela sem criar arquivo.
    click_title(&mut session, "Title renamed");
    session.key(CTRL, egui::Key::A);
    session.type_text("Cancelled title");
    session.key(NONE, egui::Key::Escape);
    session.settle();
    assert!(
        session.path("Notes/Title renamed.md").exists(),
        "Escape mantém o nome"
    );
    assert!(
        !session.path("Notes/Cancelled title.md").exists(),
        "Escape não cria o arquivo cancelado"
    );

    // Um nome que já existe não sobrescreve a outra nota.
    click_title(&mut session, "Title renamed");
    session.key(CTRL, egui::Key::A);
    session.type_text("Title conflict");
    session.key(NONE, egui::Key::Enter);
    session.settle();
    assert_eq!(
        std::fs::read_to_string(session.path("Notes/Title renamed.md")).unwrap(),
        source,
        "um nome ocupado não troca a nota atual"
    );
    assert_eq!(
        read(&session, "Notes/Title conflict.md"),
        "Do not replace this note.\n",
        "a nota que já existia não é sobrescrita"
    );
    session.key(NONE, egui::Key::Escape);

    // Sair do campo (Tab, o foco que o smoke tirava com um clique) também renomeia.
    click_title(&mut session, "Title renamed");
    session.key(CTRL, egui::Key::A);
    session.type_text("Title blur");
    session.key(NONE, egui::Key::Tab);
    session.settle();
    assert!(
        session.path("Notes/Title blur.md").exists()
            && !session.path("Notes/Title renamed.md").exists(),
        "sair do campo renomeia"
    );
    assert_eq!(
        std::fs::read_to_string(session.path("Notes/Title blur.md")).unwrap(),
        source
    );
}
