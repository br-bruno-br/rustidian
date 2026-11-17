//! Teste de estresse do leitor de Markdown, sem `cargo-fuzz` (que pede nightly e não roda no
//! Windows). Um gerador próprio, de semente fixa (xorshift), joga milhares de entradas
//! aleatórias e malignas nas funções que leem texto de nota e exige três coisas de cada uma:
//!
//! 1. não entra em pânico;
//! 2. termina em tempo limitado;
//! 3. todo trecho que devolve (`Range` de bytes) está dentro do texto, em fronteira de caractere.
//!
//! A semente é fixa para a falha se repetir: a mensagem mostra a semente, o número da rodada e
//! a entrada (cortada). Para outra rodada, mude `SEED` ou use `RUSTIDIAN_STRESS_SEED`.

use rustidian::{
    editor::Markdown,
    folding::Sections,
    math, metadata, structure,
    tables::{Action, Table},
    unlinked,
    vault::{self, Vault},
};
use std::{
    fs,
    ops::Range,
    panic::{AssertUnwindSafe, catch_unwind},
    time::{Duration, Instant},
};
use tempfile::tempdir;

const SEED: u64 = 0x5EED_2026_1116_0012;
/// Entradas de cada família de teste.
const ROUNDS: usize = 1500;
/// Uma entrada de até 256 KB deve ser lida em bem menos que isto, mesmo em build de teste.
const LIMIT: Duration = Duration::from_secs(10);

fn seed() -> u64 {
    std::env::var("RUSTIDIAN_STRESS_SEED")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(SEED)
}

/// xorshift64*: pequeno, sem dependência e igual em toda máquina.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound.max(1) as u64) as usize
    }
    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
    /// `pieces` is `base` plus something below `spread`.
    fn soup(&mut self, base: usize, spread: usize) -> String {
        let pieces = base + self.below(spread);
        soup(self, pieces)
    }
    /// `depth` is `base` plus something below `spread`.
    fn nested(&mut self, base: usize, spread: usize) -> String {
        let depth = base + self.below(spread);
        nested(self, depth)
    }
    /// `length` is `base` plus something below `spread`.
    fn random_chars(&mut self, base: usize, spread: usize) -> String {
        let length = base + self.below(spread);
        random_chars(self, length)
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

/// Pedaços que desafiam o leitor: delimitadores abertos, aninhamento, unicode difícil.
const PIECES: &[&str] = &[
    "[[",
    "]]",
    "![[",
    "|",
    "#",
    "## ",
    "$",
    "$$",
    "```",
    "```rust\n",
    "~~~",
    "`",
    "``",
    "> ",
    "> [!note]",
    "> [!warning]- Título",
    "> [!",
    ">> [!tip]+ ",
    "- ",
    "- [ ] ",
    "- [x] ",
    "1. ",
    "* ",
    "+ ",
    "| a | b |\n|---|:-:|\n| 1 | 2 |\n",
    "|---|",
    "| \\| |",
    "<div align=\"center\">",
    "</div>",
    "<br>",
    "<span style=\"color:red\">",
    "</span>",
    "<mark>",
    "==",
    "==x==",
    "**",
    "*",
    "_",
    "__",
    "~~",
    "\\",
    "\\\\",
    "---\n",
    "---",
    "+++",
    "tags: [a, b]\n",
    "aliases:\n  - x\n",
    "title: \"",
    "\n",
    "\n\n",
    "\r\n",
    "\r",
    "\t",
    "    ",
    " ",
    "é",
    "ñ",
    "日本語",
    "😀",
    "👨‍👩‍👧‍👦",
    "\u{0}",
    "\u{200d}",
    "\u{301}",
    "\u{feff}",
    "\u{202e}",
    "\u{ffff}",
    "[x](y)",
    "[x](<y z>)",
    "![i](p.png)",
    "[ref]: http://u",
    "[ref]",
    "%%",
    "^block-id",
    " ^id",
    "<",
    ">",
    "&amp;",
    "&#x0;",
    "<!--",
    "-->",
    "((",
    "))",
    "{",
    "}",
    "(",
    ")",
    "[",
    "]",
    "!",
    "^",
    "@",
    ":",
    ";",
    "\"",
    "'",
    "http://a.b/c_d_e",
    "<http://x>",
    "$$\n\\frac{a}{b}\n$$\n",
    "$x^2$",
    "\\$",
    "$$$",
    "|||",
    "[[a|b]]",
    "[[a#b]]",
    "[[a#^b]]",
    "[[a\\|b]]",
    "[[ ]]",
    "[[]]",
    "[[[[a]]]]",
    "![[a.png|100]]",
    "#tag",
    "#a/b",
    "# ",
    "Setext\n===\n",
    "Setext\n---\n",
    "***\n",
    "- - -\n",
    "<script>",
    "path:",
    "file:",
    "tag:",
    "line:",
    "content:",
    "/",
    "/(a+)+$/",
    "(",
    " OR ",
    " -",
    "-(",
];

fn soup(rng: &mut Rng, pieces: usize) -> String {
    let mut text = String::new();
    for _ in 0..pieces {
        text.push_str(rng.pick(PIECES));
        if rng.chance(15) {
            text.push_str(["a", "bc", "Texto", "x y"][rng.below(4)]);
        }
    }
    text
}

fn random_chars(rng: &mut Rng, length: usize) -> String {
    (0..length)
        .map(|_| match rng.below(10) {
            0 => char::from_u32(rng.below(0x11_0000) as u32).unwrap_or('?'),
            1 => ['\n', '\r', '\t', '\0', ' '][rng.below(5)],
            2 => [
                '[', ']', '|', '#', '$', '`', '>', '-', '*', '_', '~', '\\', '!', '<',
            ][rng.below(14)],
            3 => char::from_u32(0x300 + rng.below(0x70) as u32).unwrap_or('?'),
            _ => char::from(b'a' + rng.below(26) as u8),
        })
        .collect()
}

/// Repetições longas de um só delimitador: o caso clássico de leitor quadrático ou recursivo.
fn nested(rng: &mut Rng, depth: usize) -> String {
    let unit = *rng.pick(&[
        "[",
        "]",
        "[[",
        "]]",
        "(",
        ")",
        "*",
        "_",
        "~~",
        "`",
        "> ",
        ">",
        "- ",
        "  - ",
        "1. ",
        "#",
        "$",
        "$$",
        "|",
        "<",
        "<div>",
        "</div>",
        "\\",
        "![",
        "[a](",
        "==",
        "<!--",
        "[!note]\n> ",
        "| a ",
        "|---",
        "> [!n]\n",
    ]);
    unit.repeat(depth)
}

const SEEDS: &[&str] = &[
    "---\ntitle: Nota\ntags: [a, b]\naliases: [Um, Dois]\n---\n# Título\n\nTexto com [[Link]], [[Alvo|apelido]], ![[img.png]] e #tag.\n\n> [!note] Aviso\n> corpo $x^2$\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n- [ ] tarefa\n- [x] feita\n\n```rust\nfn main() {}\n```\n\n$$\n\\frac{a}{b}\n$$\n\nFim ==destaque== ^bloco\n",
    "Linha\r\nCRLF\r\n> [!tip]+ x\r\n> y\r\n\r\n| a |\r\n|---|\r\n| b |\r\n",
    "<div align=\"center\">\n\n# Centro\n\n</div>\n\n<br>\n\n<span style=\"color: #f00\">cor</span>\n",
];

/// Uma nota válida estragada: pedaços apagados, repetidos, trocados e com lixo no meio.
fn mutated(rng: &mut Rng) -> String {
    let mut text = (*rng.pick(SEEDS)).to_owned();
    for _ in 0..1 + rng.below(8) {
        let boundaries: Vec<usize> = text
            .char_indices()
            .map(|(at, _)| at)
            .chain([text.len()])
            .collect();
        let a = *rng.pick(&boundaries);
        let b = *rng.pick(&boundaries);
        let (a, b) = (a.min(b), a.max(b));
        match rng.below(5) {
            0 => text.replace_range(a..b, ""),
            1 => {
                let copy = text[a..b].to_owned();
                text.insert_str(a, &copy);
            }
            2 => text.insert_str(a, rng.pick(PIECES)),
            3 => text.truncate(b),
            _ => {
                let copy = text[a..b].to_owned();
                let at = *rng.pick(&boundaries);
                text.insert_str(at.min(text.len()), &copy.repeat(1 + rng.below(4)));
            }
        }
    }
    text
}

/// A entrada de uma rodada: uma das famílias, com semente derivada da rodada.
fn input(rng: &mut Rng) -> String {
    match rng.below(6) {
        0 => rng.soup(1, 40),
        1 => rng.soup(1, 400),
        2 => rng.random_chars(0, 1500),
        3 => rng.nested(1, 3000),
        4 => mutated(rng),
        _ => {
            let mut text = rng.nested(1, 200);
            text.push_str(&rng.soup(0, 40));
            text.push_str(&rng.nested(0, 200));
            text
        }
    }
}

fn shown(text: &str) -> String {
    let cut: String = text.chars().take(300).collect();
    format!("{cut:?} ({} bytes)", text.len())
}

#[track_caller]
fn inside(text: &str, range: &Range<usize>, what: &str) {
    assert!(
        range.start <= range.end
            && range.end <= text.len()
            && text.is_char_boundary(range.start)
            && text.is_char_boundary(range.end),
        "{what}: {range:?} fora do texto de {} bytes (ou fora de fronteira de caractere)",
        text.len()
    );
}

#[track_caller]
fn point(text: &str, at: usize, what: &str) {
    assert!(
        at <= text.len() && text.is_char_boundary(at),
        "{what}: {at} fora do texto de {} bytes (ou fora de fronteira de caractere)",
        text.len()
    );
}

/// Roda `check` sobre `text`: sem pânico, em tempo limitado. A falha mostra de onde veio.
fn run(family: &str, round: usize, text: &str, check: impl FnOnce(&str)) {
    let started = Instant::now();
    let outcome = catch_unwind(AssertUnwindSafe(|| check(text)));
    let elapsed = started.elapsed();
    if let Err(cause) = outcome {
        let reason = cause
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| cause.downcast_ref::<&str>().map(|s| (*s).to_owned()))
            .unwrap_or_default();
        panic!(
            "{family}: falhou na rodada {round} (semente {:#x}): {reason}\nentrada: {}",
            seed(),
            shown(text)
        );
    }
    assert!(
        elapsed < LIMIT,
        "{family}: rodada {round} levou {elapsed:?} (limite {LIMIT:?}), semente {:#x}\nentrada: {}",
        seed(),
        shown(text)
    );
}

fn check_markdown(text: &str) {
    let parsed = Markdown::parse(text);
    for span in &parsed.spans {
        inside(text, &span.range, "span de estilo");
    }
    for &start in parsed.heading_starts.iter().chain(&parsed.item_starts) {
        point(text, start, "início de título ou item");
    }
    for quote in &parsed.quotes {
        inside(text, &quote.range, "prefixo de citação");
    }
    for range in parsed
        .setext_markers
        .iter()
        .chain(&parsed.highlights)
        .chain(&parsed.html_centered)
        .chain(&parsed.html_tag_lines)
    {
        inside(text, range, "marcador");
    }
    for block in &parsed.code_blocks {
        inside(text, &block.range, "bloco de código");
        inside(text, &block.opening, "abertura do bloco");
        if let Some(closing) = &block.closing {
            inside(text, closing, "fechamento do bloco");
        }
        if let Some(content) = block.content_range(text) {
            inside(text, &content, "conteúdo do bloco");
            assert!(block.content(text).is_some());
        }
    }
    for table in &parsed.tables {
        inside(text, &table.range, "tabela");
        for (row, line) in table.rows.iter().enumerate() {
            for (column, cell) in line.cells.iter().enumerate() {
                inside(text, cell, "célula");
                let _ = table.cell_text(text, row, column);
            }
        }
    }
    for callout in &parsed.callouts {
        for (range, what) in [
            (&callout.range, "callout"),
            (&callout.header, "cabeçalho do callout"),
            (&callout.marker, "marcador do callout"),
            (&callout.body, "corpo do callout"),
        ] {
            inside(text, range, what);
        }
    }
    for task in &parsed.tasks {
        inside(text, &task.prefix, "prefixo de tarefa");
    }
    for math in &parsed.maths {
        inside(text, &math.range, "fórmula");
    }
    // Dobras e o que mais lê a análise pronta.
    for hidden in Sections::new(text, &parsed).hidden(&Default::default()) {
        inside(text, &hidden, "trecho dobrado");
    }
}

fn check_wikilinks_and_math(text: &str) {
    for (range, target) in vault::wiki_spans(text) {
        inside(text, &range, "wikilink");
        let _ = target;
    }
    let _ = vault::wiki_links(text);
    for span in math::spans(text) {
        inside(text, &span.range, "fórmula");
    }
}

fn check_structure(text: &str) {
    let _ = structure::outline(text);
    // As âncoras são posições em CARACTERES, não em bytes.
    let characters = text.chars().count();
    for (_, at) in structure::block_anchors(text) {
        assert!(
            at <= characters,
            "âncora de bloco {at} depois do fim ({characters})"
        );
    }
    for block in structure::blocks(text) {
        let _ = block;
    }
    for fragment in ["", "a", "^b", "Título", "t#x", "^"] {
        if let Some(at) = structure::find_anchor(text, fragment) {
            assert!(
                at <= characters,
                "âncora achada {at} depois do fim ({characters})"
            );
        }
    }
    let _ = metadata::frontmatter(text);
    let analysis = metadata::analyze(text);
    let _ = (analysis.aliases, analysis.tags);
    // `lower_same_len` promete o mesmo número de bytes: as posições dependem disso.
    let lowered = unlinked::lower_same_len(text);
    assert_eq!(lowered.len(), text.len(), "lower_same_len mudou o tamanho");
    let names = vec![
        "a".to_owned(),
        "Título".to_owned(),
        "é".to_owned(),
        "[[".to_owned(),
    ];
    // A função recebe uma fatia de intervalos: um só intervalo é de propósito.
    #[allow(clippy::single_range_in_vec_init)]
    let skipped = [0..text.len().min(3)];
    for range in unlinked::spans(text, &names, &skipped) {
        inside(text, &range, "menção sem link");
    }
}

fn check_tables_and_callouts(text: &str) {
    let parsed = Markdown::parse(text);
    for table in &parsed.tables {
        let rows = table.rows.len();
        let columns = table.alignments.len();
        let actions = [
            Action::InsertRow(rows),
            Action::InsertRow(0),
            Action::RemoveRow(rows.saturating_sub(1)),
            Action::MoveRow { from: 0, to: rows },
            Action::InsertColumn(columns),
            Action::RemoveColumn(0),
            Action::MoveColumn {
                from: 0,
                to: columns,
            },
            Action::Sort {
                column: 0,
                descending: true,
            },
            Action::Clear {
                rows: 0..rows,
                columns: 0..columns,
            },
            Action::RemoveRows(1..rows),
            Action::RemoveColumns(0..columns),
            Action::Cell {
                row: 0,
                column: 0,
                text: "x|y\n\u{301}".into(),
            },
            Action::Cell {
                row: rows + 3,
                column: columns + 3,
                text: "fora".into(),
            },
        ];
        for action in actions {
            let mut copy = text.to_owned();
            if Table::apply(table, &mut copy, action).is_ok() {
                // O resultado continua sendo texto que o leitor aceita.
                let _ = Markdown::parse(&copy);
            }
        }
        let _ = table.copy(text, 0..rows, 0..columns);
    }
    for callout in &parsed.callouts {
        let mut copy = text.to_owned();
        let _ = callout.change_kind(&mut copy, rustidian::callouts::Kind::Warning);
        let _ = Markdown::parse(&copy);
    }
}

fn many(family: &str, extra: usize, check: fn(&str)) {
    let mut rng = Rng::new(seed() ^ (family.len() as u64 * 0x9E37_79B9));
    for round in 0..ROUNDS + extra {
        let text = input(&mut rng);
        run(family, round, &text, check);
    }
}

#[test]
fn markdown_parse_survives_hostile_input() {
    many("Markdown::parse", 0, check_markdown);
}

#[test]
fn wikilinks_and_math_survive_hostile_input() {
    many("wikilinks e math", 0, check_wikilinks_and_math);
}

#[test]
fn structure_metadata_and_mentions_survive_hostile_input() {
    many("estrutura e metadados", 0, check_structure);
}

#[test]
fn table_and_callout_edits_survive_hostile_input() {
    many("tabelas e callouts", 0, check_tables_and_callouts);
}

#[test]
fn large_degenerate_inputs_finish_in_bounded_time() {
    let mut rng = Rng::new(seed() ^ 0xB16);
    for round in 0..24 {
        let mut text = String::new();
        while text.len() < 128 * 1024 {
            if rng.chance(50) {
                text.push_str(&rng.nested(50, 2000));
            } else {
                text.push_str(&rng.soup(20, 200));
            }
        }
        text.truncate(256 * 1024);
        while !text.is_char_boundary(text.len()) {
            text.pop();
        }
        run("entrada grande", round, &text, |text| {
            check_markdown(text);
            check_wikilinks_and_math(text);
            check_structure(text);
        });
    }
}

/// Consultas do usuário: tudo o que a caixa de busca pode receber.
#[test]
fn query_parse_and_matching_survive_hostile_input() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("a.md"),
        "---\ntags: [x]\naliases: [Um]\n---\n# A\nTexto [[b]] #tag\n",
    )
    .unwrap();
    fs::write(dir.path().join("b.md"), "outro\n- [ ] item\nlinha dois\n").unwrap();
    fs::create_dir(dir.path().join("pasta")).unwrap();
    fs::write(dir.path().join("pasta/c.md"), "").unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    let mut rng = Rng::new(seed() ^ 0x0E27);
    let operators = [
        "path:", "file:", "tag:", "line:", "content:", "[", "]", "/", "\"", "OR", "-", "(", ")",
        "[prop:", "prop:x", "=", ":", "\\", "^", "$", ".*", "(a+)+", "\\p{L}", "(?i)", "(?s)", "{",
        "}", "*", "+", "?", "é", "日本", "😀", "\u{0}", " ", "  ", "\t", "\n",
    ];
    for round in 0..ROUNDS * 4 {
        let mut query = String::new();
        for _ in 0..rng.below(14) {
            if rng.chance(70) {
                query.push_str(rng.pick(&operators));
            } else {
                query.push_str(&rng.random_chars(0, 12));
            }
        }
        if rng.chance(5) {
            query = rng.nested(1, 3000);
        }
        let started = Instant::now();
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let _ = rustidian::query::Query::parse(&query).map(|parsed| parsed.source().len());
            if let Ok(found) = vault.query(&query) {
                assert!(found.len() <= vault.notes.len());
            }
        }));
        assert!(
            outcome.is_ok(),
            "Query: pânico na rodada {round} (semente {:#x}): {}",
            seed(),
            shown(&query)
        );
        assert!(
            started.elapsed() < LIMIT,
            "Query: rodada {round} levou {:?}: {}",
            started.elapsed(),
            shown(&query)
        );
    }
}

/// `Note::new` roda a análise de metadados em tudo o que o índice lê: o cofre inteiro é o teste.
#[test]
fn indexing_a_vault_of_hostile_notes_works() {
    let dir = tempdir().unwrap();
    let mut rng = Rng::new(seed() ^ 0x1D3);
    let mut expected = 0;
    for number in 0..120 {
        let text = input(&mut rng);
        // Texto com NUL ainda é uma nota legível pelo índice.
        fs::write(dir.path().join(format!("n{number}.md")), text).unwrap();
        expected += 1;
    }
    let started = Instant::now();
    let vault = Vault::open(dir.path()).unwrap();
    assert_eq!(vault.notes.len(), expected);
    assert!(started.elapsed() < LIMIT * 3, "{:?}", started.elapsed());
    for (position, note) in vault.notes.iter().enumerate() {
        // Links, backlinks e menções de cada nota hostil também precisam responder.
        let _ = vault.backlinks(&note.path);
        if position % 8 == 0 {
            let _ = vault.backlink_mentions(&note.path, &[]);
            let _ = vault.unlinked_mentions(&note.path, &[]);
        }
        for (range, target) in vault::wiki_spans(&note.text) {
            inside(&note.text, &range, "wikilink da nota");
            let _ = vault.resolve(&note.path, &target);
        }
    }
}

/// Achado deste teste: 3000 parênteses abertos estouravam a pilha (e estouro de pilha não se
/// captura: encerra o app). Agora a busca recusa grupos fundos demais.
#[test]
fn deeply_nested_query_groups_are_refused_instead_of_overflowing_the_stack() {
    let query = format!("{}a{}", "(".repeat(5000), ")".repeat(5000));
    assert!(rustidian::query::Query::parse(&query).is_err());
    assert!(rustidian::query::Query::parse(&"-(".repeat(5000)).is_err());
    assert!(rustidian::query::Query::parse(&"line:(".repeat(5000)).is_err());
    let reasonable = format!("{}a{}", "(".repeat(20), ")".repeat(20));
    assert!(rustidian::query::Query::parse(&reasonable).is_ok());
}

/// Achado deste teste: `1. 1. 1. ...` (uma lista dentro da outra, 40 mil vezes) fazia
/// `structure::blocks` quadrático. Listas de verdade continuam com os seus blocos.
#[test]
fn nested_lists_keep_their_blocks_and_a_pathological_one_stays_fast() {
    let started = Instant::now();
    let _ = structure::blocks(&"1. ".repeat(40_000));
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "{:?}",
        started.elapsed()
    );

    let note = "- a\n  - b\n    - c ^fundo\n      - d ^mais\n";
    let anchors = structure::block_anchors(note);
    let ids: Vec<&str> = anchors.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["fundo", "mais"]);
    // Item dentro de citação não vira bloco: só a citação inteira, sem id próprio.
    let quoted = structure::blocks(
        "> - item ^q
",
    );
    assert_eq!(quoted.len(), 1);
    assert!(quoted[0].id.is_none());
}
