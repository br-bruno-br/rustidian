//! The same available commands and shortcuts feed the palette and settings.
use nucleo_matcher::{
    Config, Matcher,
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    Global,
    Tab,
    File,
    Document,
    Editor,
}

impl Scope {
    fn includes(self, required: Self) -> bool {
        self >= required
    }
}

pub const ADD_UNIQUE_LINK: &str = "Unique note creator: Add unique internal link";
pub const DUPLICATE_FILE: &str = "Make a copy of the current file";
pub const MOVE_FILE: &str = "Move current file to another folder";
// Histórico H3/H4
pub const UNDO_BRANCHES: &str = "History: Undo branches";
pub const FIND_ERASED: &str = "History: Find what I erased";
pub const PASSAGE_HISTORY: &str = "History: History of this passage";
pub const UNDO_CLOSE_TAB: &str = "Undo close tab";
pub const SPLIT_RIGHT: &str = "Split right";
pub const SPLIT_DOWN: &str = "Split down";
pub const CLOSE_PANE: &str = "Close split pane";
pub const TOGGLE_CANVAS_LIBRARY: &str = "Canvas: Toggle note library";
pub const FOCUS_OTHER_PANE: &str = "Focus the other pane";
pub const TOGGLE_NOTE_BOARD: &str = "Board: Toggle between note and board";
pub const TOGGLE_NOTE_BOARD_SHORTCUT: &str = "Ctrl+Shift+B";
pub const GO_TO_NOTE: &str = "Board: Go to the note";
pub const GO_TO_BOARD: &str = "Board: Go to the board";
pub const GO_TO_LIST: &str = "Board: Go to the list";
pub const EXPORT_BOARD_AS_NOTE: &str = "Board: Export the list as a note (copy)";
pub const CLOSE_OTHER_TABS: &str = "Close all other tabs";
pub const OPEN_FILE: &str = "Open file…";
pub const OPEN_FOLDER: &str = "Open folder…";
pub const OPEN_FILE_SHORTCUT: &str = "Ctrl+Shift+O";
pub const OPEN_FOLDER_SHORTCUT: &str = "Ctrl+Alt+O";
#[cfg(feature = "agente")]
pub const OPEN_AGENT: &str = "Agent: Open agent tab";
pub const OPEN_GRAPH: &str = "Graph view: Open graph view";
pub const OPEN_LOCAL_GRAPH: &str = "Graph view: Open local graph";
pub const INCREASE_FONT_SIZE: &str = "Increase font size";
pub const DECREASE_FONT_SIZE: &str = "Decrease font size";
pub const INSERT_TABLE: &str = "Tables: Insert table";
pub const TABLE_FROM_SELECTION: &str = "Tables: Convert selection to table";
pub const FORMAT_TABLE: &str = "Tables: Align the table at the cursor";
pub const FORMAT_ALL_TABLES: &str = "Tables: Align all tables in the note";

pub struct Command {
    pub name: &'static str,
    pub shortcut: &'static str,
    pub scope: Scope,
}

impl AsRef<str> for Command {
    fn as_ref(&self) -> &str {
        self.name
    }
}

pub const ALL: &[Command] = &[
    Command {
        name: OPEN_FILE,
        shortcut: OPEN_FILE_SHORTCUT,
        scope: Scope::Global,
    },
    Command {
        name: OPEN_FOLDER,
        shortcut: OPEN_FOLDER_SHORTCUT,
        scope: Scope::Global,
    },
    #[cfg(feature = "agente")]
    Command {
        name: OPEN_AGENT,
        shortcut: "Ctrl+Shift+A",
        scope: Scope::Global,
    },
    Command {
        name: OPEN_GRAPH,
        shortcut: "",
        scope: Scope::Global,
    },
    Command {
        name: OPEN_LOCAL_GRAPH,
        shortcut: "",
        scope: Scope::Document,
    },
    Command {
        name: INCREASE_FONT_SIZE,
        shortcut: "",
        scope: Scope::Global,
    },
    Command {
        name: DECREASE_FONT_SIZE,
        shortcut: "",
        scope: Scope::Global,
    },
    Command {
        name: CLOSE_OTHER_TABS,
        shortcut: "",
        scope: Scope::Tab,
    },
    Command {
        name: UNDO_CLOSE_TAB,
        shortcut: "Ctrl+Shift+T",
        scope: Scope::Global,
    },
    Command {
        name: SPLIT_RIGHT,
        shortcut: "",
        scope: Scope::Tab,
    },
    Command {
        name: SPLIT_DOWN,
        shortcut: "",
        scope: Scope::Tab,
    },
    Command {
        name: CLOSE_PANE,
        shortcut: "",
        scope: Scope::Global,
    },
    Command {
        name: FOCUS_OTHER_PANE,
        shortcut: "",
        scope: Scope::Global,
    },
    Command {
        name: TOGGLE_CANVAS_LIBRARY,
        shortcut: "",
        scope: Scope::Document,
    },
    Command {
        name: TOGGLE_NOTE_BOARD,
        shortcut: TOGGLE_NOTE_BOARD_SHORTCUT,
        scope: Scope::Document,
    },
    Command {
        name: GO_TO_NOTE,
        shortcut: "",
        scope: Scope::Document,
    },
    Command {
        name: GO_TO_BOARD,
        shortcut: "",
        scope: Scope::Document,
    },
    Command {
        name: GO_TO_LIST,
        shortcut: "",
        scope: Scope::Document,
    },
    Command {
        name: EXPORT_BOARD_AS_NOTE,
        shortcut: "",
        scope: Scope::Document,
    },
    Command {
        name: MOVE_FILE,
        shortcut: "",
        scope: Scope::File,
    },
    Command {
        name: DUPLICATE_FILE,
        shortcut: "",
        scope: Scope::File,
    },
    Command {
        name: "Toggle pin",
        shortcut: "",
        scope: Scope::Tab,
    },
    Command {
        name: "New tab",
        shortcut: "Ctrl+T",
        scope: Scope::Global,
    },
    Command {
        name: "Close current tab",
        shortcut: "Ctrl+W",
        scope: Scope::Tab,
    },
    Command {
        name: "Go to next tab",
        shortcut: "Ctrl+Tab",
        scope: Scope::Tab,
    },
    Command {
        name: "Go to previous tab",
        shortcut: "Ctrl+Shift+Tab",
        scope: Scope::Tab,
    },
    Command {
        name: ADD_UNIQUE_LINK,
        shortcut: "",
        scope: Scope::Editor,
    },
    Command {
        name: INSERT_TABLE,
        shortcut: "",
        scope: Scope::Editor,
    },
    Command {
        name: TABLE_FROM_SELECTION,
        shortcut: "",
        scope: Scope::Editor,
    },
    Command {
        name: FORMAT_TABLE,
        shortcut: "",
        scope: Scope::Editor,
    },
    Command {
        name: FORMAT_ALL_TABLES,
        shortcut: "",
        scope: Scope::Editor,
    },
    Command {
        name: "Unique note creator: Create new unique note",
        shortcut: "",
        scope: Scope::Global,
    },
    Command {
        name: "Random note: Open random note",
        shortcut: "",
        scope: Scope::Global,
    },
    Command {
        name: "File recovery: Open local history",
        shortcut: "",
        scope: Scope::Global,
    },
    // Histórico H3/H4
    Command {
        name: UNDO_BRANCHES,
        shortcut: "",
        scope: Scope::Document,
    },
    Command {
        name: FIND_ERASED,
        shortcut: "",
        scope: Scope::Document,
    },
    Command {
        name: PASSAGE_HISTORY,
        shortcut: "",
        scope: Scope::Document,
    },
    Command {
        name: "Workspaces: Manage workspace layouts",
        shortcut: "",
        scope: Scope::Global,
    },
    Command {
        name: "Workspaces: Load workspace layout",
        shortcut: "",
        scope: Scope::Global,
    },
    Command {
        name: "Workspaces: Save layout",
        shortcut: "",
        scope: Scope::Global,
    },
    Command {
        name: "Workspaces: Save and load another layout",
        shortcut: "",
        scope: Scope::Global,
    },
    Command {
        name: "New note",
        shortcut: "Ctrl+N",
        scope: Scope::Global,
    },
    Command {
        name: "New canvas",
        shortcut: "",
        scope: Scope::Global,
    },
    Command {
        name: "New folder",
        shortcut: "",
        scope: Scope::Global,
    },
    Command {
        name: "Daily notes: Open today's daily note",
        shortcut: "",
        scope: Scope::Global,
    },
    Command {
        name: "Templates: Insert template",
        shortcut: "",
        scope: Scope::Document,
    },
    Command {
        name: "Templates: Insert current date",
        shortcut: "",
        scope: Scope::Document,
    },
    Command {
        name: "Templates: Insert current time",
        shortcut: "",
        scope: Scope::Document,
    },
    Command {
        name: "Open a file",
        shortcut: "Ctrl+O",
        scope: Scope::Global,
    },
    Command {
        name: "Open vault",
        shortcut: "",
        scope: Scope::Global,
    },
    Command {
        name: "Open another vault",
        shortcut: "",
        scope: Scope::Global,
    },
    Command {
        name: "Create new vault",
        shortcut: "",
        scope: Scope::Global,
    },
    Command {
        name: "Save note",
        shortcut: "Ctrl+S",
        scope: Scope::Document,
    },
    Command {
        name: "Save a copy",
        shortcut: "",
        scope: Scope::Document,
    },
    Command {
        name: "Reading view",
        shortcut: "Ctrl+E",
        scope: Scope::Document,
    },
    Command {
        name: "Live preview",
        shortcut: "",
        scope: Scope::Document,
    },
    Command {
        name: "Source editing",
        shortcut: "",
        scope: Scope::Document,
    },
    Command {
        name: "Split source and preview",
        shortcut: "",
        scope: Scope::Document,
    },
    Command {
        name: "Add property",
        shortcut: "Ctrl+;",
        scope: Scope::Document,
    },
    Command {
        name: crate::highlight::NativeColor::Blue.command(),
        shortcut: "",
        scope: Scope::Editor,
    },
    Command {
        name: crate::highlight::NativeColor::Green.command(),
        shortcut: "",
        scope: Scope::Editor,
    },
    Command {
        name: crate::highlight::NativeColor::Orange.command(),
        shortcut: "",
        scope: Scope::Editor,
    },
    Command {
        name: crate::highlight::NativeColor::Purple.command(),
        shortcut: "",
        scope: Scope::Editor,
    },
    Command {
        name: crate::highlight::NativeColor::Red.command(),
        shortcut: "",
        scope: Scope::Editor,
    },
    Command {
        name: crate::highlight::NativeColor::Yellow.command(),
        shortcut: "",
        scope: Scope::Editor,
    },
    Command {
        name: "Highlight selection",
        shortcut: "Ctrl+Shift+H",
        scope: Scope::Document,
    },
    Command {
        name: crate::highlight::Color::Cyan.command(),
        shortcut: "",
        scope: Scope::Document,
    },
    Command {
        name: crate::highlight::Color::Green.command(),
        shortcut: "",
        scope: Scope::Document,
    },
    Command {
        name: crate::highlight::Color::Orange.command(),
        shortcut: "",
        scope: Scope::Document,
    },
    Command {
        name: crate::highlight::Color::Red.command(),
        shortcut: "",
        scope: Scope::Document,
    },
    Command {
        name: crate::highlight::Color::Yellow.command(),
        shortcut: "",
        scope: Scope::Document,
    },
    Command {
        name: "Rename or move file",
        shortcut: "F2",
        scope: Scope::Global,
    },
    Command {
        name: "Move file to system Trash",
        shortcut: "Delete",
        scope: Scope::Global,
    },
    Command {
        name: "Bookmark current file",
        shortcut: "",
        scope: Scope::Global,
    },
    Command {
        name: "Show bookmarks",
        shortcut: "",
        scope: Scope::Global,
    },
    Command {
        name: "Toggle left sidebar",
        shortcut: "",
        scope: Scope::Global,
    },
    Command {
        name: "Toggle right sidebar",
        shortcut: "",
        scope: Scope::Global,
    },
    Command {
        name: "Switch appearance",
        shortcut: "",
        scope: Scope::Global,
    },
    Command {
        name: "Refresh vault",
        shortcut: "",
        scope: Scope::Global,
    },
    Command {
        name: "Open settings",
        shortcut: "Ctrl+,",
        scope: Scope::Global,
    },
];

/// The name shown to the user. The English `name` stays the command's identity (dispatch, the
/// screen test's probe ids); the palette shows this and matches a query against either.
pub fn label(name: &str) -> &str {
    match name {
        ADD_UNIQUE_LINK => "Criador de nota única: Adicionar link interno único",
        DUPLICATE_FILE => "Fazer uma cópia do arquivo atual",
        MOVE_FILE => "Mover o arquivo atual para outra pasta",
        UNDO_CLOSE_TAB => "Reabrir aba fechada",
        SPLIT_RIGHT => "Dividir à direita",
        SPLIT_DOWN => "Dividir abaixo",
        CLOSE_PANE => "Fechar o painel dividido",
        FOCUS_OTHER_PANE => "Ir para o outro painel",
        TOGGLE_CANVAS_LIBRARY => "Quadro: Mostrar ou esconder a biblioteca de notas",
        TOGGLE_NOTE_BOARD => "Quadro: Alternar entre nota, quadro e lista",
        GO_TO_NOTE => "Ir para a nota",
        GO_TO_BOARD => "Ir para o quadro",
        GO_TO_LIST => "Ir para a lista",
        EXPORT_BOARD_AS_NOTE => "Quadro: Exportar a lista como nota (cópia)",
        CLOSE_OTHER_TABS => "Fechar todas as outras abas",
        OPEN_FILE => "Abrir arquivo…",
        OPEN_FOLDER => "Abrir pasta…",
        OPEN_GRAPH => "Visão de grafo: Abrir visão de grafo",
        OPEN_LOCAL_GRAPH => "Visão de grafo: Abrir grafo local",
        INCREASE_FONT_SIZE => "Aumentar tamanho da fonte",
        DECREASE_FONT_SIZE => "Diminuir tamanho da fonte",
        INSERT_TABLE => "Tabelas: Inserir tabela",
        TABLE_FROM_SELECTION => "Tabelas: Converter a seleção em tabela",
        FORMAT_TABLE => "Tabelas: Alinhar a tabela sob o cursor",
        FORMAT_ALL_TABLES => "Tabelas: Alinhar todas as tabelas da nota",
        "Agent: Open agent tab" => "Agente: Abrir aba do agente",
        "Toggle pin" => "Fixar ou desafixar aba",
        "New tab" => "Nova aba",
        "Close current tab" => "Fechar aba atual",
        "Go to next tab" => "Ir para a próxima aba",
        "Go to previous tab" => "Ir para a aba anterior",
        "Unique note creator: Create new unique note" => "Criador de nota única: Criar nota única",
        "Random note: Open random note" => "Nota aleatória: Abrir nota aleatória",
        "File recovery: Open local history" => "Recuperação de arquivo: Abrir histórico local",
        // Histórico H3/H4
        UNDO_BRANCHES => "Histórico: Galhos do desfazer",
        FIND_ERASED => "Histórico: Achar o que apaguei",
        PASSAGE_HISTORY => "Histórico: Histórico deste trecho",
        "Workspaces: Manage workspace layouts" => {
            "Espaços de trabalho: Gerenciar layouts de espaço de trabalho"
        }
        "Workspaces: Load workspace layout" => {
            "Espaços de trabalho: Carregar layout de espaço de trabalho"
        }
        "Workspaces: Save layout" => "Espaços de trabalho: Salvar layout",
        "Workspaces: Save and load another layout" => {
            "Espaços de trabalho: Salvar e carregar outro layout"
        }
        "New note" => "Nova nota",
        "New canvas" => "Novo quadro",
        "New folder" => "Nova pasta",
        "Daily notes: Open today's daily note" => "Notas diárias: Abrir a nota diária de hoje",
        "Templates: Insert template" => "Modelos: Inserir modelo",
        "Templates: Insert current date" => "Modelos: Inserir data atual",
        "Templates: Insert current time" => "Modelos: Inserir hora atual",
        "Open a file" => "Abrir um arquivo",
        "Open vault" => "Abrir cofre",
        "Open another vault" => "Abrir outro cofre",
        "Create new vault" => "Criar novo cofre",
        "Save note" => "Salvar nota",
        "Save a copy" => "Salvar uma cópia",
        "Reading view" => "Modo de leitura",
        "Live preview" => "Pré-visualização ao vivo",
        "Source editing" => "Edição do código-fonte",
        "Split source and preview" => "Dividir código-fonte e pré-visualização",
        "Add property" => "Adicionar propriedade",
        "Highlight Cyan" => "Destacar em ciano",
        "Highlight Green" => "Destacar em verde",
        "Highlight Orange" => "Destacar em laranja",
        "Highlight Red" => "Destacar em vermelho",
        "Highlight Yellow" => "Destacar em amarelo",
        "Toggle blue highlight" => "Alternar destaque azul",
        "Toggle green highlight" => "Alternar destaque verde",
        "Toggle orange highlight" => "Alternar destaque laranja",
        "Toggle purple highlight" => "Alternar destaque roxo",
        "Toggle red highlight" => "Alternar destaque vermelho",
        "Toggle yellow highlight" => "Alternar destaque amarelo",
        "Highlight selection" => "Destacar seleção",
        "Rename or move file" => "Renomear ou mover arquivo",
        "Move file to system Trash" => "Mover arquivo para a Lixeira",
        "Bookmark current file" => "Favoritar o arquivo atual",
        "Show bookmarks" => "Mostrar favoritos",
        "Toggle left sidebar" => "Alternar painel lateral esquerdo",
        "Toggle right sidebar" => "Alternar painel lateral direito",
        "Switch appearance" => "Alternar aparência",
        "Refresh vault" => "Atualizar cofre",
        "Open settings" => "Abrir configurações",
        other => other,
    }
}

pub fn ranked(query: &str, scope: Scope) -> Vec<&'static Command> {
    let query = query.trim();
    let pattern = Pattern::new(
        query,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );
    let mut matcher = Matcher::new(Config::DEFAULT);
    let available = || ALL.iter().filter(|command| scope.includes(command.scope));
    // A query matches the identifying (English) name, fuzzily, or the name shown (Portuguese),
    // by whole pieces: a fuzzy match on the longer Portuguese text would find too much.
    let shown_pattern = Pattern::new(
        query,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Substring,
    );
    let mut scored: Vec<(&'static Command, u32)> = pattern
        .match_list(available(), &mut matcher)
        .into_iter()
        .collect();
    for (shown, score) in shown_pattern.match_list(available().map(Shown), &mut matcher) {
        match scored
            .iter_mut()
            .find(|(known, _)| std::ptr::eq(*known, shown.0))
        {
            Some(entry) => entry.1 = entry.1.max(score),
            None => scored.push((shown.0, score)),
        }
    }
    // O casamento por pedaços do nucleo falha quando o nome mostrado tem letra acentuada e a
    // busca não (digitar "direita" não achava "Dividir à direita"). Este complemento compara sem
    // acento nem maiúscula e só acrescenta o que o nucleo não achou.
    let words: Vec<String> = query.split_whitespace().map(fold).collect();
    if !words.is_empty() {
        for command in available() {
            if scored
                .iter()
                .any(|(known, _)| std::ptr::eq(*known, command))
            {
                continue;
            }
            let shown = fold(label(command.name));
            if words.iter().all(|word| shown.contains(word.as_str())) {
                scored.push((command, 1));
            }
        }
    }
    scored.sort_by_key(|entry| std::cmp::Reverse(entry.1));
    let mut matches: Vec<_> = scored.into_iter().map(|(command, _)| command).collect();
    if let Some(index) = matches.iter().position(|command| {
        command.name.eq_ignore_ascii_case(query) || label(command.name).eq_ignore_ascii_case(query)
    }) {
        let exact = matches.remove(index);
        matches.insert(0, exact);
    }
    matches
}

/// Minúsculas e sem acento (só o que o português usa), para comparar o que se digita com o nome
/// mostrado.
fn fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|character| match character {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            other => other,
        })
        .collect()
}

struct Shown(&'static Command);

impl AsRef<str> for Shown {
    fn as_ref(&self) -> &str {
        label(self.0.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_portable_and_native_highlight_names_win_over_similar_commands() {
        for command in crate::highlight::Color::ALL
            .into_iter()
            .map(|color| color.command())
            .chain(
                crate::highlight::NativeColor::ALL
                    .into_iter()
                    .map(|color| color.command()),
            )
        {
            assert_eq!(ranked(command, Scope::Editor)[0].name, command);
            assert_eq!(
                ranked(&command.to_lowercase(), Scope::Editor)[0].name,
                command
            );
        }
        assert!(ranked("not an available command 987654", Scope::Editor).is_empty());
    }
    #[test]
    fn a_busca_acha_o_nome_mostrado_com_ou_sem_acento() {
        for query in [
            "Dividir à direita",
            "dividir a direita",
            "direita",
            "à direita",
        ] {
            assert_eq!(ranked(query, Scope::Editor)[0].name, SPLIT_RIGHT, "{query}");
        }
        assert_eq!(ranked("Dividir abaixo", Scope::Tab)[0].name, SPLIT_DOWN);
        assert!(ranked("Dividir à direita", Scope::Global).is_empty());
        assert_eq!(
            ranked("outro painel", Scope::Global)[0].name,
            FOCUS_OTHER_PANE
        );
    }

    #[test]
    fn native_color_toggles_require_an_actual_document_editor() {
        for color in crate::highlight::NativeColor::ALL {
            assert_eq!(
                ranked(color.command(), Scope::Editor)[0].name,
                color.command()
            );
            for unavailable in [Scope::Document, Scope::File, Scope::Tab, Scope::Global] {
                assert!(
                    ranked(color.command(), unavailable)
                        .iter()
                        .all(|command| command.name != color.command())
                );
            }
        }
    }

    #[test]
    fn unique_link_requires_an_editor_but_note_navigation_remains_global() {
        assert_eq!(
            ranked(ADD_UNIQUE_LINK, Scope::Editor)[0].name,
            ADD_UNIQUE_LINK
        );
        assert!(ranked(ADD_UNIQUE_LINK, Scope::Document).is_empty());
        assert!(ranked(ADD_UNIQUE_LINK, Scope::Tab).is_empty());
        assert!(ranked(ADD_UNIQUE_LINK, Scope::File).is_empty());
        assert!(ranked(ADD_UNIQUE_LINK, Scope::Global).is_empty());
        for name in [
            "Random note: Open random note",
            "Unique note creator: Create new unique note",
            OPEN_GRAPH,
        ] {
            assert_eq!(ranked(name, Scope::Global)[0].name, name);
        }
    }

    #[test]
    fn tab_actions_need_a_tab_but_creation_remains_available_without_one() {
        for name in [
            "Toggle pin",
            CLOSE_OTHER_TABS,
            "Close current tab",
            "Go to next tab",
            "Go to previous tab",
        ] {
            assert_eq!(ranked(name, Scope::Tab)[0].name, name);
            assert!(ranked(name, Scope::Global).is_empty());
        }
        assert_eq!(ranked("New tab", Scope::Global)[0].name, "New tab");
        assert_eq!(ranked(DUPLICATE_FILE, Scope::File)[0].name, DUPLICATE_FILE);
        assert!(ranked(DUPLICATE_FILE, Scope::Tab).is_empty());
        assert_eq!(ranked(MOVE_FILE, Scope::File)[0].name, MOVE_FILE);
        assert!(ranked(MOVE_FILE, Scope::Tab).is_empty());
        assert_eq!(
            ranked(OPEN_LOCAL_GRAPH, Scope::Document)[0].name,
            OPEN_LOCAL_GRAPH
        );
        for unavailable in [Scope::Global, Scope::Tab, Scope::File] {
            assert!(
                ranked(OPEN_LOCAL_GRAPH, unavailable)
                    .iter()
                    .all(|command| command.name != OPEN_LOCAL_GRAPH)
            );
        }
    }

    #[test]
    fn os_atalhos_do_quadro_sao_unicos_e_os_comandos_aparecem_na_busca() {
        let mut seen = std::collections::HashSet::new();
        for command in ALL.iter().filter(|command| !command.shortcut.is_empty()) {
            assert!(
                seen.insert(command.shortcut),
                "atalho repetido: {}",
                command.shortcut
            );
        }
        assert_eq!(TOGGLE_NOTE_BOARD_SHORTCUT, "Ctrl+Shift+B");
        assert_eq!(
            ranked("alternar nota quadro", Scope::Document)[0].name,
            TOGGLE_NOTE_BOARD
        );
        // Um atalho só (Ctrl+Shift+B cicla); ir direto a um modo é pela paleta, sem atalho.
        assert!(ALL.iter().all(|command| command.shortcut != "Ctrl+Alt+B"));
        for (query, wanted) in [
            ("ir para a nota", GO_TO_NOTE),
            ("ir para o quadro", GO_TO_BOARD),
            ("ir para a lista", GO_TO_LIST),
        ] {
            assert_eq!(ranked(query, Scope::Document)[0].name, wanted, "{query}");
            assert_eq!(label(wanted).to_lowercase(), query);
        }
        assert!(ranked(EXPORT_BOARD_AS_NOTE, Scope::Global).is_empty());
    }

    #[test]
    fn fuzzy_commands_find_abbreviations_and_exclude_image_edits() {
        assert_eq!(ranked("srcedt", Scope::Document)[0].name, "Source editing");
        assert_eq!(
            ranked("Highlight selection", Scope::Document)[0].name,
            "Highlight selection"
        );
        assert!(ranked("Highlight", Scope::File).is_empty());
        assert_eq!(
            ranked("", Scope::File)
                .iter()
                .filter(|command| command.name == "Open settings")
                .count(),
            1
        );
    }
}
