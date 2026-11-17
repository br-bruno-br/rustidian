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

// ---------------------------------------------------------------------------------------------
// bookmarks
// ---------------------------------------------------------------------------------------------

fn bookmarked(session: &Session) -> Vec<String> {
    session
        .app()
        .bookmarks
        .items
        .iter()
        .map(|item| item.path.to_string_lossy().replace('\\', "/"))
        .collect()
}

/// O JSON da sessão gravada fora do cofre (favoritos, fixações, preferências).
fn saved_state(session: &Session) -> serde_json::Value {
    let root = session.app().vault.as_ref().unwrap().root.clone();
    let state = rustidian::paths::state_file(&root, "", "json").expect("arquivo de estado");
    serde_json::from_str(&std::fs::read_to_string(state).expect("sessão gravada"))
        .expect("sessão em JSON")
}

fn saved_bookmarks(session: &Session) -> Vec<String> {
    saved_state(session)["bookmarks"]["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| item["path"].as_str().unwrap_or_default().replace('\\', "/"))
                .collect()
        })
        .unwrap_or_default()
}

/// Preenche o diálogo aberto como o usuário: Ctrl+A, o texto, Enter.
fn fill_dialog(session: &mut Session, text: &str) {
    session.key(CTRL, egui::Key::A);
    session.type_text(text);
    session.key(egui::Modifiers::NONE, egui::Key::Enter);
}

#[test]
fn favoritar_a_nota_atual_grava_na_hora_e_repetir_nao_duplica() {
    let mut session = Session::open();
    session.open_note("Home");
    session.command("Source editing");
    session.command("Bookmark current file");
    session.command("Bookmark current file");
    assert_eq!(bookmarked(&session), ["Home.md"]);
    assert_eq!(
        saved_bookmarks(&session),
        ["Home.md"],
        "gravado fora do cofre, sem esperar"
    );
    assert_eq!(
        std::fs::read_to_string(session.path("Home.md")).unwrap(),
        HOME
    );
    // Remover o favorito não apaga a nota.
    let before = session.app().bookmarks.clone();
    session.app_mut().bookmarks.remove(Path::new("Home.md"));
    assert!(session.app_mut().save_bookmarks(before));
    assert!(saved_bookmarks(&session).is_empty());
    assert!(session.path("Home.md").is_file());
}

#[test]
fn titulo_do_favorito_guarda_texto_persa_e_a_ordem_persiste() {
    let mut session = Session::open();
    for name in ["Home", "Second", "Alpha"] {
        session.open_note(name);
        session.command("Bookmark current file");
    }
    assert_eq!(bookmarked(&session), ["Home.md", "Second.md", "Alpha.md"]);
    // Título pelo diálogo, como o item "Editar título" do menu do favorito.
    session.app_mut().ask(Dialog::BookmarkTitle);
    session.app_mut().dialog_path = Some(PathBuf::from("Second.md"));
    session.app_mut().dialog_input = String::new();
    session.settle();
    fill_dialog(&mut session, "Writing فارسی");
    assert_eq!(
        session.app().bookmarks.items[1].title.as_deref(),
        Some("Writing فارسی")
    );
    assert_eq!(
        saved_state(&session)["bookmarks"]["items"][1]["title"],
        "Writing فارسی"
    );
    // Subir o último (a ação "up" do menu) e conferir a ordem gravada.
    let before = session.app().bookmarks.clone();
    session
        .app_mut()
        .bookmarks
        .move_item(Path::new("Alpha.md"), -1);
    assert!(session.app_mut().save_bookmarks(before));
    assert_eq!(
        saved_bookmarks(&session),
        ["Home.md", "Alpha.md", "Second.md"]
    );
    assert_eq!(
        std::fs::read_to_string(session.path("Second.md")).unwrap(),
        SECOND
    );
}

#[test]
fn renomear_arquivo_e_pasta_atualiza_o_caminho_do_favorito_sem_perder_o_titulo() {
    let mut session = Session::open();
    write_note(
        &session,
        "Bookmark folder/Bookmark check.md",
        "# Bookmark check\n\nOriginal 🦀.\n",
    );
    refresh(&mut session, 4);
    session.open_note("Bookmark check");
    session.command("Bookmark current file");
    let before = session.app().bookmarks.clone();
    session
        .app_mut()
        .bookmarks
        .set_title(Path::new("Bookmark folder/Bookmark check.md"), "Writing");
    assert!(session.app_mut().save_bookmarks(before));
    // F2 renomeia a nota atual.
    session.key(egui::Modifiers::NONE, egui::Key::F2);
    fill_dialog(&mut session, "Bookmark folder/Bookmark renamed.md");
    session.wait_for(|app| app.dialog.is_none());
    assert!(
        session
            .path("Bookmark folder/Bookmark renamed.md")
            .is_file()
    );
    assert_eq!(
        bookmarked(&session),
        ["Bookmark folder/Bookmark renamed.md"]
    );
    assert_eq!(
        session.app().bookmarks.items[0].title.as_deref(),
        Some("Writing")
    );
    // Renomear a pasta leva o favorito junto.
    session
        .app_mut()
        .ask_file(Dialog::RenameNote, PathBuf::from("Bookmark folder"));
    session.settle();
    fill_dialog(&mut session, "Pinned notes");
    session.wait_for(|app| app.dialog.is_none());
    assert_eq!(bookmarked(&session), ["Pinned notes/Bookmark renamed.md"]);
    assert_eq!(
        session.app().bookmarks.items[0].title.as_deref(),
        Some("Writing")
    );
    session.app_mut().write_session().unwrap();
    assert_eq!(
        saved_bookmarks(&session),
        ["Pinned notes/Bookmark renamed.md"]
    );
}

#[test]
fn falha_ao_gravar_a_sessao_desfaz_o_favorito_e_a_nova_tentativa_grava() {
    let mut session = Session::open();
    session.open_note("Home");
    session.command("Bookmark current file");
    let root = session.app().vault.as_ref().unwrap().root.clone();
    let state = rustidian::paths::state_file(&root, "", "json").expect("estado");
    let held = state.with_extension("held");
    std::fs::rename(&state, &held).unwrap();
    std::fs::create_dir(&state).unwrap();
    session.open_note("Second");
    session.command("Bookmark current file");
    assert_eq!(
        bookmarked(&session),
        ["Home.md"],
        "o favorito novo foi desfeito"
    );
    assert!(
        session.app().error.is_some(),
        "a falha aparece para a pessoa"
    );
    std::fs::remove_dir(&state).unwrap();
    std::fs::rename(&held, &state).unwrap();
    session.command("Bookmark current file");
    assert_eq!(bookmarked(&session), ["Home.md", "Second.md"]);
    assert_eq!(saved_bookmarks(&session), ["Home.md", "Second.md"]);
}

// ---------------------------------------------------------------------------------------------
// workspaces
// ---------------------------------------------------------------------------------------------

fn workspace_names(session: &Session) -> Vec<String> {
    let root = session.app().vault.as_ref().unwrap().root.clone();
    rustidian::workspaces::Workspaces::open(&root)
        .expect("lista de layouts")
        .entries()
        .iter()
        .map(|entry| entry.name.clone())
        .collect()
}

fn save_layout(session: &mut Session, name: &str) {
    session.command("Workspaces: Manage workspace layouts");
    session.type_text(name);
    session.key(egui::Modifiers::NONE, egui::Key::Enter);
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
}

fn load_layout(session: &mut Session, name: &str) {
    session.command("Workspaces: Load workspace layout");
    session.type_text(name);
    session.key(egui::Modifiers::NONE, egui::Key::Enter);
    session.settle();
}

#[test]
fn cancelar_o_nome_nao_cria_layout() {
    let mut session = Session::open();
    session.open_note("Home");
    session.command("Workspaces: Manage workspace layouts");
    assert!(session.app().workspace_manager.open);
    session.type_text("Cancelled layout");
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    assert!(!session.app().workspace_manager.open);
    assert!(workspace_names(&session).is_empty());
}

#[test]
fn salvar_e_carregar_layouts_restaura_abas_modos_fixacao_e_barras() {
    let mut session = Session::open();
    write_note(
        &session,
        "Workspace A.md",
        "# Workspace A\n\nFirst saved layout.\n",
    );
    write_note(
        &session,
        "Workspace B.md",
        "# Workspace B\n\nSecond saved layout.\n",
    );
    refresh(&mut session, 5);
    session.open_note("Workspace A");
    session.command("Source editing");
    session.app_mut().show_left = true;
    session.app_mut().show_right = false;
    session.app_mut().sidebar_widths = [330.0, 234.0];
    session.command("Reading view");
    save_layout(&mut session, "Writing");
    assert_eq!(workspace_names(&session), ["Writing"]);
    // Segundo layout: duas abas, a B ativa em Source, a A fixada, barra esquerda recolhida.
    open_in_new_tab(&mut session, "Workspace B");
    session.command("Source editing");
    session.app_mut().tabs[0].pinned = true;
    session.app_mut().show_left = false;
    session.app_mut().show_right = true;
    session.app_mut().sidebar_widths = [330.0, 260.0];
    save_layout(&mut session, "Reading");
    assert_eq!(workspace_names(&session), ["Writing", "Reading"]);
    load_layout(&mut session, "Writing");
    assert_eq!(tab_files(&session), ["Workspace A.md"]);
    assert_eq!(active_view(&session), View::Read);
    assert!(session.app().show_left && !session.app().show_right);
    load_layout(&mut session, "Reading");
    assert_eq!(tab_files(&session), ["Workspace A.md", "Workspace B.md"]);
    assert_eq!(pins(&session), [true, false]);
    assert_eq!(active_file(&session), "Workspace B.md");
    assert_eq!(active_view(&session), View::Source);
    assert!(!session.app().show_left && session.app().show_right);
    // Carregar não mexe nos bytes das notas.
    assert_eq!(
        std::fs::read_to_string(session.path("Workspace A.md")).unwrap(),
        "# Workspace A\n\nFirst saved layout.\n"
    );
}

// ---------------------------------------------------------------------------------------------
// note-dialogs
// ---------------------------------------------------------------------------------------------

#[test]
fn nota_nova_nasce_vazia_o_titulo_selecionado_renomeia_e_enter_leva_ao_corpo() {
    let mut session = Session::open();
    session.key(CTRL, egui::Key::N);
    assert!(
        session.app().dialog.is_none(),
        "criar nota não abre um modal de nome"
    );
    let first = session.path("Untitled.md");
    assert!(first.is_file());
    assert_eq!(
        std::fs::read(&first).unwrap(),
        b"",
        "a nota nasce sem cabeçalho duplicado"
    );
    session.type_text("Quick thought");
    session.key(egui::Modifiers::NONE, egui::Key::Enter);
    let renamed = session.path("Quick thought.md");
    session.wait_for(|_| renamed.is_file() && !first.exists());
    session.type_text("Write immediately after naming.");
    session.key(CTRL, egui::Key::S);
    assert_eq!(
        std::fs::read_to_string(&renamed).unwrap(),
        "Write immediately after naming."
    );
}

#[test]
fn criar_nota_pelo_comando_usa_nomes_sem_colisao_e_nao_toca_nas_existentes() {
    let mut session = Session::open();
    session.key(CTRL, egui::Key::N);
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    session.command("New note");
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    session.command("New note");
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    for name in ["Untitled.md", "Untitled 1.md", "Untitled 2.md"] {
        assert_eq!(std::fs::read(session.path(name)).unwrap(), b"", "{name}");
    }
    assert!(session.app().dialog.is_none());
}

#[test]
fn formularios_padrao_abrem_cancelam_e_deixam_a_digitacao_intacta() {
    let mut session = Session::open();
    session.open_note("Second");
    session.command("Source editing");
    for name in [
        "New folder",
        "Rename or move file",
        "Save a copy",
        "Add property",
    ] {
        session.command(name);
        assert!(session.app().dialog.is_some(), "{name}: o diálogo abre");
        assert!(session.has_label("Cancelar"), "{name}: botão Cancelar");
        session.key(egui::Modifiers::NONE, egui::Key::Escape);
        assert!(session.app().dialog.is_none(), "{name}: Escape fecha");
        session.key(CTRL, egui::Key::End);
        session.type_text(" mais");
        session.key(CTRL, egui::Key::S);
        assert_eq!(
            std::fs::read_to_string(session.path("Second.md")).unwrap(),
            format!("{SECOND} mais")
        );
        session.key(CTRL, egui::Key::Z);
        session.key(CTRL, egui::Key::S);
        assert_eq!(
            std::fs::read_to_string(session.path("Second.md")).unwrap(),
            SECOND
        );
    }
}

#[test]
fn salvar_uma_copia_e_adicionar_propriedade_gravam_o_que_foi_pedido() {
    let mut session = Session::open();
    session.open_note("Second");
    session.command("Save a copy");
    fill_dialog(&mut session, "Copias/Second backup.md");
    session.wait_for(|app| app.dialog.is_none());
    assert_eq!(
        std::fs::read_to_string(session.path("Copias/Second backup.md")).unwrap(),
        SECOND
    );
    // Propriedade nova entra no cabeçalho da nota ativa (a cópia).
    session.command("Add property");
    session.type_text("status");
    session.key(egui::Modifiers::NONE, egui::Key::Enter);
    session.wait_for(|app| app.dialog.is_none());
    assert!(
        session.active_text().starts_with("---\nstatus:"),
        "{}",
        session.active_text()
    );
    // Repetir a mesma propriedade é recusado com mensagem.
    session.command("Add property");
    session.type_text("status");
    session.key(egui::Modifiers::NONE, egui::Key::Enter);
    session.wait_for(|app| app.error.is_some());
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    assert_eq!(
        std::fs::read_to_string(session.path("Second.md")).unwrap(),
        SECOND
    );
}

#[test]
fn dialogo_da_lixeira_mostra_o_alvo_e_cancelar_preserva_a_nota() {
    let mut session = Session::open();
    session.open_note("Second");
    session
        .app_mut()
        .ask_file(Dialog::TrashNote, PathBuf::from("Second.md"));
    session.settle();
    assert!(session.app().dialog == Some(Dialog::TrashNote));
    assert!(
        session.has_label("Second.md"),
        "o alvo aparece como rótulo, não como campo"
    );
    assert!(session.has_label("Não perguntar de novo"));
    click_label(&mut session, "Cancelar");
    assert!(session.app().dialog.is_none());
    assert_eq!(
        std::fs::read_to_string(session.path("Second.md")).unwrap(),
        SECOND
    );
    assert!(
        session.app().confirm_trash,
        "cancelar não grava a escolha de não perguntar"
    );
}

// ---------------------------------------------------------------------------------------------
// folders
// ---------------------------------------------------------------------------------------------

fn rename_dialog(session: &mut Session, from: &str, to: &str) {
    session
        .app_mut()
        .ask_file(Dialog::RenameNote, PathBuf::from(from));
    session.settle();
    fill_dialog(session, to);
}

fn md_files(session: &Session) -> Vec<String> {
    file_set(session)
        .into_iter()
        .filter(|name| name.ends_with(".md"))
        .collect()
}

#[test]
fn nova_pasta_aparece_sem_nota_e_repetir_o_nome_falha_sem_mexer_em_nada() {
    let mut session = Session::open();
    let notes = md_files(&session);
    session.command("New folder");
    fill_dialog(&mut session, "GUI empty");
    session.wait_for(|app| app.dialog.is_none());
    assert!(session.path("GUI empty").is_dir());
    assert_eq!(
        std::fs::read_dir(session.path("GUI empty"))
            .unwrap()
            .count(),
        0
    );
    assert!(
        session
            .app()
            .vault
            .as_ref()
            .unwrap()
            .folders
            .contains(&PathBuf::from("GUI empty"))
    );
    session.command("New folder");
    fill_dialog(&mut session, "GUI empty");
    session.settle();
    assert!(
        session.app().error.is_some(),
        "a pasta repetida é recusada em voz alta"
    );
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    assert_eq!(md_files(&session), notes);
}

#[test]
fn renomear_pasta_vazia_nao_ganha_extensao_md_nem_nota() {
    let mut session = Session::open();
    std::fs::create_dir(session.path("External.empty")).unwrap();
    session.command("Refresh vault");
    session.wait_for(|app| {
        app.loading.is_none()
            && app
                .vault
                .as_ref()
                .is_some_and(|vault| vault.folders.contains(&PathBuf::from("External.empty")))
    });
    rename_dialog(&mut session, "External.empty", "Renamed empty");
    session.wait_for(|app| app.dialog.is_none());
    assert!(!session.path("External.empty").exists());
    assert!(session.path("Renamed empty").is_dir());
    assert_eq!(
        std::fs::read_dir(session.path("Renamed empty"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn mover_pasta_aninhada_atualiza_ligacoes_abas_e_historico_e_recusa_destinos_inseguros() {
    let mut session = Session::open();
    let parent = "# Moving parent\n\n[Home](../Home.md)\n![[Move source/Nested/photo.png|64]]\n";
    let child = "# Moving child\n\n[[Move source/Moving parent|parent]]\n";
    let links = "# Folder links\n\n[[Move source/Nested/Moving child|child]]\n[Parent](Move%20source/Moving%20parent.md)\n";
    write_note(&session, "Move source/Moving parent.md", parent);
    write_note(&session, "Move source/Nested/Moving child.md", child);
    std::fs::create_dir_all(session.path("Move source/Empty")).unwrap();
    std::fs::create_dir_all(session.path("Move target")).unwrap();
    write_note(&session, "Folder links.md", links);
    let image: Vec<u8> = (0..=255u8).collect();
    std::fs::write(session.path("Move source/Nested/photo.png"), &image).unwrap();
    refresh(&mut session, 6);
    session.open_note("Moving child");
    session.command("Source editing");
    open_in_new_tab(&mut session, "Folder links");
    session.command("Source editing");
    // No Windows, renomear uma pasta com subpastas vigiadas falha com "Acesso negado" (achado
    // do harness, ver o teste ignorado abaixo). Aqui o vigia é solto para testar o resto.
    session.app_mut().watcher = None;
    session.app_mut().watch_events = None;
    rename_dialog(&mut session, "Move source", "Move target/Renamed");
    session.wait_for(|app| app.dialog.is_none());
    let moved = session.path("Move target/Renamed");
    assert!(!session.path("Move source").exists());
    assert!(moved.join("Empty").is_dir(), "pastas vazias vão junto");
    assert_eq!(
        std::fs::read(moved.join("Nested/photo.png")).unwrap(),
        image
    );
    let expected_child = child.replace("Move source/", "Move target/Renamed/");
    let expected_parent = parent
        .replace("../Home.md", "../../Home.md")
        .replace("Move source/", "Move target/Renamed/");
    let expected_links = links
        .replace("Move source/", "Move target/Renamed/")
        .replace("Move%20source/", "Move%20target/Renamed/");
    assert_eq!(
        std::fs::read_to_string(moved.join("Nested/Moving child.md")).unwrap(),
        expected_child
    );
    assert_eq!(
        std::fs::read_to_string(moved.join("Moving parent.md")).unwrap(),
        expected_parent
    );
    assert_eq!(
        std::fs::read_to_string(session.path("Folder links.md")).unwrap(),
        expected_links
    );
    // As abas seguem a pasta: editar e salvar grava no caminho novo, nas duas.
    session.key(CTRL, egui::Key::End);
    session.type_text(" AFTER_FOLDER_RENAME");
    session.key(CTRL, egui::Key::S);
    assert_eq!(
        std::fs::read_to_string(session.path("Folder links.md")).unwrap(),
        format!("{expected_links} AFTER_FOLDER_RENAME")
    );
    session.key(CTRL | egui::Modifiers::SHIFT, egui::Key::Tab);
    assert_eq!(active_file(&session), "Moving child.md");
    session.key(CTRL, egui::Key::End);
    session.type_text(" AFTER_MOVE");
    session.key(CTRL, egui::Key::S);
    assert_eq!(
        std::fs::read_to_string(moved.join("Nested/Moving child.md")).unwrap(),
        format!("{expected_child} AFTER_MOVE")
    );
    // Destinos inseguros: dentro de si mesma e com "#" no nome. A pasta fica onde está.
    for target in ["Move target/Renamed/Inside", "Move target/Bad#name"] {
        rename_dialog(&mut session, "Move target/Renamed", target);
        session.settle();
        assert!(session.app().error.is_some(), "{target}: recusado");
        assert!(moved.is_dir(), "{target}");
        session.key(egui::Modifiers::NONE, egui::Key::Escape);
        session.app_mut().error = None;
    }
    assert_eq!(
        std::fs::read_to_string(moved.join("Nested/Moving child.md")).unwrap(),
        format!("{expected_child} AFTER_MOVE")
    );
}

// ---------------------------------------------------------------------------------------------
// folder-create
// ---------------------------------------------------------------------------------------------

fn new_folder_in(session: &mut Session, folder: &str, name: &str) {
    session.app_mut().ask(Dialog::NewFolder);
    session.app_mut().dialog_path = Some(PathBuf::from(folder));
    session.app_mut().dialog_input = "Untitled".into();
    session.settle();
    fill_dialog(session, name);
}

fn folder_session() -> Session {
    let mut session = Session::open();
    std::fs::create_dir_all(session.path("Create here/Nested")).unwrap();
    std::fs::create_dir_all(session.path("Elsewhere")).unwrap();
    write_note(&session, "Elsewhere/Active.md", "ACTIVE_ELSEWHERE\n");
    write_note(&session, "Untitled.md", "KEEP_ROOT\n");
    write_note(&session, "Create here/Untitled.md", "KEEP_PARENT\n");
    write_note(&session, "Create here/Nested/Untitled.md", "KEEP_NESTED\n");
    refresh(&mut session, 7);
    session.open_note("Elsewhere/Active");
    session
}

#[test]
fn nova_nota_numa_pasta_fechada_cria_filha_unica_renomeia_no_lugar_e_salva() {
    let mut session = folder_session();
    session.app_mut().create_note_in(Path::new("Create here"));
    session.settle();
    assert_eq!(active_file(&session), "Untitled 1.md");
    assert!(session.path("Create here/Untitled 1.md").is_file());
    assert_eq!(
        std::fs::read(session.path("Create here/Untitled 1.md")).unwrap(),
        b""
    );
    session.type_text("Created child");
    session.key(egui::Modifiers::NONE, egui::Key::Enter);
    let renamed = session.path("Create here/Created child.md");
    session.wait_for(|_| renamed.is_file());
    assert!(!session.path("Create here/Untitled 1.md").exists());
    assert!(
        !session.path("Created child.md").exists(),
        "renomeia dentro da pasta, não na raiz"
    );
    session.type_text("CHILD_BODY");
    session.key(CTRL, egui::Key::S);
    assert_eq!(std::fs::read_to_string(&renamed).unwrap(), "CHILD_BODY");
    assert_eq!(
        std::fs::read_to_string(session.path("Elsewhere/Active.md")).unwrap(),
        "ACTIVE_ELSEWHERE\n"
    );
}

#[test]
fn nome_so_de_pasta_fica_preso_a_pasta_clicada_e_aceita_aninhadas_e_unicode() {
    let mut session = folder_session();
    new_folder_in(&mut session, "Create here", "Child folder");
    session.wait_for(|app| app.dialog.is_none());
    assert!(session.path("Create here/Child folder").is_dir());
    assert!(!session.path("Child folder").exists());
    assert!(!session.path("Elsewhere/Child folder").exists());
    new_folder_in(&mut session, "Create here/Nested", "Nested child");
    session.wait_for(|app| app.dialog.is_none());
    assert!(session.path("Create here/Nested/Nested child").is_dir());
    assert!(!session.path("Nested child").exists());
    new_folder_in(&mut session, "Create here", "Child café");
    session.wait_for(|app| app.dialog.is_none());
    assert!(session.path("Create here/Child café").is_dir());
    // A raiz do cofre vale para criar itens independentes da nota ativa.
    new_folder_in(&mut session, "", "Root child");
    session.wait_for(|app| app.dialog.is_none());
    assert!(session.path("Root child").is_dir());
    assert!(!session.path("Create here/Root child").exists());
    session.app_mut().create_note_in(Path::new(""));
    session.settle();
    assert!(session.path("Untitled 1.md").is_file());
}

#[test]
fn pasta_repetida_com_escape_ou_vazia_falha_sem_criar_caminhos() {
    let mut session = folder_session();
    new_folder_in(&mut session, "Create here", "Nested");
    session.settle();
    assert!(session.app().error.is_some(), "a pasta já existe");
    session.app_mut().error = None;
    fill_dialog(&mut session, "../Must not escape");
    session.settle();
    assert!(session.app().error.is_some());
    assert!(!session.path("Create here/Must not escape").exists());
    assert!(!session.path("Must not escape").exists());
    session.app_mut().error = None;
    fill_dialog(&mut session, "");
    session.settle();
    assert!(session.app().error.is_some());
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    let before = file_set(&session);
    new_folder_in_cancelled(&mut session);
    assert_eq!(file_set(&session), before);
    // As notas protegidas continuam com os bytes originais.
    for (path, text) in [
        ("Untitled.md", "KEEP_ROOT\n"),
        ("Create here/Untitled.md", "KEEP_PARENT\n"),
        ("Create here/Nested/Untitled.md", "KEEP_NESTED\n"),
    ] {
        assert_eq!(std::fs::read_to_string(session.path(path)).unwrap(), text);
    }
}

fn new_folder_in_cancelled(session: &mut Session) {
    session.app_mut().ask(Dialog::NewFolder);
    session.app_mut().dialog_path = Some(PathBuf::from("Create here"));
    session.app_mut().dialog_input = "Untitled".into();
    session.settle();
    session.type_text("Cancelled child");
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
}

// ---------------------------------------------------------------------------------------------
// file-copy
// ---------------------------------------------------------------------------------------------

/// PNG de 1x1 pixel.
const PNG: [u8; 67] = [
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

#[test]
fn fazer_uma_copia_abre_a_proxima_copia_numerada_numa_aba_nova_com_os_mesmos_bytes() {
    let mut session = Session::open();
    let source = b"# Copy source\r\n\r\nExact bytes stay here.\r\n";
    std::fs::write(session.path("Copy source.md"), source).unwrap();
    std::fs::write(session.path("Copy source 1.md"), b"KEEP_COPY_COLLISION\n").unwrap();
    refresh(&mut session, 5);
    session.open_note("Home");
    session
        .app_mut()
        .duplicate_file(Path::new("Copy source.md"));
    session.settle();
    assert_eq!(active_file(&session), "Copy source 2.md");
    assert!(session.app().dialog.is_none(), "copiar não pede nome");
    assert_eq!(
        std::fs::read(session.path("Copy source 2.md")).unwrap(),
        source
    );
    assert_eq!(
        std::fs::read(session.path("Copy source.md")).unwrap(),
        source
    );
    assert_eq!(
        std::fs::read(session.path("Copy source 1.md")).unwrap(),
        b"KEEP_COPY_COLLISION\n"
    );
    assert!(tab_files(&session).contains(&"Home.md".to_owned()));
    // O comando da paleta copia a cópia usando o nome completo; a aba fixada continua.
    session.command("Toggle pin");
    session.command("Make a copy of the current file");
    session.wait_for(|app| {
        app.tabs
            .get(app.active)
            .is_some_and(|tab| tab.document.path.ends_with("Copy source 2 1.md"))
    });
    assert_eq!(
        std::fs::read(session.path("Copy source 2 1.md")).unwrap(),
        source
    );
    assert!(tab_files(&session).contains(&"Copy source 2.md".to_owned()));
}

#[test]
fn copiar_imagem_binaria_preserva_os_bytes_e_sem_arquivo_o_comando_some() {
    let mut session = Session::open();
    std::fs::write(session.path("Copy image.png"), PNG).unwrap();
    refresh(&mut session, 3);
    session
        .app_mut()
        .duplicate_file(Path::new("Copy image.png"));
    session.settle();
    assert_eq!(active_file(&session), "Copy image 1.png");
    assert_eq!(
        std::fs::read(session.path("Copy image 1.png")).unwrap(),
        PNG
    );
    assert_eq!(std::fs::read(session.path("Copy image.png")).unwrap(), PNG);
    session.command("Make a copy of the current file");
    session.wait_for(|app| {
        app.tabs
            .get(app.active)
            .is_some_and(|tab| tab.document.path.ends_with("Copy image 1 1.png"))
    });
    assert_eq!(
        std::fs::read(session.path("Copy image 1 1.png")).unwrap(),
        PNG
    );
    // Numa aba em branco o comando não é oferecido.
    session.app_mut().tabs.clear();
    session.command("New tab");
    session.key(CTRL, egui::Key::P);
    session.type_text("Make a copy");
    assert!(!session.has_label("Make a copy of the current file"));
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
}

// ---------------------------------------------------------------------------------------------
// folder-copy
// ---------------------------------------------------------------------------------------------

/// Caminho relativo → bytes (arquivo) ou `None` (pasta), para comparar árvores inteiras.
fn snapshot(folder: &Path) -> std::collections::BTreeMap<String, Option<Vec<u8>>> {
    let mut out = std::collections::BTreeMap::new();
    for entry in walkdir::WalkDir::new(folder) {
        let entry = entry.unwrap();
        let relative = entry
            .path()
            .strip_prefix(folder)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        out.insert(
            relative,
            entry
                .file_type()
                .is_file()
                .then(|| std::fs::read(entry.path()).unwrap()),
        );
    }
    out
}

fn no_staging(session: &Session) {
    let leaked: Vec<_> = std::fs::read_dir(session.path(""))
        .unwrap()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".rustidian-copy-")
        })
        .collect();
    assert!(leaked.is_empty(), "sobrou pasta temporária de cópia");
}

#[test]
fn copiar_pasta_leva_notas_binarios_ocultos_e_vazias_e_so_salva_o_rascunho_da_origem() {
    let mut session = Session::open();
    let source = session.path("Recipe.v1 Ω");
    std::fs::create_dir_all(source.join("Nested")).unwrap();
    std::fs::create_dir_all(source.join("Empty")).unwrap();
    std::fs::create_dir_all(source.join(".hidden")).unwrap();
    std::fs::write(source.join(".hidden/secret.bin"), b"\x00PRIVATE\xff\x00").unwrap();
    std::fs::write(source.join("A.md"), "# A\n\n![[Image.png]]\n").unwrap();
    std::fs::write(
        source.join("Raw.md"),
        b"\xef\xbb\xbf# Raw\r\n\r\nRAW_CRLF_EXACT\r\n",
    )
    .unwrap();
    std::fs::write(source.join("Nested/Child.md"), "# Child\n\n[[../A|A]]\n").unwrap();
    let blob: Vec<u8> = (0..=255u8).cycle().take(1024).collect();
    std::fs::write(source.join("Nested/data.bin"), &blob).unwrap();
    std::fs::write(source.join("Image.png"), PNG).unwrap();
    std::fs::create_dir(session.path("Empty copy")).unwrap();
    std::fs::write(session.path("Empty copy 1"), "KEEP_OCCUPIED_FILE\n").unwrap();
    refresh(&mut session, 6);
    // Rascunho da pasta de origem + outro rascunho sem relação.
    session.open_note("Recipe.v1 Ω/Nested/Child");
    session.command("Source editing");
    session.key(CTRL, egui::Key::End);
    session.type_text("SOURCE_DRAFT");
    open_in_new_tab(&mut session, "Home");
    session.command("Source editing");
    session.key(CTRL, egui::Key::End);
    session.type_text("UNRELATED_DRAFT");
    assert_eq!(
        std::fs::read_to_string(session.path("Home.md")).unwrap(),
        HOME
    );
    session.app_mut().duplicate_file(Path::new("Recipe.v1 Ω"));
    session.settle();
    if !cfg!(unix) {
        // A cópia protegida de pastas só existe no Linux: fora dele o app diz isso e não deixa
        // cópia parcial nem pasta temporária.
        let error = session.app().error.clone().unwrap_or_default();
        assert!(error.contains("só está disponível no Linux"), "{error}");
        assert!(!session.path("Recipe.v1 Ω 1").exists());
        no_staging(&session);
        assert_eq!(snapshot(&source).len(), 10);
        return;
    }
    assert!(
        std::fs::read_to_string(source.join("Nested/Child.md"))
            .unwrap()
            .ends_with("SOURCE_DRAFT"),
        "o rascunho da origem foi salvo antes de copiar"
    );
    assert_eq!(
        std::fs::read_to_string(session.path("Home.md")).unwrap(),
        HOME,
        "o rascunho alheio continua só na memória"
    );
    let first = session.path("Recipe.v1 Ω 1");
    assert_eq!(snapshot(&first), snapshot(&source));
    assert_eq!(
        active_file(&session),
        "Home.md",
        "copiar pasta não troca a aba"
    );
    no_staging(&session);
    // Repetir numera de novo mantendo o nome completo com ponto e Unicode.
    session.app_mut().duplicate_file(Path::new("Recipe.v1 Ω"));
    assert_eq!(snapshot(&session.path("Recipe.v1 Ω 2")), snapshot(&source));
    // Pasta vazia: cópia vazia, e o arquivo que ocupa "Empty copy 1" é pulado, intacto.
    session.app_mut().duplicate_file(Path::new("Empty copy"));
    assert!(session.path("Empty copy 2").is_dir());
    assert_eq!(
        std::fs::read_dir(session.path("Empty copy 2"))
            .unwrap()
            .count(),
        0
    );
    assert_eq!(
        std::fs::read_to_string(session.path("Empty copy 1")).unwrap(),
        "KEEP_OCCUPIED_FILE\n"
    );
    no_staging(&session);
}

// ---------------------------------------------------------------------------------------------
// file-move
// ---------------------------------------------------------------------------------------------

fn move_to(session: &mut Session, folder: &str, modifiers: egui::Modifiers) {
    session.command("Move current file to another folder");
    session.type_text(folder);
    session.key(modifiers, egui::Key::Enter);
}

#[test]
fn mover_arquivo_cancelar_no_seletor_nao_muda_nada_e_mover_preserva_bytes_crlf() {
    let mut session = Session::open();
    let bytes = b"# Move source\r\n\r\nKeep this document.\r\n";
    std::fs::write(session.path("Move source.md"), bytes).unwrap();
    std::fs::create_dir(session.path("Move destination")).unwrap();
    refresh(&mut session, 4);
    session.open_note("Move source");
    session.command("Move current file to another folder");
    assert!(session.app().file_move.active());
    session.type_text("Move destination");
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    assert!(!session.app().file_move.active());
    assert_eq!(
        std::fs::read(session.path("Move source.md")).unwrap(),
        bytes
    );
    assert_eq!(
        std::fs::read_dir(session.path("Move destination"))
            .unwrap()
            .count(),
        0
    );
    move_to(&mut session, "Move destination", egui::Modifiers::NONE);
    session.wait_for(|app| !app.file_move.active());
    assert!(!session.path("Move source.md").exists());
    assert_eq!(
        std::fs::read(session.path("Move destination/Move source.md")).unwrap(),
        bytes
    );
    assert!(
        session.app().tabs[session.app().active]
            .document
            .path
            .ends_with("Move destination/Move source.md"),
        "a aba acompanha o arquivo movido"
    );
}

#[test]
fn mover_cria_pasta_aninhada_e_a_escolha_do_dialogo_de_ligacoes_vale_para_cada_caso() {
    let mut session = Session::open();
    let bytes = b"# Move source\r\n\r\nKeep this document. BEFOREMOVE\r\n";
    write_note(
        &session,
        "Move destination/Placeholder.md",
        "# Placeholder\n",
    );
    std::fs::write(session.path("Move destination/Move source.md"), bytes).unwrap();
    // Ligações com o caminho completo: só elas precisam de reescrita quando o arquivo muda de pasta.
    let reference = b"[[Move destination/Move source|Alias]]\r\n[Label](Move%20destination/Move%20source.md)\r\n";
    std::fs::write(session.path("Move reference.md"), reference).unwrap();
    refresh(&mut session, 6);
    session.open_note("Move destination/Move source");
    // Enter simples cria a pasta oferecida; "Só desta vez" reescreve os dois formatos.
    move_to(&mut session, "Created/Nested", egui::Modifiers::NONE);
    session.wait_for(|app| app.file_move.active());
    assert!(session.path("Created/Nested/Move source.md").is_file());
    assert_eq!(
        std::fs::read(session.path("Move reference.md")).unwrap(),
        reference
    );
    assert!(session.has_label("Atualizar links"));
    click_label(&mut session, "Só desta vez");
    let updated =
        b"[[Created/Nested/Move source|Alias]]\r\n[Label](Created/Nested/Move%20source.md)\r\n";
    assert_eq!(
        std::fs::read(session.path("Move reference.md")).unwrap(),
        updated
    );
    assert_eq!(
        std::fs::read(session.path("Created/Nested/Move source.md")).unwrap(),
        bytes
    );
    // De volta à raiz com "Não atualizar": a referência fica como está.
    move_to(&mut session, "/", egui::Modifiers::NONE);
    session.wait_for(|app| app.file_move.active());
    assert!(session.path("Move source.md").is_file());
    click_label(&mut session, "Não atualizar");
    assert_eq!(
        std::fs::read(session.path("Move reference.md")).unwrap(),
        updated
    );
    assert_eq!(
        std::fs::read(session.path("Move source.md")).unwrap(),
        bytes
    );
    assert_eq!(active_file(&session), "Move source.md");
}

#[test]
fn sempre_atualizar_persiste_e_o_proximo_movimento_nao_pergunta() {
    let mut session = Session::open();
    write_note(
        &session,
        "Move destination/Move source.md",
        "# Move source\n",
    );
    std::fs::write(
        session.path("Move reference.md"),
        "[[Move destination/Move source|Alias]]\n",
    )
    .unwrap();
    refresh(&mut session, 5);
    session.open_note("Move destination/Move source");
    move_to(&mut session, "/", egui::Modifiers::NONE);
    session.wait_for(|app| app.file_move.active());
    click_label(&mut session, "Sempre atualizar");
    assert!(session.app().always_update_links);
    assert_eq!(saved_state(&session)["always_update_links"], true);
    assert_eq!(
        std::fs::read_to_string(session.path("Move reference.md")).unwrap(),
        "[[Move source|Alias]]\n"
    );
    // O destino já tem um "Move source": o arquivo ganha número e a ligação precisa mudar.
    // Com "sempre atualizar" ligado isso acontece sem diálogo.
    std::fs::write(
        session.path("Move destination/Move source.md"),
        "KEEP_OCCUPIED\n",
    )
    .unwrap();
    refresh(&mut session, 6);
    move_to(&mut session, "Move destination", egui::Modifiers::NONE);
    session.settle();
    session.wait_for(|app| !app.file_move.active());
    assert!(session.path("Move destination/Move source 1.md").is_file());
    assert_eq!(
        std::fs::read_to_string(session.path("Move destination/Move source.md")).unwrap(),
        "KEEP_OCCUPIED\n"
    );
    assert_eq!(
        std::fs::read_to_string(session.path("Move reference.md")).unwrap(),
        "[[Move destination/Move source 1|Alias]]\n"
    );
}

#[test]
fn destino_inseguro_fica_como_erro_no_seletor_e_nao_move_nem_escapa_do_cofre() {
    let mut session = Session::open();
    std::fs::write(session.path("Move source.md"), "# Move source\n").unwrap();
    refresh(&mut session, 4);
    session.open_note("Move source");
    let before = file_set(&session);
    move_to(&mut session, "../Move outside", egui::Modifiers::SHIFT);
    session.settle();
    assert!(
        session.app().file_move.active(),
        "o seletor continua aberto com o erro"
    );
    assert_eq!(file_set(&session), before);
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    assert!(session.path("Move source.md").is_file());
}

// ---------------------------------------------------------------------------------------------
// trash-options
// ---------------------------------------------------------------------------------------------

#[test]
fn lixeira_local_do_cofre_guarda_os_bytes_e_a_lixeira_do_sistema_nunca_e_usada() {
    // O app só manda para a Lixeira do SISTEMA quando a sincronização está desligada, e este
    // teste jamais confirma essa ação. Aqui só a lixeira do próprio cofre (`.trash/`).
    let mut session = Session::open();
    std::fs::write(
        session.path("Trash first.md"),
        "# Contextual deletion\n\nKeep this text.\n",
    )
    .unwrap();
    refresh(&mut session, 4);
    session
        .app_mut()
        .vault
        .as_mut()
        .unwrap()
        .trash_entry_in_vault(Path::new("Trash first.md"))
        .expect("lixeira do cofre");
    assert!(!session.path("Trash first.md").exists());
    let kept: Vec<_> = walkdir::WalkDir::new(session.path(".trash"))
        .into_iter()
        .flatten()
        .filter(|entry| entry.file_type().is_file())
        .collect();
    assert_eq!(kept.len(), 1);
    assert_eq!(
        std::fs::read(kept[0].path()).unwrap(),
        b"# Contextual deletion\n\nKeep this text.\n"
    );
}

#[test]
fn letra_d_digitada_no_editor_e_texto_comum_e_nunca_apaga_arquivo() {
    let mut session = Session::open();
    std::fs::write(
        session.path("Trash first.md"),
        "# Contextual deletion\n\nKeep this text.\n",
    )
    .unwrap();
    refresh(&mut session, 4);
    session.open_note("Trash first");
    session.command("Source editing");
    session.key(CTRL, egui::Key::End);
    session.type_text("d");
    session.key(CTRL, egui::Key::S);
    assert_eq!(
        std::fs::read_to_string(session.path("Trash first.md")).unwrap(),
        "# Contextual deletion\n\nKeep this text.\nd"
    );
    // Abrir e cancelar o diálogo de lixeira com a confirmação ligada não grava preferência.
    session
        .app_mut()
        .ask_file(Dialog::TrashNote, PathBuf::from("Trash first.md"));
    session.settle();
    assert!(session.app().dialog.is_some());
    session.app_mut().dialog_skip_trash = true;
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    assert!(session.app().dialog.is_none());
    assert!(
        session.app().confirm_trash,
        "cancelar não grava 'não perguntar de novo'"
    );
    assert!(session.path("Trash first.md").is_file());
}

// ---------------------------------------------------------------------------------------------
// explorer
// ---------------------------------------------------------------------------------------------

#[test]
fn explorador_com_milhares_de_notas_lista_so_as_visiveis_e_abre_a_ultima() {
    let mut session = Session::open();
    for number in 0..3000 {
        write_note(
            &session,
            &format!("Explorer bulk/Note{number:05}.md"),
            &format!("# Note{number:05}\n\nShort content.\n"),
        );
    }
    refresh(&mut session, 3003);
    assert_eq!(session.app().vault.as_ref().unwrap().notes.len(), 3003);
    // A lista é virtual: nunca desenha as três mil linhas de uma vez.
    let drawn = session.harness.query_all_by_label_contains("Note0").count();
    assert!(drawn < 400, "{drawn} linhas desenhadas");
    // Abrir a última pelo Abrir rápido a revela no explorador.
    session.open_note("Note02999");
    assert_eq!(active_file(&session), "Note02999.md");
    assert_eq!(session.active_text(), "# Note02999\n\nShort content.\n");
}

#[test]
#[ignore = "achado do harness: no Windows renomear/mover pasta com subpastas falha com 'Acesso negado' enquanto o vigia de arquivos está ligado; o app não deve ser alterado nesta fatia"]
fn renomear_pasta_com_subpastas_com_o_vigia_ligado() {
    let mut session = Session::open();
    write_note(
        &session,
        "Move source/Nested/Moving child.md",
        "# Moving child\n",
    );
    std::fs::create_dir_all(session.path("Move target")).unwrap();
    refresh(&mut session, 4);
    rename_dialog(&mut session, "Move source", "Move target/Renamed");
    session.wait_for(|app| app.dialog.is_none());
    assert!(
        session
            .path("Move target/Renamed/Nested/Moving child.md")
            .is_file()
    );
    assert!(!session.path("Move source").exists());
}

// ---------------------------------------------------------------------------------------------
// palettes
// ---------------------------------------------------------------------------------------------

#[test]
fn as_quatro_paletas_aplicam_cores_gravam_a_escolha_e_valem_para_o_app_inteiro() {
    use crate::theme::palette::{self, Palette};
    let mut session = Session::open();
    session.open_note("Second");
    let expected = [
        (Palette::Teal, "teal", (23, 27, 29), (247, 246, 242)),
        (
            Palette::LoveSpace,
            "love-space",
            (29, 23, 27),
            (251, 245, 245),
        ),
        (
            Palette::ElpisGold,
            "elpis-gold",
            (28, 27, 23),
            (251, 248, 239),
        ),
        (
            Palette::Signature,
            "signature",
            (31, 31, 40),
            (252, 245, 242),
        ),
    ];
    for (selected, name, dark_rgb, light_rgb) in expected {
        for (dark, rgb) in [(true, dark_rgb), (false, light_rgb)] {
            let color = selected.background(dark);
            assert_eq!((color.r(), color.g(), color.b()), rgb, "{name} dark={dark}");
        }
        // O mesmo que a página Aparência faz ao trocar de cartão.
        let ctx = session.app().ctx.clone();
        palette::select(&ctx, selected);
        session.app_mut().palette = selected;
        palette::remember(selected);
        session.app_mut().write_session().expect("sessão gravada");
        session.settle();
        assert!(palette::current(&ctx) == selected);
        assert_eq!(saved_state(&session)["palette"], name);
        assert!(
            palette::app_wide() == selected,
            "vale para qualquer cofre aberto depois"
        );
    }
    assert_eq!(
        std::fs::read_to_string(session.path("Second.md")).unwrap(),
        SECOND
    );
    // Volta ao padrão para não vazar a escolha para os outros testes do processo.
    let ctx = session.app().ctx.clone();
    palette::select(&ctx, Palette::Teal);
    palette::remember(Palette::Teal);
    assert!(palette::app_wide() == Palette::Teal);
}

#[test]
fn a_pagina_de_configuracoes_abre_e_fecha_sem_tocar_na_nota() {
    let mut session = Session::open();
    session.open_note("Second");
    session.key(CTRL, egui::Key::Comma);
    assert!(session.app().dialog == Some(Dialog::Settings));
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    assert!(session.app().dialog.is_none());
    session.command("Open settings");
    assert!(session.app().dialog == Some(Dialog::Settings));
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    assert_eq!(session.active_text(), SECOND);
    assert_eq!(
        std::fs::read_to_string(session.path("Second.md")).unwrap(),
        SECOND
    );
}

// ---------------------------------------------------------------------------------------------
// settings
// ---------------------------------------------------------------------------------------------

#[test]
fn preferencias_do_editor_definem_o_modo_das_abas_novas_e_sao_gravadas() {
    let mut session = Session::open();
    session.open_note("Second");
    session.command("Reading view");
    {
        let app = session.app_mut();
        app.editor_preferences.reading_view = true;
        app.editor_preferences.source_mode = true;
        app.editor_preferences.inline_title = false;
        app.editor_preferences.readable_line_length = false;
        app.settings_saved();
        app.write_session().expect("sessão gravada");
    }
    let preferences = &saved_state(&session)["editor_preferences"];
    assert_eq!(preferences["reading_view"], true);
    assert_eq!(preferences["source_mode"], true);
    assert_eq!(preferences["inline_title"], false);
    assert_eq!(preferences["readable_line_length"], false);
    // Aba nova abre em Leitura; Ctrl+E volta ao modo de edição escolhido (Source).
    open_in_new_tab(&mut session, "Home");
    assert_eq!(active_view(&session), View::Read);
    session.key(CTRL, egui::Key::E);
    assert_eq!(active_view(&session), View::Source);
    session.app_mut().editor_preferences.show_mode = false;
    session.app_mut().write_session().unwrap();
    assert_eq!(
        saved_state(&session)["editor_preferences"]["show_mode"],
        false
    );
    assert_eq!(
        std::fs::read_to_string(session.path("Second.md")).unwrap(),
        SECOND
    );
}

#[test]
fn fonte_do_documento_para_nos_limites_10_e_30_e_a_gravacao_que_falha_desfaz() {
    let mut session = Session::open();
    session.open_note("Second");
    session.app_mut().set_document_font_size(29);
    session.command("Increase font size");
    assert_eq!(session.app().document_font_size, 30);
    session.command("Increase font size");
    assert_eq!(session.app().document_font_size, 30, "30 é o teto");
    assert_eq!(saved_state(&session)["document_font_size"], 30);
    session.app_mut().set_document_font_size(11);
    session.command("Decrease font size");
    assert_eq!(session.app().document_font_size, 10);
    session.command("Decrease font size");
    assert_eq!(session.app().document_font_size, 10, "10 é o piso");
    // Sem poder gravar o estado, o tamanho volta ao anterior e o erro aparece.
    let root = session.app().vault.as_ref().unwrap().root.clone();
    let state = rustidian::paths::state_file(&root, "", "json").expect("estado");
    let held = state.with_extension("held");
    std::fs::rename(&state, &held).unwrap();
    std::fs::create_dir(&state).unwrap();
    session.command("Increase font size");
    assert_eq!(session.app().document_font_size, 10);
    assert!(
        session
            .app()
            .error
            .as_deref()
            .is_some_and(|error| error.starts_with("As configurações não foram salvas"))
    );
    std::fs::remove_dir(&state).unwrap();
    std::fs::rename(&held, &state).unwrap();
    session.command("Increase font size");
    assert_eq!(session.app().document_font_size, 11);
    assert_eq!(saved_state(&session)["document_font_size"], 11);
    assert_eq!(
        std::fs::read_to_string(session.path("Second.md")).unwrap(),
        SECOND
    );
    session.app_mut().set_document_font_size(18);
}

#[test]
fn aparencia_clara_e_escura_alternam_e_ficam_gravadas() {
    let mut session = Session::open();
    session.open_note("Second");
    let before = session.app().dark;
    session.command("Switch appearance");
    assert_eq!(session.app().dark, !before);
    session.app_mut().write_session().unwrap();
    assert_eq!(saved_state(&session)["dark"], !before);
    session.command("Switch appearance");
    assert_eq!(session.app().dark, before);
    // O app guarda a escolha para todos os cofres; devolve o padrão ao fim.
    crate::theme::palette::remember_appearance(crate::theme::Appearance::System);
    assert_eq!(
        std::fs::read_to_string(session.path("Second.md")).unwrap(),
        SECOND
    );
}

// ---------------------------------------------------------------------------------------------
// recovery
// ---------------------------------------------------------------------------------------------

fn history_versions(session: &Session, path: &str) -> Vec<rustidian::recovery::Version> {
    session
        .app()
        .recovery_history
        .as_ref()
        .and_then(|history| history.versions(Path::new(path)).ok())
        .unwrap_or_default()
}

fn wait_versions(session: &mut Session, path: &str, count: usize) {
    for _ in 0..800 {
        session.harness.step();
        if history_versions(session, path).len() >= count {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("o histórico de {path} não chegou a {count} versões");
}

const RECOVERY_ORIGINAL: &[u8] = "# Original\r\n\r\nسلام\r\n- [x] Keep this\r\n".as_bytes();

fn recovery_session() -> Session {
    let mut session = Session::open();
    std::fs::write(session.path("Recovery target.md"), RECOVERY_ORIGINAL).unwrap();
    refresh(&mut session, 4);
    session.app_mut().recovery_config.interval_minutes = 0.0;
    session.open_note("Recovery target");
    session.command("Source editing");
    wait_versions(&mut session, "Recovery target.md", 1);
    session
}

#[test]
fn historico_local_guarda_a_versao_inicial_e_cada_salvamento_com_os_bytes_exatos() {
    let mut session = recovery_session();
    let history = session.app().recovery_history.clone().unwrap();
    let first = history_versions(&session, "Recovery target.md")
        .last()
        .unwrap()
        .id;
    assert_eq!(
        history
            .content(Path::new("Recovery target.md"), first)
            .unwrap()
            .as_bytes(),
        RECOVERY_ORIGINAL,
        "CRLF e persa preservados"
    );
    // O intervalo da configuração entra na sessão gravada.
    session.app_mut().write_session().unwrap();
    assert_eq!(
        saved_state(&session)["recovery_config"]["intervalMinutes"],
        0.0
    );
    // Uma versão nova depois de salvar um texto diferente.
    session.key(CTRL, egui::Key::A);
    session.type_text("# Changed\n\nsalvo\n");
    session.key(CTRL, egui::Key::S);
    wait_versions(&mut session, "Recovery target.md", 2);
    let versions = history_versions(&session, "Recovery target.md");
    assert_eq!(
        history
            .content(Path::new("Recovery target.md"), versions[0].id)
            .unwrap(),
        "# Changed\n\nsalvo\n"
    );
    // O diálogo do histórico abre pelo comando e mostra Restaurar e Copiar.
    session.command("File recovery: Open local history");
    session.wait_for(|app| app.recovery_manager.active());
    session.wait_for(|app| app.dialog.is_none());
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
}

fn restore_request(session: &Session, version: u64) -> crate::recovery_ui::Restore {
    crate::recovery_ui::Restore {
        expected: session.app().tabs[session.app().active].document.clone(),
        version,
    }
}

#[test]
fn restaurar_uma_versao_devolve_os_bytes_exatos_e_guarda_a_versao_deslocada() {
    let mut session = recovery_session();
    let first = history_versions(&session, "Recovery target.md")
        .last()
        .unwrap()
        .id;
    session.key(CTRL, egui::Key::A);
    session.type_text("# Changed\n\nsalvo\n");
    session.key(CTRL, egui::Key::S);
    wait_versions(&mut session, "Recovery target.md", 2);
    let request = restore_request(&session, first);
    session.app_mut().restore_snapshot(request);
    session.settle();
    assert_eq!(
        std::fs::read(session.path("Recovery target.md")).unwrap(),
        RECOVERY_ORIGINAL
    );
    assert_eq!(session.active_text().as_bytes(), RECOVERY_ORIGINAL);
    // A versão que foi trocada continua no histórico.
    wait_versions(&mut session, "Recovery target.md", 3);
    let history = session.app().recovery_history.clone().unwrap();
    let all = history_versions(&session, "Recovery target.md");
    assert!(all.iter().any(|version| {
        history
            .content(Path::new("Recovery target.md"), version.id)
            .is_ok_and(|text| text == "# Changed\n\nsalvo\n")
    }));
}

#[test]
fn nota_alterada_fora_do_dialogo_recusa_restaurar_e_preserva_a_substituta() {
    let mut session = recovery_session();
    let first = history_versions(&session, "Recovery target.md")
        .last()
        .unwrap()
        .id;
    let request = restore_request(&session, first);
    std::fs::write(session.path("Recovery target.md"), b"# EXTERNAL_RECOVERY\n").unwrap();
    session.app_mut().restore_snapshot(request);
    session.settle();
    assert!(
        session.app().error.is_some(),
        "a recusa aparece para a pessoa"
    );
    assert_eq!(
        std::fs::read(session.path("Recovery target.md")).unwrap(),
        b"# EXTERNAL_RECOVERY\n"
    );
}

#[test]
fn restaurar_com_edicoes_nao_salvas_pergunta_cancelar_mantem_e_salvar_guarda_no_historico() {
    let mut session = recovery_session();
    // Sem salvar automático, para o texto ficar mesmo só na memória.
    session.app_mut().autosave_enabled = false;
    let first = history_versions(&session, "Recovery target.md")
        .last()
        .unwrap()
        .id;
    session.key(CTRL, egui::Key::A);
    session.type_text("UNSAVED_RECOVERY\n");
    assert_eq!(session.active_text(), "UNSAVED_RECOVERY\n");
    let request = restore_request(&session, first);
    session.app_mut().pending = Some(Pending::RestoreSnapshot(Box::new(request)));
    session.settle();
    click_label(&mut session, "Cancelar");
    assert!(!has_pending(&session));
    assert_eq!(
        session.active_text(),
        "UNSAVED_RECOVERY\n",
        "o buffer não salvo continua"
    );
    assert_eq!(
        std::fs::read(session.path("Recovery target.md")).unwrap(),
        RECOVERY_ORIGINAL
    );
    // "Salvar e continuar" grava o texto antes de restaurar: ele fica no histórico.
    let request = restore_request(&session, first);
    session.app_mut().pending = Some(Pending::RestoreSnapshot(Box::new(request)));
    session.settle();
    click_label(&mut session, "Salvar e continuar");
    session.wait_for(|app| app.pending.is_none());
    assert_eq!(
        std::fs::read(session.path("Recovery target.md")).unwrap(),
        RECOVERY_ORIGINAL
    );
    wait_versions(&mut session, "Recovery target.md", 3);
    let history = session.app().recovery_history.clone().unwrap();
    assert!(
        history_versions(&session, "Recovery target.md")
            .iter()
            .any(|version| history
                .content(Path::new("Recovery target.md"), version.id)
                .is_ok_and(|text| text == "UNSAVED_RECOVERY\n"))
    );
}

// ---------------------------------------------------------------------------------------------
// backlink-context
// ---------------------------------------------------------------------------------------------

struct BacklinkFixture {
    session: Session,
    source_text: String,
    expected: Vec<(String, usize, String)>,
}

fn backlink_fixture() -> BacklinkFixture {
    let mut session = Session::open();
    let alias = "[[Backlink Target#Section|alias]]";
    let same = "[[Backlink Target]]";
    let markdown = "[markdown alias](../Backlink%20Target.md#Section)";
    let embed = "[[Backlink Target#^id]]";
    let image = "![note image](../Backlink%20Target.md)";
    write_note(
        &session,
        "Backlink Target.md",
        "# Backlink Target\n\n## Section\n\nTARGET BODY ^id\n\n[[Backlink Target]]\n",
    );
    let source_text = format!(
        "---\ntitle: \"Café 👋\"\n---\n# Context\n\n{}CAFÉ 👋 BEFORE {alias} CAFÉ AFTER\n\nSAME ONE {same} MIDDLE {same} SAME END\n\n{markdown}\n\n!{embed}\n\n{image}\n\n```md\n[[Backlink Target]]\n[code](../Backlink%20Target.md)\n```\n\n`[[Backlink Target]]` and ` [inline](../Backlink%20Target.md) `\n\n\\[[Backlink Target]]\n\n$[[Backlink Target]]$\n\n$[math](../Backlink%20Target.md)$\n",
        "Padding before actual reference.\n".repeat(82)
    );
    write_note(&session, "Backlink Sources/Context.md", &source_text);
    let other_link = "[[Backlink Target|other alias]]";
    let other_text = format!(
        "# Other\n\n{}{other_link} DIFFERENT CONTEXT\n",
        "長い café prefix ".repeat(10)
    );
    write_note(&session, "Backlink Other/Context.md", &other_text);
    let windows_link = "[reference label][target]";
    let windows_text = format!(
        "# Windows\r\n\r\nUnicode café 👋\r\nCRLF BEFORE {windows_link} CRLF AFTER\r\n\r\n[target]: ../Backlink%20Target.md#Section\r\n"
    );
    std::fs::create_dir_all(session.path("Backlink Sources")).unwrap();
    std::fs::write(
        session.path("Backlink Sources/Windows.md"),
        windows_text.as_bytes(),
    )
    .unwrap();
    refresh(&mut session, 7);
    let mut expected = Vec::new();
    for (raw, occurrence) in [
        (alias, 0),
        (same, 0),
        (same, 1),
        (markdown, 0),
        (embed, 0),
        (image, 0),
    ] {
        let position = source_text
            .match_indices(raw)
            .map(|(index, _)| index)
            .nth(occurrence)
            .unwrap();
        expected.push((
            "Backlink Sources/Context.md".to_owned(),
            position,
            raw.to_owned(),
        ));
    }
    expected.push((
        "Backlink Other/Context.md".to_owned(),
        other_text.find(other_link).unwrap(),
        other_link.to_owned(),
    ));
    expected.push((
        "Backlink Sources/Windows.md".to_owned(),
        windows_text.find(windows_link).unwrap(),
        windows_link.to_owned(),
    ));
    BacklinkFixture {
        session,
        source_text,
        expected,
    }
}

fn mention_key(mention: &rustidian::vault::LinkedMention) -> (String, usize, String) {
    (
        mention.path.to_string_lossy().replace('\\', "/"),
        mention.range.start,
        mention.link.clone(),
    )
}

#[test]
fn mencoes_de_entrada_trazem_o_intervalo_exato_em_bytes_e_ignoram_codigo_escapes_e_matematica() {
    let fixture = backlink_fixture();
    let vault = fixture.session.app().vault.as_ref().unwrap();
    let mut mentions: Vec<_> = vault
        .backlink_mentions(Path::new("Backlink Target.md"), &[])
        .iter()
        .map(mention_key)
        .collect();
    mentions.retain(|(path, _, _)| !path.ends_with("Backlink Target.md"));
    let mut expected = fixture.expected.clone();
    expected.sort();
    mentions.sort();
    assert_eq!(mentions.len(), 8, "só as oito menções reais: {mentions:?}");
    assert_eq!(mentions, expected);
    // O intervalo cobre exatamente os bytes da ligação (frontmatter, Unicode e CRLF inclusive).
    for (path, start, raw) in &expected {
        let text = std::fs::read_to_string(fixture.session.path(path)).unwrap();
        assert_eq!(&text[*start..*start + raw.len()], raw, "{path}");
    }
}

#[test]
fn abrir_uma_mencao_seleciona_a_ocorrencia_exata_e_a_aba_fixada_do_alvo_e_poupada() {
    let mut fixture = backlink_fixture();
    let session = &mut fixture.session;
    session.open_note("Backlink Target");
    session.command("Toggle pin");
    let target_id = session.app().tabs[0].id();
    let mentions = session
        .app()
        .vault
        .as_ref()
        .unwrap()
        .backlink_mentions(Path::new("Backlink Target.md"), &[]);
    let mention = mentions
        .iter()
        .find(|mention| mention.link == "[[Backlink Target#Section|alias]]")
        .cloned()
        .expect("menção com apelido");
    session.app_mut().open_linked_mention(mention.clone());
    session.settle();
    // A fixada do alvo fica; a fonte abre em outra aba.
    assert_eq!(tab_files(session), ["Backlink Target.md", "Context.md"]);
    assert_eq!(session.app().tabs[0].id(), target_id);
    let text = session.active_text();
    assert_eq!(
        &text[mention.range.clone()],
        "[[Backlink Target#Section|alias]]"
    );
    assert_eq!(text, fixture.source_text);
    assert_eq!(
        std::fs::read_to_string(session.path("Backlink Sources/Context.md")).unwrap(),
        fixture.source_text,
        "abrir uma menção não altera a nota"
    );
}

#[test]
fn mencao_de_uma_nota_com_rascunho_usa_o_buffer_aberto_sem_gravar() {
    let mut fixture = backlink_fixture();
    let session = &mut fixture.session;
    write_note(
        session,
        "Backlink Sources/Draft.md",
        "# Draft\n\nORIGINAL DRAFT TEXT\n",
    );
    refresh(session, 8);
    session.app_mut().autosave_enabled = false;
    session.open_note("Backlink Sources/Draft");
    session.command("Source editing");
    session.key(CTRL, egui::Key::End);
    session.type_text("\nNEW BEFORE [[Backlink Target]] NEW AFTER");
    assert_eq!(
        std::fs::read_to_string(session.path("Backlink Sources/Draft.md")).unwrap(),
        "# Draft\n\nORIGINAL DRAFT TEXT\n"
    );
    let buffer = session.active_text();
    let open = [(Path::new("Backlink Sources/Draft.md"), buffer.as_str())];
    let mentions = session
        .app()
        .vault
        .as_ref()
        .unwrap()
        .backlink_mentions(Path::new("Backlink Target.md"), &open);
    assert_eq!(
        mentions
            .iter()
            .filter(|mention| mention.path.ends_with("Draft.md"))
            .count(),
        1,
        "a menção só existe no buffer"
    );
    assert_eq!(mentions.len(), 9, "oito reais mais a do rascunho");
}

// ---------------------------------------------------------------------------------------------
// link-fragments
// ---------------------------------------------------------------------------------------------

fn fragment_session() -> Session {
    let mut session = Session::open();
    let padding = "Top padding paragraph.\n\n".repeat(55);
    let original = format!(
        "---\naliases: [FragmentAlias]\nfake: hidden ^properties\n---\n# Fragment Target\n\n{padding}## Alpha **section**\n\nAlpha content.\n\n## Beta café\n\nParagraph chosen ^block-one\n\n- A list block ^list-two\n\n`literal ^inline-fake`\n\nMiddle ^middle-fake tail\n\n```md\n# Fake code heading\nCode ^code-fake\n```\n"
    );
    write_note(&session, "Fragment Target.md", &original);
    write_note(&session, "Fragment Source.md", "");
    refresh(&mut session, 5);
    session
}

#[test]
fn ancora_de_titulo_e_de_bloco_existem_e_inexistente_vira_erro_visivel() {
    let mut session = fragment_session();
    session.open_note("Fragment Source");
    for target in [
        "Fragment Target#Alpha section",
        "Fragment Target#Beta café",
        "Fragment Target#^block-one",
        "Fragment Target#^list-two",
    ] {
        session.app_mut().follow_link(target);
        session.settle();
        assert_eq!(active_file(&session), "Fragment Target.md", "{target}");
        assert!(
            session.app().error.is_none(),
            "{target}: {:?}",
            session.app().error
        );
        session.key(egui::Modifiers::ALT, egui::Key::ArrowLeft);
        assert_eq!(active_file(&session), "Fragment Source.md");
    }
    // Marcadores no código, no meio da linha e nas propriedades não são âncoras.
    for target in [
        "Fragment Target#^inline-fake",
        "Fragment Target#^middle-fake",
        "Fragment Target#^code-fake",
        "Fragment Target#missing",
    ] {
        session.app_mut().error = None;
        session.app_mut().follow_link(target);
        session.settle();
        assert!(
            session.app().error.is_some(),
            "{target} não deveria existir"
        );
        session.key(egui::Modifiers::ALT, egui::Key::ArrowLeft);
    }
}

#[test]
fn completar_ligacao_com_titulo_oferece_so_os_titulos_reais_e_enter_insere() {
    let mut session = fragment_session();
    session.open_note("Fragment Source");
    session.command("Source editing");
    session.key(CTRL, egui::Key::End);
    session.type_text("[[Fragment Target#");
    session.settle();
    assert!(session.has_label("Alpha section"), "candidato de título");
    assert!(session.has_label("Beta café"));
    assert!(
        !session.has_label("Fake code heading"),
        "título dentro de código não entra"
    );
    session.key(egui::Modifiers::NONE, egui::Key::ArrowDown);
    session.key(egui::Modifiers::NONE, egui::Key::Enter);
    session.key(CTRL, egui::Key::S);
    let saved = std::fs::read_to_string(session.path("Fragment Source.md")).unwrap();
    assert!(
        saved.starts_with("[[Fragment Target#Alpha section]]"),
        "{saved:?}"
    );
}

#[test]
fn completar_bloco_ignora_maiusculas_e_tab_insere_sem_candidato_escape_cancela() {
    let mut session = fragment_session();
    session.open_note("Fragment Source");
    session.command("Source editing");
    session.key(CTRL, egui::Key::End);
    session.type_text("[[Fragment Target#^BLoCK-oNe");
    session.settle();
    assert!(session.has_label("block-one"));
    session.key(egui::Modifiers::NONE, egui::Key::Tab);
    session.key(CTRL, egui::Key::S);
    let saved = std::fs::read_to_string(session.path("Fragment Source.md")).unwrap();
    assert!(
        saved.contains("[[Fragment Target#^block-one]]"),
        "{saved:?}"
    );
    // Sem candidato nenhum, Escape deixa o texto digitado como está.
    session.key(CTRL, egui::Key::A);
    session.type_text("[[Fragment Target#missing");
    session.settle();
    assert!(!session.has_label("Fragment Target#missing"));
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    session.key(CTRL, egui::Key::S);
    let saved = std::fs::read_to_string(session.path("Fragment Source.md")).unwrap();
    assert!(saved.contains("[[Fragment Target#missing"), "{saved:?}");
    assert!(!saved.contains('^'), "{saved:?}");
}

#[test]
fn completar_apelido_e_ligacao_so_de_arquivo() {
    let mut session = fragment_session();
    session.open_note("Fragment Source");
    session.command("Source editing");
    session.key(CTRL, egui::Key::End);
    session.type_text("[[FragmentAlias#BEt");
    session.settle();
    assert!(session.has_label("Beta café"));
    session.key(egui::Modifiers::NONE, egui::Key::Enter);
    session.key(CTRL, egui::Key::S);
    let saved = std::fs::read_to_string(session.path("Fragment Source.md")).unwrap();
    assert!(saved.contains("[[Fragment Target#Beta café]]"), "{saved:?}");
    // Ligação a arquivo sem fragmento.
    session.key(CTRL, egui::Key::A);
    session.type_text("[[Fragment Tar");
    session.settle();
    assert!(session.has_label("Fragment Target"));
    session.key(egui::Modifiers::NONE, egui::Key::Enter);
    session.key(CTRL, egui::Key::S);
    let saved = std::fs::read_to_string(session.path("Fragment Source.md")).unwrap();
    assert!(saved.contains("[[Fragment Target]]"), "{saved:?}");
    assert!(
        std::fs::read_to_string(session.path("Fragment Target.md"))
            .unwrap()
            .contains("^block-one")
    );
}
