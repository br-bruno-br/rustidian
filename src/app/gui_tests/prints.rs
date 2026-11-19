//! Capturas de tela sem janela para quem não está diante do PC (R9/R10). O app roda no harness
//! do `egui_kittest` e o `wgpu` desenha o quadro numa textura em memória (adaptador de software
//! quando existir): nenhuma janela é aberta e nenhum mouse/teclado real é usado.
//!
//! Não roda no `cargo test` comum. Uso:
//! `RUSTIDIAN_PRINTS_DIR=<pasta> cargo test --bin rustidian prints_para_o_dono -- --ignored`
//! Opcional: `RUSTIDIAN_DICT_DIR` aponta a pasta com `pt_BR.aff`/`pt_BR.dic`; sem ela vale a
//! pasta de dados do app do usuário (lida antes de o estado ser isolado).

use super::*;

const AURORA: &str =
    "# Projeto Aurora\n\nPlano da nova vitrine da loja, com prazos e responsáveis.\n";
const REUNIAO: &str = "# Reunião de segunda\n\nFalamos sobre o Projeto Aurora e definimos o prazo.\nA Maria vai revisar o texto da vitrine até sexta.\n";
const DIARIO: &str = "# Diário\n\nHoje avancei no projeto aurora com o fornecedor.\nTambém vi [[Projeto Aurora]] de novo, esta menção já tem link.\n";
const RECEITA: &str = "# Receita de bolo\n\nO caxorro correu pelo jardin e comeu o bolo da vizinha.\n\nMisture a farinha com os ovos e leve ao forno por quarenta minutos. Depois, deixe esfriar e sirva com cafe.\n";

fn open_with(notes: &[(&str, &str)]) -> Session {
    // Antes de isolar o estado: guarda de onde vem o dicionário pt-BR.
    let dictionary = std::env::var_os("RUSTIDIAN_DICT_DIR")
        .map(PathBuf::from)
        .or_else(rustidian::spell::dictionary_dir);
    isolate_user_state();
    if let (Some(from), Some(to)) = (dictionary, rustidian::spell::dictionary_dir()) {
        std::fs::create_dir_all(&to).expect("pasta do dicionário isolada");
        for name in ["pt_BR.aff", "pt_BR.dic"] {
            let _ = std::fs::copy(from.join(name), to.join(name));
        }
    }
    let folder = tempfile::tempdir().expect("cofre temporário");
    for (name, text) in notes {
        std::fs::write(folder.path().join(format!("{name}.md")), text).expect("nota");
    }
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

fn save(session: &mut Session, dir: &Path, name: &str) {
    session.settle();
    // Duas renderizações: a primeira põe no atlas as letras que acabaram de aparecer.
    let _ = session.harness.render();
    session.settle();
    let image = session.harness.render().expect("renderização com wgpu");
    assert!(image.width() > 0 && image.height() > 0);
    image.save(dir.join(name)).expect("gravar o PNG");
    eprintln!("gravado: {}", dir.join(name).display());
}

#[test]
#[ignore = "gera PNGs para o dono; precisa de RUSTIDIAN_PRINTS_DIR"]
fn prints_para_o_dono() {
    let dir = PathBuf::from(
        std::env::var_os("RUSTIDIAN_PRINTS_DIR").expect("defina RUSTIDIAN_PRINTS_DIR"),
    );
    std::fs::create_dir_all(&dir).expect("pasta dos prints");
    let notes = [
        ("Projeto Aurora", AURORA),
        ("Reunião de segunda", REUNIAO),
        ("Diário", DIARIO),
        ("Receita de bolo", RECEITA),
    ];

    // 1. Corretor: nota com palavras erradas e o menu do botão direito sobre uma delas.
    let mut session = open_with(&notes);
    session.open_note("Receita de bolo");
    session.wait_for(|app| app.editor.spelling.active());
    session.settle();
    {
        // Botão direito sobre "caxorro" (a posição vale para a janela de 1280x800 e o tema
        // padrão; RUSTIDIAN_PRINTS_CLICK="x,y" troca o ponto se o layout mudar).
        let hint = std::env::var("RUSTIDIAN_PRINTS_CLICK").unwrap_or_else(|_| "420,300".into());
        let (x, y) = hint.split_once(',').expect("x,y");
        let pos = egui::pos2(x.parse().unwrap(), y.parse().unwrap());
        session.harness.event(egui::Event::PointerMoved(pos));
        session.harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        session.harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        session.settle();
    }
    save(&mut session, &dir, "corretor.png");

    // 2. Menções sem link da "Projeto Aurora".
    let mut session = open_with(&notes);
    session.open_note("Projeto Aurora");
    session.app_mut().show_right = true;
    session.app_mut().right_tab = 1;
    session.app_mut().unlinked_open = true;
    session.settle();
    session.settle();
    save(&mut session, &dir, "mencoes-sem-link.png");

    // 3. Visão geral: explorador, abas e uma nota.
    let mut session = open_with(&notes);
    session.open_note("Reunião de segunda");
    session.open_note("Projeto Aurora");
    save(&mut session, &dir, "geral.png");
}

/// R15: duas notas lado a lado, cada painel com as suas abas. Grava `dividir.png` na pasta de
/// `RUSTIDIAN_PRINTS_DIR` (ex.: `C:\dev\_wt\prints-r15`).
#[test]
#[ignore = "gera PNGs para o dono; precisa de RUSTIDIAN_PRINTS_DIR"]
fn print_da_tela_dividida() {
    let dir = PathBuf::from(
        std::env::var_os("RUSTIDIAN_PRINTS_DIR").expect("defina RUSTIDIAN_PRINTS_DIR"),
    );
    std::fs::create_dir_all(&dir).expect("pasta dos prints");
    let notes = [
        ("Projeto Aurora", AURORA),
        ("Reunião de segunda", REUNIAO),
        ("Diário", DIARIO),
        ("Receita de bolo", RECEITA),
    ];
    let mut session = open_with(&notes);
    // Painel da esquerda com duas abas (Projeto Aurora e Diário); a Reunião abre no da direita.
    session.open_note("Projeto Aurora");
    session.app_mut().open_note_at(Path::new("Diário.md"), true);
    session.settle();
    session.app_mut().open_note_in_other_pane(
        Path::new("Reunião de segunda.md"),
        rustidian::panes::Orientation::Side,
    );
    session.settle();
    session.app_mut().focus_pane(0);
    session.settle();
    save(&mut session, &dir, "dividir.png");
}

/// H1: um quadro com quatro cartões de nota vivos (um deles em edição) e a biblioteca aberta.
/// Grava `quadro-vivo.png` na pasta de `RUSTIDIAN_PRINTS_DIR` (ex.: `C:\dev\_wt\prints-h1`).
#[test]
#[ignore = "gera PNGs para o dono; precisa de RUSTIDIAN_PRINTS_DIR"]
fn print_do_quadro_vivo() {
    use super::canvas::{double_click, drag, state};
    let dir = PathBuf::from(
        std::env::var_os("RUSTIDIAN_PRINTS_DIR").expect("defina RUSTIDIAN_PRINTS_DIR"),
    );
    std::fs::create_dir_all(&dir).expect("pasta dos prints");
    let aurora = "# Projeto Aurora\n\nPlano da nova vitrine da loja.\n\n## Prazos\n- Layout: **segunda**\n- Fotos dos produtos: quarta\n- Publicar: *sexta*\n\n## Responsáveis\nMaria revisa o texto; Bruno aprova o preço.\n";
    let reuniao = "# Reunião de segunda\n\nFalamos sobre o Projeto Aurora e definimos o prazo.\n\n> A Maria vai revisar o texto da vitrine até sexta.\n\n- Pauta fechada\n- Próxima reunião na quarta\n- Levar as fotos novas\n- Conferir o estoque da vitrine\n- Separar as caixas\n";
    let diario = "# Diário\n\nHoje avancei no projeto com o fornecedor.\nTambém vi `Projeto Aurora` de novo.\n\n- Ligar para o fornecedor\n- Revisar a lista de preços\n";
    let receita = "# Receita de bolo\n\nMisture a farinha com os ovos e leve ao forno por quarenta minutos.\n\n1. Bater os ovos\n2. Juntar a farinha\n3. Assar\n";
    let notes = [
        ("Projeto Aurora", aurora),
        ("Reunião de segunda", reuniao),
        ("Diário", diario),
        ("Receita de bolo", receita),
        ("Ideias", "# Ideias\n"),
        ("Compras", "# Compras\n"),
    ];
    let mut session = open_with(&notes);
    let board = r#"{
	"nodes":[
		{"id":"aurora","type":"file","x":0,"y":0,"width":340,"height":270,"file":"Projeto Aurora.md"},
		{"id":"reuniao","type":"file","x":380,"y":0,"width":340,"height":270,"file":"Reunião de segunda.md"},
		{"id":"diario","type":"file","x":0,"y":310,"width":340,"height":250,"file":"Diário.md"},
		{"id":"receita","type":"file","x":380,"y":310,"width":340,"height":250,"file":"Receita de bolo.md","color":"4"}
	],
	"edges":[
		{"id":"e1","fromNode":"aurora","fromSide":"right","toNode":"reuniao","toSide":"left","label":"origem"},
		{"id":"e2","fromNode":"aurora","fromSide":"bottom","toNode":"diario","toSide":"top"}
	]
}"#;
    std::fs::write(session.path("Projeto.canvas"), board).expect("quadro");
    session.settle();
    session
        .app_mut()
        .open_note(std::path::Path::new("Projeto.canvas"));
    session.app_mut().show_left = false;
    session.settle();
    // Abre a biblioteca, empurra o quadro para a direita (a biblioteca cobre a esquerda) e entra
    // em edição no cartão da Aurora.
    session.command("Canvas: Toggle note library");
    session.settle();
    let area = state(&session).node_screen_rect("receita").unwrap();
    drag(
        &mut session,
        egui::pos2(area.right() + 40.0, area.bottom() + 40.0),
        egui::pos2(area.right() + 40.0 + 230.0, area.bottom() + 40.0),
    );
    let body = {
        let rect = state(&session).node_screen_rect("aurora").unwrap();
        egui::pos2(rect.center().x, rect.min.y + rect.height() * 0.7)
    };
    double_click(&mut session, body);
    session.type_text("\n\n- Conferir o orçamento");
    save(&mut session, &dir, "quadro-vivo.png");
}

/// H2b: um cofre pequeno em volta de uma nota "Projeto Aurora" que liga a outras cinco.
fn cofre_do_projeto() -> Session {
    let aurora = "# Projeto Aurora\n\nPlano da nova vitrine da loja.\n\nOlhe a [[Reunião de segunda]] e o [[Diário]]; os preços estão na [[Tabela de preços]].\nO texto final fica em [[Vitrine]] e as fotos na [[Receita de bolo]] (sim, o fotógrafo anotou lá).\n\n## Prazos\n- Layout: segunda\n- Publicar: sexta\n";
    let reuniao = "# Reunião de segunda\n\nFalamos sobre o prazo da vitrine.\n\n- Pauta fechada\n- Próxima reunião na quarta\n";
    let diario = "# Diário\n\nHoje avancei no projeto com o fornecedor.\nLigar para o fornecedor e revisar a lista de preços.\n";
    let precos = "# Tabela de preços\n\nCamiseta 49,90; caneca 29,90; caderno 24,90.\n";
    let vitrine = "# Vitrine\n\nTexto da vitrine: três destaques e uma chamada para o Pix.\n";
    let receita = "# Receita de bolo\n\nMisture a farinha com os ovos e leve ao forno.\n";
    open_with(&[
        ("Projeto Aurora", aurora),
        ("Reunião de segunda", reuniao),
        ("Diário", diario),
        ("Tabela de preços", precos),
        ("Vitrine", vitrine),
        ("Receita de bolo", receita),
    ])
}

/// H2b: o quadro gerado a partir da nota (Ctrl+Shift+B). Grava `quadro-da-nota.png` na pasta de
/// `RUSTIDIAN_PRINTS_DIR` (ex.: `C:\dev\_wt\prints-h2b`).
#[test]
#[ignore = "gera PNGs para o dono; precisa de RUSTIDIAN_PRINTS_DIR"]
fn print_do_quadro_da_nota() {
    let dir = PathBuf::from(
        std::env::var_os("RUSTIDIAN_PRINTS_DIR").expect("defina RUSTIDIAN_PRINTS_DIR"),
    );
    std::fs::create_dir_all(&dir).expect("pasta dos prints");
    let mut session = cofre_do_projeto();
    session.open_note("Projeto Aurora");
    session.app_mut().show_left = false;
    session.key(CTRL | egui::Modifiers::SHIFT, egui::Key::B);
    session.wait_for(|app| {
        app.tabs
            .get(app.active)
            .is_some_and(|tab| tab.document.path == Path::new("Projeto Aurora.canvas"))
    });
    session.settle();
    save(&mut session, &dir, "quadro-da-nota.png");
}

/// H2b: a visão Lista do mesmo quadro, depois de o dono agrupar duas notas e juntar um cartão de
/// texto e uma ligação. Grava `quadro-lista.png`.
#[test]
#[ignore = "gera PNGs para o dono; precisa de RUSTIDIAN_PRINTS_DIR"]
fn print_da_lista_do_quadro() {
    let dir = PathBuf::from(
        std::env::var_os("RUSTIDIAN_PRINTS_DIR").expect("defina RUSTIDIAN_PRINTS_DIR"),
    );
    std::fs::create_dir_all(&dir).expect("pasta dos prints");
    let mut session = cofre_do_projeto();
    session.open_note("Projeto Aurora");
    session.app_mut().show_left = false;
    session.key(CTRL | egui::Modifiers::SHIFT, egui::Key::B);
    session.wait_for(|app| {
        app.tabs
            .get(app.active)
            .is_some_and(|tab| tab.document.path == Path::new("Projeto Aurora.canvas"))
    });
    // O "trabalho do dono" sobre o quadro gerado: um grupo em volta das duas primeiras notas
    // ligadas, um cartão de texto solto e um link.
    let index = session.app().active;
    let mut board =
        rustidian::canvas::Canvas::parse(&session.app().tabs[index].document.text).unwrap();
    let first = board.nodes[1].rect();
    let second = board.nodes[2].rect();
    let left = first.x.min(second.x) - 30.0;
    let top = first.y.min(second.y) - 60.0;
    let right = (first.x + first.w).max(second.x + second.w) + 30.0;
    let bottom = (first.y + first.h).max(second.y + second.h) + 30.0;
    let group = board.add_group("Para esta semana", left, top);
    if let Some(node) = board.node_mut(&group) {
        node.width = right - left;
        node.height = bottom - top;
    }
    let text = board.add_text(-200.0, 700.0);
    board.set_text(&text, "Lembrar: confirmar o fornecedor antes de sexta");
    let link = board.add_link("https://exemplo.com/fornecedor", 200.0, 700.0);
    board.connect(&text, &link, None, None);
    session.app_mut().tabs[index].document.text = board.to_json();
    session.settle();
    session.key(CTRL | egui::Modifiers::SHIFT, egui::Key::B);
    assert!(session.app().tabs[index].board_list);
    save(&mut session, &dir, "quadro-lista.png");
}

fn ux_wait(session: &mut Session, name: &'static str) {
    session.wait_for(move |app| {
        app.tabs
            .get(app.active)
            .is_some_and(|t| t.document.path == Path::new(name))
    });
    session.settle();
}

/// O fluxo de usabilidade nota, quadro e Lista com o seletor do cabeçalho (`ux_fluxo`): gera uma
/// sequência de PNGs em `RUSTIDIAN_PRINTS_DIR`. Para olhar o fluxo com os próprios olhos; não
/// afirma pixels.
#[test]
#[ignore = "UX: gera prints do fluxo nota, quadro e Lista"]
fn ux_fluxo() {
    use super::canvas::{move_to, rect_of};
    use super::nota_quadro::click_segment;
    let dir = PathBuf::from(std::env::var_os("RUSTIDIAN_PRINTS_DIR").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = cofre_do_projeto();
    std::fs::write(s.path("Solta.md"), "# Solta\n\nUma nota sem nenhum link.\n").unwrap();
    std::fs::write(
        s.path("Ideias.canvas"),
        r#"{"nodes":[{"id":"t","type":"text","x":100,"y":50,"width":260,"height":120,"text":"Ideia solta\nsegunda linha"}],"edges":[]}"#,
    )
    .unwrap();
    s.settle();
    s.open_note("Projeto Aurora");
    save(&mut s, &dir, "01-nota-aberta.png");
    click_segment(&mut s, "Quadro");
    ux_wait(&mut s, "Projeto Aurora.canvas");
    save(&mut s, &dir, "02-quadro-gerado.png");
    click_segment(&mut s, "Lista");
    save(&mut s, &dir, "03-lista.png");
    // O trecho do item mostra o cartão no quadro.
    s.harness
        .get_by_label_contains("Plano da nova vitrine")
        .click();
    s.settle();
    s.settle();
    save(&mut s, &dir, "04-trecho-volta-ao-quadro-foco.png");
    click_segment(&mut s, "Nota");
    ux_wait(&mut s, "Projeto Aurora.md");
    save(&mut s, &dir, "05-volta-a-nota-pelo-seletor.png");
    click_segment(&mut s, "Quadro");
    ux_wait(&mut s, "Projeto Aurora.canvas");
    save(&mut s, &dir, "06-quadro-ja-existia.png");
    // O atalho cicla: quadro -> lista -> nota.
    let cycle = |s: &mut Session| s.key(CTRL | egui::Modifiers::SHIFT, egui::Key::B);
    cycle(&mut s);
    s.settle();
    save(&mut s, &dir, "07-atalho-quadro-para-lista.png");
    cycle(&mut s);
    ux_wait(&mut s, "Projeto Aurora.md");
    save(&mut s, &dir, "08-atalho-lista-para-nota.png");
    // Título do item abre a nota.
    click_segment(&mut s, "Lista");
    ux_wait(&mut s, "Projeto Aurora.canvas");
    s.harness.get_all_by_label("Diário").last().unwrap().click();
    s.settle();
    s.settle();
    ux_wait(&mut s, "Diário.md");
    save(&mut s, &dir, "09-titulo-do-item-abre-a-nota.png");
    // Paleta
    s.key(CTRL, egui::Key::P);
    s.type_text("ir para");
    save(&mut s, &dir, "10-paleta-ir-para.png");
    s.key(egui::Modifiers::NONE, egui::Key::Escape);
    // Nota sem links
    s.open_note("Solta");
    save(&mut s, &dir, "11-nota-sem-links.png");
    click_segment(&mut s, "Quadro");
    ux_wait(&mut s, "Solta.canvas");
    save(&mut s, &dir, "12-quadro-de-nota-sem-links.png");
    // Quadro sem nota par: "Nota" pergunta antes de criar.
    s.app_mut().open_note(Path::new("Ideias.canvas"));
    s.settle();
    save(&mut s, &dir, "13-quadro-sem-nota-par.png");
    click_segment(&mut s, "Nota");
    save(&mut s, &dir, "14-confirmacao-criar-a-nota.png");
    s.harness.get_by_label("Criar").click();
    s.settle();
    s.settle();
    ux_wait(&mut s, "Ideias.md");
    save(&mut s, &dir, "15-nota-criada-no-modo-nota.png");
    click_segment(&mut s, "Quadro");
    ux_wait(&mut s, "Ideias.canvas");
    save(&mut s, &dir, "16-quadro-agrupado-sob-a-nota.png");
    // Cartão de texto selecionado: o botão Transformar em nota à vista.
    let c = rect_of(&s, "t").center();
    move_to(&mut s, c);
    for pressed in [true, false] {
        s.harness.event(egui::Event::PointerButton {
            pos: c,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        s.harness.step();
    }
    s.settle();
    save(&mut s, &dir, "17-botao-transformar-em-nota.png");
    // Menu do cartão de texto (o botão se esconde enquanto o menu está aberto).
    move_to(&mut s, c);
    for pressed in [true, false] {
        s.harness.event(egui::Event::PointerButton {
            pos: c,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        s.harness.step();
    }
    s.settle();
    save(&mut s, &dir, "18-menu-do-cartao-texto.png");
    s.harness.get_by_label("Transformar em nota").click();
    s.settle();
    s.settle();
    save(&mut s, &dir, "19-transformado-em-nota.png");
    // Lista do quadro Ideias, com o botão de exportar
    click_segment(&mut s, "Lista");
    save(&mut s, &dir, "20-lista-ideias.png");
}
