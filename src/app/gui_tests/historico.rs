//! Histórico das notas (H1): gravação agrupada "como o Word", migração, rename, restaurar e a tela
//! de versões. O armazenamento tem testes próprios em `src/historico_fundo.rs` e no crate
//! `nota-historico`; aqui vale o que o usuário vê no app.
use super::*;
use egui_kittest::kittest::NodeT;
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
    assert_eq!(
        textos(&session, "Pasta/Nova.md").len(),
        3,
        "{:?} {:?}",
        versoes(&session, "Pasta/Nova.md")
            .iter()
            .map(|v| v.origem)
            .collect::<Vec<_>>(),
        textos(&session, "Pasta/Nova.md")
    );
    // Renomear a pasta leva as notas dela.
    // O watcher e o índice ainda leem a nota recém-renomeada; o Windows recusa renomear a pasta
    // enquanto um deles a tem aberta ("Acesso negado"), sem mudar nada. Espera o cofre assentar
    // e repete só essa recusa; qualquer outro erro derruba o teste.
    session.wait_for(|app| {
        app.vault.as_ref().is_some_and(|vault| {
            vault
                .folders
                .iter()
                .any(|pasta| pasta == Path::new("Pasta"))
                && vault.note(Path::new("Pasta/Nova.md")).is_some()
        })
    });
    // Confirma o diálogo direto: o que se testa é o histórico, não a tecla Enter.
    for tentativa in 0.. {
        let app = session.app_mut();
        app.ask_file(Dialog::RenameNote, PathBuf::from("Pasta"));
        app.dialog_input = "Arquivo".into();
        if app.submit_dialog(Dialog::RenameNote) {
            app.dialog = None;
            break;
        }
        let erro = app.error.clone().unwrap_or_default();
        assert!(
            erro.contains("Acesso negado") && tentativa < 200,
            "renomear a pasta falhou: {erro}"
        );
        app.dialog = None;
        session.harness.step();
        std::thread::sleep(Duration::from_millis(25));
    }
    session.settle();
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
        // A coluna da esquerda é a lista; o rótulo da barra do tempo (à direita) também diz "você".
        .filter(|retangulo| retangulo.min.x < 400.0)
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

// ------------------------------------------------------------------ H2: máquina do tempo

/// O meio de uma semana de uns 20 dias atrás: versões gravadas em segundos seguidos a partir
/// daqui caem no mesmo balde semanal da poda, sem depender do dia em que o teste roda.
fn meio_de_uma_semana_antiga() -> SystemTime {
    const DIA: u64 = 86_400;
    let agora = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let dias = agora / DIA - 20;
    let segunda = ((dias + 3) / 7) * 7 - 3;
    SystemTime::UNIX_EPOCH + Duration::from_secs((segunda + 2) * DIA + DIA / 2)
}

/// Cinco versões do diário (as quatro primeiras numa semana antiga) e a tela de versões aberta,
/// com a mais nova escolhida e o texto dela carregado.
fn tela_com_cinco_versoes() -> Session {
    let mut session = Session::open_with(&[("Diário de bordo.md", DIA_E.as_bytes())]);
    session.wait_for(|app| app.historico.is_some());
    let antiga = meio_de_uma_semana_antiga();
    {
        let historico = session.app().historico.as_ref().unwrap();
        let nota = Path::new("Diário de bordo.md");
        for (indice, texto) in [DIA_A, DIA_B, DIA_C, DIA_D].into_iter().enumerate() {
            let quando = antiga + Duration::from_secs(indice as u64);
            historico.gravar_em(nota, texto, Origem::Editor, quando);
        }
        let ha_tres_minutos = SystemTime::now() - Duration::from_secs(180);
        historico.gravar_em(nota, DIA_E, Origem::Editor, ha_tres_minutos);
    }
    session.open_note("Diário de bordo");
    esperar_versoes(&mut session, "Diário de bordo.md", 5);
    session.command("File recovery: Open local history");
    session.wait_for(|app| app.recovery_manager.active());
    esperar_a_tela(&mut session);
    session
}

#[track_caller]
fn esperar_a_tela(session: &mut Session) {
    // Primeiro deixa os quadros chegarem (a leitura começa num deles), depois espera acabar.
    session.settle();
    session.wait_for(|app| !app.recovery_manager.ocupado());
    session.settle();
}

fn ids_do_diario(session: &Session) -> Vec<u64> {
    versoes(session, "Diário de bordo.md")
        .iter()
        .map(|versao| versao.id.0)
        .collect()
}

#[test]
fn a_barra_do_tempo_anda_de_versao_em_versao() {
    let mut session = tela_com_cinco_versoes();
    let ids = ids_do_diario(&session);
    assert_eq!(session.app().recovery_manager.escolhida(), Some(ids[4]));
    assert!(session.has_label("Linha do tempo"), "a barra aparece");
    session.harness.get_by_label("Versão anterior").click();
    esperar_a_tela(&mut session);
    session.harness.get_by_label("Versão anterior").click();
    esperar_a_tela(&mut session);
    let manager = &session.app().recovery_manager;
    assert_eq!(
        manager.escolhida(),
        Some(ids[2]),
        "duas para trás: a 3ª mais nova"
    );
    assert_eq!(manager.texto_mostrado(), Some(DIA_C));
    session.harness.get_by_label("Próxima versão").click();
    esperar_a_tela(&mut session);
    let manager = &session.app().recovery_manager;
    assert_eq!(manager.escolhida(), Some(ids[3]));
    assert_eq!(manager.texto_mostrado(), Some(DIA_D));
}

#[test]
fn varias_posicoes_no_mesmo_quadro_ficam_com_a_ultima() {
    let mut session = tela_com_cinco_versoes();
    let ids = ids_do_diario(&session);
    let ctx = session.app().ctx.clone();
    {
        let manager = &mut session.app_mut().recovery_manager;
        // 0 = a mais antiga. A primeira começa a leitura; as outras só marcam que falta reler.
        for posicao in [0, 1, 3, 2] {
            manager.ir_para(posicao, &ctx);
        }
        assert!(manager.releitura_pendente(), "só uma leitura por vez");
        assert_eq!(manager.escolhida(), Some(ids[2]), "a escolha já é a última");
    }
    esperar_a_tela(&mut session);
    let manager = &session.app().recovery_manager;
    assert_eq!(
        manager.escolhida(),
        Some(ids[2]),
        "a barra não pulou de volta"
    );
    assert_eq!(
        manager.texto_mostrado(),
        Some(DIA_C),
        "o texto é o da última posição"
    );
    assert!(!manager.releitura_pendente());
}

#[test]
fn a_barra_e_os_botoes_seguem_habilitados_com_a_leitura_em_andamento() {
    let mut session = tela_com_cinco_versoes();
    let ids = ids_do_diario(&session);
    let _leitura = session.app_mut().recovery_manager.simular_leitura();
    session.settle();
    assert!(session.app().recovery_manager.ocupado());
    let desabilitado = |session: &Session, nome: &str| {
        session
            .harness
            .get_by_label(nome)
            .accesskit_node()
            .is_disabled()
    };
    assert!(
        desabilitado(&session, "Restaurar"),
        "Restaurar espera a leitura"
    );
    assert!(
        !desabilitado(&session, "Linha do tempo"),
        "a barra não desabilita"
    );
    assert!(!desabilitado(&session, "Versão anterior"));
    // E o botão anda mesmo assim: a escolha muda na hora e a releitura fica marcada.
    session.harness.get_by_label("Versão anterior").click();
    session.settle();
    let manager = &session.app().recovery_manager;
    assert_eq!(manager.escolhida(), Some(ids[3]));
    assert!(manager.releitura_pendente());
}

#[test]
fn dar_nome_pela_tela_aparece_na_lista_e_a_poda_so_leva_as_sem_nome() {
    let mut session = tela_com_cinco_versoes();
    let ids = ids_do_diario(&session);
    // E, D, C, B: a segunda versão é a que ganha o nome.
    for _ in 0..3 {
        session.harness.get_by_label("Versão anterior").click();
        esperar_a_tela(&mut session);
    }
    assert_eq!(session.app().recovery_manager.escolhida(), Some(ids[1]));
    assert!(session.has_label("Dar nome"));
    click_label(&mut session, "Dar nome");
    assert!(session.has_label("Nome da versão"));
    assert!(session.has_label("Versões com nome nunca saem do histórico."));
    session.type_text("Antes da feira");
    // Os testes clicam no botão: o Enter antes de o campo ter foco é intermitente.
    click_label(&mut session, "Salvar");
    esperar_a_tela(&mut session);
    let infos = versoes(&session, "Diário de bordo.md");
    assert_eq!(infos[1].nome.as_deref(), Some("Antes da feira"));
    assert_eq!(infos.iter().filter(|info| info.nome.is_some()).count(), 1);
    assert!(session.has_label("Antes da feira"), "a linha mostra o nome");
    assert!(session.has_label("Mudar nome"), "o botão agora muda o nome");
    // O filtro só aparece com alguma nomeada e deixa só ela na lista.
    let antes = session.harness.query_all_by_label_contains("você").count();
    click_label(&mut session, "Só com nome");
    let depois = session.harness.query_all_by_label_contains("você").count();
    assert!(depois < antes, "o filtro tira linhas: {antes} -> {depois}");
    assert!(
        session.app().recovery_manager.escolhida().is_some(),
        "a barra segue sobre todas as versões"
    );
    // Retenção zerada: as antigas sem nome saem (a mais nova da semana fica), a nomeada não.
    let politica = nota_historico::PoliticaRetencao {
        tudo_ate: Duration::ZERO,
        intervalo: Duration::ZERO,
        intervalo_ate: Duration::ZERO,
        por_dia_ate: Duration::ZERO,
    };
    let historico = session.app_mut().historico.as_mut().unwrap();
    historico.ajustar_politica(politica);
    let relatorio = historico
        .podar_agora()
        .unwrap()
        .recv_timeout(Duration::from_secs(20))
        .expect("a poda respondeu")
        .unwrap();
    assert!(relatorio.versoes_removidas >= 2, "{relatorio:?}");
    let depois = versoes(&session, "Diário de bordo.md");
    assert!(
        depois
            .iter()
            .any(|info| info.id.0 == ids[1] && info.nome.is_some())
    );
    assert!(
        !depois.iter().any(|info| info.id.0 == ids[0]),
        "a antiga sem nome saiu"
    );
    assert!(depois.len() < 5);
}

#[test]
fn tirar_o_nome_devolve_a_versao_ao_normal() {
    let mut session = tela_com_cinco_versoes();
    let ids = ids_do_diario(&session);
    let ctx = session.app().ctx.clone();
    session.app_mut().recovery_manager.ir_para(1, &ctx);
    esperar_a_tela(&mut session);
    click_label(&mut session, "Dar nome");
    session.type_text("Rascunho bom");
    click_label(&mut session, "Salvar");
    esperar_a_tela(&mut session);
    assert!(versoes(&session, "Diário de bordo.md")[1].nome.is_some());
    click_label(&mut session, "Mudar nome");
    click_label(&mut session, "Tirar o nome");
    esperar_a_tela(&mut session);
    assert!(versoes(&session, "Diário de bordo.md")[1].nome.is_none());
    assert_eq!(session.app().recovery_manager.escolhida(), Some(ids[1]));
    assert!(session.has_label("Dar nome"));
}

const COM_SENHA: &str = "# Cofre\n\nA senha do wifi é senha-123-secreta e não pode vazar.\n";

fn tela_com_senha_no_historico() -> Session {
    let mut session = Session::open_with(&[("Cofre.md", COM_SENHA.as_bytes())]);
    session.wait_for(|app| app.historico.is_some());
    {
        let historico = session.app().historico.as_ref().unwrap();
        let nota = Path::new("Cofre.md");
        let antes = SystemTime::now() - Duration::from_secs(3600);
        for (texto, segundos) in [
            ("# Cofre\n\nrascunho\n", 600),
            ("# Cofre\n\nA senha do wifi é senha-123-secreta.\n", 1800),
            (COM_SENHA, 3000),
        ] {
            historico.gravar_em(
                nota,
                texto,
                Origem::Editor,
                antes + Duration::from_secs(segundos),
            );
        }
    }
    session.open_note("Cofre");
    esperar_versoes(&mut session, "Cofre.md", 3);
    session.command("File recovery: Open local history");
    session.wait_for(|app| app.recovery_manager.active());
    esperar_a_tela(&mut session);
    session
}

#[test]
fn apagar_do_historico_tira_o_trecho_de_todas_as_versoes() {
    let mut session = tela_com_senha_no_historico();
    assert!(
        textos(&session, "Cofre.md")
            .iter()
            .any(|t| t.contains("senha-123-secreta")),
        "a senha está no histórico antes"
    );
    click_label(&mut session, "Apagar do histórico…");
    assert!(session.has_label("Apagar um trecho do histórico"));
    assert!(session.has_label("O trecho some de todas as versões desta nota."));
    assert!(session.has_label("Em todas as notas do cofre"));
    session.type_text("senha-123-secreta");
    session.settle();
    assert!(
        session.has_label("A nota ainda tem este trecho."),
        "o texto atual da nota ainda tem a senha"
    );
    click_label(&mut session, "Apagar");
    esperar_a_tela(&mut session);
    for texto in textos(&session, "Cofre.md") {
        assert!(!texto.contains("senha-123-secreta"), "sobrou: {texto}");
    }
    assert!(
        session.has_label("Apagado de 2 versões"),
        "o aviso conta as versões"
    );
    assert!(
        !session.has_label("Apagar um trecho do histórico"),
        "o diálogo fechou"
    );
}

#[test]
fn apagar_pede_pelo_menos_tres_caracteres_e_cancelar_nao_apaga() {
    let mut session = tela_com_senha_no_historico();
    click_label(&mut session, "Apagar do histórico…");
    session.type_text("se");
    session.settle();
    let desabilitado = |session: &Session| {
        session
            .harness
            .get_by_label("Apagar")
            .accesskit_node()
            .is_disabled()
    };
    assert!(desabilitado(&session), "2 caracteres não bastam");
    assert!(!session.has_label("A nota ainda tem este trecho."));
    session.type_text("nha");
    session.settle();
    assert!(!desabilitado(&session), "3 ou mais habilita");
    session.type_text("-que-nao-esta-na-nota");
    session.settle();
    assert!(
        !session.has_label("A nota ainda tem este trecho."),
        "o trecho não está na nota"
    );
    click_label(&mut session, "Cancelar");
    assert!(!session.has_label("Apagar um trecho do histórico"));
    assert!(
        textos(&session, "Cofre.md")
            .iter()
            .any(|t| t.contains("senha-123-secreta")),
        "cancelar não apaga nada"
    );
    // O trecho que nunca esteve no histórico só avisa.
    click_label(&mut session, "Apagar do histórico…");
    session.type_text("trecho que nunca existiu");
    click_label(&mut session, "Apagar");
    esperar_a_tela(&mut session);
    assert!(session.has_label("O trecho não estava no histórico"));
}

#[test]
fn fechar_a_tela_com_o_dialogo_filho_aberto_nao_deixa_estado_preso() {
    let mut session = tela_com_senha_no_historico();
    click_label(&mut session, "Apagar do histórico…");
    assert!(session.app().recovery_manager.active());
    click_label(&mut session, "Cancelar");
    click_label(&mut session, "Dar nome");
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    esperar_a_tela(&mut session);
    assert!(
        session.app().recovery_manager.active(),
        "a tela principal segue aberta"
    );
    session.key(egui::Modifiers::NONE, egui::Key::Escape);
    esperar_a_tela(&mut session);
    assert!(
        !session.app().recovery_manager.active(),
        "fechou e nada ficou preso"
    );
}

/// Como `prints.rs::save`: renderiza duas vezes, porque a primeira põe no atlas as letras que
/// acabaram de aparecer.
fn gravar_print_h2(session: &mut Session, pasta: &Path, nome: &str) {
    for _ in 0..20 {
        session.settle();
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = session.harness.render();
    session.settle();
    let imagem = session.harness.render().expect("renderização com wgpu");
    assert!(imagem.width() > 0 && imagem.height() > 0);
    imagem.save(pasta.join(nome)).expect("gravar o PNG");
    eprintln!("gravado: {}", pasta.join(nome).display());
}

const SENHA_DA_CASA: &str = "casa-4821-wifi";

/// Grava `1-linha-do-tempo.png` a `4-apagar-do-historico.png` em `RUSTIDIAN_PRINTS_DIR`
/// (ex.: `C:\dev\_wt\prints-rustidian\h2`).
#[test]
#[ignore = "gera PNGs para o dono; precisa de RUSTIDIAN_PRINTS_DIR"]
fn prints_do_historico_h2() {
    let pasta = PathBuf::from(
        std::env::var_os("RUSTIDIAN_PRINTS_DIR").expect("defina RUSTIDIAN_PRINTS_DIR"),
    );
    std::fs::create_dir_all(&pasta).expect("pasta dos prints");
    let atual = format!("{DIA_E}\nSenha do wifi da casa: {SENHA_DA_CASA}\n");
    let mut session = Session::open_with(&[("Diário de bordo.md", atual.as_bytes())]);
    session.wait_for(|app| app.historico.is_some());
    let agora = SystemTime::now();
    {
        let historico = session.app().historico.as_ref().unwrap();
        let nota = Path::new("Diário de bordo.md");
        for (texto, origem, segundos) in [
            (DIA_A, Origem::Editor, 2 * 86_400 + 3600),
            (DIA_B, Origem::Editor, 86_400 + 7200),
            (DIA_C, Origem::Externa, 3 * 3600),
            (DIA_D, Origem::Sync, 40 * 60),
            (atual.as_str(), Origem::Editor, 3 * 60),
        ] {
            historico.gravar_em(nota, texto, origem, agora - Duration::from_secs(segundos));
        }
    }
    session.open_note("Diário de bordo");
    esperar_versoes(&mut session, "Diário de bordo.md", 5);
    session.command("File recovery: Open local history");
    session.wait_for(|app| app.recovery_manager.active());
    esperar_a_tela(&mut session);
    let ctx = session.app().ctx.clone();

    // 1. A barra no meio, com o que mudou nessa versão em verde e vermelho.
    session.app_mut().recovery_manager.ir_para(2, &ctx);
    esperar_a_tela(&mut session);
    assert_eq!(session.app().recovery_manager.texto_mostrado(), Some(DIA_C));
    gravar_print_h2(&mut session, &pasta, "1-linha-do-tempo.png");

    // 2. Duas versões com nome: aparecem na lista e como marcas na barra.
    for (posicao, nome) in [(1, "Antes da feira"), (3, "Versão que mandei")] {
        session.app_mut().recovery_manager.ir_para(posicao, &ctx);
        esperar_a_tela(&mut session);
        click_label(&mut session, "Dar nome");
        session.type_text(nome);
        click_label(&mut session, "Salvar");
        esperar_a_tela(&mut session);
    }
    session.app_mut().recovery_manager.ir_para(2, &ctx);
    esperar_a_tela(&mut session);
    gravar_print_h2(&mut session, &pasta, "2-versoes-com-nome.png");

    // 3. O diálogo para dar (ou mudar) o nome, com um nome já escrito.
    session.app_mut().recovery_manager.ir_para(1, &ctx);
    esperar_a_tela(&mut session);
    click_label(&mut session, "Mudar nome");
    gravar_print_h2(&mut session, &pasta, "3-dar-nome.png");
    click_label(&mut session, "Cancelar");

    // 4. Apagar um trecho: a nota ainda tem a senha, e o aviso aparece.
    click_label(&mut session, "Apagar do histórico…");
    session.type_text(SENHA_DA_CASA);
    gravar_print_h2(&mut session, &pasta, "4-apagar-do-historico.png");
    assert!(session.has_label("A nota ainda tem este trecho."));
}
