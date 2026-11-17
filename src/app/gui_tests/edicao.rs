//! R8b: fluxos de edição de `tests/gui_smoke.py` migrados para o egui_kittest (ver mod.rs).
//!
//! Cada teste monta o próprio cofre com as notas de que precisa, abre uma delas no modo
//! pedido, manda teclado e texto como eventos do egui e confere os bytes gravados no disco
//! (o mesmo critério do smoke em Python, que lia o arquivo depois de Ctrl+S).

use super::*;

const NONE: egui::Modifiers = egui::Modifiers::NONE;
const SHIFT: egui::Modifiers = egui::Modifiers::SHIFT;
const MODES: [&str; 2] = ["Source editing", "Live preview"];

fn key_events(modifiers: egui::Modifiers, key: egui::Key) -> Vec<egui::Event> {
    let mut events = Vec::new();
    if modifiers != NONE {
        events.push(egui::Event::ModifiersChanged(modifiers));
    }
    for pressed in [true, false] {
        events.push(egui::Event::Key {
            key,
            pressed,
            modifiers,
            repeat: false,
            physical_key: None,
        });
    }
    if modifiers != NONE {
        events.push(egui::Event::ModifiersChanged(NONE));
    }
    events
}

/// Um passo "enfileirado": todos os eventos chegam ao app no mesmo quadro (o smoke em
/// Python congelava o processo com SIGSTOP para conseguir o mesmo).
enum Step {
    Text(&'static str),
    Key(egui::Modifiers, egui::Key),
}

impl Session {
    /// Abre o app com um cofre que, além das três notas de sempre, tem estes arquivos.
    pub(super) fn open_with(files: &[(&str, &[u8])]) -> Self {
        isolate_user_state();
        let folder = tempfile::tempdir().expect("cofre temporário");
        for (name, bytes) in [
            ("Home.md", HOME.as_bytes()),
            ("Second.md", SECOND.as_bytes()),
            ("Alpha.md", ALPHA.as_bytes()),
        ]
        .into_iter()
        .chain(files.iter().copied())
        {
            let path = folder.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("pasta da nota");
            }
            std::fs::write(path, bytes).expect("arquivo do cofre");
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

    /// Abre o arquivo (com extensão) pelo Abrir rápido, como o usuário.
    fn open_file(&mut self, name: &str) {
        self.key(CTRL, egui::Key::O);
        self.type_text(name);
        self.key(NONE, egui::Key::Enter);
        let wanted = name.to_owned();
        self.wait_for(|app| {
            app.tabs
                .get(app.active)
                .is_some_and(|tab| tab.document.path.ends_with(&wanted))
        });
        self.settle();
    }

    /// Abre a nota `name` (sem `.md`) no modo pedido, com o cursor no fim.
    fn edit(&mut self, name: &str, mode: &str) {
        self.open_note(name);
        self.command(mode);
        self.settle();
        self.key(CTRL, egui::Key::End);
    }

    /// Todos os eventos no mesmo quadro.
    fn queued(&mut self, steps: &[Step]) {
        let mut events = Vec::new();
        for step in steps {
            match step {
                Step::Text(text) => {
                    events.extend(text.chars().map(|c| egui::Event::Text(c.to_string())))
                }
                Step::Key(modifiers, key) => events.extend(key_events(*modifiers, *key)),
            }
        }
        self.harness.input_mut().events.extend(events);
        self.frame();
        for _ in 0..6 {
            self.frame();
        }
    }

    /// Um quadro com o gancho `raw_input_hook` do app, que o harness não chama: é ele que
    /// devolve ao quadro seguinte os eventos adiados (Tab e Enter enfileirados).
    fn frame(&mut self) {
        let ctx = self.harness.ctx.clone();
        let mut raw = self.harness.input_mut().clone();
        eframe::App::raw_input_hook(self.harness.state_mut(), &ctx, &mut raw);
        *self.harness.input_mut() = raw;
        self.harness.step();
    }

    /// Um caractere por evento e por quadro, como o teclado de verdade.
    fn type_chars(&mut self, text: &str) {
        for c in text.chars() {
            self.harness.event(egui::Event::Text(c.to_string()));
            self.harness.step();
        }
        self.settle();
    }

    fn paste(&mut self, text: &str) {
        self.harness.event(egui::Event::Paste(text.to_owned()));
        self.settle();
    }

    fn select_all(&mut self) {
        self.key(CTRL, egui::Key::A);
    }

    /// Ctrl+S e os bytes que ficaram no disco.
    fn save_and_read(&mut self, name: &str) -> Vec<u8> {
        self.key(CTRL, egui::Key::S);
        std::fs::read(self.path(name)).expect("nota gravada")
    }

    fn assert_saved(&mut self, name: &str, expected: &[u8], what: &str) {
        let actual = self.save_and_read(name);
        assert_eq!(
            String::from_utf8_lossy(&actual),
            String::from_utf8_lossy(expected),
            "{what}"
        );
    }

    fn active_tab_count(&self) -> usize {
        self.app().tabs.len()
    }
}

// ---------------------------------------------------------------- undo

#[test]
fn undo_desfaz_um_grupo_de_digitacao_por_vez_e_refaz_com_ctrl_y() {
    for mode in MODES {
        let mut session = Session::open();
        session.edit("Home", mode);
        session.type_text("FIRST_UNDO_GROUP");
        // A pausa separa os grupos de desfazer.
        std::thread::sleep(Duration::from_millis(1300));
        session.type_text("SECOND_UNDO_GROUP");
        session.key(CTRL, egui::Key::Z);
        session.assert_saved(
            "Home.md",
            format!("{HOME}FIRST_UNDO_GROUP").as_bytes(),
            &format!("{mode}: desfazer manteve a digitação anterior"),
        );
        session.key(CTRL, egui::Key::Y);
        session.assert_saved(
            "Home.md",
            format!("{HOME}FIRST_UNDO_GROUPSECOND_UNDO_GROUP").as_bytes(),
            &format!("{mode}: refazer"),
        );
        session.key(CTRL, egui::Key::Z);
        session.key(CTRL, egui::Key::Z);
        session.assert_saved(
            "Home.md",
            HOME.as_bytes(),
            &format!("{mode}: segundo desfazer volta ao original"),
        );
    }
}

// ---------------------------------------------------------------- unicode-queued

#[test]
fn texto_persa_grego_e_zwnj_enfileirado_grava_bytes_exatos() {
    let original = "# Keyboard calibration\n\n";
    let mut session = Session::open_with(&[("Calibra.md", original.as_bytes())]);
    session.edit("Calibra", "Source editing");
    session.type_text("ASCII_BASE");
    session.assert_saved(
        "Calibra.md",
        format!("{original}ASCII_BASE").as_bytes(),
        "ASCII digitado",
    );
    // Colar por cima de tudo devolve o original.
    session.select_all();
    session
        .harness
        .event(egui::Event::Paste(original.to_owned()));
    session.settle();
    session.assert_saved("Calibra.md", original.as_bytes(), "colar o original");
    // Persa, grego e ZWNJ chegam como eventos de texto no mesmo quadro.
    let suffix = "سلام π صفحه\u{200c}کلید";
    let events: Vec<egui::Event> = suffix
        .chars()
        .map(|c| egui::Event::Text(c.to_string()))
        .collect();
    session.harness.input_mut().events.extend(events);
    session.harness.step();
    session.settle();
    session.assert_saved(
        "Calibra.md",
        format!("{original}{suffix}").as_bytes(),
        "texto Unicode enfileirado",
    );
}

// ---------------------------------------------------------------- list-indent

type Chord = (egui::Modifiers, egui::Key);

enum Action {
    Press(Chord),
    QueuedTabs,
    QueuedSelection,
    QueuedCaret,
}

struct IndentCase {
    label: &'static str,
    original: &'static [u8],
    selection: &'static [Chord],
    action: Action,
    expected: &'static [u8],
}

const SOURCE_LIST: &[u8] = b"- Alpha\n- Beta\n";
const TAB: Chord = (NONE, egui::Key::Tab);
const SHIFT_TAB: Chord = (SHIFT, egui::Key::Tab);
const ALL: Chord = (CTRL, egui::Key::A);
const TOP: Chord = (CTRL, egui::Key::Home);
const LINE_END: Chord = (NONE, egui::Key::End);
const SHIFT_DOWN: Chord = (SHIFT, egui::Key::ArrowDown);

fn indent_cases() -> Vec<IndentCase> {
    vec![
        IndentCase {
            label: "selected",
            original: SOURCE_LIST,
            selection: &[ALL],
            action: Action::Press(TAB),
            expected: b"\t- Alpha\n\t- Beta\n",
        },
        IndentCase {
            label: "spaces",
            original: b"    - Alpha\n    - Beta\n",
            selection: &[ALL],
            action: Action::Press(SHIFT_TAB),
            expected: SOURCE_LIST,
        },
        IndentCase {
            label: "boundary",
            original: SOURCE_LIST,
            selection: &[TOP, SHIFT_DOWN],
            action: Action::Press(TAB),
            expected: b"\t- Alpha\n- Beta\n",
        },
        IndentCase {
            label: "caret",
            original: SOURCE_LIST,
            selection: &[TOP, LINE_END],
            action: Action::Press(TAB),
            expected: b"\t- Alpha\n- Beta\n",
        },
        IndentCase {
            label: "root-outdent",
            original: SOURCE_LIST,
            selection: &[ALL],
            action: Action::Press(SHIFT_TAB),
            expected: SOURCE_LIST,
        },
        IndentCase {
            label: "quoted",
            original: b"> - Alpha\n> - Beta\n",
            selection: &[ALL],
            action: Action::Press(TAB),
            expected: b"> \t- Alpha\n> \t- Beta\n",
        },
        IndentCase {
            label: "tasks",
            original: b"1. [ ] Alpha\n2. [x] Beta\n",
            selection: &[ALL],
            action: Action::Press(TAB),
            expected: b"\t1. [ ] Alpha\n\t2. [x] Beta\n",
        },
        IndentCase {
            label: "quoted-code",
            original: b"```\n> Alpha\n> Beta\n```\n",
            selection: &[ALL],
            action: Action::Press(TAB),
            expected: b"\t```\n\t> Alpha\n\t> Beta\n\t```\n",
        },
        IndentCase {
            label: "unicode",
            original: "- فارسی\r\n- café 👋\r\n".as_bytes(),
            selection: &[ALL],
            action: Action::Press(TAB),
            expected: "\t- فارسی\r\n\t- café 👋\r\n".as_bytes(),
        },
        IndentCase {
            label: "queued",
            original: SOURCE_LIST,
            selection: &[ALL],
            action: Action::QueuedTabs,
            expected: b"\t\t- Alpha\n\t\t- Beta\n",
        },
        IndentCase {
            label: "queued-selection",
            original: SOURCE_LIST,
            selection: &[],
            action: Action::QueuedSelection,
            expected: b"\t- Alpha\n\t- Beta\n",
        },
        IndentCase {
            label: "queued-caret",
            original: SOURCE_LIST,
            selection: &[],
            action: Action::QueuedCaret,
            expected: b"\t- Alpha\n- Beta\n",
        },
        IndentCase {
            label: "plain",
            original: b"plain",
            selection: &[],
            action: Action::Press(TAB),
            expected: b"plain\t",
        },
    ]
}

#[test]
fn tab_e_shift_tab_recuam_listas_nos_dois_editores() {
    for mode in MODES {
        let cases = indent_cases();
        let names: Vec<String> = (0..cases.len()).map(|n| format!("Lista{n:02}")).collect();
        let files: Vec<(String, &[u8])> = cases
            .iter()
            .zip(&names)
            .map(|(case, name)| (format!("{name}.md"), case.original))
            .collect();
        let borrowed: Vec<(&str, &[u8])> = files
            .iter()
            .map(|(name, bytes)| (name.as_str(), *bytes))
            .collect();
        let mut session = Session::open_with(&borrowed);
        for (case, name) in cases.iter().zip(&names) {
            let file = format!("{name}.md");
            let what = format!("{mode} / {}", case.label);
            session.edit(name, mode);
            for (modifiers, key) in case.selection {
                session.key(*modifiers, *key);
            }
            match &case.action {
                Action::Press((modifiers, key)) => session.key(*modifiers, *key),
                Action::QueuedTabs => session.queued(&[
                    Step::Key(NONE, egui::Key::Tab),
                    Step::Key(NONE, egui::Key::Tab),
                ]),
                Action::QueuedSelection => session.queued(&[
                    Step::Key(CTRL, egui::Key::A),
                    Step::Key(NONE, egui::Key::Tab),
                ]),
                Action::QueuedCaret => session.queued(&[
                    Step::Key(CTRL, egui::Key::Home),
                    Step::Key(NONE, egui::Key::End),
                    Step::Key(NONE, egui::Key::Tab),
                ]),
            }
            session.assert_saved(&file, case.expected, &what);
            if case.label == "selected" {
                session.key(CTRL, egui::Key::Z);
                session.assert_saved(&file, case.original, &format!("{what}: desfazer"));
                session.key(CTRL | SHIFT, egui::Key::Z);
                session.assert_saved(&file, case.expected, &format!("{what}: refazer"));
                session.key(SHIFT, egui::Key::Tab);
                session.assert_saved(&file, case.original, &format!("{what}: Shift+Tab"));
                session.key(CTRL, egui::Key::Z);
                session.assert_saved(&file, case.expected, &format!("{what}: desfazer o recuo"));
            }
            if case.label == "caret" {
                session.type_text("!");
                session.assert_saved(&file, b"\t- Alpha!\n- Beta\n", &format!("{what}: digitar"));
            }
        }
    }
}

#[test]
fn lista_em_texto_puro_mantem_tab_literal() {
    let mut session = Session::open_with(&[("Literal.txt", b"- Alpha\n")]);
    session.open_file("Literal.txt");
    session.key(CTRL, egui::Key::End);
    session.key(CTRL, egui::Key::Home);
    session.key(NONE, egui::Key::End);
    session.key(NONE, egui::Key::Tab);
    session.assert_saved("Literal.txt", b"- Alpha\t\n", "Tab literal no .txt");
}

// ---------------------------------------------------------------- list-enter

#[test]
fn enter_continua_listas_e_sai_de_item_vazio() {
    // (rótulo, original, depois do Enter, texto digitado em seguida, quantas vezes ←)
    let cases: [(&str, &str, &str, &str, usize); 9] = [
        ("bullet", "- Alpha", "- Alpha\n- ", "Beta", 0),
        ("number", "9. Alpha", "9. Alpha\n10. ", "Beta", 0),
        ("task", "- [x] Alpha", "- [x] Alpha\n- [ ] ", "Beta", 0),
        (
            "nested",
            "- Parent\n\t- Alpha",
            "- Parent\n\t- Alpha\n\t- ",
            "Beta",
            0,
        ),
        ("indented-code", "\t- Alpha", "\t- Alpha\n", "", 0),
        ("quote", "> - Alpha", "> - Alpha\n> - ", "Beta", 0),
        ("unicode", "- فارسی", "- فارسی\n- ", "café 👋", 0),
        ("empty", "- Alpha\n- ", "- Alpha\n", "", 0),
        ("split", "- AlphaBeta", "- Alpha\n- Beta", "", 4),
    ];
    for mode in MODES {
        let files: Vec<(String, Vec<u8>)> = cases
            .iter()
            .map(|(label, source, ..)| (format!("Cont{label}.md"), source.as_bytes().to_vec()))
            .collect();
        let borrowed: Vec<(&str, &[u8])> = files
            .iter()
            .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
            .collect();
        let mut session = Session::open_with(&borrowed);
        for (label, source, entered, suffix, lefts) in cases {
            let file = format!("Cont{label}.md");
            let what = format!("{mode} / {label}");
            session.edit(&format!("Cont{label}"), mode);
            for _ in 0..lefts {
                session.key(NONE, egui::Key::ArrowLeft);
            }
            session.key(NONE, egui::Key::Enter);
            session.assert_saved(&file, entered.as_bytes(), &what);
            session.key(CTRL, egui::Key::Z);
            session.assert_saved(&file, source.as_bytes(), &format!("{what}: desfazer"));
            session.key(CTRL, egui::Key::Y);
            session.assert_saved(&file, entered.as_bytes(), &format!("{what}: refazer"));
            if !suffix.is_empty() {
                session.type_text(suffix);
                session.assert_saved(
                    &file,
                    format!("{entered}{suffix}").as_bytes(),
                    &format!("{what}: texto depois"),
                );
            }
        }
    }
}

#[test]
fn enter_em_texto_puro_nao_continua_lista() {
    let mut session = Session::open_with(&[("Contliteral.txt", b"- Alpha")]);
    session.open_file("Contliteral.txt");
    session.key(CTRL, egui::Key::End);
    session.key(NONE, egui::Key::Enter);
    session.assert_saved("Contliteral.txt", b"- Alpha\n", "texto puro");
}

// ---------------------------------------------------------------- delimiter-pairs

fn pairs_vault() -> Session {
    Session::open_with(&[
        ("Par-redondo.md", b""),
        ("Par-colchete.md", b""),
        ("Par-chave.md", b""),
        ("Par-simples.md", b""),
        ("Par-duplas.md", b""),
        ("Par-contexto.md", b"can"),
        ("Par-unicode.md", "café 👋".as_bytes()),
        ("Par-link.md", b""),
        ("Par-fila.md", b""),
        ("Par-formato.md", b"word"),
        ("Par-enter.md", b"- Item"),
        ("Par-seguir.md", b""),
        ("Par-alvo.md", b"# Pair target\n"),
        ("Par-literal.txt", b""),
    ])
}

#[test]
fn parenteses_colchetes_chaves_e_aspas_fecham_sozinhos() {
    let pairs = [
        ("Par-redondo", "(", ")"),
        ("Par-colchete", "[", "]"),
        ("Par-chave", "{", "}"),
        ("Par-simples", "'", "'"),
        ("Par-duplas", "\"", "\""),
    ];
    for mode in MODES {
        let mut session = pairs_vault();
        for (name, open, close) in pairs {
            let file = format!("{name}.md");
            let what = format!("{mode} / {open}{close}");
            session.edit(name, mode);
            session.type_chars(open);
            session.assert_saved(&file, format!("{open}{close}").as_bytes(), &what);
            session.key(NONE, egui::Key::Backspace);
            session.assert_saved(&file, b"", &format!("{what}: Backspace apaga o par"));
            session.type_chars(&format!("{open}inside{close}!"));
            session.assert_saved(
                &file,
                format!("{open}inside{close}!").as_bytes(),
                &format!("{what}: o fechamento digitado passa por cima"),
            );
            session.select_all();
            session.type_chars("word");
            session.assert_saved(&file, b"word", &format!("{what}: texto sobre a seleção"));
            session.select_all();
            session.type_chars(open);
            session.assert_saved(
                &file,
                format!("{open}word{close}").as_bytes(),
                &format!("{what}: a seleção é envolvida"),
            );
            session.key(CTRL, egui::Key::Z);
            session.assert_saved(&file, b"word", &format!("{what}: desfazer"));
            session.key(CTRL, egui::Key::Y);
            session.assert_saved(
                &file,
                format!("{open}word{close}").as_bytes(),
                &format!("{what}: refazer"),
            );
            session.type_chars("new");
            session.assert_saved(
                &file,
                format!("{open}new{close}").as_bytes(),
                &format!("{what}: a seleção interna é mantida"),
            );
        }
    }
}

#[test]
fn pares_respeitam_o_contexto_e_o_unicode() {
    for mode in MODES {
        let mut session = pairs_vault();
        session.edit("Par-contexto", mode);
        session.type_chars("'t");
        session.assert_saved("Par-contexto.md", b"can't", &format!("{mode}: apóstrofo"));
        session.select_all();
        session.type_chars("word");
        session.key(CTRL, egui::Key::Home);
        session.type_chars("(");
        session.assert_saved(
            "Par-contexto.md",
            b"(word",
            &format!("{mode}: antes de letra"),
        );
        session.select_all();
        session.paste("(");
        session.assert_saved(
            "Par-contexto.md",
            b"(",
            &format!("{mode}: colar não pareia"),
        );
        session.select_all();
        session.paste("\\");
        session.type_chars("(");
        session.assert_saved("Par-contexto.md", b"\\(", &format!("{mode}: após barra"));
        session.select_all();
        session.key(NONE, egui::Key::Backspace);
        session.type_chars("\"\\\"");
        session.assert_saved(
            "Par-contexto.md",
            b"\"\\\"\"",
            &format!("{mode}: aspas escapadas"),
        );
        session.type_chars("\"!");
        session.assert_saved(
            "Par-contexto.md",
            b"\"\\\"\"!",
            &format!("{mode}: aspas de fechamento"),
        );
        session.edit("Par-unicode", mode);
        session.select_all();
        session.type_chars("(");
        session.assert_saved(
            "Par-unicode.md",
            "(café 👋)".as_bytes(),
            &format!("{mode}: Unicode"),
        );
    }
}

#[test]
fn colchetes_duplos_completam_o_link_com_tab_inclusive_enfileirado() {
    for mode in MODES {
        let mut session = pairs_vault();
        session.edit("Par-link", mode);
        session.type_chars("[[Par-alvo");
        session.key(NONE, egui::Key::Tab);
        session.type_chars(" after");
        session.assert_saved(
            "Par-link.md",
            b"[[Par-alvo]] after",
            &format!("{mode}: Tab completa o link"),
        );
        session.edit("Par-fila", mode);
        session.queued(&[
            Step::Text("[[Par-alvo"),
            Step::Key(NONE, egui::Key::Tab),
            Step::Text(" queued"),
        ]);
        session.assert_saved(
            "Par-fila.md",
            b"[[Par-alvo]] queued",
            &format!("{mode}: Tab enfileirado"),
        );
    }
}

#[test]
fn enter_enfileirado_depois_de_parenteses_continua_a_lista() {
    for mode in MODES {
        let mut session = pairs_vault();
        session.edit("Par-enter", mode);
        session.queued(&[
            Step::Text(" ("),
            Step::Key(NONE, egui::Key::Enter),
            Step::Text("next"),
        ]);
        session.assert_saved("Par-enter.md", b"- Item (\n- next)", mode);
    }
}

#[test]
fn ctrl_enter_segue_o_link_recem_completado() {
    for mode in MODES {
        let mut session = pairs_vault();
        session.edit("Par-seguir", mode);
        session.queued(&[Step::Text("[[Par-alvo"), Step::Key(CTRL, egui::Key::Enter)]);
        session.wait_for(|app| {
            app.tabs
                .get(app.active)
                .is_some_and(|tab| tab.document.path.ends_with("Par-alvo.md"))
        });
        assert_eq!(
            session.active_tab_count(),
            2,
            "{mode}: o destino abriu numa aba"
        );
        assert_eq!(
            session.app().tabs[0].document.text,
            "[[Par-alvo]]",
            "{mode}: a nota de origem manteve o link"
        );
    }
}

#[test]
fn negrito_e_italico_alternam_marcadores_na_selecao() {
    for mode in MODES {
        let mut session = pairs_vault();
        session.edit("Par-formato", mode);
        session.select_all();
        let steps = [
            (egui::Key::B, "**word**"),
            (egui::Key::I, "***word***"),
            (egui::Key::I, "**word**"),
            (egui::Key::B, "word"),
            (egui::Key::I, "*word*"),
            (egui::Key::B, "***word***"),
            (egui::Key::B, "*word*"),
            (egui::Key::I, "word"),
        ];
        for (key, expected) in steps {
            session.key(CTRL, key);
            session.assert_saved("Par-formato.md", expected.as_bytes(), mode);
        }
    }
}

#[test]
fn texto_puro_nao_ganha_pares_de_markdown() {
    let mut session = pairs_vault();
    session.open_file("Par-literal.txt");
    session.type_chars("(");
    session.assert_saved("Par-literal.txt", b"(", "texto puro");
}
