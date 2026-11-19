//! Histórico das notas (H1): gravação agrupada "como o Word", migração, rename, restaurar e a tela
//! de versões. O armazenamento tem testes próprios em `src/historico_fundo.rs` e no crate
//! `nota-historico`; aqui vale o que o usuário vê no app.
use super::*;
use rustidian::historico_fundo::{Origem, VersaoInfo};
use std::time::SystemTime;

const ORIGINAL: &[u8] = "# Original\r\n\r\nسلام\r\n- [x] Keep this\r\n".as_bytes();

fn versoes(session: &Session, path: &str) -> Vec<VersaoInfo> {
    session
        .app()
        .historico
        .as_ref()
        .and_then(|historico| historico.leitor().versoes(Path::new(path)).ok())
        .unwrap_or_default()
}

fn textos(session: &Session, path: &str) -> Vec<String> {
    let leitor = session.app().historico.as_ref().unwrap().leitor();
    versoes(session, path)
        .iter()
        .map(|versao| leitor.ler(Path::new(path), versao.id).unwrap())
        .collect()
}

/// Passa quadros por `segundos` (a linha de fundo conta o tempo sozinha) e devolve quantas
/// versões a nota tinha em cada quadro, para ver que nenhuma tecla grava por conta própria.
fn passar_tempo(session: &mut Session, path: &str, segundos: f32) -> Vec<usize> {
    let fim = Instant::now() + Duration::from_secs_f32(segundos);
    let mut contagens = Vec::new();
    while Instant::now() < fim {
        session.harness.step();
        contagens.push(versoes(session, path).len());
        std::thread::sleep(Duration::from_millis(20));
    }
    contagens
}

#[track_caller]
fn esperar_versoes(session: &mut Session, path: &str, quantas: usize) {
    for _ in 0..1000 {
        session.harness.step();
        if versoes(session, path).len() >= quantas {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "o histórico de {path} não chegou a {quantas} versões (tem {})",
        versoes(session, path).len()
    );
}

fn digitar(session: &mut Session, texto: &str, por_quadro: usize) {
    let letras: Vec<char> = texto.chars().collect();
    for pedaco in letras.chunks(por_quadro) {
        session
            .harness
            .event(egui::Event::Text(pedaco.iter().collect()));
        session.harness.step();
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn nota_vazia() -> Session {
    let mut session = Session::open_with(&[("Escrita.md", b"")]);
    session.open_note("Escrita");
    session.command("Source editing");
    session.key(CTRL, egui::Key::End);
    // O histórico já conhece a nota aberta.
    session.wait_for(|app| app.historico.is_some());
    session.settle();
    session
}

fn sessao_com_original() -> Session {
    let mut session = Session::open_with(&[("Alvo.md", ORIGINAL)]);
    session.open_note("Alvo");
    session.command("Source editing");
    esperar_versoes(&mut session, "Alvo.md", 1);
    session
}

fn refresh(session: &mut Session) {
    session.command("Refresh vault");
    session.wait_for(|app| app.loading.is_none());
    session.settle();
}

fn click_label(session: &mut Session, label: &str) {
    session.harness.get_by_label(label).click();
    session.settle();
}

fn restaurar_pedido(session: &Session, texto: &str) -> crate::recovery_ui::Restore {
    crate::recovery_ui::Restore {
        expected: session.app().tabs[session.app().active].document.clone(),
        text: texto.to_owned(),
    }
}

#[test]
fn abrir_a_nota_guarda_a_versao_inicial_com_os_bytes_exatos() {
    let session = sessao_com_original();
    let todas = textos(&session, "Alvo.md");
    assert_eq!(todas.len(), 1);
    assert_eq!(todas[0].as_bytes(), ORIGINAL, "CRLF e persa preservados");
    assert!(
        session.path(".rustidian/historico.redb").is_file(),
        "o arquivo fica no cofre, em .rustidian"
    );
    assert_eq!(versoes(&session, "Alvo.md")[0].origem, Origem::Editor);
}

#[test]
fn digitar_uma_frase_e_esperar_a_pausa_grava_uma_versao_e_nao_uma_por_letra() {
    let mut session = nota_vazia();
    let mut durante = Vec::new();
    for letra in "Olá, mundo. ".chars() {
        session.harness.event(egui::Event::Text(letra.to_string()));
        session.harness.step();
        durante.push(versoes(&session, "Escrita.md").len());
        std::thread::sleep(Duration::from_millis(40));
    }
    assert!(
        durante.iter().all(|&n| n == 0),
        "nenhuma tecla vira versão: {durante:?}"
    );
    esperar_versoes(&mut session, "Escrita.md", 1);
    // Mais parada não grava outra: o texto é o mesmo.
    let depois = passar_tempo(&mut session, "Escrita.md", 2.6);
    assert!(depois.iter().all(|&n| n == 1), "{depois:?}");
    assert_eq!(textos(&session, "Escrita.md"), ["Olá, mundo. "]);
}

#[test]
fn quatrocentos_caracteres_sem_pausa_dao_duas_versoes() {
    let mut session = nota_vazia();
    let texto = "palavra solta ".repeat(29); // 406 caracteres, sem ponto final
    assert!((400..420).contains(&texto.len()));
    digitar(&mut session, &texto, 10);
    // 300 caracteres seguidos já viraram uma; o resto vem ao parar (o autosave também grava).
    esperar_versoes(&mut session, "Escrita.md", 2);
    let depois = passar_tempo(&mut session, "Escrita.md", 2.8);
    assert!(depois.iter().all(|&n| n == 2), "{depois:?}");
    let todas = textos(&session, "Escrita.md");
    assert_eq!(todas.len(), 2);
    assert!((300..360).contains(&todas[0].len()), "{}", todas[0].len());
    assert_eq!(todas[1], texto);
}

#[test]
fn salvar_grava_a_versao_e_o_historico_sobrevive_a_reabrir() {
    let mut session = sessao_com_original();
    session.key(CTRL, egui::Key::A);
    session.type_text("# Changed\n\nsalvo\n");
    session.key(CTRL, egui::Key::S);
    esperar_versoes(&mut session, "Alvo.md", 2);
    assert_eq!(
        textos(&session, "Alvo.md").last().unwrap(),
        "# Changed\n\nsalvo\n"
    );
    // Fechar e abrir o app: a linha de fundo grava e solta o arquivo; a nova lê o mesmo cofre.
    session.app_mut().historico = None;
    session.app_mut().historico_ui = Default::default();
    session.wait_for(|app| app.historico.is_some());
    esperar_versoes(&mut session, "Alvo.md", 2);
    let todas = textos(&session, "Alvo.md");
    assert_eq!(todas.len(), 2, "reabrir não duplica a versão-base");
    assert_eq!(todas[0].as_bytes(), ORIGINAL);
}

#[test]
fn restaurar_cria_versao_nova_com_o_texto_antigo_e_nada_se_perde() {
    let mut session = sessao_com_original();
    session.key(CTRL, egui::Key::A);
    session.type_text("# Changed\n\nsalvo\n");
    session.key(CTRL, egui::Key::S);
    esperar_versoes(&mut session, "Alvo.md", 2);
    let primeira = textos(&session, "Alvo.md")[0].clone();
    let pedido = restaurar_pedido(&session, &primeira);
    session.app_mut().restore_snapshot(pedido);
    session.settle();
    assert_eq!(
        std::fs::read(session.path("Alvo.md")).unwrap(),
        ORIGINAL,
        "os bytes exatos voltam para o arquivo"
    );
    assert_eq!(session.active_text().as_bytes(), ORIGINAL);
    esperar_versoes(&mut session, "Alvo.md", 3);
    let infos = versoes(&session, "Alvo.md");
    assert_eq!(infos.last().unwrap().origem, Origem::Restauracao);
    let todas = textos(&session, "Alvo.md");
    assert_eq!(todas.last().unwrap().as_bytes(), ORIGINAL);
    assert!(
        todas.iter().any(|texto| texto == "# Changed\n\nsalvo\n"),
        "a versão que foi trocada continua no histórico"
    );
    // E o app reaberto ainda tem tudo.
    session.app_mut().historico = None;
    session.app_mut().historico_ui = Default::default();
    session.wait_for(|app| app.historico.is_some());
    esperar_versoes(&mut session, "Alvo.md", 3);
    assert_eq!(versoes(&session, "Alvo.md").len(), 3);
}

#[test]
fn nota_alterada_fora_do_dialogo_recusa_restaurar_e_preserva_a_substituta() {
    let mut session = sessao_com_original();
    let pedido = restaurar_pedido(&session, "# Texto antigo\n");
    std::fs::write(session.path("Alvo.md"), b"# EXTERNAL_RECOVERY\n").unwrap();
    session.app_mut().restore_snapshot(pedido);
    session.settle();
    assert!(
        session.app().error.is_some(),
        "a recusa aparece para a pessoa"
    );
    assert_eq!(
        std::fs::read(session.path("Alvo.md")).unwrap(),
        b"# EXTERNAL_RECOVERY\n"
    );
}

#[test]
fn restaurar_com_edicoes_nao_salvas_pergunta_cancelar_mantem_e_salvar_guarda_no_historico() {
    let mut session = sessao_com_original();
    // Sem salvar automático, para o texto ficar mesmo só na memória.
    session.app_mut().autosave_enabled = false;
    session.key(CTRL, egui::Key::A);
    session.type_text("UNSAVED_RECOVERY\n");
    assert_eq!(session.active_text(), "UNSAVED_RECOVERY\n");
    let primeira = textos(&session, "Alvo.md")[0].clone();
    let pedido = restaurar_pedido(&session, &primeira);
    session.app_mut().pending = Some(Pending::RestoreSnapshot(Box::new(pedido)));
    session.settle();
    click_label(&mut session, "Cancelar");
    assert!(session.app().pending.is_none());
    assert_eq!(session.active_text(), "UNSAVED_RECOVERY\n");
    assert_eq!(std::fs::read(session.path("Alvo.md")).unwrap(), ORIGINAL);
    // "Salvar e continuar" grava o texto antes de restaurar: ele fica no histórico.
    let pedido = restaurar_pedido(&session, &primeira);
    session.app_mut().pending = Some(Pending::RestoreSnapshot(Box::new(pedido)));
    session.settle();
    click_label(&mut session, "Salvar e continuar");
    session.wait_for(|app| app.pending.is_none());
    assert_eq!(std::fs::read(session.path("Alvo.md")).unwrap(), ORIGINAL);
    esperar_versoes(&mut session, "Alvo.md", 3);
    assert!(
        textos(&session, "Alvo.md")
            .iter()
            .any(|texto| texto == "UNSAVED_RECOVERY\n")
    );
}

#[test]
fn renomear_ou_mover_a_nota_pelo_app_leva_o_historico() {
    let mut session = Session::open_with(&[("Pasta/Antiga.md", b"# Antiga\n\nprimeiro texto\n")]);
    session.open_note("Antiga");
    session.command("Source editing");
    session.key(CTRL, egui::Key::End);
    esperar_versoes(&mut session, "Pasta/Antiga.md", 1);
    session.type_text("segunda linha\n");
    session.key(CTRL, egui::Key::S);
    esperar_versoes(&mut session, "Pasta/Antiga.md", 2);
    // F2 renomeia a nota atual.
    session.key(egui::Modifiers::NONE, egui::Key::F2);
    session.key(CTRL, egui::Key::A);
    session.type_text("Pasta/Nova.md");
    session.key(egui::Modifiers::NONE, egui::Key::Enter);
    session.wait_for(|app| app.dialog.is_none());
    assert!(session.path("Pasta/Nova.md").is_file());
    esperar_versoes(&mut session, "Pasta/Nova.md", 2);
    assert!(versoes(&session, "Pasta/Antiga.md").is_empty());
    // A nota segue gravando na mesma cadeia, sem recomeçar do zero.
    session.type_text("terceira linha\n");
    session.key(CTRL, egui::Key::S);
    esperar_versoes(&mut session, "Pasta/Nova.md", 3);
    assert_eq!(textos(&session, "Pasta/Nova.md").len(), 3);
    // Renomear a pasta leva as notas dela.
    session
        .app_mut()
        .ask_file(Dialog::RenameNote, PathBuf::from("Pasta"));
    session.settle();
    session.key(CTRL, egui::Key::A);
    session.type_text("Arquivo");
    session.key(egui::Modifiers::NONE, egui::Key::Enter);
    session.wait_for(|app| app.dialog.is_none());
    esperar_versoes(&mut session, "Arquivo/Nova.md", 3);
    assert!(versoes(&session, "Pasta/Nova.md").is_empty());
}

#[test]
fn mudanca_de_fora_e_do_sync_vira_versao_com_a_origem() {
    let mut session = Session::open_with(&[("Fora.md", b"# Fora\n\nversao um\n")]);
    session.open_note("Fora");
    esperar_versoes(&mut session, "Fora.md", 1);
    std::fs::write(
        session.path("Fora.md"),
        b"# Fora\n\nversao dois, de outro programa\n",
    )
    .unwrap();
    refresh(&mut session);
    esperar_versoes(&mut session, "Fora.md", 2);
    // A mudança que chega durante um ciclo do sync leva a marca "sync".
    session.app_mut().historico_ui.sync_ate = Some(Instant::now() + Duration::from_secs(30));
    std::fs::write(
        session.path("Fora.md"),
        b"# Fora\n\nversao tres, veio do celular\n",
    )
    .unwrap();
    refresh(&mut session);
    esperar_versoes(&mut session, "Fora.md", 3);
    let origens: Vec<Origem> = versoes(&session, "Fora.md")
        .iter()
        .map(|versao| versao.origem)
        .collect();
    assert_eq!(origens, [Origem::Editor, Origem::Externa, Origem::Sync]);
    // O app salvando a própria nota não aparece como "mudou fora".
    session.command("Source editing");
    session.key(CTRL, egui::Key::End);
    session.type_text("meu texto\n");
    session.key(CTRL, egui::Key::S);
    esperar_versoes(&mut session, "Fora.md", 4);
    refresh(&mut session);
    let depois = passar_tempo(&mut session, "Fora.md", 1.0);
    assert!(depois.iter().all(|&n| n == 4), "{depois:?}");
    assert_eq!(versoes(&session, "Fora.md")[3].origem, Origem::Editor);
}

#[test]
fn historico_que_nao_abre_deixa_a_nota_abrir_normal_e_avisa_uma_vez() {
    let mut session = Session::open_with(&[("Livre.md", b"# Livre\n\ntexto\n")]);
    // Outra instância do app já é dona do arquivo do histórico: solta o desta e deixa o arquivo
    // preso por fora antes de a linha de fundo voltar.
    session.app_mut().historico = None;
    session.app_mut().historico_ui = Default::default();
    std::fs::create_dir_all(session.path(".rustidian")).unwrap();
    let _dona =
        nota_historico::Historico::abrir(&session.path(".rustidian/historico.redb")).unwrap();
    session.wait_for(|app| app.historico.is_some());
    session.open_note("Livre");
    session.command("Source editing");
    session.wait_for(|app| {
        app.error
            .as_deref()
            .is_some_and(|e| e.contains("histórico"))
    });
    assert_eq!(session.active_text(), "# Livre\n\ntexto\n");
    session.key(CTRL, egui::Key::End);
    session.type_text("mais\n");
    session.key(CTRL, egui::Key::S);
    assert_eq!(
        std::fs::read_to_string(session.path("Livre.md")).unwrap(),
        "# Livre\n\ntexto\nmais\n",
        "salvar funciona sem histórico"
    );
}

#[test]
fn a_tela_de_versoes_tem_alvos_de_32_px_e_mostra_origem_e_tamanho() {
    let mut session = sessao_com_original();
    session.key(CTRL, egui::Key::A);
    session.type_text("# Changed\n\nsalvo\n");
    session.key(CTRL, egui::Key::S);
    esperar_versoes(&mut session, "Alvo.md", 2);
    session.command("File recovery: Open local history");
    session.wait_for(|app| app.recovery_manager.active());
    session.wait_for(|app| app.dialog.is_none());
    for _ in 0..30 {
        session.settle();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(session.has_label("você"), "a origem aparece na lista");
    assert!(session.has_label("mesmo tamanho") || session.has_label("+"));
    let restaurar = session.harness.get_by_label("Restaurar").rect();
    assert!(restaurar.height() >= 32.0, "Restaurar: {restaurar:?}");
    let linhas: Vec<_> = session
        .harness
        .query_all_by_label_contains("você")
        .map(|nodo| nodo.rect())
        .collect();
    assert!(linhas.len() >= 2, "uma linha por versão");
    for retangulo in linhas {
        assert!(retangulo.height() >= 32.0, "linha da lista: {retangulo:?}");
    }
    for nome in ["Mostrar alterações", "Comparar com o texto atual"] {
        let retangulo = session.harness.get_by_label(nome).rect();
        assert!(retangulo.height() >= 32.0, "{nome}: {retangulo:?}");
    }
}

// ------------------------------------------------------------------------------------- prints

fn gravar_print(session: &mut Session, pasta: &Path, nome: &str) {
    session.settle();
    let imagem = session.harness.render().expect("renderização com wgpu");
    assert!(imagem.width() > 0 && imagem.height() > 0);
    imagem.save(pasta.join(nome)).expect("gravar o PNG");
    eprintln!("gravado: {}", pasta.join(nome).display());
}

const DIA_A: &str = "# Diário de bordo\n\nAcordei cedo e fui ao mercado comprar pão.\n";
const DIA_B: &str =
    "# Diário de bordo\n\nAcordei cedo e fui ao mercado comprar pão. A fila estava grande.\n";
const DIA_C: &str = "# Diário de bordo\n\nAcordei cedo e fui à feira comprar pão e frutas. A fila estava grande.\n\nDepois voltei para casa.\n";
const DIA_D: &str = "# Diário de bordo\n\nAcordei cedo e fui à feira comprar pão e frutas. A fila estava pequena.\n\nDepois voltei para casa e escrevi estas linhas.\n";
const DIA_E: &str = "# Diário de bordo\n\nAcordei cedo e fui à feira comprar pão e frutas. A fila estava pequena.\n\nDepois voltei para casa e escrevi estas linhas. À tarde, li um capítulo inteiro.\n";

/// Grava `historico-h1/*.png` em `RUSTIDIAN_PRINTS_DIR` (ex.: `C:\dev\_wt\prints-ux\historico-h1`).
#[test]
#[ignore = "gera PNGs para o dono; precisa de RUSTIDIAN_PRINTS_DIR"]
fn prints_do_historico_h1() {
    let pasta = PathBuf::from(
        std::env::var_os("RUSTIDIAN_PRINTS_DIR").expect("defina RUSTIDIAN_PRINTS_DIR"),
    );
    std::fs::create_dir_all(&pasta).expect("pasta dos prints");
    let mut session = Session::open_with(&[("Diário de bordo.md", DIA_E.as_bytes())]);
    session.wait_for(|app| app.historico.is_some());
    let agora = SystemTime::now();
    let nota = Path::new("Diário de bordo.md");
    {
        // Um histórico de exemplo, com horas e origens diferentes. Entra antes de a nota abrir,
        // e o texto da última é o do arquivo: abrir não acrescenta versão.
        let historico = session.app().historico.as_ref().unwrap();
        for (texto, origem, segundos) in [
            (DIA_A, Origem::Editor, 2 * 86_400 + 3600),
            (DIA_B, Origem::Editor, 86_400 + 7200),
            (DIA_C, Origem::Externa, 3 * 3600),
            (DIA_D, Origem::Sync, 40 * 60),
            (DIA_E, Origem::Editor, 3 * 60),
        ] {
            historico.gravar_em(nota, texto, origem, agora - Duration::from_secs(segundos));
        }
    }
    session.open_note("Diário de bordo");
    esperar_versoes(&mut session, "Diário de bordo.md", 5);
    session.command("File recovery: Open local history");
    session.wait_for(|app| app.recovery_manager.active());
    for _ in 0..40 {
        session.settle();
        std::thread::sleep(Duration::from_millis(20));
    }
    gravar_print(&mut session, &pasta, "1-lista-de-versoes.png");
    // Escolhe a versão que mudou fora do app: o antes e depois por palavra.
    session
        .harness
        .get_by_label_contains("mudou fora do app")
        .click();
    for _ in 0..40 {
        session.settle();
        std::thread::sleep(Duration::from_millis(20));
    }
    gravar_print(&mut session, &pasta, "2-antes-e-depois-por-palavra.png");
    // Restaura a segunda versão da lista (a de "ontem", ou a hora que for quando o teste roda).
    session
        .harness
        .query_all_by_label_contains("você")
        .nth(1)
        .expect("a segunda linha da lista")
        .click();
    for _ in 0..30 {
        session.settle();
        std::thread::sleep(Duration::from_millis(20));
    }
    session.harness.get_by_label("Restaurar").click();
    for _ in 0..60 {
        session.settle();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        session.has_label("você · restaurada"),
        "a versão nova aparece no topo da lista"
    );
    gravar_print(&mut session, &pasta, "3-depois-de-restaurar.png");
    assert_eq!(
        std::fs::read_to_string(session.path("Diário de bordo.md")).unwrap(),
        DIA_B
    );
}
