//! Configurações > Recuperação de arquivos (H2): os campos agora têm efeito e a linha "Espaço"
//! mostra quanto o histórico ocupa e compacta sob pedido.
use super::*;
use rustidian::historico_fundo::Origem;
use std::time::SystemTime;

const NOTA: &str = "# Diário\n\nprimeira versão do texto\n";

/// Cofre com uma nota e algumas versões já gravadas, na página Recuperação das configurações.
fn abrir_recuperacao() -> Session {
    let mut session = Session::open();
    session.wait_for(|app| app.historico.is_some());
    {
        let historico = session.app().historico.as_ref().unwrap();
        let agora = SystemTime::now();
        for (i, texto) in [NOTA, "# Diário\n\nsegunda versão do texto\n"]
            .into_iter()
            .enumerate()
        {
            historico.gravar_em(
                Path::new("Home.md"),
                texto,
                Origem::Editor,
                agora - Duration::from_secs(3600 * (2 - i as u64)),
            );
        }
    }
    session.command("Open settings");
    session.wait_for(|app| app.dialog == Some(Dialog::Settings));
    session.app_mut().settings_page = settings_ui::Page::Recovery;
    session.settle();
    session
}

/// Passa quadros até algum rótulo conter o texto (as respostas vêm da linha de fundo).
#[track_caller]
fn esperar_rotulo(session: &mut Session, texto: &str) {
    for _ in 0..2000 {
        session.harness.step();
        if session.has_label(texto) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("o rótulo com {texto:?} não apareceu em 20 s");
}

#[test]
fn a_pagina_mostra_quantas_versoes_e_notas_o_historico_tem() {
    let mut session = abrir_recuperacao();
    esperar_rotulo(&mut session, "em disco");
    // O cofre de teste tem 1 nota com 2 versões gravadas aqui (e a do carregamento).
    assert!(session.has_label("versões de 1 nota"), "rótulo do espaço");
    assert!(session.has_label("Intervalo das versões antigas"));
    assert!(session.has_label("Guardar todas as versões por"));
    assert!(session.has_label("Compactar agora"));
}

#[test]
fn compactar_agora_termina_com_o_aviso_de_antes_e_depois() {
    let mut session = abrir_recuperacao();
    esperar_rotulo(&mut session, "em disco");
    session.harness.get_by_label("Compactar agora").click();
    esperar_rotulo(&mut session, "Compactado:");
    assert!(session.has_label("→"), "antes → depois");
    // Os números são pedidos de novo e o botão volta.
    esperar_rotulo(&mut session, "em disco");
    session.wait_for(|app| !app.espaco.compactando() && !app.espaco.calculando());
}

#[test]
fn mudar_os_dias_chega_na_politica_enviada_ao_historico() {
    let mut session = Session::open();
    let antes = session.app().recovery_config.politica().tudo_ate;
    session.app_mut().recovery_config.keep_days = 10.0;
    let depois = session.app().recovery_config.politica().tudo_ate;
    assert_ne!(antes, depois);
    assert_eq!(depois, Duration::from_secs(10 * 86_400));
}

#[test]
fn com_o_historico_desligado_o_rotulo_avisa_e_o_botao_fica_parado() {
    let mut session = abrir_recuperacao();
    session.app_mut().recovery_config.enabled = false;
    session.settle();
    assert!(session.has_label("Histórico desligado"));
    session.harness.get_by_label("Compactar agora").click();
    session.settle();
    assert!(!session.app().espaco.compactando());
}

#[test]
#[ignore = "gera PNGs para o dono; precisa de RUSTIDIAN_PRINTS_DIR"]
fn prints_das_configuracoes_h2() {
    let pasta = PathBuf::from(
        std::env::var_os("RUSTIDIAN_PRINTS_DIR").expect("defina RUSTIDIAN_PRINTS_DIR"),
    );
    std::fs::create_dir_all(&pasta).expect("pasta dos prints");
    let mut session = abrir_recuperacao();
    esperar_rotulo(&mut session, "em disco");
    session.settle();
    // Duas renderizações: a primeira põe no atlas as letras que acabaram de aparecer.
    let _ = session.harness.render();
    session.settle();
    let imagem = session.harness.render().expect("renderização com wgpu");
    imagem
        .save(pasta.join("5-configuracoes-do-historico.png"))
        .expect("gravar o PNG");
}
