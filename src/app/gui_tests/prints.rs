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
