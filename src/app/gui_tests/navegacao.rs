//! R8b: fluxos de `tests/gui_smoke.py` migrados para o egui_kittest (ver mod.rs).
//!
//! Navegação e espaço de trabalho: abas, favoritos, paletas, criação de notas e pastas,
//! mover/copiar arquivos, lixeira, configurações, recuperação e layouts.
//!
//! Fora do alcance de um harness sem janela (e por isso não migrado): cor e pixel medidos em
//! captura de tela, arrastar com mouse real, seletor nativo de pasta/arquivo, ícone da janela
//! do sistema, reinício do processo e `chmod` de pasta (substituído, quando faz sentido, por
//! arquivo somente leitura).

#[allow(unused_imports)]
use super::*;

use rustidian::tab::View;

/// Nome de arquivo (sem pastas) de cada aba, na ordem da barra.
fn tab_files(session: &Session) -> Vec<String> {
    session
        .app()
        .tabs
        .iter()
        .map(|tab| {
            tab.document
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
        .collect()
}

fn pins(session: &Session) -> Vec<bool> {
    session.app().tabs.iter().map(|tab| tab.pinned).collect()
}

fn active_file(session: &Session) -> String {
    tab_files(session)
        .get(session.app().active)
        .cloned()
        .unwrap_or_default()
}

fn active_view(session: &Session) -> View {
    session.app().tabs[session.app().active].view
}

/// Abre uma nota numa aba nova: Ctrl+T e depois o Abrir rápido.
fn open_in_new_tab(session: &mut Session, name: &str) {
    session.key(CTRL, egui::Key::T);
    session.open_note(name);
}

/// Cria `Nome.md` com o texto dado no cofre do teste.
fn write_note(session: &Session, name: &str, text: &str) {
    let path = session.path(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("pasta da nota");
    }
    std::fs::write(path, text).expect("nota de teste");
}

/// Reindexa o cofre (o comando "Refresh vault") e espera a lista de notas refletir o disco.
fn refresh(session: &mut Session, notes: usize) {
    session.command("Refresh vault");
    session.wait_for(|app| {
        app.loading.is_none()
            && app
                .vault
                .as_ref()
                .is_some_and(|vault| vault.notes.len() >= notes)
    });
}

fn click_label(session: &mut Session, label: &str) {
    session.harness.get_by_label(label).click();
    session.settle();
}

fn has_pending(session: &Session) -> bool {
    session.app().pending.is_some()
}

fn closed_len(session: &Session) -> usize {
    session.app().closed_tabs.len()
}

fn set_readonly(path: &Path, readonly: bool) {
    let mut permissions = std::fs::metadata(path).expect("metadados").permissions();
    permissions.set_readonly(readonly);
    std::fs::set_permissions(path, permissions).expect("permissão do arquivo");
}

// ---------------------------------------------------------------------------------------------
// tab-state
// ---------------------------------------------------------------------------------------------

#[test]
fn fechar_uma_aba_anterior_preserva_o_desfazer_da_aba_ativa() {
    let mut session = Session::open();
    session.open_note("Home");
    open_in_new_tab(&mut session, "Second");
    session.command("Source editing");
    session.key(CTRL, egui::Key::End);
    session.type_text("TAB_STATE_SENTINEL");
    session.key(CTRL, egui::Key::S);
    assert_eq!(
        std::fs::read_to_string(session.path("Second.md")).unwrap(),
        format!("{SECOND}TAB_STATE_SENTINEL")
    );
    // Vai para a aba anterior (Home) e a fecha: a Second volta a ser a ativa.
    session.key(CTRL | egui::Modifiers::SHIFT, egui::Key::Tab);
    assert_eq!(active_file(&session), "Home.md");
    session.key(CTRL, egui::Key::W);
    assert_eq!(tab_files(&session), ["Second.md"]);
    // O histórico de desfazer da Second continua: desfazer, refazer e desfazer de novo.
    session.key(CTRL, egui::Key::End);
    session.key(CTRL, egui::Key::Z);
    session.key(CTRL, egui::Key::S);
    assert_eq!(
        std::fs::read_to_string(session.path("Second.md")).unwrap(),
        SECOND
    );
    assert_eq!(
        std::fs::read_to_string(session.path("Home.md")).unwrap(),
        HOME
    );
    session.key(CTRL, egui::Key::Y);
    session.key(CTRL, egui::Key::S);
    assert_eq!(
        std::fs::read_to_string(session.path("Second.md")).unwrap(),
        format!("{SECOND}TAB_STATE_SENTINEL")
    );
    session.key(CTRL, egui::Key::Z);
    session.key(CTRL, egui::Key::S);
    assert_eq!(
        std::fs::read_to_string(session.path("Second.md")).unwrap(),
        SECOND
    );
}

// ---------------------------------------------------------------------------------------------
// tab-actions
// ---------------------------------------------------------------------------------------------

#[test]
fn fixar_aba_protege_a_nota_e_abrir_outra_cria_aba_nova() {
    let mut session = Session::open();
    session.open_note("Home");
    session.command("Toggle pin");
    assert_eq!(pins(&session), [true]);
    // Abrir outra nota pelo Abrir rápido não substitui a aba fixada.
    session.open_note("Second");
    assert_eq!(tab_files(&session), ["Home.md", "Second.md"]);
    assert_eq!(pins(&session), [true, false]);
    assert_eq!(session.active_text(), SECOND);
    // O estado de fixação entra na sessão gravada fora do cofre.
    session.app_mut().write_session().expect("sessão gravada");
    let root = session.app().vault.as_ref().unwrap().root.clone();
    let state = rustidian::paths::state_file(&root, "", "json").expect("arquivo de estado");
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(state).unwrap()).unwrap();
    assert_eq!(saved["pinned"], serde_json::json!([true, false]));
    assert_eq!(
        std::fs::read_to_string(session.path("Home.md")).unwrap(),
        HOME
    );
}

#[test]
fn comandos_de_aba_ciclam_criam_e_fecham_sem_criar_arquivos() {
    let mut session = Session::open();
    session.open_note("Home");
    open_in_new_tab(&mut session, "Second");
    open_in_new_tab(&mut session, "Alpha");
    assert_eq!(active_file(&session), "Alpha.md");
    session.command("Go to previous tab");
    assert_eq!(active_file(&session), "Second.md");
    session.command("Go to next tab");
    assert_eq!(active_file(&session), "Alpha.md");
    // O ciclo dá a volta nas pontas.
    session.command("Go to next tab");
    assert_eq!(active_file(&session), "Home.md");
    session.command("Go to previous tab");
    assert_eq!(active_file(&session), "Alpha.md");
    let before = std::fs::read_dir(session.path("")).unwrap().count();
    session.command("New tab");
    assert_eq!(session.app().tabs.len(), 4);
    assert!(session.app().tabs[session.app().active].document.is_blank());
    session.command("Close current tab");
    assert_eq!(active_file(&session), "Alpha.md");
    assert_eq!(std::fs::read_dir(session.path("")).unwrap().count(), before);
}

#[test]
fn ctrl_w_numa_aba_fixada_primeiro_desafixa_depois_fecha() {
    let mut session = Session::open();
    session.open_note("Home");
    session.command("Toggle pin");
    open_in_new_tab(&mut session, "Second");
    session.key(CTRL | egui::Modifiers::SHIFT, egui::Key::Tab);
    assert_eq!(active_file(&session), "Home.md");
    session.key(CTRL, egui::Key::W);
    assert_eq!(
        tab_files(&session),
        ["Home.md", "Second.md"],
        "o primeiro Ctrl+W só desafixa"
    );
    assert_eq!(pins(&session), [false, false]);
    assert_eq!(closed_len(&session), 0, "desafixar não entra no histórico");
    session.key(CTRL, egui::Key::W);
    assert_eq!(tab_files(&session), ["Second.md"]);
}

#[test]
fn uma_aba_em_branco_pode_ser_fixada_e_fechada_sem_criar_nota() {
    let mut session = Session::open();
    session.app_mut().tabs.clear();
    session.app_mut().active = 0;
    session.settle();
    let before = std::fs::read_dir(session.path("")).unwrap().count();
    session.command("New tab");
    session.command("Toggle pin");
    assert_eq!(pins(&session), [true]);
    session.key(CTRL, egui::Key::W);
    assert_eq!(pins(&session), [false]);
    session.key(CTRL, egui::Key::W);
    assert!(session.app().tabs.is_empty());
    assert_eq!(std::fs::read_dir(session.path("")).unwrap().count(), before);
}

#[test]
fn clique_numa_ligacao_de_aba_fixada_abre_o_destino_em_outra_aba() {
    let mut session = Session::open();
    session.open_note("Home");
    session.command("Toggle pin");
    session.command("Source editing");
    // Ctrl+Enter sobre [[Second]] segue o link; com a aba fixada ele nunca troca a fixada.
    session.key(CTRL, egui::Key::Home);
    session.app_mut().follow_link("Second");
    session.settle();
    assert_eq!(tab_files(&session), ["Home.md", "Second.md"]);
    assert_eq!(pins(&session), [true, false]);
    assert_eq!(
        std::fs::read_to_string(session.path("Home.md")).unwrap(),
        HOME
    );
}

// ---------------------------------------------------------------------------------------------
// tab-restore
// ---------------------------------------------------------------------------------------------

#[test]
fn desfazer_fechamento_sem_historico_nao_cria_aba() {
    let mut session = Session::open();
    session.open_note("Home");
    assert_eq!(closed_len(&session), 0);
    session.key(CTRL | egui::Modifiers::SHIFT, egui::Key::T);
    assert_eq!(
        tab_files(&session),
        ["Home.md"],
        "sem histórico a aba atual fica como está"
    );
    // Fechar uma aba em branco não entra no histórico.
    session.key(CTRL, egui::Key::T);
    session.key(CTRL, egui::Key::W);
    assert_eq!(closed_len(&session), 0);
}

#[test]
fn reabrir_aba_devolve_nota_e_modo_na_ordem_do_ultimo_fechamento() {
    let mut session = Session::open();
    session.open_note("Home");
    session.command("Source editing");
    open_in_new_tab(&mut session, "Second");
    session.command("Reading view");
    open_in_new_tab(&mut session, "Alpha");
    session.command("Live preview");
    assert_eq!(tab_files(&session), ["Home.md", "Second.md", "Alpha.md"]);
    // Fecha a do meio: a vizinha da direita (Alpha) passa a ser a ativa.
    session.key(CTRL | egui::Modifiers::SHIFT, egui::Key::Tab);
    assert_eq!(active_file(&session), "Second.md");
    session.key(CTRL, egui::Key::W);
    assert_eq!(tab_files(&session), ["Home.md", "Alpha.md"]);
    assert_eq!(active_file(&session), "Alpha.md");
    session.key(CTRL | egui::Modifiers::SHIFT, egui::Key::T);
    assert_eq!(tab_files(&session), ["Home.md", "Alpha.md", "Second.md"]);
    assert_eq!(active_file(&session), "Second.md");
    assert_eq!(active_view(&session), View::Read);
    // Dois fechamentos voltam na ordem inversa, pelo atalho e pelo comando da paleta.
    session.key(CTRL, egui::Key::W);
    session.key(CTRL | egui::Modifiers::SHIFT, egui::Key::Tab);
    assert_eq!(active_file(&session), "Home.md");
    session.key(CTRL, egui::Key::W);
    assert_eq!(tab_files(&session), ["Alpha.md"]);
    session.key(CTRL | egui::Modifiers::SHIFT, egui::Key::T);
    assert_eq!(tab_files(&session), ["Alpha.md", "Home.md"]);
    assert_eq!(active_view(&session), View::Source);
    session.command("Undo close tab");
    assert_eq!(tab_files(&session), ["Alpha.md", "Home.md", "Second.md"]);
    assert_eq!(active_view(&session), View::Read);
    for (name, text) in [
        ("Home.md", HOME),
        ("Second.md", SECOND),
        ("Alpha.md", ALPHA),
    ] {
        assert_eq!(std::fs::read_to_string(session.path(name)).unwrap(), text);
    }
}

#[test]
fn reabrir_uma_aba_le_o_arquivo_de_novo_do_disco() {
    let mut session = Session::open();
    session.open_note("Home");
    open_in_new_tab(&mut session, "Second");
    session.key(CTRL, egui::Key::W);
    let external = "# Second\n\nEXTERNAL_SAVED_BYTES\n";
    std::fs::write(session.path("Second.md"), external).unwrap();
    session.key(CTRL | egui::Modifiers::SHIFT, egui::Key::T);
    assert_eq!(active_file(&session), "Second.md");
    assert_eq!(
        session.active_text(),
        external,
        "a aba reaberta traz o que está no disco"
    );
}

#[test]
fn reabrir_aba_de_arquivo_que_sumiu_abre_aba_vazia_sem_recriar_o_arquivo() {
    let mut session = Session::open();
    session.open_note("Home");
    open_in_new_tab(&mut session, "Second");
    session.key(CTRL, egui::Key::W);
    let removed = session.path("Second.removed");
    std::fs::rename(session.path("Second.md"), &removed).unwrap();
    session.key(CTRL | egui::Modifiers::SHIFT, egui::Key::T);
    assert!(
        !session.path("Second.md").exists(),
        "reabrir não recria o arquivo"
    );
    assert!(session.app().tabs[session.app().active].document.is_blank());
    assert_eq!(
        closed_len(&session),
        0,
        "a entrada do histórico foi consumida"
    );
    assert_eq!(std::fs::read_to_string(&removed).unwrap(), SECOND);
}

#[test]
fn fechar_fixada_so_desafixa_e_a_reaberta_volta_sem_fixacao() {
    let mut session = Session::open();
    session.open_note("Home");
    open_in_new_tab(&mut session, "Second");
    session.command("Toggle pin");
    session.key(CTRL, egui::Key::W);
    assert_eq!(pins(&session), [false, false]);
    assert_eq!(closed_len(&session), 0);
    session.key(CTRL, egui::Key::W);
    assert_eq!(tab_files(&session), ["Home.md"]);
    session.key(CTRL | egui::Modifiers::SHIFT, egui::Key::T);
    assert_eq!(tab_files(&session), ["Home.md", "Second.md"]);
    assert_eq!(pins(&session), [false, false]);
}

#[test]
fn fechar_as_outras_abas_mantem_a_ativa_e_as_fixadas_e_reabre_na_ordem_visual() {
    let mut session = Session::open();
    session.open_note("Home");
    session.command("Toggle pin");
    open_in_new_tab(&mut session, "Second");
    open_in_new_tab(&mut session, "Alpha");
    write_note(&session, "Beta.md", "# Beta\n");
    refresh(&mut session, 4);
    open_in_new_tab(&mut session, "Beta");
    assert_eq!(
        tab_files(&session),
        ["Home.md", "Second.md", "Alpha.md", "Beta.md"]
    );
    // Seleciona a terceira aba e fecha todas as outras (menos a fixada).
    session.app_mut().active = 2;
    session.command("Close all other tabs");
    assert_eq!(tab_files(&session), ["Home.md", "Alpha.md"]);
    assert_eq!(active_file(&session), "Alpha.md");
    assert_eq!(pins(&session), [true, false]);
    // Sem nada para fechar o comando some da paleta e não faz nada.
    session.key(CTRL, egui::Key::P);
    session.type_text("Close all other");
    assert!(!session.has_label("Close all other tabs"));
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    // A ordem de reabertura é a da remoção visual: Beta e depois Second.
    session.key(CTRL | egui::Modifiers::SHIFT, egui::Key::T);
    assert_eq!(tab_files(&session), ["Home.md", "Alpha.md", "Beta.md"]);
    session.key(CTRL | egui::Modifiers::SHIFT, egui::Key::T);
    assert_eq!(
        tab_files(&session),
        ["Home.md", "Alpha.md", "Beta.md", "Second.md"]
    );
}

// ---------------------------------------------------------------------------------------------
// tab-close-menu
// ---------------------------------------------------------------------------------------------

/// Quatro abas A..D (Menu A.md ...); A fixada. O menu de contexto da aba é mouse; aqui chamamos
/// as mesmas operações que os itens do menu chamam.
fn menu_session() -> Session {
    let mut session = Session::open();
    for name in ["A", "B", "C", "D"] {
        write_note(
            &session,
            &format!("Menu {name}.md"),
            &format!("MENU_{name}_ORIGINAL\n"),
        );
    }
    refresh(&mut session, 7);
    session.open_note("Menu A");
    session.command("Toggle pin");
    for name in ["B", "C", "D"] {
        open_in_new_tab(&mut session, &format!("Menu {name}"));
    }
    session
}

fn menu_tabs(session: &Session) -> String {
    tab_files(session)
        .iter()
        .map(|name| {
            name.trim_start_matches("Menu ")
                .trim_end_matches(".md")
                .to_owned()
        })
        .collect()
}

fn tab_id(session: &Session, letter: &str) -> u64 {
    session
        .app()
        .tabs
        .iter()
        .find(|tab| tab.document.path.ends_with(format!("Menu {letter}.md")))
        .map(|tab| tab.id())
        .expect("aba do menu")
}

#[test]
fn fechar_abas_a_direita_usa_a_aba_clicada_e_poupa_as_fixadas() {
    let mut session = menu_session();
    assert_eq!(menu_tabs(&session), "ABCD");
    // Fixa também a D, seleciona C e fecha à direita de B (aba clicada em segundo plano).
    session.app_mut().active = 3;
    session.command("Toggle pin");
    session.app_mut().active = 2;
    let b = tab_id(&session, "B");
    session.app_mut().request_close_tabs_to_right(b);
    session.settle();
    assert_eq!(menu_tabs(&session), "ABD", "só a C, não fixada, fecha");
    assert_eq!(active_file(&session), "Menu B.md");
    assert_eq!(pins(&session), [true, false, true]);
    session.key(CTRL | egui::Modifiers::SHIFT, egui::Key::T);
    assert_eq!(menu_tabs(&session), "ABDC");
    // À direita da última nada fecha.
    let c = tab_id(&session, "C");
    session.app_mut().request_close_tabs_to_right(c);
    assert_eq!(menu_tabs(&session), "ABDC");
}

#[test]
fn fechar_outras_remove_dos_dois_lados_poupa_fixadas_e_reabre_na_ordem_visual() {
    let mut session = menu_session();
    session.app_mut().tabs[0].pinned = false; // só a D ficará fixada
    session.app_mut().active = 3;
    session.command("Toggle pin"); // D fixada
    session.app_mut().active = 2;
    let b = tab_id(&session, "B");
    session.app_mut().request_close_other_tabs_for(b);
    session.settle();
    assert_eq!(menu_tabs(&session), "BD");
    assert_eq!(active_file(&session), "Menu B.md");
    // De novo não há o que fechar.
    session.app_mut().request_close_other_tabs_for(b);
    assert_eq!(menu_tabs(&session), "BD");
    session.key(CTRL | egui::Modifiers::SHIFT, egui::Key::T);
    assert_eq!(menu_tabs(&session), "BDC");
    session.key(CTRL | egui::Modifiers::SHIFT, egui::Key::T);
    assert_eq!(menu_tabs(&session), "BDCA");
}

#[test]
fn salvar_que_falha_pergunta_antes_de_fechar_e_cancelar_nao_fecha_nada() {
    let mut session = menu_session();
    session.app_mut().active = 2; // C
    session.command("Source editing");
    session.key(CTRL, egui::Key::End);
    session.type_text("SAVE_DRAFT");
    set_readonly(&session.path("Menu C.md"), true);
    let b = tab_id(&session, "B");
    session.app_mut().request_close_other_tabs_for(b);
    session.settle();
    assert!(
        has_pending(&session),
        "o salvamento falhou: a pergunta aparece"
    );
    assert_eq!(menu_tabs(&session), "ABCD");
    click_label(&mut session, "Cancelar");
    assert!(!has_pending(&session));
    assert_eq!(menu_tabs(&session), "ABCD");
    assert_eq!(
        std::fs::read_to_string(session.path("Menu C.md")).unwrap(),
        "MENU_C_ORIGINAL\n"
    );
    // Volta a permitir a escrita: "Salvar e continuar" grava o rascunho e fecha o grupo.
    set_readonly(&session.path("Menu C.md"), false);
    session.app_mut().request_close_other_tabs_for(b);
    session.settle();
    if has_pending(&session) {
        click_label(&mut session, "Salvar e continuar");
    }
    assert_eq!(menu_tabs(&session), "AB");
    assert_eq!(
        std::fs::read_to_string(session.path("Menu C.md")).unwrap(),
        "MENU_C_ORIGINAL\nSAVE_DRAFT"
    );
}

#[test]
fn descartar_fecha_o_grupo_sem_gravar_o_rascunho_que_nao_pode_ser_salvo() {
    let mut session = menu_session();
    session.app_mut().active = 0; // A, fixada
    session.command("Source editing");
    session.key(CTRL, egui::Key::End);
    session.type_text("DISCARD_DRAFT");
    // A fixada não fecha; selecionamos B e fechamos à direita: a C e a D (limpas) fecham sem pergunta.
    let b = tab_id(&session, "B");
    session.app_mut().request_close_tabs_to_right(b);
    session.settle();
    assert_eq!(menu_tabs(&session), "AB");
    // Uma aba suja e sem como salvar exige "Descartar".
    open_in_new_tab(&mut session, "Menu C");
    session.command("Source editing");
    session.key(CTRL, egui::Key::End);
    session.type_text("DISCARD_DRAFT");
    set_readonly(&session.path("Menu C.md"), true);
    session.app_mut().request_close_tabs_to_right(b);
    session.settle();
    assert!(has_pending(&session));
    assert_eq!(menu_tabs(&session), "ABC");
    click_label(&mut session, "Descartar");
    assert_eq!(menu_tabs(&session), "AB");
    set_readonly(&session.path("Menu C.md"), false);
    assert_eq!(
        std::fs::read_to_string(session.path("Menu C.md")).unwrap(),
        "MENU_C_ORIGINAL\n"
    );
    // A reaberta lê os bytes salvos de novo: o rascunho descartado não volta.
    session.key(CTRL | egui::Modifiers::SHIFT, egui::Key::T);
    assert_eq!(active_file(&session), "Menu C.md");
    assert_eq!(session.active_text(), "MENU_C_ORIGINAL\n");
}

// ---------------------------------------------------------------------------------------------
// navigation
// ---------------------------------------------------------------------------------------------

const TARGET_SOURCE: &str = "# Linked target\n\nIntroduction paragraph.\n\nIntroduction paragraph.\n\nIntroduction paragraph.\n\n## Section two\n\nDestination text.\n";

fn navigation_session() -> Session {
    let mut session = Session::open();
    write_note(&session, "Notes/Linked target.md", TARGET_SOURCE);
    write_note(
        &session,
        "Notes/Navigation UI.md",
        "# Navigation UI\n\n[[Notes/Linked target|Wiki target]]\n\n[Open target](Linked%20target.md#Section%20two)\n\n[Open attachment](record.json)\n",
    );
    write_note(
        &session,
        "Notes/record.json",
        "{\"attachment\":\"original evidence\"}\n",
    );
    refresh(&mut session, 5);
    session.open_note("Navigation UI");
    session
}

#[test]
fn seguir_ligacoes_wiki_e_markdown_e_voltar_avancar_com_alt_setas() {
    let mut session = navigation_session();
    let alt = egui::Modifiers::ALT;
    session.app_mut().follow_link("Notes/Linked target");
    session.settle();
    assert_eq!(active_file(&session), "Linked target.md");
    session.key(alt, egui::Key::ArrowLeft);
    assert_eq!(active_file(&session), "Navigation UI.md");
    // A ligação Markdown com espaço codificado e título aponta para o mesmo arquivo.
    session
        .app_mut()
        .follow_link("Linked%20target.md#Section%20two");
    session.settle();
    assert_eq!(active_file(&session), "Linked target.md");
    assert!(
        session.app().error.is_none(),
        "o título da ligação existe: sem erro de âncora"
    );
    session.key(alt, egui::Key::ArrowLeft);
    assert_eq!(active_file(&session), "Navigation UI.md");
    session.key(alt, egui::Key::ArrowRight);
    assert_eq!(active_file(&session), "Linked target.md");
    session.key(alt, egui::Key::ArrowLeft);
    assert_eq!(active_file(&session), "Navigation UI.md");
    assert_eq!(
        session.app().tabs.len(),
        1,
        "seguir ligações troca a nota da mesma aba"
    );
    assert_eq!(
        std::fs::read_to_string(session.path("Notes/Linked target.md")).unwrap(),
        TARGET_SOURCE
    );
}

#[test]
fn ligacao_com_o_gesto_de_nova_aba_mantem_a_origem() {
    let mut session = navigation_session();
    let before = session.app().tabs.len();
    session.app_mut().new_tab_gesture = true;
    session.app_mut().follow_link("Notes/Linked target");
    session.app_mut().new_tab_gesture = false;
    session.settle();
    assert_eq!(session.app().tabs.len(), before + 1);
    assert_eq!(active_file(&session), "Linked target.md");
    session.key(CTRL, egui::Key::W);
    assert_eq!(active_file(&session), "Navigation UI.md");
}

#[test]
fn documento_de_texto_do_cofre_abre_no_editor_de_texto_sem_criar_arquivos() {
    let mut session = navigation_session();
    let before = std::fs::read_dir(session.path("Notes")).unwrap().count();
    session.app_mut().follow_link("record.json");
    session.settle();
    assert_eq!(active_file(&session), "record.json");
    assert_eq!(
        session.active_text(),
        "{\"attachment\":\"original evidence\"}\n"
    );
    assert_eq!(active_view(&session), View::Source);
    assert_eq!(
        std::fs::read_dir(session.path("Notes")).unwrap().count(),
        before
    );
    assert!(!session.path("Notes/record.json.md").exists());
    // Voltar devolve a nota de origem.
    session.key(egui::Modifiers::ALT, egui::Key::ArrowLeft);
    assert_eq!(active_file(&session), "Navigation UI.md");
}

#[test]
fn ligacao_para_uri_de_arquivo_nao_cria_notas_nem_altera_o_arquivo_fora_do_cofre() {
    let mut session = navigation_session();
    let outside = tempfile::tempdir().unwrap();
    let evidence = outside.path().join("record #1.json");
    std::fs::write(&evidence, "{\"proof\":\"original evidence\"}\n").unwrap();
    let uri = format!(
        "file:///{}",
        evidence
            .to_string_lossy()
            .replace('\\', "/")
            .replace('#', "%23")
            .replace(' ', "%20")
    );
    let notes_before = session.app().vault.as_ref().unwrap().notes.len();
    session.app_mut().follow_link(&uri);
    session.settle();
    assert_eq!(
        std::fs::read_to_string(&evidence).unwrap(),
        "{\"proof\":\"original evidence\"}\n"
    );
    assert!(!session.path("file:").exists(), "uma URI não vira nota");
    assert_eq!(
        session.app().vault.as_ref().unwrap().notes.len(),
        notes_before
    );
}

#[test]
fn ctrl_enter_sobre_uma_ligacao_abre_o_destino_numa_aba_nova() {
    let mut session = Session::open();
    write_note(
        &session,
        "Queued origin.md",
        "# Queued origin\n\n[[Queued target]]",
    );
    write_note(
        &session,
        "Queued target.md",
        "# Queued target\n\nIndependent destination.\n",
    );
    refresh(&mut session, 5);
    session.open_note("Queued origin");
    session.command("Source editing");
    session.key(CTRL, egui::Key::End);
    let before = session.app().tabs.len();
    session.key(CTRL, egui::Key::Enter);
    session.wait_for(|app| app.tabs.len() == before + 1);
    assert_eq!(active_file(&session), "Queued target.md");
    assert_eq!(
        std::fs::read_to_string(session.path("Queued origin.md")).unwrap(),
        "# Queued origin\n\n[[Queued target]]"
    );
    // A nova aba tem o próprio histórico: voltar não a leva à origem.
    session.key(egui::Modifiers::ALT, egui::Key::ArrowLeft);
    assert_eq!(active_file(&session), "Queued target.md");
    session.key(CTRL, egui::Key::W);
    assert_eq!(active_file(&session), "Queued origin.md");
}

// ---------------------------------------------------------------------------------------------
// quick-create
// ---------------------------------------------------------------------------------------------

/// Abre o Abrir rápido (Ctrl+O) com `query` já digitada.
fn quick_query(session: &mut Session, query: &str) {
    session.key(CTRL, egui::Key::O);
    session.type_text(query);
}

fn file_set(session: &Session) -> std::collections::BTreeSet<String> {
    fn walk(base: &Path, folder: &Path, out: &mut std::collections::BTreeSet<String>) {
        for entry in std::fs::read_dir(folder).unwrap().flatten() {
            let path = entry.path();
            out.insert(
                path.strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            );
            if path.is_dir() {
                walk(base, &path, out);
            }
        }
    }
    let mut out = std::collections::BTreeSet::new();
    let root = session.path("");
    walk(&root, &root, &mut out);
    out
}

fn wait_active_is(session: &mut Session, suffix: &'static str) {
    session.wait_for(|app| {
        app.tabs
            .get(app.active)
            .is_some_and(|tab| tab.document.path.ends_with(suffix))
    });
}

#[test]
fn abrir_rapido_sem_resultado_cria_a_nota_nomeada_e_abre_editavel() {
    let mut session = Session::open();
    write_note(&session, "Quick origin.md", "ORIGIN\n");
    refresh(&mut session, 4);
    session.open_note("Quick origin");
    session.command("Source editing");
    session.key(CTRL, egui::Key::End);
    session.type_text("DRAFT_TAIL");
    quick_query(&mut session, "Fresh idea ZQ901");
    assert!(
        session.has_label("Criar nota"),
        "sem resultado o botão aparece"
    );
    session.key(egui::Modifiers::NONE, egui::Key::Enter);
    wait_active_is(&mut session, "Fresh idea ZQ901.md");
    assert_eq!(
        std::fs::read_to_string(session.path("Fresh idea ZQ901.md")).unwrap(),
        ""
    );
    // O rascunho da nota de origem foi salvo antes de trocar de nota.
    assert_eq!(
        std::fs::read_to_string(session.path("Quick origin.md")).unwrap(),
        "ORIGIN\nDRAFT_TAIL"
    );
    session.key(CTRL, egui::Key::End);
    session.type_text("CREATED_BODY");
    session.key(CTRL, egui::Key::S);
    assert_eq!(
        std::fs::read_to_string(session.path("Fresh idea ZQ901.md")).unwrap(),
        "CREATED_BODY"
    );
    // O histórico guarda a volta e a ida.
    session.key(egui::Modifiers::ALT, egui::Key::ArrowLeft);
    assert_eq!(active_file(&session), "Quick origin.md");
    session.key(egui::Modifiers::ALT, egui::Key::ArrowRight);
    assert_eq!(active_file(&session), "Fresh idea ZQ901.md");
}

#[test]
fn ctrl_enter_e_origem_fixada_criam_a_nota_numa_aba_nova() {
    let mut session = Session::open();
    write_note(&session, "Quick origin.md", "ORIGIN\n");
    refresh(&mut session, 4);
    session.open_note("Quick origin");
    quick_query(&mut session, "Separate tab ZQ902");
    session.key(CTRL, egui::Key::Enter);
    session.wait_for(|app| app.tabs.len() == 2);
    assert_eq!(
        tab_files(&session),
        ["Quick origin.md", "Separate tab ZQ902.md"]
    );
    // Com a origem fixada, Enter simples também abre aba nova e preserva a origem.
    session.app_mut().active = 0;
    session.command("Toggle pin");
    quick_query(&mut session, "Pinned creation ZQ903");
    session.key(egui::Modifiers::NONE, egui::Key::Enter);
    session.wait_for(|app| app.tabs.len() == 3);
    assert_eq!(active_file(&session), "Pinned creation ZQ903.md");
    assert_eq!(tab_files(&session)[0], "Quick origin.md");
    assert!(pins(&session)[0]);
}

#[test]
fn nomes_aninhados_pastas_novas_e_unicode_criam_o_caminho_pedido() {
    let mut session = Session::open();
    std::fs::create_dir(session.path("Quick nested")).unwrap();
    refresh(&mut session, 3);
    quick_query(&mut session, "Quick nested/New child ZQ904.md");
    session.key(egui::Modifiers::NONE, egui::Key::Enter);
    wait_active_is(&mut session, "New child ZQ904.md");
    assert!(session.path("Quick nested/New child ZQ904.md").is_file());
    assert!(!session.path("Quick nested/New child ZQ904.md.md").exists());
    quick_query(&mut session, "Quick made/New deeper ZQ905");
    session.key(egui::Modifiers::NONE, egui::Key::Enter);
    wait_active_is(&mut session, "New deeper ZQ905.md");
    assert!(session.path("Quick made").is_dir());
    quick_query(&mut session, "Fresh café ZQ906");
    session.key(egui::Modifiers::NONE, egui::Key::Enter);
    wait_active_is(&mut session, "Fresh café ZQ906.md");
    assert_eq!(
        std::fs::read_to_string(session.path("Fresh café ZQ906.md")).unwrap(),
        ""
    );
}

#[test]
fn nota_existente_abre_sem_criar_nem_sobrescrever() {
    let mut session = Session::open();
    write_note(&session, "Existing target.md", "KEEP_EXISTING\n");
    refresh(&mut session, 4);
    let before = file_set(&session);
    quick_query(&mut session, "Existing target.md");
    assert!(
        !session.has_label("Criar nota"),
        "com acerto não se oferece criar"
    );
    session.key(egui::Modifiers::NONE, egui::Key::Enter);
    wait_active_is(&mut session, "Existing target.md");
    assert_eq!(file_set(&session), before);
    assert_eq!(
        std::fs::read_to_string(session.path("Existing target.md")).unwrap(),
        "KEEP_EXISTING\n"
    );
}

#[test]
fn caminhos_invalidos_falham_visivelmente_e_consultas_vazias_nao_criam_nada() {
    let mut session = Session::open();
    let before = file_set(&session);
    for value in ["../Outside ZQ907", ".hidden/New ZQ908", "vendor/New ZQ909"] {
        quick_query(&mut session, value);
        session.key(egui::Modifiers::NONE, egui::Key::Enter);
        assert!(
            session.app().error.is_some(),
            "{value}: o erro precisa aparecer"
        );
        assert_eq!(file_set(&session), before, "{value}");
        session.key(egui::Modifiers::NONE, egui::Key::Escape);
        session.app_mut().error = None;
    }
    // Caminho absoluto fora do cofre.
    let outside = tempfile::tempdir().unwrap();
    let absolute = outside.path().join("Outside ZQ907");
    quick_query(&mut session, &absolute.to_string_lossy());
    session.key(egui::Modifiers::NONE, egui::Key::Enter);
    assert!(session.app().error.is_some());
    assert!(!outside.path().join("Outside ZQ907.md").exists());
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    // Cancelar, vazio e só espaços não criam arquivos nem pastas.
    quick_query(&mut session, "Cancelled ZQ910");
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    quick_query(&mut session, "   ");
    assert!(!session.has_label("Criar nota"));
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    assert_eq!(file_set(&session), before);
}

// ---------------------------------------------------------------------------------------------
// quick-ranking
// ---------------------------------------------------------------------------------------------

fn ranked(session: &mut Session, query: &str) -> Vec<String> {
    session
        .app_mut()
        .files
        .rank(query, 100)
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

fn ranking_session() -> Session {
    let mut session = Session::open();
    for label in ["Closed A", "Closed B", "Closed C"] {
        write_note(
            &session,
            &format!("{label}.md"),
            &format!("# {label}\n\nORDERED_QUERY_FIXTURE\n"),
        );
    }
    write_note(
        &session,
        "Quick aliases.md",
        "---\naliases: [Thinking circle, یادداشت فارسی]\n---\n# Alias target\n",
    );
    refresh(&mut session, 7);
    session
}

#[test]
fn consultas_exatas_minusculas_e_abreviadas_devolvem_so_a_nota_pedida() {
    let mut session = ranking_session();
    assert_eq!(ranked(&mut session, "Closed C"), ["Closed C.md"]);
    assert_eq!(ranked(&mut session, "closed c"), ["Closed C.md"]);
    assert_eq!(ranked(&mut session, "Closed B"), ["Closed B.md"]);
    assert_eq!(ranked(&mut session, "ClsdC"), ["Closed C.md"]);
}

#[test]
fn apelidos_com_varias_palavras_e_unicode_resolvem_para_o_arquivo_canonico() {
    let mut session = ranking_session();
    assert_eq!(
        ranked(&mut session, "Thinking circle"),
        ["Quick aliases.md"]
    );
    assert_eq!(ranked(&mut session, "یادداشت فارسی"), ["Quick aliases.md"]);
}

#[test]
fn palavra_repetida_e_nome_inexistente_nao_dao_linhas_e_enter_cria_a_nota_vazia() {
    let mut session = ranking_session();
    assert!(ranked(&mut session, "Closed Closed").is_empty());
    assert!(ranked(&mut session, "No matching needle qzx").is_empty());
    quick_query(&mut session, "No matching needle qzx");
    session.key(egui::Modifiers::NONE, egui::Key::Enter);
    wait_active_is(&mut session, "No matching needle qzx.md");
    assert_eq!(
        std::fs::read_to_string(session.path("No matching needle qzx.md")).unwrap(),
        ""
    );
}

#[test]
fn consulta_vazia_lista_arquivos_e_cancelar_preserva_a_nota_atual_e_os_bytes() {
    let mut session = ranking_session();
    session.open_note("Closed A");
    let rows = ranked(&mut session, "");
    assert!((1..=100).contains(&rows.len()), "{rows:?}");
    session.key(CTRL, egui::Key::O);
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    assert_eq!(active_file(&session), "Closed A.md");
    for label in ["Closed A", "Closed B", "Closed C"] {
        assert_eq!(
            std::fs::read_to_string(session.path(&format!("{label}.md"))).unwrap(),
            format!("# {label}\n\nORDERED_QUERY_FIXTURE\n")
        );
    }
}
