//! Corretor ortográfico (R10): palavras desconhecidas do texto em prosa.
//!
//! O motor é o `spellbook` (Hunspell em Rust puro). O dicionário NÃO vai no binário: o par
//! `.aff`/`.dic` (o pt-BR é o VERO, do LibreOffice) fica na pasta de dados do app e é baixado
//! uma vez, com o sha256 fixo abaixo conferido antes de gravar. Nada aqui toca a tela:
//! a interface só pergunta quais palavras da faixa visível estão erradas.
//!
//! Peças:
//! - [`words`]: as palavras de prosa de uma nota, em bytes e em caracteres, sem front matter,
//!   código, matemática, links, URLs, números e siglas;
//! - [`Engine`]: o dicionário carregado (`check` e `suggest`);
//! - [`Personal`]: a lista pessoal (um arquivo de texto, uma palavra por linha);
//! - [`Checker`]: motor + lista pessoal + "ignorar nesta sessão" + cache de veredito por palavra;
//! - [`SpellService`]: carrega e baixa o dicionário em segundo plano e guarda o estado da nota.

use std::collections::{HashMap, HashSet};
use std::io::Write as _;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

/// Um dos dois arquivos do dicionário e de onde vem.
pub struct Source {
    pub name: &'static str,
    pub url: &'static str,
    /// sha256 em hex minúsculo do arquivo inteiro, conferido antes de gravar.
    pub sha256: &'static str,
}

/// Um idioma: o par `.aff`/`.dic` e de onde baixá-lo.
pub struct Language {
    pub code: &'static str,
    pub label: &'static str,
    pub aff: Source,
    pub dic: Source,
}

/// Revisão fixa do repositório LibreOffice/dictionaries (última que mexeu em `pt_BR.aff` e
/// `pt_BR.dic`, 12/11/2021, VERO 3.2). Fixar o commit mantém o sha256 valendo para sempre.
pub const PT_BR: Language = Language {
    code: "pt_BR",
    label: "pt-BR",
    aff: Source {
        name: "pt_BR.aff",
        url: "https://raw.githubusercontent.com/LibreOffice/dictionaries/5cf23af85b47be6e398add826da1a02175be4238/pt_BR/pt_BR.aff",
        sha256: "21d8ad2a769a60e17e2b5ea4ef11d4d593a58b9e2a82d642ef82d6a4c5523865",
    },
    dic: Source {
        name: "pt_BR.dic",
        url: "https://raw.githubusercontent.com/LibreOffice/dictionaries/5cf23af85b47be6e398add826da1a02175be4238/pt_BR/pt_BR.dic",
        sha256: "a38bfb26b68ece2834e79fe83e48d5792652970ace12db89d1b9674bf9933183",
    },
};

/// Notas maiores que isto não são verificadas (a tokenização refaz a nota inteira a cada edição).
pub const MAX_NOTE_BYTES: usize = 2 * 1024 * 1024;
/// Teto de cada arquivo baixado (o `.dic` do VERO tem 4,5 MB).
const MAX_DOWNLOAD_BYTES: u64 = 16 * 1024 * 1024;
/// Palavras mais compridas que isto não são verificadas.
const MAX_WORD_CHARS: usize = 48;

/// A pasta dos dicionários e da lista pessoal: `<pasta de dados do app>/dicionarios`.
pub fn dictionary_dir() -> Option<PathBuf> {
    crate::paths::data_base().map(|base| crate::paths::app_directory(&base).join("dicionarios"))
}

/// A lista pessoal de um idioma, ao lado do dicionário.
pub fn personal_file(dir: &Path, language: &Language) -> PathBuf {
    dir.join(format!("pessoal-{}.txt", language.code))
}

/// Os dois arquivos do dicionário estão na pasta?
pub fn installed(dir: &Path, language: &Language) -> bool {
    [&language.aff, &language.dic]
        .iter()
        .all(|source| dir.join(source.name).is_file())
}

// ---------------------------------------------------------------------------------------------
// Tokenização
// ---------------------------------------------------------------------------------------------

/// Uma palavra de prosa: onde está em bytes (para fatiar o texto) e em caracteres (para o egui).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Word {
    pub bytes: Range<usize>,
    pub chars: Range<usize>,
}

/// Fim do front matter (`---` ... `---`), ou 0. Sem ler o YAML: um YAML quebrado continua sendo
/// cabeçalho e não vira "prosa errada".
fn front_matter_end(text: &str) -> usize {
    let mut lines = text.split_inclusive('\n');
    if lines
        .next()
        .is_none_or(|line| line.trim_end_matches(['\r', '\n']) != "---")
    {
        return 0;
    }
    let mut offset = text.find('\n').map_or(text.len(), |position| position + 1);
    for line in lines {
        let end = offset + line.len();
        if matches!(line.trim_end_matches(['\r', '\n']), "---" | "...") {
            return end;
        }
        offset = end;
    }
    0
}

fn overlaps(range: &Range<usize>, others: &[Range<usize>]) -> bool {
    others
        .iter()
        .any(|other| other.start < range.end && other.end > range.start)
}

/// Faixas de uma linha que não são prosa: código em linha, wikilinks, destino de link,
/// tags HTML, chamadas `[!tipo]`, definição de referência e fichas que parecem URL, e-mail
/// ou nome de arquivo.
fn skipped_in_line(line: &str) -> Vec<Range<usize>> {
    let bytes = line.as_bytes();
    let mut skip = crate::unlinked::inline_code(line);
    let trimmed = line.trim_start();
    // `[id]: https://...` define uma referência e não tem prosa.
    if trimmed.starts_with('[')
        && let Some(close) = trimmed.find("]:")
        && !trimmed[..close].contains(' ')
    {
        skip.push(0..line.len());
        return skip;
    }
    let mut cursor = 0;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor += 2,
            b'[' if line[cursor..].starts_with("[[") => match line[cursor + 2..].find("]]") {
                Some(end) => {
                    let end = cursor + 2 + end + 2;
                    skip.push(cursor..end);
                    cursor = end;
                }
                None => cursor += 2,
            },
            b'[' if line[cursor..].starts_with("[!") => match line[cursor..].find(']') {
                Some(end) => {
                    skip.push(cursor..cursor + end + 1);
                    cursor += end + 1;
                }
                None => cursor += 1,
            },
            b']' if line[cursor..].starts_with("](") => {
                // `(destino)`: os parênteses podem aparecer dentro do endereço.
                let mut depth = 0usize;
                let mut end = None;
                for (offset, c) in line[cursor + 1..].char_indices() {
                    match c {
                        '(' => depth += 1,
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                end = Some(cursor + 1 + offset + 1);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                let end = end.unwrap_or(line.len());
                skip.push(cursor + 1..end);
                cursor = end;
            }
            b'<' if bytes
                .get(cursor + 1)
                .is_some_and(|b| b.is_ascii_alphabetic() || matches!(b, b'/' | b'!')) =>
            {
                match line[cursor..].find('>') {
                    Some(end) => {
                        skip.push(cursor..cursor + end + 1);
                        cursor += end + 1;
                    }
                    None => cursor += 1,
                }
            }
            _ => cursor += 1,
        }
    }
    // Fichas separadas por espaço que não são palavras: URL, e-mail, caminho, `arquivo.ext`.
    let mut start = None;
    for (offset, c) in line
        .char_indices()
        .chain(std::iter::once((line.len(), ' ')))
    {
        match (start, c.is_whitespace()) {
            (None, false) => start = Some(offset),
            (Some(from), true) => {
                // O destino `](...)` já foi marcado acima; só o texto antes dele conta aqui.
                let head = line[from..offset].split("](").next().unwrap_or_default();
                if looks_technical(head.trim_matches(|c: char| !c.is_alphanumeric())) {
                    skip.push(from..from + head.len());
                }
                start = None;
            }
            _ => {}
        }
    }
    skip
}

fn looks_technical(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    if token.contains("://")
        || token.contains('@')
        || token.starts_with("www.")
        || token.contains('\\')
        || token.contains('/')
    {
        return true;
    }
    // `exemplo.com`, `main.rs`, `foto.png`: um ponto no meio e uma extensão curta em minúsculas.
    token.rsplit_once('.').is_some_and(|(stem, extension)| {
        !stem.is_empty()
            && !extension.is_empty()
            && extension.len() <= 5
            && extension
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            && stem.chars().next_back().is_some_and(char::is_alphanumeric)
    })
}

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || ('\u{300}'..='\u{36f}').contains(&c)
}

fn joiner(c: char) -> bool {
    matches!(c, '-' | '\'' | '\u{2019}')
}

/// Uma "palavra" candidata vale a pena checar? Fora números, snake_case, siglas e CamelCase.
fn checkable(word: &str) -> bool {
    if word.chars().count() < 2 || word.chars().count() > MAX_WORD_CHARS {
        return false;
    }
    if word.chars().any(|c| c.is_numeric() || c == '_') {
        return false;
    }
    // Cada trecho entre hífens e apóstrofos: maiúscula depois da primeira letra = sigla ou
    // CamelCase (ERP, iPhone, GitHub), que o Hunspell só marcaria como erro.
    !word
        .split(joiner)
        .any(|part| part.chars().skip(1).any(char::is_uppercase))
}

/// As palavras de prosa de `text`, em ordem. Fora: front matter, cercas e código em linha,
/// matemática, wikilinks, destinos de link, URLs, tags HTML, `#tags`, `@menções`, números,
/// palavras com dígitos ou `_`, siglas e CamelCase.
pub fn words(text: &str) -> Vec<Word> {
    if text.len() > MAX_NOTE_BYTES {
        return Vec::new();
    }
    let body = front_matter_end(text);
    let maths: Vec<Range<usize>> = if text.contains('$') {
        crate::math::spans(text)
            .into_iter()
            .map(|math| math.range)
            .collect()
    } else {
        Vec::new()
    };
    let mut out = Vec::new();
    // Contagem de caracteres incremental: as palavras saem em ordem crescente de byte.
    let (mut last_byte, mut last_char) = (0usize, 0usize);
    let mut math_from = 0usize;
    for line in crate::vault::prose_lines(text) {
        let offset = line.as_ptr() as usize - text.as_ptr() as usize;
        if offset + line.len() <= body {
            continue;
        }
        let skip = skipped_in_line(line);
        let chars: Vec<(usize, char)> = line.char_indices().collect();
        let mut index = 0;
        while index < chars.len() {
            if !word_char(chars[index].1) {
                index += 1;
                continue;
            }
            let first = index;
            let mut last = index; // último caractere que entra na palavra
            index += 1;
            while index < chars.len() {
                let c = chars[index].1;
                if word_char(c) {
                    last = index;
                    index += 1;
                } else if joiner(c)
                    && chars
                        .get(index + 1)
                        .is_some_and(|(_, next)| word_char(*next))
                {
                    index += 1;
                } else {
                    break;
                }
            }
            let (mut start, end) = (chars[first].0, chars[last].0 + chars[last].1.len_utf8());
            // `_itálico_` e `__negrito__` não são parte da palavra.
            while line[start..end].starts_with('_') && start < end {
                start += 1;
            }
            let word_end = line[start..end].trim_end_matches('_').len() + start;
            if start >= word_end {
                continue;
            }
            let (start, end) = (start, word_end);
            let text_word = &line[start..end];
            if !checkable(text_word) {
                continue;
            }
            let before = line[..start].chars().next_back();
            let after = line[end..].chars().next();
            if matches!(before, Some('#' | '@' | '^' | '\\' | '$'))
                || (before == Some(':') && after == Some(':'))
            {
                continue;
            }
            let range = start..end;
            if offset + start < body || overlaps(&range, &skip) {
                continue;
            }
            let absolute = offset + start..offset + end;
            while math_from < maths.len() && maths[math_from].end <= absolute.start {
                math_from += 1;
            }
            if maths[math_from..]
                .iter()
                .take_while(|math| math.start < absolute.end)
                .any(|math| math.end > absolute.start)
            {
                continue;
            }
            last_char += text[last_byte..absolute.start].chars().count();
            last_byte = absolute.start;
            let from = last_char;
            last_char += text[last_byte..absolute.end].chars().count();
            last_byte = absolute.end;
            out.push(Word {
                bytes: absolute,
                chars: from..last_char,
            });
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Motor
// ---------------------------------------------------------------------------------------------

/// O dicionário Hunspell carregado. Só leitura: pode ser dividido entre threads.
pub struct Engine {
    dict: spellbook::Dictionary,
}

impl Engine {
    /// A partir do texto de um par `.aff`/`.dic` (um BOM no começo é ignorado).
    pub fn new(aff: &str, dic: &str) -> Result<Self, String> {
        let aff = aff.trim_start_matches('\u{feff}');
        let dic = dic.trim_start_matches('\u{feff}');
        spellbook::Dictionary::new(aff, dic)
            .map(|dict| Self { dict })
            .map_err(|error| format!("dicionário inválido: {error}"))
    }

    /// Lê `<dir>/<aff>` e `<dir>/<dic>` do idioma.
    pub fn load(dir: &Path, language: &Language) -> Result<Self, String> {
        let read = |source: &Source| {
            std::fs::read_to_string(dir.join(source.name))
                .map_err(|error| format!("não consegui ler {}: {error}", source.name))
        };
        Self::new(&read(&language.aff)?, &read(&language.dic)?)
    }

    pub fn check(&self, word: &str) -> bool {
        self.dict.check(word)
    }

    /// Até `max` correções para `word`.
    pub fn suggest(&self, word: &str, max: usize) -> Vec<String> {
        let mut out = Vec::new();
        self.dict.suggest(word, &mut out);
        let mut out = order_suggestions(word, out);
        out.truncate(max);
        out
    }
}

/// O spellbook devolve "casa a" e "casa-a" (a palavra partida) antes das palavras inteiras.
/// Palavras inteiras primeiro, partidas por último; a ordem do motor se mantém dentro de cada grupo.
fn order_suggestions(word: &str, mut suggestions: Vec<String>) -> Vec<String> {
    let hyphenated = word.contains('-');
    suggestions.sort_by_key(|suggestion| {
        (
            suggestion.contains(' '),
            !hyphenated && suggestion.contains('-'),
        )
    });
    suggestions.dedup();
    suggestions
}

/// `word` está certa para o motor? `casa-pão` e `d'água` passam também quando cada pedaço passa.
fn engine_accepts(engine: &Engine, word: &str) -> bool {
    if engine.check(word) {
        return true;
    }
    if !word.contains(joiner) {
        return false;
    }
    word.split(joiner)
        .all(|part| part.chars().count() < 2 || engine.check(part))
}

// ---------------------------------------------------------------------------------------------
// Lista pessoal
// ---------------------------------------------------------------------------------------------

/// Palavras do usuário. Uma palavra em minúsculas vale em qualquer caixa; uma com maiúscula
/// (`Replay`) vale como está e em CAIXA ALTA.
#[derive(Default)]
pub struct Personal {
    words: HashSet<String>,
    uppercase: HashSet<String>,
    path: Option<PathBuf>,
}

impl Personal {
    pub fn from_text(text: &str) -> Self {
        let mut personal = Self::default();
        for line in text.lines() {
            personal.insert(line);
        }
        personal
    }

    /// Lê a lista de `path` (que pode não existir ainda) e passa a gravar nela.
    pub fn open(path: PathBuf) -> Self {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let mut personal = Self::from_text(text.trim_start_matches('\u{feff}'));
        personal.path = Some(path);
        personal
    }

    fn insert(&mut self, word: &str) -> bool {
        let word = word.trim();
        if word.is_empty()
            || word.chars().count() > MAX_WORD_CHARS
            || word.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            return false;
        }
        self.uppercase.insert(word.to_uppercase());
        self.words.insert(word.to_string())
    }

    pub fn contains(&self, word: &str) -> bool {
        self.words.contains(word)
            || self.words.contains(&word.to_lowercase())
            || (!word.chars().any(char::is_lowercase) && self.uppercase.contains(word))
    }

    pub fn len(&self) -> usize {
        self.words.len()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// Guarda `word` e acrescenta uma linha ao arquivo.
    pub fn add(&mut self, word: &str) -> std::io::Result<()> {
        if !self.insert(word) {
            return Ok(());
        }
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let needs_break = std::fs::read(path)
            .map(|bytes| !bytes.is_empty() && !bytes.ends_with(b"\n"))
            .unwrap_or(false);
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        if needs_break {
            file.write_all(b"\n")?;
        }
        writeln!(file, "{}", word.trim())
    }
}

// ---------------------------------------------------------------------------------------------
// Verificador
// ---------------------------------------------------------------------------------------------

/// Motor + lista pessoal + palavras ignoradas na sessão + cache de veredito por palavra.
#[derive(Default)]
pub struct Checker {
    engine: Option<Arc<Engine>>,
    personal: Personal,
    ignored: HashSet<String>,
    verdicts: HashMap<String, bool>,
    /// Quantas vezes o motor foi de fato consultado (o resto saiu do cache).
    pub lookups: usize,
}

impl Checker {
    pub fn new(engine: Option<Arc<Engine>>, personal: Personal) -> Self {
        Self {
            engine,
            personal,
            ..Self::default()
        }
    }

    pub fn set_engine(&mut self, engine: Option<Arc<Engine>>) {
        self.engine = engine;
        self.verdicts.clear();
    }

    pub fn set_personal(&mut self, personal: Personal) {
        self.personal = personal;
        self.verdicts.clear();
    }

    pub fn has_engine(&self) -> bool {
        self.engine.is_some()
    }

    pub fn engine(&self) -> Option<Arc<Engine>> {
        self.engine.clone()
    }

    pub fn ignore(&mut self, word: &str) {
        self.ignored.insert(word.to_string());
        self.verdicts.remove(word);
    }

    pub fn add_personal(&mut self, word: &str) -> std::io::Result<()> {
        let result = self.personal.add(word);
        self.verdicts.clear();
        result
    }

    /// O veredito de `word`, do cache quando já foi dado. `None` sem dicionário.
    fn verdict(&mut self, word: &str) -> Option<bool> {
        if let Some(known) = self.verdicts.get(word) {
            return Some(*known);
        }
        let engine = self.engine.as_ref()?;
        self.lookups += 1;
        let ok = self.ignored.contains(word)
            || self.personal.contains(word)
            || engine_accepts(engine, word);
        self.verdicts.insert(word.to_string(), ok);
        Some(ok)
    }

    pub fn is_ok(&mut self, word: &str) -> bool {
        self.verdict(word).unwrap_or(true)
    }

    /// As palavras erradas entre as de `words` que tocam `visible` (em caracteres).
    /// No máximo `budget` consultas novas ao motor por chamada; o segundo valor diz que parou
    /// no teto e que uma próxima chamada acha mais.
    pub fn flagged(
        &mut self,
        text: &str,
        words: &[Word],
        visible: Range<usize>,
        budget: usize,
    ) -> (Vec<Word>, bool) {
        let first = words.partition_point(|word| word.chars.end <= visible.start);
        let before = self.lookups;
        let mut out = Vec::new();
        for word in &words[first..] {
            if word.chars.start >= visible.end {
                break;
            }
            if self.lookups - before >= budget
                && !self.verdicts.contains_key(&text[word.bytes.clone()])
            {
                return (out, true);
            }
            if !self.is_ok(&text[word.bytes.clone()]) {
                out.push(word.clone());
            }
        }
        (out, false)
    }
}

// ---------------------------------------------------------------------------------------------
// Download
// ---------------------------------------------------------------------------------------------

/// Baixa e confere o dicionário com `fetch` (o HTTP vem de fora, para testar sem rede) e só
/// então grava, via arquivo temporário. Qualquer arquivo com sha256 diferente é recusado e
/// nada é gravado.
pub fn download(
    dir: &Path,
    language: &Language,
    fetch: &dyn Fn(&str) -> Result<Vec<u8>, String>,
) -> Result<(), String> {
    let mut fetched = Vec::new();
    for source in [&language.aff, &language.dic] {
        let bytes = fetch(source.url)?;
        let got = nota_sync::protocolo::sha256_hex(&bytes);
        if got != source.sha256 {
            return Err(format!(
                "{} veio diferente do esperado (sha256 {got}); nada foi gravado",
                source.name
            ));
        }
        fetched.push((source, bytes));
    }
    std::fs::create_dir_all(dir).map_err(|error| format!("não consegui criar a pasta: {error}"))?;
    for (source, bytes) in fetched {
        let part = dir.join(format!("{}.part", source.name));
        std::fs::write(&part, bytes)
            .and_then(|()| std::fs::rename(&part, dir.join(source.name)))
            .map_err(|error| format!("não consegui gravar {}: {error}", source.name))?;
    }
    Ok(())
}

/// GET por HTTPS (só `https://`), com tempo limite e teto de tamanho. Sem rede, devolve o erro.
pub fn http_fetch(url: &str) -> Result<Vec<u8>, String> {
    if !url.starts_with("https://") {
        return Err("só https é aceito para baixar o dicionário".into());
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(180)))
        .timeout_connect(Some(Duration::from_secs(20)))
        .max_redirects(0)
        .build()
        .into();
    let mut response = agent
        .get(url)
        .call()
        .map_err(|error| format!("sem conexão com o servidor do dicionário: {error}"))?;
    response
        .body_mut()
        .with_config()
        .limit(MAX_DOWNLOAD_BYTES)
        .read_to_vec()
        .map_err(|error| format!("o download do dicionário falhou: {error}"))
}

// ---------------------------------------------------------------------------------------------
// Serviço
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// Desligado ou ainda não começou.
    Idle,
    /// Lendo o dicionário do disco.
    Loading,
    Ready,
    /// Os arquivos não estão na pasta de dados.
    Missing,
    Downloading,
}

enum Job {
    Loaded(Result<Engine, String>),
    Downloaded(Result<(), String>),
}

/// A nota que já foi dividida em palavras (refeita só quando o texto muda).
#[derive(Default)]
struct Tokens {
    text: String,
    words: Vec<Word>,
}

/// O estado do corretor do editor: liga/desliga, dicionário (carregado e baixado em segundo
/// plano), lista pessoal e as palavras da nota aberta.
pub struct SpellService {
    enabled: bool,
    dir: Option<PathBuf>,
    language: &'static Language,
    status: Status,
    /// O último erro de carga ou download, para a tela de configurações.
    pub message: Option<String>,
    checker: Checker,
    tokens: Tokens,
    job: Option<Receiver<Job>>,
}

impl Default for SpellService {
    fn default() -> Self {
        Self::new(dictionary_dir(), &PT_BR)
    }
}

impl SpellService {
    pub fn new(dir: Option<PathBuf>, language: &'static Language) -> Self {
        Self {
            enabled: false,
            dir,
            language,
            status: Status::Idle,
            message: None,
            checker: Checker::default(),
            tokens: Tokens::default(),
            job: None,
        }
    }

    pub fn language(&self) -> &'static Language {
        self.language
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Os arquivos do dicionário estão na pasta de dados?
    pub fn is_installed(&self) -> bool {
        self.dir
            .as_deref()
            .is_some_and(|dir| installed(dir, self.language))
    }

    /// Ligado e com dicionário carregado: o editor pode pedir palavras erradas.
    pub fn active(&self) -> bool {
        self.enabled && self.status == Status::Ready
    }

    /// Há trabalho em segundo plano (a tela pede um novo quadro até ele acabar).
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// Chamado a cada quadro: começa a carregar quando ligado e, se há resultado de um trabalho
    /// em segundo plano, o recolhe. Devolve `true` quando algo mudou na tela.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        if let Some(receiver) = &self.job {
            match receiver.try_recv() {
                Ok(Job::Loaded(Ok(engine))) => {
                    self.checker.set_engine(Some(Arc::new(engine)));
                    self.status = Status::Ready;
                    self.message = None;
                    self.job = None;
                    changed = true;
                }
                Ok(Job::Loaded(Err(error))) => {
                    self.status = Status::Missing;
                    self.message = Some(error);
                    self.job = None;
                    changed = true;
                }
                Ok(Job::Downloaded(Ok(()))) => {
                    self.job = None;
                    self.status = Status::Idle;
                    self.message = None;
                    changed = true;
                }
                Ok(Job::Downloaded(Err(error))) => {
                    self.job = None;
                    self.status = Status::Missing;
                    self.message = Some(error);
                    changed = true;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.job = None;
                    self.status = Status::Missing;
                    self.message = Some("o trabalho do dicionário parou de repente".into());
                    changed = true;
                }
            }
        }
        if self.enabled && self.status == Status::Idle && self.job.is_none() {
            self.start_loading();
            changed = true;
        }
        changed
    }

    fn start_loading(&mut self) {
        let Some(dir) = self.dir.clone() else {
            self.status = Status::Missing;
            self.message = Some("não achei a pasta de dados do app".into());
            return;
        };
        self.checker
            .set_personal(Personal::open(personal_file(&dir, self.language)));
        if !installed(&dir, self.language) {
            self.status = Status::Missing;
            return;
        }
        let language = self.language;
        let (sender, receiver) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("spell-load".into())
            .spawn(move || {
                let _ = sender.send(Job::Loaded(Engine::load(&dir, language)));
            });
        match spawned {
            Ok(_) => {
                self.status = Status::Loading;
                self.job = Some(receiver);
            }
            Err(error) => {
                self.status = Status::Missing;
                self.message = Some(format!(
                    "não consegui iniciar a leitura do dicionário: {error}"
                ));
            }
        }
    }

    /// Baixa o dicionário do idioma (o botão "Baixar dicionário pt-BR").
    pub fn start_download(&mut self) {
        self.start_download_with(http_fetch);
    }

    pub fn start_download_with(
        &mut self,
        fetch: impl Fn(&str) -> Result<Vec<u8>, String> + Send + 'static,
    ) {
        if self.job.is_some() {
            return;
        }
        let Some(dir) = self.dir.clone() else {
            self.message = Some("não achei a pasta de dados do app".into());
            return;
        };
        let language = self.language;
        let (sender, receiver) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("spell-download".into())
            .spawn(move || {
                let _ = sender.send(Job::Downloaded(download(&dir, language, &fetch)));
            });
        match spawned {
            Ok(_) => {
                self.status = Status::Downloading;
                self.message = None;
                self.job = Some(receiver);
            }
            Err(error) => {
                self.message = Some(format!("não consegui iniciar o download: {error}"));
            }
        }
    }

    /// As palavras erradas de `text` que tocam `visible` (caracteres), no máximo `budget`
    /// consultas novas por chamada. O segundo valor `true` pede outro quadro.
    pub fn flagged(
        &mut self,
        text: &str,
        visible: Range<usize>,
        budget: usize,
    ) -> (Vec<Word>, bool) {
        if !self.active() {
            return (Vec::new(), false);
        }
        if self.tokens.text != text {
            self.tokens.text.clear();
            self.tokens.text.push_str(text);
            self.tokens.words = words(text);
        }
        self.checker
            .flagged(&self.tokens.text, &self.tokens.words, visible, budget)
    }

    /// Até 5 correções para `word`. Precisa do dicionário carregado.
    pub fn suggestions(&self, word: &str) -> Vec<String> {
        self.checker
            .engine()
            .map(|engine| engine.suggest(word, 5))
            .unwrap_or_default()
    }

    /// O motor, para quem quer sugerir fora do quadro (em outra thread).
    pub fn engine(&self) -> Option<Arc<Engine>> {
        self.checker.engine()
    }

    /// "Adicionar ao dicionário": grava na lista pessoal e some o sublinhado.
    pub fn add_to_personal(&mut self, word: &str) -> std::io::Result<()> {
        self.checker.add_personal(word)
    }

    /// "Ignorar nesta sessão".
    pub fn ignore(&mut self, word: &str) {
        self.checker.ignore(word);
    }
}

#[cfg(test)]
mod tests;
