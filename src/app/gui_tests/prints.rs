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
