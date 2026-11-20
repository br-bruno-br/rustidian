//! Histórico das notas, H3 e H4: o desfazer que fica depois de fechar o app, os galhos dele, e as
//! duas telas do escritor ("Achar o que apaguei" e "Histórico deste trecho"). A lógica tem testes
//! próprios no crate `nota-historico`; aqui vale o que a pessoa faz e vê.
use super::*;
use egui_kittest::kittest::NodeT;
use rustidian::historico_fundo::Galhos;
use std::sync::mpsc::Receiver;

const BASE: &str = "# Alvo\n\nTexto de partida.\n";

/// Fecha o app (a linha de fundo grava e solta o arquivo) e abre outro no mesmo cofre, como quem
/// fecha e abre o programa.
fn reabrir(session: Session) -> Session {
    let Session { folder, harness } = session;
    drop(harness);
    let root = folder.path().to_path_buf();
    let harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .build_eframe(move |cc| NotesApp::new(cc, &root, Instant::now(), None));
    let mut session = Session { folder, harness };
    session.wait_for(|app| app.vault.is_some() && app.loading.is_none());
    session
}

fn abrir_nota(nome: &str, texto: &str) -> Session {
    let arquivo = format!("{nome}.md");
    let mut session = Session::open_with(&[(arquivo.as_str(), texto.as_bytes())]);
    preparar(&mut session, nome);
    session
}

/// Abre a nota em modo de texto, com o cursor no fim e o histórico já conhecendo a nota.
fn preparar(session: &mut Session, nome: &str) {
    session.open_note(nome);
    session.command("Source editing");
    session.key(CTRL, egui::Key::End);
    session.wait_for(|app| app.historico.is_some());
    session.settle();
}

fn resposta<T>(rx: Receiver<Result<T, String>>) -> T {
    rx.recv_timeout(Duration::from_secs(90))
        .expect("a linha de fundo respondeu")
        .expect("sem erro")
}

fn galhos(session: &Session, nota: &str) -> Galhos {
    let historico = session.app().historico.as_ref().expect("histórico aberto");
    resposta(historico.galhos(Path::new(nota)).expect("pedido enviado"))
}

/// Espera (passando quadros) a árvore do desfazer chegar a `passos` passos.
#[track_caller]
fn esperar_passos(session: &mut Session, nota: &str, passos: usize) {
    for _ in 0..600 {
        session.harness.step();
        if galhos(session, nota).passos >= passos {
            return;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    panic!(
        "o desfazer de {nota} não chegou a {passos} passos (tem {})",
        galhos(session, nota).passos
    );
}

#[track_caller]
fn esperar_versoes(session: &mut Session, nota: &str, quantas: usize) {
    for _ in 0..1000 {
        session.harness.step();
        let n = session
            .app()
            .historico
            .as_ref()
            .and_then(|h| h.leitor().versoes(Path::new(nota)).ok())
            .map_or(0, |v| v.len());
        if n >= quantas {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("o histórico de {nota} não chegou a {quantas} versões");
}

/// Troca o texto inteiro da nota pelo da pessoa digitando (Ctrl+A e depois o texto).
fn substituir_tudo(session: &mut Session, texto: &str) {
    session.key(CTRL, egui::Key::A);
    session.type_text(texto);
}

fn clicar_habilitado(session: &mut Session, rotulo: &str, indice: usize) {
    let botoes: Vec<_> = session
        .harness
        .query_all_by_label(rotulo)
        .filter(|n| !n.accesskit_node().is_disabled())
        .collect();
    assert!(
        indice < botoes.len(),
        "o botão {rotulo} #{indice} não existe (há {})",
        botoes.len()
    );
    botoes[indice].click();
    session.settle();
}

#[test]
fn ctrl_z_depois_de_fechar_e_abrir_o_app_desfaz_o_que_foi_digitado_antes() {
    let mut session = abrir_nota("Alvo", BASE);
    session.type_text("Frase nova da sessão anterior.");
    session.key(CTRL, egui::Key::S);
    let digitado = session.active_text();
    assert_eq!(digitado, format!("{BASE}Frase nova da sessão anterior."));
    esperar_passos(&mut session, "Alvo.md", 2);

    let mut session = reabrir(session);
    preparar(&mut session, "Alvo");
    assert_eq!(session.active_text(), digitado, "o arquivo voltou igual");
    // O Ctrl+Z da sessão nova chega aos passos da anterior (a semente vem da linha de fundo).
    for _ in 0..300 {
        session.key(CTRL, egui::Key::Z);
        if session.active_text() == BASE {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(session.active_text(), BASE, "desfez a digitação de antes");
}

#[test]
fn nota_alterada_fora_do_app_nao_aplica_o_desfazer_guardado() {
    let mut session = abrir_nota("Alvo", BASE);
    session.type_text(" salvo");
    session.key(CTRL, egui::Key::S);
    esperar_passos(&mut session, "Alvo.md", 2);
    let caminho = session.path("Alvo.md");
    let session = reabrir(session);
    // Com o app fechado, outro programa muda o arquivo.
    std::fs::write(&caminho, "# Outro programa\n").unwrap();
    let mut session = reabrir(session);
    preparar(&mut session, "Alvo");
    assert_eq!(session.active_text(), "# Outro programa\n");
    for _ in 0..40 {
        session.key(CTRL, egui::Key::Z);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        session.active_text(),
        "# Outro programa\n",
        "o desfazer de antes não vale para um texto que mudou por fora"
    );
    let g = galhos(&session, "Alvo.md");
    assert_eq!(
        g.arquivadas.len(),
        1,
        "a árvore antiga ficou só como histórico"
    );
}

#[test]
fn desfazer_e_digitar_outra_coisa_cria_um_galho_e_o_dialogo_volta_a_ele() {
    let mut session = abrir_nota("Alvo", BASE);
    session.type_text("primeira ideia");
    esperar_passos(&mut session, "Alvo.md", 2);
    session.key(CTRL, egui::Key::Z);
    assert_eq!(session.active_text(), BASE);
    session.type_text("segunda ideia");
    esperar_passos(&mut session, "Alvo.md", 3);
    let g = galhos(&session, "Alvo.md");
    assert_eq!(g.galhos.len(), 2, "o caminho antigo virou um galho: {g:?}");

    session.command("Galhos do desfazer");
    session.wait_for(|app| app.escritor.carregou());
    assert_eq!(session.app().escritor.vista(), escritor_ui::Vista::Galhos);
    assert!(session.has_label("Galhos do desfazer"));
    assert!(session.has_label("Você está aqui"));
    // O botão habilitado é o do galho que não é o atual.
    clicar_habilitado(&mut session, "Ir para este galho", 0);
    session.wait_for(|app| !app.escritor.open);
    session.wait_for(|app| {
        app.tabs
            .get(app.active)
            .is_some_and(|t| t.document.text.ends_with("primeira ideia"))
    });
    assert_eq!(session.active_text(), format!("{BASE}primeira ideia"));
    // Foi uma edição normal: o Ctrl+Z volta para onde a pessoa estava.
    session.key(CTRL, egui::Key::Z);
    assert_eq!(session.active_text(), format!("{BASE}segunda ideia"));
}

#[test]
fn achar_o_que_apaguei_lista_busca_e_reinsere_no_cursor() {
    let mut session = abrir_nota("Escrita", "");
    session.type_text("Primeira frase que fica. Segunda frase que vou apagar depois. ");
    esperar_versoes(&mut session, "Escrita.md", 1);
    substituir_tudo(&mut session, "Primeira frase que fica. ");
    esperar_versoes(&mut session, "Escrita.md", 2);

    session.command("Achar o que apaguei");
    session.wait_for(|app| app.escritor.carregou());
    assert_eq!(session.app().escritor.vista(), escritor_ui::Vista::Apagados);
    let achados: Vec<String> = session
        .app()
        .escritor
        .apagados()
        .iter()
        .map(|a| a.trecho.clone())
        .collect();
    assert!(
        achados
            .iter()
            .any(|t| t.contains("Segunda frase que vou apagar")),
        "{achados:?}"
    );
    assert!(
        !achados
            .iter()
            .any(|t| t.contains("Primeira frase que fica")),
        "o que está no texto de agora não é apagado: {achados:?}"
    );
    assert!(session.has_label("Reinserir no cursor"));
    assert!(session.has_label("Copiar"));

    // A busca estreita por palavra, sem ligar para acento e maiúscula.
    session.type_text("APAGAR");
    session.settle();
    assert!(session.has_label("Segunda frase que vou apagar"));
    session.key(CTRL, egui::Key::A);
    session.type_text("zebra");
    session.settle();
    assert!(session.has_label("Nenhum trecho apagado tem essas palavras."));
    session.key(CTRL, egui::Key::A);
    session.type_text("segunda");
    session.settle();

    clicar_habilitado(&mut session, "Reinserir no cursor", 0);
    session.wait_for(|app| !app.escritor.open);
    session.wait_for(|app| {
        app.tabs
            .get(app.active)
            .is_some_and(|t| t.document.text.contains("Segunda frase que vou apagar"))
    });
    assert!(
        session
            .active_text()
            .starts_with("Primeira frase que fica. ")
    );
    // Reinserir é uma edição como qualquer outra: o Ctrl+Z tira.
    session.key(CTRL, egui::Key::Z);
    assert_eq!(session.active_text(), "Primeira frase que fica. ");
}

#[test]
fn historico_deste_trecho_mostra_como_a_selecao_era_nas_versoes() {
    let mut session = abrir_nota("Casa", "A casa é azul.\n");
    substituir_tudo(&mut session, "A casa é vermelha.\n");
    esperar_versoes(&mut session, "Casa.md", 2);
    // Seleciona "vermelha": fim do texto, volta antes do ponto e pega a palavra.
    session.key(CTRL, egui::Key::End);
    session.key(egui::Modifiers::NONE, egui::Key::ArrowLeft);
    session.key(egui::Modifiers::NONE, egui::Key::ArrowLeft);
    session.key(CTRL | egui::Modifiers::SHIFT, egui::Key::ArrowLeft);

    session.command("Histórico deste trecho");
    session.wait_for(|app| app.escritor.carregou());
    assert_eq!(session.app().escritor.vista(), escritor_ui::Vista::Trecho);
    let textos: Vec<String> = session
        .app()
        .escritor
        .trecho()
        .iter()
        .map(|m| m.texto.clone())
        .collect();
    assert!(textos.iter().any(|t| t.contains("vermelha")), "{textos:?}");
    assert!(textos.iter().any(|t| t.contains("azul")), "{textos:?}");
    assert!(session.has_label("Trocar a seleção por esta"));
}

#[test]
fn apagar_do_historico_tira_o_texto_do_achar_apagado_e_dos_galhos() {
    let segredo = "SENHA-DO-WIFI-9871";
    let mut session = abrir_nota("Segredos", "");
    session.type_text(&format!("Colei sem querer: {segredo} e segui. "));
    esperar_versoes(&mut session, "Segredos.md", 1);
    esperar_passos(&mut session, "Segredos.md", 2);
    substituir_tudo(&mut session, "Nota limpa. ");
    esperar_versoes(&mut session, "Segredos.md", 2);
    esperar_passos(&mut session, "Segredos.md", 3);

    // Antes da limpeza o trecho aparece em "achar o que apaguei".
    session.command("Achar o que apaguei");
    session.wait_for(|app| app.escritor.carregou());
    assert!(
        session
            .app()
            .escritor
            .apagados()
            .iter()
            .any(|a| a.trecho.contains(segredo))
    );
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    session.wait_for(|app| !app.escritor.open);

    // "Apagar do histórico" (H2).
    let rx = session
        .app()
        .historico
        .as_ref()
        .unwrap()
        .apagar_texto(Path::new("Segredos.md"), segredo)
        .unwrap();
    assert!(resposta(rx) >= 1);

    session.command("Achar o que apaguei");
    session.wait_for(|app| app.escritor.carregou());
    assert!(
        !session
            .app()
            .escritor
            .apagados()
            .iter()
            .any(|a| a.trecho.contains(segredo)),
        "o texto apagado do histórico não volta pela busca"
    );
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    session.wait_for(|app| !app.escritor.open);

    let g = galhos(&session, "Segredos.md");
    assert!(
        !g.galhos.iter().any(|galho| galho.trecho.contains(segredo)),
        "nem os galhos do desfazer guardam o texto: {g:?}"
    );
}
