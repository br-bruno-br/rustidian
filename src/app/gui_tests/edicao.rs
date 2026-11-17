//! R8b: fluxos de edição de `tests/gui_smoke.py` migrados para o egui_kittest (ver mod.rs).
//!
//! Cada teste monta o próprio cofre com as notas de que precisa, abre uma delas no modo
//! pedido, manda teclado e texto como eventos do egui e confere os bytes gravados no disco
//! (o mesmo critério do smoke em Python, que lia o arquivo depois de Ctrl+S).

use super::*;
use egui_kittest::kittest::NodeT;

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

// ---------------------------------------------------------------- ajudantes de widgets

impl Session {
    /// Papéis, rótulos e valores da árvore de acessibilidade, para mensagens de falha.
    fn tree_summary(&self) -> String {
        self.harness
            .query_all_by(|_| true)
            .filter_map(|node| {
                let node = node.accesskit_node();
                let label = node.label();
                let value = node.value();
                (label.is_some() || value.is_some()).then(|| {
                    format!(
                        "{:?} label={:?} value={:?}",
                        node.role(),
                        label.unwrap_or_default(),
                        value.unwrap_or_default()
                    )
                })
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn count_role(&self, role: egui::Role) -> usize {
        self.harness.query_all_by_role(role).count()
    }

    fn has_value(&self, value: &str) -> bool {
        self.harness.query_all_by_value(value).next().is_some()
    }

    fn click_role_nth(&mut self, role: egui::Role, nth: usize) {
        {
            let nodes: Vec<_> = self.harness.query_all_by_role(role).collect();
            nodes
                .get(nth)
                .unwrap_or_else(|| {
                    panic!(
                        "não há o {nth}º {role:?}; há {}\n{}",
                        nodes.len(),
                        self.tree_summary()
                    )
                })
                .click();
        }
        self.settle();
    }

    fn click_value(&mut self, value: &str) {
        {
            let node = self
                .harness
                .query_all_by_value(value)
                .next()
                .unwrap_or_else(|| {
                    panic!(
                        "sem controle com o valor {value:?}\n{}",
                        self.tree_summary()
                    )
                });
            node.click();
        }
        self.settle();
    }

    fn click_label(&mut self, label: &str) {
        {
            let node = self
                .harness
                .query_all_by_label(label)
                .next()
                .unwrap_or_else(|| {
                    panic!(
                        "sem controle com o rótulo {label:?}\n{}",
                        self.tree_summary()
                    )
                });
            node.click();
        }
        self.settle();
    }

    fn right_click_label(&mut self, label: &str) {
        {
            let node = self
                .harness
                .query_all_by_label(label)
                .next()
                .unwrap_or_else(|| {
                    panic!(
                        "sem controle com o rótulo {label:?}\n{}",
                        self.tree_summary()
                    )
                });
            node.click_secondary();
        }
        self.settle();
    }

    fn right_click_value(&mut self, value: &str) {
        {
            let node = self
                .harness
                .query_all_by_value(value)
                .next()
                .unwrap_or_else(|| {
                    panic!(
                        "sem controle com o valor {value:?}\n{}",
                        self.tree_summary()
                    )
                });
            node.click_secondary();
        }
        self.settle();
    }

    fn ctrl_click_value(&mut self, value: &str) {
        {
            let node = self
                .harness
                .query_all_by_value(value)
                .next()
                .unwrap_or_else(|| {
                    panic!(
                        "sem controle com o valor {value:?}\n{}",
                        self.tree_summary()
                    )
                });
            node.click_modifiers(CTRL);
        }
        self.settle();
    }

    fn active_path_ends_with(&self, name: &str) -> bool {
        self.app()
            .tabs
            .get(self.app().active)
            .is_some_and(|tab| tab.document.path.ends_with(name))
    }

    /// Espera o disco ter exatamente estes bytes (o autosave e a janela do watcher são
    /// assíncronos), em vez de supor um tempo fixo.
    fn wait_for_file(&mut self, name: &str, expected: &[u8]) {
        let path = self.path(name);
        for _ in 0..300 {
            if std::fs::read(&path).is_ok_and(|bytes| bytes == expected) {
                return;
            }
            self.harness.step();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            String::from_utf8_lossy(&std::fs::read(&path).unwrap_or_default()),
            String::from_utf8_lossy(expected),
            "o arquivo {name} não chegou ao conteúdo esperado em 3 s"
        );
    }
}

// ---------------------------------------------------------------- tasks

const TASKS: &str = "# Task GUI\n\n- [ ] Native task\n- [x] Done فارسی\n  - [ ] Nested\n\n```md\n- [ ] Literal code\n```\n";

#[test]
fn tarefas_na_leitura_e_no_live_gravam_markdown_exato_com_desfazer_e_refazer() {
    let mut session = Session::open_with(&[("Tarefas.md", TASKS.as_bytes())]);
    session.edit("Tarefas", "Source editing");
    session.type_text("control");
    session.assert_saved(
        "Tarefas.md",
        format!("{TASKS}control").as_bytes(),
        "digitação comum no Source",
    );
    session.key(CTRL, egui::Key::Z);
    session.assert_saved("Tarefas.md", TASKS.as_bytes(), "desfazer o controle");
    assert_eq!(
        session.count_role(egui::Role::CheckBox),
        0,
        "o Source mantém a sintaxe literal, sem caixas"
    );

    let checked = TASKS.replace("- [ ] Native task", "- [x] Native task");
    session.command("Reading view");
    session.settle();
    assert_eq!(
        session.count_role(egui::Role::CheckBox),
        3,
        "a leitura mostra as três tarefas e ignora a do bloco de código"
    );
    session.click_role_nth(egui::Role::CheckBox, 0);
    session.assert_saved("Tarefas.md", checked.as_bytes(), "marcar na leitura");
    session.click_role_nth(egui::Role::CheckBox, 0);
    session.assert_saved("Tarefas.md", TASKS.as_bytes(), "desmarcar na leitura");

    session.command("Live preview");
    session.settle();
    assert_eq!(
        session.count_role(egui::Role::CheckBox),
        3,
        "Live: três caixas"
    );
    session.click_role_nth(egui::Role::CheckBox, 0);
    session.assert_saved("Tarefas.md", checked.as_bytes(), "marcar no Live");
    session.key(CTRL, egui::Key::Z);
    session.assert_saved("Tarefas.md", TASKS.as_bytes(), "desfazer a marcação");
    session.key(CTRL, egui::Key::Y);
    session.assert_saved("Tarefas.md", checked.as_bytes(), "refazer a marcação");
    session.command("Source editing");
    session.settle();
    assert_eq!(
        session.count_role(egui::Role::CheckBox),
        0,
        "voltar ao Source tira as caixas"
    );
}

#[test]
fn lista_de_tarefas_frouxa_nao_muda_os_bytes_na_leitura() {
    let loose = "# Loose checklist\n\n- [ ] First paragraph\n\n- [x] Second paragraph\n";
    let mut session = Session::open_with(&[("Frouxa.md", loose.as_bytes())]);
    session.open_note("Frouxa");
    session.command("Reading view");
    session.settle();
    assert_eq!(session.count_role(egui::Role::CheckBox), 2);
    assert_eq!(
        std::fs::read(session.path("Frouxa.md")).unwrap(),
        loose.as_bytes()
    );
}

// ---------------------------------------------------------------- tables

const TABLE_INPUT: &str =
    "# Table input\n\n| A | B |\n| --- | --- |\n| old | 1 |\n| other | 2 |\n\nAfter\n";

#[test]
fn celula_de_tabela_recebe_texto_tab_e_enter_no_mesmo_quadro() {
    let mut session = Session::open_with(&[("Entrada.md", TABLE_INPUT.as_bytes())]);
    session.open_note("Entrada");
    session.command("Live preview");
    session.settle();
    session.click_value("old");
    session.select_all();
    session.queued(&[
        Step::Text("First | queued"),
        Step::Key(NONE, egui::Key::Tab),
        Step::Text("8"),
        Step::Key(NONE, egui::Key::Enter),
        Step::Text("9"),
        Step::Key(CTRL, egui::Key::S),
    ]);
    let expected = TABLE_INPUT
        .replace("old | 1", "First \\| queued | 8")
        .replace("other | 2", "other | 9");
    session.wait_for_file("Entrada.md", expected.as_bytes());
    // Escape devolve a digitação imediata ao texto comum.
    session.key(NONE, egui::Key::Escape);
    session.key(CTRL, egui::Key::End);
    session.type_text("PROSE");
    session.assert_saved(
        "Entrada.md",
        format!("{expected}PROSE").as_bytes(),
        "prosa depois da tabela",
    );
}

const TABLE_LINKS: &str =
    "# Table links\n\n| Destination | Literal |\n| --- | --- |\n| [[Home]] | `[[Never open]]` |\n";

#[test]
fn links_em_celulas_abrem_com_ctrl_enter_e_ctrl_clique_menos_dentro_de_codigo() {
    let mut session = Session::open_with(&[("Links.md", TABLE_LINKS.as_bytes())]);
    session.open_note("Links");
    session.command("Live preview");
    session.settle();
    // O link dentro de código inline continua literal.
    session.click_value("`[[Never open]]`");
    session.key(CTRL, egui::Key::Enter);
    assert!(session.active_path_ends_with("Links.md"));
    assert!(!session.path("Never open.md").exists());
    // O link de verdade abre, inclusive digitado e seguido no mesmo quadro.
    session.click_value("[[Home]]");
    session.select_all();
    session.queued(&[Step::Text("[[Home]]"), Step::Key(CTRL, egui::Key::Enter)]);
    session.wait_for(|app| {
        app.tabs
            .get(app.active)
            .is_some_and(|tab| tab.document.path.ends_with("Home.md"))
    });
    assert_eq!(
        std::fs::read(session.path("Links.md")).unwrap(),
        TABLE_LINKS.as_bytes()
    );
    // Ctrl+clique também segue.
    session.open_note("Links");
    session.command("Live preview");
    session.settle();
    session.ctrl_click_value("[[Home]]");
    session.wait_for(|app| {
        app.tabs
            .get(app.active)
            .is_some_and(|tab| tab.document.path.ends_with("Home.md"))
    });
    assert_eq!(
        std::fs::read(session.path("Links.md")).unwrap(),
        TABLE_LINKS.as_bytes()
    );
}

const TABLE_EDIT: &str = "# Visual table GUI\n\n| Item | Value |\n| :--- | ---: |\n| **Alpha** | 2 |\n| Beta | 1 |\n\nAfter table π\n";

#[test]
fn tabela_visual_edita_celulas_com_unicode_barra_escapada_tab_e_desfazer() {
    let mut session = Session::open_with(&[("Visual.md", TABLE_EDIT.as_bytes())]);
    session.open_note("Visual");
    session.command("Source editing");
    session.settle();
    assert!(
        !session.has_value("**Alpha**"),
        "o Source não mostra células editáveis"
    );
    session.command("Live preview");
    session.settle();
    session.click_value("**Alpha**");
    session.select_all();
    session.type_text("Edited π | value");
    session.key(NONE, egui::Key::Tab);
    session.type_text("5");
    let expected = TABLE_EDIT
        .replace("**Alpha**", "Edited π \\| value")
        .replace("| 2 |", "| 5 |");
    session.assert_saved("Visual.md", expected.as_bytes(), "edição de células");
    // O desfazer é o do documento inteiro.
    session.key(CTRL, egui::Key::Z);
    session.assert_saved(
        "Visual.md",
        expected.replace("| 5 |", "| 2 |").as_bytes(),
        "desfazer",
    );
    session.key(CTRL, egui::Key::Y);
    session.assert_saved("Visual.md", expected.as_bytes(), "refazer");
    // Enter desce, Shift+Tab volta; o último trecho vai no mesmo quadro.
    session.key(NONE, egui::Key::Enter);
    session.type_text("7");
    session.key(SHIFT, egui::Key::Tab);
    session.queued(&[
        Step::Text("Queued | cell"),
        Step::Key(NONE, egui::Key::Tab),
        Step::Key(CTRL, egui::Key::S),
    ]);
    let expected = expected.replace("| Beta | 1 |", "| Queued \\| cell | 7 |");
    session.wait_for_file("Visual.md", expected.as_bytes());
}
