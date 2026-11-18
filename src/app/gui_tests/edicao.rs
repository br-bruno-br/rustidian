//! R8b: fluxos de edição de `tests/gui_smoke.py` migrados para o egui_kittest (ver mod.rs).
//!
//! Cada teste monta o próprio cofre com as notas de que precisa, abre uma delas no modo
//! pedido, manda teclado e texto como eventos do egui e confere os bytes gravados no disco
//! (o mesmo critério do smoke em Python, que lia o arquivo depois de Ctrl+S).

use super::*;
use egui_kittest::kittest::NodeT;

pub(super) const NONE: egui::Modifiers = egui::Modifiers::NONE;
pub(super) const SHIFT: egui::Modifiers = egui::Modifiers::SHIFT;
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
pub(super) enum Step {
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
    pub(super) fn edit(&mut self, name: &str, mode: &str) {
        self.open_note(name);
        self.command(mode);
        self.settle();
        self.key(CTRL, egui::Key::End);
    }

    /// Todos os eventos no mesmo quadro.
    pub(super) fn queued(&mut self, steps: &[Step]) {
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
    pub(super) fn frame(&mut self) {
        let ctx = self.harness.ctx.clone();
        let mut raw = self.harness.input_mut().clone();
        eframe::App::raw_input_hook(self.harness.state_mut(), &ctx, &mut raw);
        *self.harness.input_mut() = raw;
        self.harness.step();
    }

    /// Um caractere por evento e por quadro, como o teclado de verdade.
    pub(super) fn type_chars(&mut self, text: &str) {
        for c in text.chars() {
            self.harness.event(egui::Event::Text(c.to_string()));
            self.harness.step();
        }
        self.settle();
    }

    pub(super) fn paste(&mut self, text: &str) {
        self.harness.event(egui::Event::Paste(text.to_owned()));
        self.settle();
    }

    pub(super) fn select_all(&mut self) {
        self.key(CTRL, egui::Key::A);
    }

    /// Ctrl+S e os bytes que ficaram no disco.
    pub(super) fn save_and_read(&mut self, name: &str) -> Vec<u8> {
        self.key(CTRL, egui::Key::S);
        std::fs::read(self.path(name)).expect("nota gravada")
    }

    pub(super) fn assert_saved(&mut self, name: &str, expected: &[u8], what: &str) {
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

    pub(super) fn has_value(&self, value: &str) -> bool {
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

    pub(super) fn click_value(&mut self, value: &str) {
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

    pub(super) fn click_label(&mut self, label: &str) {
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
    pub(super) fn wait_for_file(&mut self, name: &str, expected: &[u8]) {
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

impl Session {
    pub(super) fn right_click_role_nth(&mut self, role: egui::Role, nth: usize) {
        {
            let nodes: Vec<_> = self.harness.query_all_by_role(role).collect();
            nodes
                .get(nth)
                .unwrap_or_else(|| panic!("não há o {nth}º {role:?}\n{}", self.tree_summary()))
                .click_secondary();
        }
        self.settle();
    }

    /// Menu de contexto da célula (linha, coluna) de uma tabela de `colunas` colunas, onde a
    /// linha 0 é o cabeçalho, e o item `label`; depois Ctrl+S e os bytes do disco.
    fn table_action(
        &mut self,
        name: &str,
        row: usize,
        col: usize,
        colunas: usize,
        label: &str,
    ) -> String {
        self.right_click_role_nth(egui::Role::TextInput, row * colunas + col);
        self.click_label(label);
        String::from_utf8(self.save_and_read(name)).expect("UTF-8")
    }
}

#[test]
fn menu_da_tabela_insere_apaga_move_ordena_e_alinha_com_desfazer_do_documento() {
    let base = "# Visual table GUI\n\n| Item | Value |\n| :--- | ---: |\n| Edited π \\| value | 5 |\n| Queued \\| cell | 7 |\n\nAfter table π\n";
    let mut session = Session::open_with(&[("Menu.md", base.as_bytes())]);
    session.open_note("Menu");
    session.command("Live preview");
    session.settle();

    let inserted = base.replace(
        "| Queued \\| cell | 7 |",
        "|  |  |\n| Queued \\| cell | 7 |",
    );
    assert_eq!(
        session.table_action("Menu.md", 1, 0, 2, "Inserir linha abaixo"),
        inserted
    );
    session.key(CTRL, egui::Key::Z);
    session.assert_saved("Menu.md", base.as_bytes(), "desfazer a linha inserida");
    session.key(CTRL, egui::Key::Y);
    session.assert_saved("Menu.md", inserted.as_bytes(), "refazer a linha inserida");
    assert_eq!(
        session.table_action("Menu.md", 2, 0, 2, "Excluir linha"),
        base
    );
    let moved = base.replace(
        "| Edited π \\| value | 5 |\n| Queued \\| cell | 7 |",
        "| Queued \\| cell | 7 |\n| Edited π \\| value | 5 |",
    );
    assert_eq!(
        session.table_action("Menu.md", 1, 0, 2, "Mover linha para baixo"),
        moved
    );
    assert_eq!(
        session.table_action("Menu.md", 2, 0, 2, "Mover linha para cima"),
        base
    );
    assert_eq!(
        session.table_action("Menu.md", 1, 1, 2, "Ordem decrescente"),
        moved
    );
    assert_eq!(
        session.table_action("Menu.md", 1, 1, 2, "Ordem crescente"),
        base
    );

    // Colunas: a barra escapada pertence à célula, não separa colunas.
    let expanded = base
        .replace("| Item | Value |", "| Item |  | Value |")
        .replace("| :--- | ---: |", "| :--- | --- | ---: |")
        .replace(" value | 5 |", " value |  | 5 |")
        .replace(" cell | 7 |", " cell |  | 7 |");
    assert_eq!(
        session.table_action("Menu.md", 1, 0, 2, "Inserir coluna à direita"),
        expanded
    );
    let swapped = expanded
        .replace("| Item |  | Value |", "| Item | Value |  |")
        .replace("| :--- | --- | ---: |", "| :--- | ---: | --- |")
        .replace(" value |  | 5 |", " value | 5 |  |")
        .replace(" cell |  | 7 |", " cell | 7 |  |");
    assert_eq!(
        session.table_action("Menu.md", 1, 1, 3, "Mover coluna para a direita"),
        swapped
    );
    assert_eq!(
        session.table_action("Menu.md", 1, 2, 3, "Mover coluna para a esquerda"),
        expanded
    );
    assert_eq!(
        session.table_action("Menu.md", 1, 1, 3, "Excluir coluna"),
        base
    );
    let centered = base.replace("| :--- |", "| :---: |");
    assert_eq!(
        session.table_action("Menu.md", 1, 0, 2, "Centralizar"),
        centered
    );
    session.key(CTRL, egui::Key::Z);
    session.assert_saved("Menu.md", base.as_bytes(), "desfazer o alinhamento");
}

#[test]
fn tabela_editada_sobrevive_a_reabrir_a_leitura_e_ao_dobrar_a_secao() {
    let source =
        "# Visual table GUI\n\n| Item | Value |\n| :--- | ---: |\n| Alpha | 2 |\n\nAfter\n";
    let mut session = Session::open_with(&[("Dobra.md", source.as_bytes())]);
    session.open_note("Dobra");
    session.command("Live preview");
    session.settle();
    assert!(session.has_value("Alpha"));
    // Dobrar a seção do título esconde a tabela editável; desdobrar a traz de volta.
    session.click_label("Recolher seção");
    assert!(
        !session.has_value("Alpha"),
        "a seção dobrada esconde a tabela"
    );
    session.click_label("Expandir seção");
    assert!(session.has_value("Alpha"));
    // A leitura não oferece células editáveis nem aceita digitação.
    session.command("Reading view");
    session.settle();
    assert_eq!(
        session.count_role(egui::Role::TextInput),
        0,
        "a leitura não tem células editáveis"
    );
    session.type_text("READING_TABLE_MUST_NOT_EDIT");
    session.assert_saved("Dobra.md", source.as_bytes(), "leitura somente leitura");
    // Reabrir a nota devolve a tabela editável.
    session.open_note("Home");
    session.open_note("Dobra");
    session.command("Live preview");
    session.settle();
    assert!(session.has_value("Alpha"));
    assert_eq!(
        std::fs::read(session.path("Dobra.md")).unwrap(),
        source.as_bytes()
    );
}

#[test]
fn tabelas_em_citacao_e_lista_preservam_crlf_e_a_cerca_fica_literal() {
    let source = "# Nested editing\r\n\r\n> | Q | N |\r\n> | --- | --- |\r\n> | quoted | 1 |\r\n\r\n- | L | N |\r\n  | --- | --- |\r\n  | listed | 2 |\r\n\r\n```md\r\n| Fence | Cell |\r\n| --- | --- |\r\n| Literal | 0 |\r\n```\r\n";
    let mut session = Session::open_with(&[("Aninhada.md", source.as_bytes())]);
    session.open_note("Aninhada");
    session.command("Live preview");
    session.settle();
    assert!(!session.has_value("Fence"), "a tabela da cerca é só código");
    assert_eq!(session.count_role(egui::Role::TextInput), 8);

    session.click_value("quoted");
    session.select_all();
    session.type_text("quote π | cell");
    session.save_and_read("Aninhada.md");
    session.click_value("listed");
    session.select_all();
    session.type_text("list edit");
    let mut expected = source
        .replace("quoted", "quote π \\| cell")
        .replace("listed", "list edit");
    session.assert_saved(
        "Aninhada.md",
        expected.as_bytes(),
        "edição em citação e lista",
    );

    let saved = session.table_action("Aninhada.md", 1, 0, 2, "Inserir linha abaixo");
    expected = expected.replace(
        "> | quote π \\| cell | 1 |\r\n",
        "> | quote π \\| cell | 1 |\r\n> |  |  |\r\n",
    );
    assert_eq!(saved.as_bytes(), expected.as_bytes());

    session.click_value("2");
    session.select_all();
    session.type_text("3");
    session.key(NONE, egui::Key::Enter);
    session.type_text("4");
    expected = expected.replace(
        "  | list edit | 2 |\r\n",
        "  | list edit | 3 |\r\n  |  | 4 |\r\n",
    );
    session.assert_saved(
        "Aninhada.md",
        expected.as_bytes(),
        "Enter acrescenta uma linha",
    );

    // Escape devolve a digitação ao texto comum.
    session.key(NONE, egui::Key::Escape);
    session.key(CTRL, egui::Key::End);
    session.type_text("\nNormal prose");
    session.assert_saved(
        "Aninhada.md",
        format!("{expected}\nNormal prose").as_bytes(),
        "prosa depois das tabelas",
    );
}

#[test]
fn tabela_larga_de_vinte_colunas_edita_a_ultima_celula() {
    let header: Vec<String> = (0..20).map(|n| format!("Column {n:02}")).collect();
    let rule = vec!["---"; 20];
    let values: Vec<String> = (0..20).map(|n| format!("Value {n:02}")).collect();
    let source = format!(
        "| {} |\n| {} |\n| {} |\n\nAfter wide table\n",
        header.join(" | "),
        rule.join(" | "),
        values.join(" | ")
    );
    let mut session = Session::open_with(&[("Larga.md", source.as_bytes())]);
    session.open_note("Larga");
    session.command("Live preview");
    session.settle();
    // A última célula fica fora da área visível: a rolagem horizontal a traz.
    assert!(!session.has_value("Value 19"));
    let centre = session
        .harness
        .query_all_by_value("Value 00")
        .next()
        .expect("primeira célula")
        .rect()
        .center();
    session.harness.hover_at(centre);
    session.settle();
    for _ in 0..40 {
        if session.has_value("Value 19") {
            break;
        }
        session.harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(-3.0, 0.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: NONE,
        });
        session.settle();
    }
    session.click_value("Value 19");
    session.select_all();
    session.type_text("WIDE_EDIT");
    session.assert_saved(
        "Larga.md",
        source.replace("Value 19", "WIDE_EDIT").as_bytes(),
        "última coluna",
    );
}

#[test]
fn tabela_na_leitura_mostra_as_celulas_e_abre_link_com_apelido() {
    let source = "## Table section\n\n| Item | State | Count |\n| :--- | :---: | ---: |\n| **Bold** | *Ready* | 42 |\n| [[Home\\|Home note]] | `let x = 1;` | π |\n| Empty | | 0 |\n\n## After section\n\n~~~rust\nfn main() {\n    let answer: u32 = 42;\n}\n~~~\n";
    let mut session = Session::open_with(&[("Leitura.md", source.as_bytes())]);
    session.open_note("Leitura");
    session.command("Reading view");
    session.settle();
    assert!(session.has_value("Home note"));
    assert!(
        session.has_value("Empty"),
        "a leitura mostra as células vazias e cheias"
    );
    assert_eq!(
        std::fs::read(session.path("Leitura.md")).unwrap(),
        source.as_bytes()
    );
    // O link com apelido abre a nota existente, sem criar um arquivo com a barra escapada.
    session.click_value("Home note");
    session.wait_for(|app| {
        app.tabs
            .get(app.active)
            .is_some_and(|tab| tab.document.path.ends_with("Home.md"))
    });
    assert!(!session.path("Home\\.md").exists());
    assert_eq!(
        std::fs::read(session.path("Leitura.md")).unwrap(),
        source.as_bytes()
    );
}

// ---------------------------------------------------------------- folding

impl Session {
    fn click_label_nth(&mut self, label: &str, nth: usize) {
        {
            let nodes: Vec<_> = self.harness.query_all_by_label(label).collect();
            nodes
                .get(nth)
                .unwrap_or_else(|| panic!("não há o {nth}º {label:?}\n{}", self.tree_summary()))
                .click();
        }
        self.settle();
    }
}

impl Session {
    /// Quantas seções estão dobradas: cada título dobrado oferece "Expandir seção". (Os títulos
    /// escondidos dentro de uma seção dobrada não desenham botão.)
    fn folded_sections(&self) -> usize {
        self.harness.query_all_by_label("Expandir seção").count()
    }

    fn foldable_sections(&self) -> usize {
        self.harness.query_all_by_label("Recolher seção").count()
    }
}

const FOLDING: &str = "# Folding GUI\n\nBefore\n\n## Parent\n\nPARENT_BODY\n\n### Child\n\nCHILD_BODY\n\n## Sibling\n\nSIBLING_BODY\n\n[[#Child]]";

#[test]
fn dobrar_titulos_na_edicao_guarda_o_estado_e_nunca_grava() {
    for mode in MODES {
        let mut session = Session::open_with(&[("Dobras.md", FOLDING.as_bytes())]);
        session.open_note("Dobras");
        session.command(mode);
        session.settle();
        // Os botões seguem a ordem dos títulos: 0 raiz, 1 Parent, 2 Child, 3 Sibling.
        assert_eq!(session.foldable_sections(), 4, "{mode}");
        assert_eq!(session.folded_sections(), 0, "{mode}");
        session.click_label_nth("Recolher seção", 2);
        assert_eq!(session.folded_sections(), 1, "{mode}: Child dobrado");
        session.click_label_nth("Recolher seção", 1);
        // O título filho some com o pai: só o Parent e a raiz seguem com botão de dobrar...
        assert_eq!(
            session.folded_sections(),
            1,
            "{mode}: só o Parent aparece dobrado"
        );
        assert_eq!(
            session.foldable_sections(),
            2,
            "{mode}: a raiz e o irmão continuam visíveis"
        );
        // ...e desdobrar o pai mantém o filho dobrado.
        session.click_label_nth("Expandir seção", 0);
        assert_eq!(
            session.folded_sections(),
            1,
            "{mode}: o filho segue dobrado"
        );
        assert_eq!(session.foldable_sections(), 3, "{mode}");
        // Seguir o link interno abre os ancestrais e o alvo.
        session.key(CTRL, egui::Key::End);
        session.key(CTRL, egui::Key::Enter);
        assert_eq!(session.folded_sections(), 0, "{mode}: o link abriu o alvo");
        assert_eq!(
            std::fs::read(session.path("Dobras.md")).unwrap(),
            FOLDING.as_bytes(),
            "{mode}: dobrar nunca grava"
        );
    }
}

#[test]
fn editar_com_uma_secao_dobrada_preserva_o_fonte_escondido() {
    let mut session = Session::open_with(&[("Dobras.md", FOLDING.as_bytes())]);
    session.open_note("Dobras");
    session.command("Source editing");
    session.settle();
    session.click_label_nth("Recolher seção", 1);
    assert_eq!(session.folded_sections(), 1);
    session.key(CTRL, egui::Key::End);
    session.type_text("\nVISIBLE_EDIT");
    session.assert_saved(
        "Dobras.md",
        format!("{FOLDING}\nVISIBLE_EDIT").as_bytes(),
        "o texto escondido fica nos bytes",
    );
    session.key(CTRL, egui::Key::Z);
    session.assert_saved("Dobras.md", FOLDING.as_bytes(), "desfazer");
    assert_eq!(session.folded_sections(), 1, "a dobra sobrevive à edição");
}

// ---------------------------------------------------------------- callouts

const CALLOUTS: &str = "# Callout checks\n\n> [!warning]- **Careful** π\n> Hidden body\n>\n> > [!tip]+ Nested advice\n> > Nested body\n\n> [!info] Title only\n\n```md\n> [!danger] Literal\n```\n\nFollowing prose.\n";

impl Session {
    fn has_button(&self, label: &str) -> bool {
        self.harness
            .query_all_by_role_and_label(egui::Role::Button, label)
            .next()
            .is_some()
    }
}

#[test]
fn callout_no_source_e_texto_literal_e_a_cerca_nao_vira_callout() {
    let mut session = Session::open_with(&[("Callout.md", CALLOUTS.as_bytes())]);
    session.edit("Callout", "Source editing");
    session.settle();
    assert!(
        !session.has_button("**Careful** π"),
        "o Source não desenha callouts"
    );
    session.type_text("PROSE_CONTROL");
    session.assert_saved(
        "Callout.md",
        format!("{CALLOUTS}PROSE_CONTROL").as_bytes(),
        "digitação comum",
    );
    session.key(CTRL, egui::Key::Z);
    session.assert_saved("Callout.md", CALLOUTS.as_bytes(), "desfazer");
}

#[test]
fn callouts_no_live_dobram_e_trocam_de_tipo_com_desfazer_do_documento() {
    let mut session = Session::open_with(&[("Callout.md", CALLOUTS.as_bytes())]);
    session.edit("Callout", "Live preview");
    session.key(NONE, egui::Key::Escape);
    session.settle();
    assert!(session.has_button("**Careful** π"));
    assert!(session.has_button("Title only"));
    assert!(
        !session.has_button("Literal"),
        "o marcador dentro da cerca é literal"
    );
    // O callout aninhado só aparece com o pai aberto; a dobra por padrão de `-` vale.
    assert!(!session.has_button("Nested advice"));
    session.click_label("**Careful** π");
    assert!(session.has_button("Nested advice"), "abrir mostra o filho");
    session.click_label("**Careful** π");
    assert!(
        !session.has_button("Nested advice"),
        "fechar esconde o filho"
    );
    assert_eq!(
        std::fs::read(session.path("Callout.md")).unwrap(),
        CALLOUTS.as_bytes(),
        "dobrar não grava"
    );
    // O menu de tipo troca só o marcador e compartilha o desfazer do documento.
    session.right_click_label("**Careful** π");
    session.click_label("success");
    session.assert_saved(
        "Callout.md",
        CALLOUTS.replacen("[!warning]", "[!success]", 1).as_bytes(),
        "troca de tipo",
    );
    session.key(CTRL, egui::Key::Z);
    session.assert_saved("Callout.md", CALLOUTS.as_bytes(), "desfazer o tipo");
}

#[test]
fn callout_dobrado_no_fim_do_arquivo_abre_com_ctrl_end_e_aceita_digitacao() {
    let source = "# End fold\n\n> [!note]- End\n> Last body.";
    let mut session = Session::open_with(&[("Fim.md", source.as_bytes())]);
    session.open_note("Fim");
    session.command("Live preview");
    session.settle();
    session.key(CTRL, egui::Key::End);
    session.type_text(" EOF_EDIT");
    session.assert_saved(
        "Fim.md",
        format!("{source} EOF_EDIT").as_bytes(),
        "digitação no fim",
    );
    session.key(CTRL, egui::Key::Z);
    session.assert_saved("Fim.md", source.as_bytes(), "desfazer");
}

#[test]
fn callout_na_leitura_dobra_e_guarda_tarefa_tabela_e_codigo() {
    let rich = "# Rich callout\n\n> [!tip]+ Rich content\n> - [ ] Callout task\n>\n> | Name | Count |\n> | --- | --- |\n> | [[Home]] | 2 |\n>\n> ```rust\n> let value = 7;\n> ```\n>\n> π\n\nFollowing prose.\n";
    let mut session = Session::open_with(&[("Rico.md", rich.as_bytes())]);
    session.open_note("Rico");
    session.command("Reading view");
    session.settle();
    assert_eq!(
        session.count_role(egui::Role::CheckBox),
        1,
        "tarefa dentro do callout"
    );
    assert!(session.has_value("Count"), "tabela dentro do callout");
    assert!(
        session.has_label("Copiar código"),
        "código dentro do callout"
    );
    session.click_role_nth(egui::Role::CheckBox, 0);
    let checked = rich.replace("[ ] Callout task", "[x] Callout task");
    session.assert_saved("Rico.md", checked.as_bytes(), "tarefa marcada na leitura");
    // No Live, trocar o tipo antes de um deslocamento Unicode preserva o resto e o desfazer.
    session.command("Live preview");
    session.key(NONE, egui::Key::Escape);
    session.settle();
    session.right_click_label("Rich content");
    session.click_label("warning");
    session.assert_saved(
        "Rico.md",
        checked.replacen("[!tip]", "[!warning]", 1).as_bytes(),
        "tipo alterado",
    );
    session.key(CTRL, egui::Key::Z);
    session.assert_saved("Rico.md", checked.as_bytes(), "desfazer");
}

// ---------------------------------------------------------------- inline-styles

const INLINE_STYLES: &str = include_str!("../../../tests/fixtures/inline-styles.md");

#[test]
fn estilos_inline_em_tabelas_e_callouts_sao_desenhados_sem_mudar_o_fonte() {
    let mut session = Session::open_with(&[("Estilos.md", INLINE_STYLES.as_bytes())]);
    session.open_note("Estilos");
    // Leitura: o texto sai sem os atributos, e a tabela dentro do callout também.
    session.command("Reading view");
    session.settle();
    for shown in ["8h", "11h", "49h"] {
        assert!(
            session.has_value(shown),
            "a leitura mostra {shown} sem a marcação\n{}",
            session.tree_summary()
        );
    }
    assert!(
        !session.has_value(
            "<mark style=\"background-color:rgba(46,204,113,0.4);color:inherit\">49h</mark>"
        ),
        "o atributo bruto não aparece na célula"
    );
    // Live: as células editáveis guardam a marcação original.
    session.command("Live preview");
    session.settle();
    assert!(
        session.has_value("==11h=="),
        "célula Live com o realce original"
    );
    assert_eq!(
        std::fs::read(session.path("Estilos.md")).unwrap(),
        INLINE_STYLES.as_bytes()
    );
    // Source: edição comum, desfazer e salvar devolvem o documento exato.
    session.command("Source editing");
    session.settle();
    session.key(CTRL, egui::Key::End);
    session.type_text("EDIT_UNDO_CONTROL");
    session.key(CTRL, egui::Key::Z);
    session.assert_saved("Estilos.md", INLINE_STYLES.as_bytes(), "bytes originais");
}

// ---------------------------------------------------------------- note-aids

const MEETING: &str = "Hello {{title}}\n{{date:YYYY-MM-DD}} {{time:HH:mm}}\n{{content}}\n";

fn aids_vault() -> Session {
    Session::open_with(&[
        (
            ".obsidian/templates.json",
            br#"{"folder":"Templates","dateFormat":"DD/MM/YYYY","timeFormat":"HH:mm"}"#,
        ),
        ("Templates/Meeting.md", MEETING.as_bytes()),
        (
            "Templates/Daily.md",
            b"# {{title}}\n\n- [ ] Review {{date}}\n",
        ),
        ("Alvo.md", b"# Before\n\n"),
    ])
}

fn today(format: &str) -> String {
    chrono::Local::now().format(format).to_string()
}

#[test]
fn nota_diaria_padrao_nasce_vazia_e_reabre_sem_duplicar() {
    let mut session = Session::open();
    session.command("Daily notes: Open today's daily note");
    let name = format!("{}.md", today("%Y-%m-%d"));
    session.wait_for(|app| {
        app.tabs
            .get(app.active)
            .is_some_and(|tab| tab.document.path.ends_with(&name))
    });
    assert_eq!(std::fs::read(session.path(&name)).unwrap(), b"");
    session.command("Daily notes: Open today's daily note");
    session.settle();
    assert_eq!(std::fs::read(session.path(&name)).unwrap(), b"");
    let daily: Vec<_> = std::fs::read_dir(session.path(""))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(&today("%Y-%m-%d"))
        })
        .collect();
    assert_eq!(daily.len(), 1, "só uma nota do dia");
    assert_eq!(session.active_tab_count(), 1);
}

#[test]
fn modelo_escolhido_expande_titulo_data_e_hora_com_um_desfazer() {
    let mut session = aids_vault();
    session.edit("Alvo", "Source editing");
    // Cancelar o seletor não altera nada.
    session.command("Templates: Insert template");
    session.settle();
    assert!(session.has_label("Templates/Meeting.md") || session.has_label("Meeting"));
    session.key(NONE, egui::Key::Escape);
    assert_eq!(session.active_text(), "# Before\n\n");
    session.type_text("Cancelled chooser typing");
    session.assert_saved(
        "Alvo.md",
        b"# Before\n\nCancelled chooser typing",
        "digitação depois de cancelar",
    );
    session.key(CTRL, egui::Key::Z);
    session.assert_saved("Alvo.md", b"# Before\n\n", "desfazer a digitação");
    // Escolher o modelo filtrando pelo nome.
    session.command("Templates: Insert template");
    session.type_text("Mtg");
    session.key(NONE, egui::Key::Enter);
    let inserted = String::from_utf8(session.save_and_read("Alvo.md")).unwrap();
    assert!(
        inserted.starts_with("# Before\n\nHello Alvo\n"),
        "título expandido: {inserted:?}"
    );
    assert!(
        inserted.contains("{{content}}"),
        "variável desconhecida fica literal"
    );
    assert!(!inserted.contains("{{date") && !inserted.contains("{{time"));
    assert!(
        inserted.contains(&today("%Y-%m-%d")),
        "data do modelo: {inserted:?}"
    );
    session.key(CTRL, egui::Key::Z);
    session.assert_saved("Alvo.md", b"# Before\n\n", "um desfazer remove o modelo");
    session.key(CTRL | SHIFT, egui::Key::Z);
    session.assert_saved("Alvo.md", inserted.as_bytes(), "refazer devolve o modelo");
    assert_eq!(
        std::fs::read(session.path("Templates/Meeting.md")).unwrap(),
        MEETING.as_bytes(),
        "o modelo em si não muda"
    );
}

#[test]
fn comandos_de_data_e_hora_usam_os_formatos_do_cofre() {
    let mut session = aids_vault();
    session.edit("Alvo", "Source editing");
    session.type_text("\nDATE=");
    session.command("Templates: Insert current date");
    session.key(CTRL, egui::Key::End);
    let text = String::from_utf8(session.save_and_read("Alvo.md")).unwrap();
    assert!(
        text.ends_with(&format!("DATE={}", today("%d/%m/%Y"))),
        "data no formato DD/MM/YYYY: {text:?}"
    );
    session.type_text(" TIME=");
    session.command("Templates: Insert current time");
    let text = String::from_utf8(session.save_and_read("Alvo.md")).unwrap();
    let tail = text.rsplit(" TIME=").next().unwrap();
    assert!(
        tail.len() == 5
            && tail.as_bytes()[2] == b':'
            && tail.replace(':', "").chars().all(|c| c.is_ascii_digit()),
        "hora HH:mm: {tail:?}"
    );
}

#[test]
fn nota_diaria_configurada_cria_pastas_expande_o_modelo_uma_vez_e_preserva_a_edicao() {
    let mut session = Session::open_with(&[
        (
            ".obsidian/templates.json",
            br#"{"folder":"Templates","dateFormat":"DD/MM/YYYY","timeFormat":"HH:mm"}"#,
        ),
        (
            ".obsidian/daily-notes.json",
            br#"{"folder":"Journal","format":"YYYY/MMMM/YYYY-MMM-DD","template":"Templates/Daily"}"#,
        ),
        ("Templates/Daily.md", b"# {{title}}\n\n- [ ] Review {{date}}\n"),
    ]);
    session.command("Daily notes: Open today's daily note");
    session.wait_for(|app| {
        app.tabs.get(app.active).is_some_and(|tab| {
            tab.document
                .path
                .components()
                .any(|part| part.as_os_str() == "Journal")
        })
    });
    let relative = session.app().tabs[session.app().active]
        .document
        .path
        .clone();
    let path = session.folder.path().join(&relative);
    let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        format!("# {stem}\n\n- [ ] Review {}\n", today("%d/%m/%Y"))
    );
    assert!(
        path.parent().unwrap().ends_with(today("%B")),
        "pasta do mês criada: {path:?}"
    );
    session.command("Source editing");
    session.key(CTRL, egui::Key::End);
    session.type_text("\nKept daily text");
    session.key(CTRL, egui::Key::S);
    let kept = std::fs::read_to_string(&path).unwrap();
    session.command("Daily notes: Open today's daily note");
    session.settle();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), kept);
    assert!(kept.ends_with("Kept daily text"));
}

#[test]
fn nota_diaria_com_modelo_ausente_avisa_sem_criar_pastas_nem_trocar_de_nota() {
    let mut session = Session::open();
    session.open_note("Home");
    session.app_mut().note_aids.daily.folder = "Missing daily".into();
    session.app_mut().note_aids.daily.template = "Templates/Missing".into();
    session.command("Daily notes: Open today's daily note");
    session.settle();
    assert!(session.app().error.is_some(), "o erro é mostrado");
    assert!(!session.path("Missing daily").exists());
    assert!(session.active_path_ends_with("Home.md"));
    assert_eq!(
        std::fs::read(session.path("Home.md")).unwrap(),
        HOME.as_bytes()
    );
}

// ---------------------------------------------------------------- external-rename

impl Session {
    fn wait_for_active_path(&mut self, ends_with: &str) {
        let wanted = ends_with.to_owned();
        self.wait_for(|app| {
            app.tabs
                .get(app.active)
                .is_some_and(|tab| tab.document.path.ends_with(&wanted))
        });
    }

    fn wait_for_active_text(&mut self, expected: &str) {
        let expected = expected.to_owned();
        self.wait_for(|app| {
            app.tabs
                .get(app.active)
                .is_some_and(|tab| tab.document.text == expected)
        });
    }
}

#[test]
fn renomear_ou_mover_fora_do_app_reaponta_a_aba_e_atualiza_o_texto() {
    let original = "# External original\n\nEXTERNAL_RENAME_BODY\n";
    let sibling = "# Sibling\n\nKEEP_SIBLING\n";
    let mut session = Session::open_with(&[
        (
            "Outside moves/Inner/External original.md",
            original.as_bytes(),
        ),
        (
            "Outside moves/Inner/External sibling.md",
            sibling.as_bytes(),
        ),
    ]);
    session.open_note("External original");
    session.command("Source editing");
    session.settle();
    let folder = session.path("Outside moves/Inner");
    std::fs::rename(
        folder.join("External original.md"),
        folder.join("External renamed.md"),
    )
    .unwrap();
    session.wait_for_active_path("External renamed.md");
    assert_eq!(session.active_text(), original);
    assert_eq!(
        session.active_tab_count(),
        1,
        "a aba foi reapontada, não duplicada"
    );
    assert!(!folder.join("External original.md").exists());

    // Mover a pasta inteira também reaponta a aba; o irmão fica intacto. No Windows o
    // observador de pastas segura a pasta aberta e o sistema recusa o rename externo
    // ("Acesso negado"), então esse trecho só roda onde o rename é possível.
    let moved = if cfg!(windows) {
        folder.join("External renamed.md")
    } else {
        std::fs::rename(session.path("Outside moves"), session.path("Moved outside")).unwrap();
        session.wait_for(|app| {
            app.tabs.get(app.active).is_some_and(|tab| {
                tab.document
                    .path
                    .components()
                    .any(|part| part.as_os_str() == "Moved outside")
            })
        });
        let moved = session.path("Moved outside/Inner/External renamed.md");
        assert_eq!(std::fs::read_to_string(&moved).unwrap(), original);
        assert_eq!(
            std::fs::read_to_string(session.path("Moved outside/Inner/External sibling.md"))
                .unwrap(),
            sibling
        );
        moved
    };
    // Depois do rename, edições externas continuam chegando.
    let updated = format!("{original}EXTERNAL_UPDATE\n");
    std::fs::write(&moved, &updated).unwrap();
    session.wait_for_active_text(&updated);
}

#[test]
fn renomear_fora_com_rascunho_sujo_mantem_o_rascunho_e_salva_so_no_caminho_novo() {
    let original = "# External original\n\nEXTERNAL_RENAME_BODY\n";
    let mut session = Session::open_with(&[("Dirty source.md", original.as_bytes())]);
    session.open_note("Dirty source");
    session.command("Source editing");
    session.app_mut().autosave_enabled = false;
    session.key(CTRL, egui::Key::End);
    session.type_text("LOCAL_DIRTY");
    assert_eq!(
        std::fs::read_to_string(session.path("Dirty source.md")).unwrap(),
        original,
        "sem autosave nada foi gravado"
    );
    std::fs::rename(
        session.path("Dirty source.md"),
        session.path("Dirty renamed.md"),
    )
    .unwrap();
    session.wait_for_active_path("Dirty renamed.md");
    let dirty = format!("{original}LOCAL_DIRTY");
    assert_eq!(
        session.active_text(),
        dirty,
        "o rascunho sobreviveu à troca de nome"
    );
    session.key(CTRL, egui::Key::S);
    assert_eq!(
        std::fs::read_to_string(session.path("Dirty renamed.md")).unwrap(),
        dirty
    );
    assert!(
        !session.path("Dirty source.md").exists(),
        "o nome antigo não renasce"
    );
    // O histórico de desfazer acompanhou a nota.
    session.key(CTRL, egui::Key::Z);
    session.assert_saved(
        "Dirty renamed.md",
        original.as_bytes(),
        "desfazer no caminho novo",
    );
}

#[test]
fn com_autosave_a_nota_suja_segue_dois_renomes_seguidos_e_grava_so_no_destino_final() {
    let original = "# Autosave chain\n\nBODY\n";
    let mut session = Session::open_with(&[("Chain a.md", original.as_bytes())]);
    session.open_note("Chain a");
    session.command("Source editing");
    session.key(CTRL, egui::Key::End);
    session.type_text("AUTOSAVED_MOVE");
    std::fs::rename(session.path("Chain a.md"), session.path("Chain b.md")).unwrap();
    std::fs::rename(session.path("Chain b.md"), session.path("Chain c.md")).unwrap();
    session.wait_for_active_path("Chain c.md");
    session.wait_for_file("Chain c.md", format!("{original}AUTOSAVED_MOVE").as_bytes());
    assert!(!session.path("Chain a.md").exists());
    assert!(!session.path("Chain b.md").exists());
}

// ---------------------------------------------------------------- block-completion

const BLOCKS: &str = "# Blocks\n\nUnique paragraph café chosen.\n\n- List chosen first\n- List chosen second\n\n> Quote chosen line\n> more quote\n\n> [!note] Callout chosen\n> callout body\n\n| Table chosen | Value |\n| --- | --- |\n| cell | value |\n\n```rust\n// Code chosen ^literal-id\nlet x = 1;\n```\n\nExisting paragraph ^keep-id\n\n- Standalone chosen first\n- Standalone chosen second\n\n^standalone-list\n";

/// Os IDs que o app gera (`^r` e hexadecimal), no fim de linha.
fn generated_ids(text: &str) -> Vec<String> {
    let pattern = regex::Regex::new(r"(?m)(?: |^)\^(r[0-9a-f]+)\s*$").unwrap();
    pattern
        .captures_iter(text)
        .map(|found| found[1].to_owned())
        .collect()
}

fn without_generated_ids(text: &str) -> String {
    regex::Regex::new(r"(?: |\n\n)\^r[0-9a-f]+")
        .unwrap()
        .replace_all(text, "")
        .into_owned()
}

/// O ID do último `[[...#^id]]` no fim do texto.
fn last_reference(text: &str) -> String {
    let pattern = regex::Regex::new(r"\[\[[^\n]*#\^([A-Za-z0-9-]+)\]\]$").unwrap();
    pattern
        .captures(text)
        .unwrap_or_else(|| panic!("falta a referência de bloco completada em {text:?}"))[1]
        .to_owned()
}

fn read_text(session: &Session, name: &str) -> String {
    std::fs::read_to_string(session.path(name)).unwrap()
}

#[test]
fn completar_bloco_sem_id_cria_um_id_estavel_e_so_muda_esse_id() {
    // (consulta, texto do botão da sugestão)
    let cases = [
        ("Unique paragraph", "Unique paragraph café chosen."),
        ("List chosen", "List chosen first List chosen second"),
        ("Quote chosen", "Quote chosen line more quote"),
        ("Callout chosen", "[!note] Callout chosen callout body"),
        ("Table chosen", "Table chosenValuecellvalue"),
        ("Code chosen", "// Code chosen ^literal-id let x = 1;"),
    ];
    for mode in MODES {
        let mut session = Session::open_with(&[("Blocks.md", BLOCKS.as_bytes()), ("Refs.md", b"")]);
        session.edit("Refs", mode);
        for (query, label) in cases {
            let what = format!("{mode} / {query}");
            session.key(CTRL, egui::Key::End);
            session.paste(&format!("\n[[Blocks#^{query}"));
            session.click_label(label);
            let source = String::from_utf8(session.save_and_read("Refs.md")).unwrap();
            let reference = last_reference(&source);
            let target = read_text(&session, "Blocks.md");
            assert!(
                generated_ids(&target).contains(&reference),
                "{what}: o ID já estava no disco antes do link\n{target}"
            );
            assert_eq!(
                without_generated_ids(&target),
                BLOCKS,
                "{what}: só um ID pode mudar no destino"
            );
        }
        let target = read_text(&session, "Blocks.md");
        assert_eq!(
            generated_ids(&target).len(),
            cases.len(),
            "{mode}: um ID por bloco"
        );

        // Um bloco que já tem ID reaproveita o existente, sem mexer no destino.
        let known = generated_ids(&target)[0].clone();
        session.key(CTRL, egui::Key::End);
        session.paste("\n[[Blocks#^Unique paragraph");
        session.click_label_contains("Unique paragraph café chosen.");
        let source = String::from_utf8(session.save_and_read("Refs.md")).unwrap();
        assert_eq!(last_reference(&source), known, "{mode}: reaproveita o ID");
        assert_eq!(
            read_text(&session, "Blocks.md"),
            target,
            "{mode}: destino intacto"
        );

        // O ID escrito à mão numa lista independente aponta para a lista inteira.
        session.key(CTRL, egui::Key::End);
        session.paste("\n[[Blocks#^standalone-list");
        session.click_label_contains("^standalone-list");
        let source = String::from_utf8(session.save_and_read("Refs.md")).unwrap();
        assert_eq!(last_reference(&source), "standalone-list", "{mode}");
        assert_eq!(read_text(&session, "Blocks.md"), target);
    }
}

#[test]
fn completar_bloco_na_mesma_nota_antes_e_depois_do_cursor_preserva_o_unicode() {
    for mode in MODES {
        let mut session = Session::open_with(&[
            (
                "Self.md",
                "# Self\n\nSelf chosen café paragraph.\n\n".as_bytes(),
            ),
            ("After.md", b"[[#^After chosen\n\nAfter chosen paragraph.\n"),
        ]);
        session.edit("Self", mode);
        session.paste("[[#^Self chosen");
        session.click_label("Self chosen café paragraph.");
        let text = String::from_utf8(session.save_and_read("Self.md")).unwrap();
        let reference = last_reference(&text);
        assert_eq!(generated_ids(&text), [reference], "{mode}: um ID");
        assert!(text.contains("Self chosen café paragraph."));

        session.edit("After", mode);
        session.key(CTRL, egui::Key::Home);
        session.key(NONE, egui::Key::End);
        session.click_label("After chosen paragraph.");
        let text = String::from_utf8(session.save_and_read("After.md")).unwrap();
        let reference = first_reference(&text);
        assert_eq!(
            generated_ids(&text),
            [reference],
            "{mode}: um ID, link antes do bloco"
        );
        assert!(text.contains("After chosen paragraph."));
    }
}

#[test]
fn sugestao_de_bloco_usa_o_rascunho_nao_salvo_do_destino_e_o_desfazer_o_preserva() {
    let draft_original = "# Draft\n\nDraft base.\n";
    let mut session = Session::open_with(&[
        ("Draft.md", draft_original.as_bytes()),
        ("Draftsrc.md", b""),
    ]);
    session.app_mut().autosave_enabled = false;
    session.edit("Draft", "Source editing");
    session.paste("\n\nDRAFT chosen café paragraph.");
    let expected_draft = format!("{draft_original}\n\nDRAFT chosen café paragraph.");
    assert_eq!(session.active_text(), expected_draft);
    assert_eq!(
        read_text(&session, "Draft.md"),
        draft_original,
        "o rascunho não foi salvo"
    );
    // Outra aba escreve o link para um bloco que só existe no rascunho.
    session.key(CTRL, egui::Key::T);
    session.open_note("Draftsrc");
    session.command("Source editing");
    session.settle();
    session.paste("[[Draft#^DRAFT chosen");
    session.click_label("DRAFT chosen café paragraph.");
    let source = String::from_utf8(session.save_and_read("Draftsrc.md")).unwrap();
    let reference = last_reference(&source);
    let on_disk = read_text(&session, "Draft.md");
    assert_eq!(generated_ids(&on_disk), std::slice::from_ref(&reference));
    assert_eq!(without_generated_ids(&on_disk), expected_draft);
    // Fechar a aba da origem volta ao rascunho; desfazer tira só o ID, refazer o devolve.
    session.key(CTRL, egui::Key::W);
    session.wait_for_active_path("Draft.md");
    session.key(CTRL, egui::Key::Z);
    assert_eq!(
        session.active_text(),
        expected_draft,
        "desfazer preserva o rascunho"
    );
    session.key(CTRL | SHIFT, egui::Key::Z);
    let redone = String::from_utf8(session.save_and_read("Draft.md")).unwrap();
    assert_eq!(generated_ids(&redone), [reference]);
}

impl Session {
    fn click_label_contains(&mut self, label: &str) {
        {
            let node = self
                .harness
                .query_all_by_label_contains(label)
                .next()
                .unwrap_or_else(|| panic!("sem controle com {label:?}\n{}", self.tree_summary()));
            node.click();
        }
        self.settle();
    }
}

#[test]
fn pares_de_delimitadores_tambem_funcionam_dentro_de_uma_celula_de_tabela() {
    let source = "# Pair table\n\n| Value |\n| --- |\n| old |\n";
    let mut session = Session::open_with(&[("Pares.md", source.as_bytes())]);
    session.open_note("Pares");
    session.command("Live preview");
    session.settle();
    session.click_value("old");
    session.select_all();
    session.key(NONE, egui::Key::Backspace);
    session.type_chars("(");
    let saved = String::from_utf8(session.save_and_read("Pares.md")).unwrap();
    assert!(saved.contains("| () |"), "{saved:?}");
    session.type_chars("cell)");
    let saved = String::from_utf8(session.save_and_read("Pares.md")).unwrap();
    assert!(saved.contains("| (cell) |"), "{saved:?}");
}

/// O ID do primeiro `[[#^id]]` do texto (link para um bloco da mesma nota).
fn first_reference(text: &str) -> String {
    let pattern = regex::Regex::new(r"\[\[#\^([A-Za-z0-9-]+)\]\]").unwrap();
    pattern
        .captures(text)
        .unwrap_or_else(|| panic!("sem referência em {text:?}"))[1]
        .to_owned()
}
