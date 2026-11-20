//! Markdown -> HTML SEGURO para a tela de Leitura (sem Dioxus; testa no Windows).
//!
//! Por que não usar o gerador de HTML da `pulldown-cmark`: as notas chegarão pelo sync de outro
//! aparelho, e a WebView tem ponte para o Rust. Todo o HTML sai daqui, de uma lista fixa de
//! tags e atributos, e o que vem da nota nunca vira marcação:
//!
//! - HTML cru (`Html` e `InlineHtml`) vira **texto escapado**;
//! - texto, `alt` e nomes são escapados (`& < > " '`);
//! - nenhum `href` é gerado: um link é um `<a data-a="N">` sem endereço, e o destino real fica
//!   na lista `alvos` do Rust (a WebView nunca navega para lugar nenhum);
//! - só `http(s)` é aceito como endereço externo; qualquer outro esquema (`javascript:`, `data:`,
//!   `vbscript:`, `file:`...) é descartado, em qualquer caixa e com espaços ou controles no meio;
//! - imagem só vem do cofre (quem sabe é o `Mundo`) e entra como `data:` gerado por nós.
//!
//! A caixa de tarefa é um `<span data-t="N">`; `Pagina::tarefas[N].byte` é o lugar exato do
//! `[ ]` / `[x]` no texto, para a tela trocar só aquele par de colchetes.

use std::collections::HashMap;
use std::ops::Range;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, LinkType, Options, Parser, Tag, TagEnd};

/// O que o Markdown precisa saber do cofre (separado para testar sem disco).
pub trait Mundo {
    /// Caminho da nota (com `.md`) a que `alvo` se refere, ou `None` se não existe. `alvo` é
    /// um nome ou caminho **sem** `.md` e sem `#cabeçalho`; `de` é a nota que contém o link.
    fn nota(&self, alvo: &str, de: &str) -> Option<String>;
    /// A imagem como URI `data:` se for um arquivo de imagem **de dentro do cofre**; `None`
    /// para qualquer outra coisa (fora do cofre, grande demais, não é imagem).
    fn imagem(&self, origem: &str, de: &str) -> Option<String>;
}

/// Para onde um toque em link leva.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Alvo {
    /// Abrir esta nota no app (caminho relativo ao cofre, com `.md`).
    Nota(String),
    /// Abrir no navegador do sistema (sempre `http://` ou `https://`).
    Web(String),
}

/// Uma caixa de tarefa da nota.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tarefa {
    /// Posição (em bytes) do `[` do `[ ]` / `[x]` no texto lido.
    pub byte: usize,
    pub marcada: bool,
}

/// O resultado: o HTML e as tabelas que os `data-a` e `data-t` do HTML indexam.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pagina {
    pub html: String,
    pub alvos: Vec<Alvo>,
    pub tarefas: Vec<Tarefa>,
}

/// Escapa texto para HTML (também serve dentro de atributo entre aspas).
pub fn escapar(texto: &str) -> String {
    let mut s = String::with_capacity(texto.len() + 8);
    for c in texto.chars() {
        match c {
            '&' => s.push_str("&amp;"),
            '<' => s.push_str("&lt;"),
            '>' => s.push_str("&gt;"),
            '"' => s.push_str("&quot;"),
            '\'' => s.push_str("&#39;"),
            _ => s.push(c),
        }
    }
    s
}

/// O esquema de um destino (`javascript`, `http`...) em minúsculas, ou `None` se é um caminho
/// relativo. Tira controles e espaços de dentro antes de olhar (o navegador ignora tabulação e
/// quebra de linha no meio do esquema). Dois-pontos antes de qualquer `/`, `?` ou `#` conta
/// como esquema, mesmo que mal formado (vira `"?"`, que nunca é aceito).
pub fn esquema(destino: &str) -> Option<String> {
    let limpo: String = destino
        .chars()
        .filter(|c| !c.is_control() && !c.is_whitespace())
        .collect();
    let fim = limpo.find([':', '/', '?', '#'])?;
    if limpo.as_bytes()[fim] != b':' {
        return None;
    }
    let e = &limpo[..fim];
    let bem_formado = e.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && e.chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    Some(if bem_formado {
        e.to_ascii_lowercase()
    } else {
        "?".to_string()
    })
}

/// O endereço externo aceito (`http`/`https` com algo depois), pronto para abrir; `None` para o
/// resto. Espaço no meio vira `%20`; controle no meio rejeita.
pub fn url_web(destino: &str) -> Option<String> {
    let d = destino.trim();
    if d.chars().any(char::is_control) {
        return None;
    }
    match esquema(d)?.as_str() {
        "http" | "https" => {}
        _ => return None,
    }
    let minusculo = d.to_ascii_lowercase();
    let resto = minusculo
        .strip_prefix("https://")
        .or_else(|| minusculo.strip_prefix("http://"))?;
    if resto.is_empty() || resto.starts_with('/') {
        return None;
    }
    Some(d.replace(' ', "%20"))
}

/// O "endereço" curto para mostrar ao lado do texto do link (`exemplo.com`).
fn host_de(url: &str) -> String {
    let sem_esquema = url.split_once("://").map_or(url, |(_, r)| r);
    let autoridade = sem_esquema
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(sem_esquema);
    let host = autoridade.rsplit('@').next().unwrap_or(autoridade);
    host.chars().take(60).collect()
}

fn valor_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// `%20` -> espaço (sequência inválida fica como está; bytes que não formam UTF-8 viram `�`).
pub fn decodificar_percentual(s: &str) -> String {
    let b = s.as_bytes();
    let mut saida = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(a), Some(c)) = (valor_hex(b[i + 1]), valor_hex(b[i + 2]))
        {
            saida.push(a * 16 + c);
            i += 3;
            continue;
        }
        saida.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&saida).into_owned()
}

/// Tira `#âncora`, `^bloco` e `?consulta` do fim de um destino.
fn sem_ancora(s: &str) -> &str {
    s.split(['#', '^', '?']).next().unwrap_or(s)
}

fn sem_md(s: &str) -> &str {
    let t = s.trim();
    if t.len() > 3 && t.to_ascii_lowercase().ends_with(".md") {
        &t[..t.len() - 3]
    } else {
        t
    }
}

/// O que um link vira.
enum Destino {
    Nota(String),
    /// Nota que não existe no cofre.
    Faltando,
    Web(String),
    /// Descartado: fica só o texto.
    Nenhum,
}

fn classificar(tipo: LinkType, destino: &str, de: &str, mundo: &dyn Mundo) -> Destino {
    if let Some(e) = esquema(destino) {
        return match (e.as_str(), url_web(destino)) {
            ("http" | "https", Some(u)) => Destino::Web(u),
            _ => Destino::Nenhum,
        };
    }
    let eh_wiki = matches!(tipo, LinkType::WikiLink { .. });
    let sem = sem_ancora(destino);
    let decodificado = decodificar_percentual(sem);
    let alvo = sem_md(&decodificado);
    if alvo.is_empty() {
        return Destino::Nenhum; // `[[#cabeçalho]]`, `[x](#ancora)`
    }
    // Link comum só vale para nota: `Nota.md`. Wiki aceita nome sem extensão.
    if !eh_wiki && !decodificado.trim().to_ascii_lowercase().ends_with(".md") {
        return Destino::Nenhum;
    }
    match mundo.nota(alvo, de) {
        Some(c) => Destino::Nota(c),
        None => Destino::Faltando,
    }
}

/// Nome do arquivo de um destino de imagem, para o texto "imagem: nome".
fn nome_da_imagem(destino: &str) -> String {
    let sem = decodificar_percentual(sem_ancora(destino.trim()));
    let ultimo = sem.rsplit('/').next().unwrap_or(&sem).trim();
    ultimo.to_string()
}

/// Só letras, números e `+-_#.` da linguagem de um bloco de código (vai para um `class`).
fn linguagem(info: &str) -> String {
    info.split(|c: char| c.is_whitespace() || c == '{' || c == ',')
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || "+-_#.".contains(*c))
        .take(24)
        .collect()
}

fn classe_alinhamento(a: Alignment) -> &'static str {
    match a {
        Alignment::None => "",
        Alignment::Left => " class=\"a-e\"",
        Alignment::Center => " class=\"a-c\"",
        Alignment::Right => " class=\"a-d\"",
    }
}

struct ImagemAberta {
    destino: String,
    alt: String,
    /// Imagens dentro do texto alternativo de outra: só conta a profundidade.
    fundo: u32,
}

struct Renderizador<'a> {
    de: &'a str,
    mundo: &'a dyn Mundo,
    saida: String,
    /// Saídas guardadas enquanto uma nota de rodapé é escrita à parte.
    guardadas: Vec<String>,
    alvos: Vec<Alvo>,
    tarefas: Vec<Tarefa>,
    /// O que escrever quando o link aberto terminar.
    fecho_link: Vec<String>,
    imagem: Option<ImagemAberta>,
    alinhamentos: Vec<Alignment>,
    coluna: usize,
    no_cabecalho: bool,
    numeros: HashMap<String, usize>,
    definicoes: Vec<(usize, String)>,
    rodape_atual: Option<usize>,
}

impl<'a> Renderizador<'a> {
    fn novo(de: &'a str, mundo: &'a dyn Mundo) -> Self {
        Self {
            de,
            mundo,
            saida: String::new(),
            guardadas: Vec::new(),
            alvos: Vec::new(),
            tarefas: Vec::new(),
            fecho_link: Vec::new(),
            imagem: None,
            alinhamentos: Vec::new(),
            coluna: 0,
            no_cabecalho: false,
            numeros: HashMap::new(),
            definicoes: Vec::new(),
            rodape_atual: None,
        }
    }

    fn numero(&mut self, nome: &str) -> usize {
        let proximo = self.numeros.len() + 1;
        *self.numeros.entry(nome.to_string()).or_insert(proximo)
    }

    fn texto(&mut self, s: &str) {
        self.saida.push_str(&escapar(s));
    }

    fn evento(&mut self, evento: Event<'_>, faixa: Range<usize>) {
        if self.imagem.is_some() {
            self.dentro_da_imagem(evento);
            return;
        }
        match evento {
            Event::Start(tag) => self.abrir(tag),
            Event::End(fim) => self.fechar(fim),
            Event::Text(s) => self.texto(&s),
            Event::Code(s) => {
                self.saida.push_str("<code>");
                self.texto(&s);
                self.saida.push_str("</code>");
            }
            // HTML cru da nota: NUNCA vira marcação, só texto.
            Event::Html(s) | Event::InlineHtml(s) => self.texto(&s),
            Event::InlineMath(s) | Event::DisplayMath(s) => {
                self.saida.push_str("<code>");
                self.texto(&s);
                self.saida.push_str("</code>");
            }
            Event::FootnoteReference(nome) => {
                let n = self.numero(&nome);
                self.saida
                    .push_str(&format!("<sup class=\"nota-rodape\">{n}</sup>"));
            }
            Event::SoftBreak => self.saida.push('\n'),
            Event::HardBreak => self.saida.push_str("<br>\n"),
            Event::Rule => self.saida.push_str("<hr>\n"),
            Event::TaskListMarker(marcada) => {
                let i = self.tarefas.len();
                self.tarefas.push(Tarefa {
                    byte: faixa.start,
                    marcada,
                });
                self.saida.push_str(&format!(
                    "<span class=\"caixa\" role=\"checkbox\" tabindex=\"0\" aria-checked=\"{marcada}\" data-t=\"{i}\"></span>"
                ));
            }
        }
    }

    /// Enquanto uma imagem está aberta só interessa o texto dela (vira o `alt`).
    fn dentro_da_imagem(&mut self, evento: Event<'_>) {
        let Some(img) = self.imagem.as_mut() else {
            return;
        };
        match evento {
            Event::Text(s) | Event::Code(s) | Event::Html(s) | Event::InlineHtml(s) => {
                img.alt.push_str(&s)
            }
            Event::Start(Tag::Image { .. }) => img.fundo += 1,
            Event::End(TagEnd::Image) if img.fundo > 0 => img.fundo -= 1,
            Event::End(TagEnd::Image) => {
                let img = self.imagem.take().expect("imagem aberta");
                self.fim_da_imagem(&img);
            }
            Event::SoftBreak | Event::HardBreak => img.alt.push(' '),
            _ => {}
        }
    }

    fn fim_da_imagem(&mut self, img: &ImagemAberta) {
        let nome = nome_da_imagem(&img.destino);
        let dentro = if esquema(&img.destino).is_some() {
            None
        } else {
            let origem = decodificar_percentual(sem_ancora(img.destino.trim()));
            self.mundo.imagem(&origem, self.de)
        };
        match dentro {
            Some(uri) => self.saida.push_str(&format!(
                "<img class=\"imagem\" src=\"{}\" alt=\"{}\">",
                escapar(&uri),
                escapar(&img.alt)
            )),
            None => {
                let rotulo = if nome.is_empty() {
                    "imagem".to_string()
                } else {
                    format!("imagem: {nome}")
                };
                self.saida.push_str(&format!(
                    "<span class=\"imagem-fora\">{}</span>",
                    escapar(&rotulo)
                ));
            }
        }
    }

    fn abrir(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.saida.push_str("<p>"),
            Tag::Heading { level, .. } => {
                let n = level as usize;
                self.saida.push_str(&format!("<h{n}>"));
            }
            Tag::BlockQuote(_) => self.saida.push_str("<blockquote>\n"),
            Tag::CodeBlock(tipo) => {
                let lang = match tipo {
                    CodeBlockKind::Fenced(info) => linguagem(&info),
                    CodeBlockKind::Indented => String::new(),
                };
                if lang.is_empty() {
                    self.saida.push_str("<pre class=\"codigo\"><code>");
                } else {
                    self.saida.push_str(&format!(
                        "<pre class=\"codigo\"><code class=\"lang-{lang}\">"
                    ));
                }
            }
            Tag::HtmlBlock => self.saida.push_str("<pre class=\"html-cru\">"),
            Tag::List(None) => self.saida.push_str("<ul>\n"),
            Tag::List(Some(1)) => self.saida.push_str("<ol>\n"),
            Tag::List(Some(n)) => self.saida.push_str(&format!("<ol start=\"{n}\">\n")),
            Tag::Item => self.saida.push_str("<li>"),
            Tag::FootnoteDefinition(nome) => {
                let n = self.numero(&nome);
                self.guardadas.push(std::mem::take(&mut self.saida));
                self.rodape_atual = Some(n);
            }
            Tag::DefinitionList => self.saida.push_str("<dl>\n"),
            Tag::DefinitionListTitle => self.saida.push_str("<dt>"),
            Tag::DefinitionListDefinition => self.saida.push_str("<dd>"),
            Tag::Table(alinhamentos) => {
                self.alinhamentos = alinhamentos;
                self.saida.push_str("<div class=\"tabela\"><table>\n");
            }
            Tag::TableHead => {
                self.no_cabecalho = true;
                self.coluna = 0;
                self.saida.push_str("<thead><tr>");
            }
            Tag::TableRow => {
                self.coluna = 0;
                self.saida.push_str("<tr>");
            }
            Tag::TableCell => {
                let a = self
                    .alinhamentos
                    .get(self.coluna)
                    .copied()
                    .unwrap_or(Alignment::None);
                self.coluna += 1;
                let t = if self.no_cabecalho { "th" } else { "td" };
                self.saida
                    .push_str(&format!("<{t}{}>", classe_alinhamento(a)));
            }
            Tag::Emphasis => self.saida.push_str("<em>"),
            Tag::Strong => self.saida.push_str("<strong>"),
            Tag::Strikethrough => self.saida.push_str("<del>"),
            Tag::Superscript => self.saida.push_str("<sup>"),
            Tag::Subscript => self.saida.push_str("<sub>"),
            Tag::Link {
                link_type,
                dest_url,
                ..
            } => self.abrir_link(link_type, &dest_url),
            Tag::Image { dest_url, .. } => {
                self.imagem = Some(ImagemAberta {
                    destino: dest_url.to_string(),
                    alt: String::new(),
                    fundo: 0,
                });
            }
            Tag::MetadataBlock(_) => self
                .saida
                .push_str("<details class=\"propriedades\"><summary>Propriedades</summary><pre>"),
        }
    }

    fn abrir_link(&mut self, tipo: LinkType, destino: &str) {
        match classificar(tipo, destino, self.de, self.mundo) {
            Destino::Nota(caminho) => {
                let i = self.alvos.len();
                self.alvos.push(Alvo::Nota(caminho));
                self.saida.push_str(&format!(
                    "<a class=\"link-nota\" role=\"link\" tabindex=\"0\" data-a=\"{i}\">"
                ));
                self.fecho_link.push("</a>".into());
            }
            Destino::Web(url) => {
                let i = self.alvos.len();
                // O texto do autolink já é o endereço; nos outros mostramos o servidor.
                let mostra_host = !matches!(tipo, LinkType::Autolink);
                let host = host_de(&url);
                self.alvos.push(Alvo::Web(url));
                self.saida.push_str(&format!(
                    "<a class=\"link-web\" role=\"link\" tabindex=\"0\" data-a=\"{i}\">"
                ));
                let mut fecho = String::from("</a>");
                if mostra_host && !host.is_empty() {
                    fecho.push_str(&format!(
                        " <span class=\"host\">({})</span>",
                        escapar(&host)
                    ));
                }
                self.fecho_link.push(fecho);
            }
            Destino::Faltando => {
                self.saida.push_str("<span class=\"link-quebrado\">");
                self.fecho_link.push("</span>".into());
            }
            Destino::Nenhum => {
                self.saida.push_str("<span class=\"sem-link\">");
                self.fecho_link.push("</span>".into());
            }
        }
    }

    fn fechar(&mut self, fim: TagEnd) {
        match fim {
            TagEnd::Paragraph => self.saida.push_str("</p>\n"),
            TagEnd::Heading(nivel) => {
                let n = nivel as usize;
                self.saida.push_str(&format!("</h{n}>\n"));
            }
            TagEnd::BlockQuote(_) => self.saida.push_str("</blockquote>\n"),
            TagEnd::CodeBlock => self.saida.push_str("</code></pre>\n"),
            TagEnd::HtmlBlock => self.saida.push_str("</pre>\n"),
            TagEnd::List(ordenada) => {
                self.saida
                    .push_str(if ordenada { "</ol>\n" } else { "</ul>\n" })
            }
            TagEnd::Item => self.saida.push_str("</li>\n"),
            TagEnd::FootnoteDefinition => {
                if let (Some(n), Some(anterior)) = (self.rodape_atual.take(), self.guardadas.pop())
                {
                    let html = std::mem::replace(&mut self.saida, anterior);
                    self.definicoes.push((n, html));
                }
            }
            TagEnd::DefinitionList => self.saida.push_str("</dl>\n"),
            TagEnd::DefinitionListTitle => self.saida.push_str("</dt>\n"),
            TagEnd::DefinitionListDefinition => self.saida.push_str("</dd>\n"),
            TagEnd::Table => self.saida.push_str("</tbody></table></div>\n"),
            TagEnd::TableHead => {
                self.no_cabecalho = false;
                self.saida.push_str("</tr></thead><tbody>\n");
            }
            TagEnd::TableRow => self.saida.push_str("</tr>\n"),
            TagEnd::TableCell => {
                self.saida
                    .push_str(if self.no_cabecalho { "</th>" } else { "</td>" })
            }
            TagEnd::Emphasis => self.saida.push_str("</em>"),
            TagEnd::Strong => self.saida.push_str("</strong>"),
            TagEnd::Strikethrough => self.saida.push_str("</del>"),
            TagEnd::Superscript => self.saida.push_str("</sup>"),
            TagEnd::Subscript => self.saida.push_str("</sub>"),
            TagEnd::Link => {
                if let Some(fecho) = self.fecho_link.pop() {
                    self.saida.push_str(&fecho);
                }
            }
            TagEnd::Image => {}
            TagEnd::MetadataBlock(_) => self.saida.push_str("</pre></details>\n"),
        }
    }

    fn terminar(mut self) -> Pagina {
        self.definicoes.sort_by_key(|(n, _)| *n);
        if !self.definicoes.is_empty() {
            self.saida
                .push_str("<section class=\"rodape-notas\"><hr><ol>\n");
            for (n, html) in std::mem::take(&mut self.definicoes) {
                self.saida
                    .push_str(&format!("<li value=\"{n}\">{html}</li>\n"));
            }
            self.saida.push_str("</ol></section>\n");
        }
        Pagina {
            html: self.saida,
            alvos: self.alvos,
            tarefas: self.tarefas,
        }
    }
}

/// Converte a nota em HTML seguro. `de` é o caminho da nota (para resolver links e imagens
/// relativos).
pub fn renderizar(texto: &str, de: &str, mundo: &dyn Mundo) -> Pagina {
    let opcoes = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_WIKILINKS
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS;
    let mut r = Renderizador::novo(de, mundo);
    for (evento, faixa) in Parser::new_ext(texto, opcoes).into_offset_iter() {
        r.evento(evento, faixa);
    }
    r.terminar()
}

/// Troca o `[ ]` por `[x]` (ou o contrário) que começa em `byte`, e só ele. `None` se ali não
/// há uma caixa de tarefa (o texto mudou).
pub fn alternar_tarefa(texto: &str, byte: usize) -> Option<String> {
    let b = texto.as_bytes();
    if b.get(byte) != Some(&b'[') || b.get(byte + 2) != Some(&b']') {
        return None;
    }
    let novo = match b.get(byte + 1)? {
        b' ' => 'x',
        b'x' | b'X' => ' ',
        _ => return None,
    };
    let mut s = String::with_capacity(texto.len());
    s.push_str(&texto[..byte + 1]);
    s.push(novo);
    s.push_str(&texto[byte + 2..]);
    Some(s)
}

#[cfg(test)]
mod testes {
    use super::*;

    /// Um cofre de mentira: resolve só o que foi cadastrado.
    #[derive(Default)]
    struct Falso {
        notas: Vec<(&'static str, &'static str)>,
        imagens: Vec<&'static str>,
    }

    impl Mundo for Falso {
        fn nota(&self, alvo: &str, _de: &str) -> Option<String> {
            self.notas
                .iter()
                .find(|(a, _)| *a == alvo)
                .map(|(_, c)| c.to_string())
        }
        fn imagem(&self, origem: &str, _de: &str) -> Option<String> {
            self.imagens
                .contains(&origem)
                .then(|| "data:image/png;base64,AAAA".to_string())
        }
    }

    fn mundo() -> Falso {
        Falso {
            notas: vec![
                ("Projeto", "Trabalho/Projeto.md"),
                ("Pasta/Outra nota", "Pasta/Outra nota.md"),
            ],
            imagens: vec!["foto.png", "img/dentro.png"],
        }
    }

    fn md(texto: &str) -> Pagina {
        renderizar(texto, "Nota.md", &mundo())
    }

    /// Confere que TODA tag e atributo do HTML está na lista permitida. Como o texto vem
    /// escapado, qualquer `<` do HTML é uma tag nossa.
    fn so_marcacao_permitida(html: &str) {
        const TAGS: &[&str] = &[
            "p",
            "h1",
            "h2",
            "h3",
            "h4",
            "h5",
            "h6",
            "blockquote",
            "pre",
            "code",
            "ul",
            "ol",
            "li",
            "dl",
            "dt",
            "dd",
            "table",
            "thead",
            "tbody",
            "tr",
            "th",
            "td",
            "div",
            "em",
            "strong",
            "del",
            "sup",
            "sub",
            "a",
            "span",
            "br",
            "hr",
            "img",
            "section",
            "details",
            "summary",
        ];
        const ATRIBUTOS: &[&str] = &[
            "class",
            "role",
            "tabindex",
            "aria-checked",
            "data-a",
            "data-t",
            "start",
            "value",
            "src",
            "alt",
        ];
        let mut resto = html;
        while let Some(i) = resto.find('<') {
            let depois = &resto[i + 1..];
            let fim = depois.find('>').expect("tag sem fechar");
            let tag = &depois[..fim];
            let tag = tag.trim_start_matches('/');
            let mut partes = tag.splitn(2, char::is_whitespace);
            let nome = partes.next().unwrap();
            assert!(TAGS.contains(&nome), "tag não permitida: <{tag}> em {html}");
            if let Some(atrs) = partes.next() {
                // name="valor" ... (o valor nunca tem aspas, vem escapado)
                let mut a = atrs.trim();
                while !a.is_empty() {
                    let eq = a.find("=\"").expect("atributo sem valor entre aspas");
                    let nome_atr = a[..eq].trim();
                    assert!(
                        ATRIBUTOS.contains(&nome_atr),
                        "atributo não permitido: {nome_atr} em <{tag}>"
                    );
                    let valor = &a[eq + 2..];
                    let f = valor.find('"').expect("valor sem fechar");
                    a = valor[f + 1..].trim();
                }
            }
            resto = &depois[fim + 1..];
        }
    }

    // ---------- 1. HTML cru nunca executa ----------

    #[test]
    fn html_cru_vira_texto_escapado() {
        let p = md(
            "<script>alert(1)</script>\n\ntexto <img src=x onerror=alert(1)> e <b onclick=\"x()\">negrito</b>\n\n<div onclick=\"roubar()\">\nbloco\n</div>\n",
        );
        assert!(!p.html.contains("<script"), "{}", p.html);
        assert!(!p.html.contains("<img"), "{}", p.html);
        assert!(!p.html.contains("<b "), "{}", p.html);
        assert!(!p.html.contains("<div onclick"), "{}", p.html);
        assert!(
            p.html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
            "{}",
            p.html
        );
        assert!(
            p.html.contains("&lt;img src=x onerror=alert(1)&gt;"),
            "{}",
            p.html
        );
        assert!(p.html.contains("&lt;b onclick=&quot;x()&quot;&gt;"));
        so_marcacao_permitida(&p.html);
    }

    #[test]
    fn html_cru_dentro_de_outros_blocos_tambem_e_texto() {
        let p = md(
            "> <script>x</script>\n\n- item <iframe src=//evil></iframe>\n\n| a |\n|---|\n| <svg onload=x> |\n\n`<script>` e\n\n```\n<script>y</script>\n```\n\n[<img src=x onerror=1>](Projeto.md)\n\n![<svg onload=1>](foto.png)\n\n<!-- comentário -->\n\n<style>*{display:none}</style>\n\n<a href=\"javascript:alert(1)\">clique</a>\n",
        );
        assert!(!p.html.contains("<script"), "{}", p.html);
        assert!(!p.html.contains("<iframe"), "{}", p.html);
        assert!(!p.html.contains("<svg"), "{}", p.html);
        assert!(!p.html.contains("<style"), "{}", p.html);
        assert!(!p.html.contains("<a href"), "{}", p.html);
        so_marcacao_permitida(&p.html);
    }

    #[test]
    fn texto_com_aspas_e_e_comercial_e_escapado_nos_atributos() {
        let p = md("![a\"b'c<d>&e](foto.png)\n\n![x\" onerror=\"1](https://x/y.png)\n");
        assert!(p.html.contains("alt=\"a&quot;b&#39;c&lt;d&gt;&amp;e\""));
        assert!(!p.html.contains("onerror=\"1"));
        so_marcacao_permitida(&p.html);
    }

    #[test]
    fn linguagem_do_bloco_de_codigo_nao_escapa_do_atributo() {
        let p = md("```rust\" onclick=\"x\nfn a() {}\n```\n\n```c++{.x}\nint a;\n```\n");
        assert!(p.html.contains("class=\"lang-rust\""), "{}", p.html);
        assert!(p.html.contains("class=\"lang-c++\""), "{}", p.html);
        assert!(!p.html.contains("onclick"));
        so_marcacao_permitida(&p.html);
    }

    // ---------- 2. URLs perigosas ----------

    fn nenhum_link(texto: &str) {
        let p = md(texto);
        assert!(p.alvos.is_empty(), "virou link: {texto:?} -> {:?}", p.alvos);
        assert!(!p.html.contains("data-a"), "{texto:?} -> {}", p.html);
        assert!(!p.html.contains("href"), "{texto:?} -> {}", p.html);
        so_marcacao_permitida(&p.html);
    }

    #[test]
    fn links_javascript_data_e_vbscript_sao_descartados() {
        for texto in [
            "[a](javascript:alert(1))",
            "[a](JaVaScRiPt:alert(1))",
            "[a](JAVASCRIPT:alert(1))",
            "[a](<  javascript:alert(1)>)",
            "[a](<\tjavascript:alert(1)>)",
            "[a](java&#9;script:alert(1))",
            "[a](java&#10;script:alert(1))",
            "[a](&#106;avascript:alert(1))",
            "[a](&#x6A;avascript:alert(1))",
            "[a](data:text/html,<script>alert(1)</script>)",
            "[a](DaTa:text/html;base64,PHNjcmlwdD4=)",
            "[a](<  data:text/html,x>)",
            "[a](vbscript:msgbox(1))",
            "[a](VBScript:x)",
            "[a](file:///etc/passwd)",
            "[a](intent://x#Intent;end)",
            "[a](content://x)",
            "[a](mailto:x@y.com)",
            "[a](ftp://x/y)",
            "[a](//evil.com/x)",
            "[a](tel:123)",
            "<javascript:alert(1)>",
            "<JaVaScRiPt:alert(1)>",
            "<data:text/html,x>",
            "[ref]\n\n[ref]: javascript:alert(1)",
            "[ref]\n\n[ref]: DATA:text/html,x",
            "![x](javascript:alert(1))",
            "![x](data:text/html,x)",
        ] {
            nenhum_link(texto);
        }
    }

    #[test]
    fn link_descartado_mantem_o_texto() {
        let p = md("[clique aqui](javascript:alert(1))");
        assert!(p.html.contains("clique aqui"));
        assert!(!p.html.contains("javascript"));
    }

    #[test]
    fn esquema_ignora_controles_e_espacos_no_meio() {
        assert_eq!(esquema("java\tscript:x").as_deref(), Some("javascript"));
        assert_eq!(esquema("  JaVa\nScRiPt :x").as_deref(), Some("javascript"));
        assert_eq!(esquema("\u{1}data:x").as_deref(), Some("data"));
        assert_eq!(esquema("Nota.md"), None);
        assert_eq!(esquema("Pasta/a:b.md"), None);
        assert_eq!(esquema("#x"), None);
        // Mal formado também conta como esquema (nunca aceito).
        assert_eq!(esquema("1x:y").as_deref(), Some("?"));
        assert_eq!(esquema("a b:c").as_deref(), Some("ab"));
    }

    #[test]
    fn url_web_so_aceita_http_e_https() {
        assert_eq!(
            url_web("https://exemplo.com/a b").as_deref(),
            Some("https://exemplo.com/a%20b")
        );
        assert_eq!(
            url_web(" HTTP://Exemplo.com ").as_deref(),
            Some("HTTP://Exemplo.com")
        );
        for ruim in [
            "javascript:x",
            "http:foo",
            "http://",
            "https:///x",
            "ftp://x",
            "data:x",
            "http://x\ny",
            "Nota.md",
            "",
        ] {
            assert_eq!(url_web(ruim), None, "{ruim:?}");
        }
    }

    // ---------- 3. Links entre notas e web ----------

    #[test]
    fn wikilinks_abrem_a_nota_pela_lista_de_alvos() {
        for texto in [
            "[[Projeto]]",
            "[[Projeto|ver o projeto]]",
            "[[Projeto#Meta]]",
            "[[Projeto.md]]",
            "[ver](Projeto.md)",
            "[ver](Projeto.md#meta)",
            "[ver](Projeto.MD)",
            "[ver](<Projeto.md>)",
        ] {
            let p = md(texto);
            assert_eq!(
                p.alvos,
                [Alvo::Nota("Trabalho/Projeto.md".into())],
                "{texto:?} -> {}",
                p.html
            );
            assert!(p.html.contains("data-a=\"0\""));
            // Nada de endereço no HTML: a WebView nunca navega.
            assert!(!p.html.contains("href"));
            assert!(!p.html.contains("Trabalho/Projeto.md"));
            so_marcacao_permitida(&p.html);
        }
    }

    #[test]
    fn texto_do_wikilink_com_pothole_e_o_apelido() {
        let p = md("[[Projeto|o grande projeto]]");
        assert!(p.html.contains(">o grande projeto</a>"), "{}", p.html);
        let p = md("[[Projeto]]");
        assert!(p.html.contains(">Projeto</a>"));
    }

    #[test]
    fn link_com_caminho_e_espaco_percentual() {
        let p = md("[x](Pasta/Outra%20nota.md#sec)");
        assert_eq!(p.alvos, [Alvo::Nota("Pasta/Outra nota.md".into())]);
    }

    #[test]
    fn link_para_nota_que_nao_existe_e_para_ancora_nao_e_clicavel() {
        for texto in [
            "[[Fantasma]]",
            "[x](Fantasma.md)",
            "[[#cabeçalho]]",
            "[x](#sec)",
        ] {
            let p = md(texto);
            assert!(p.alvos.is_empty(), "{texto:?}");
            assert!(!p.html.contains("data-a"));
        }
        let p = md("[[Fantasma]]");
        assert!(p.html.contains("link-quebrado"));
        // Link comum para o que não é nota: só texto.
        let p = md("[x](documento.pdf) [y](pasta/)");
        assert!(p.alvos.is_empty());
    }

    #[test]
    fn links_http_abrem_no_navegador_e_mostram_o_servidor() {
        let p = md("[veja](https://exemplo.com/caminho?q=1) e [outro](HTTP://Outro.org)");
        assert_eq!(
            p.alvos,
            [
                Alvo::Web("https://exemplo.com/caminho?q=1".into()),
                Alvo::Web("HTTP://Outro.org".into()),
            ]
        );
        assert!(p.html.contains("(exemplo.com)"), "{}", p.html);
        assert!(!p.html.contains("href"));
        assert!(!p.html.contains("https://exemplo.com/caminho"));
        so_marcacao_permitida(&p.html);
    }

    #[test]
    fn autolink_http_mostra_o_proprio_endereco() {
        let p = md("<https://exemplo.com/a>");
        assert_eq!(p.alvos, [Alvo::Web("https://exemplo.com/a".into())]);
        assert!(p.html.contains(">https://exemplo.com/a</a>"), "{}", p.html);
    }

    #[test]
    fn servidor_do_link_nao_se_deixa_enganar_por_usuario() {
        let p = md("[x](https://banco.com@evil.com/p)");
        assert!(p.html.contains("(evil.com)"), "{}", p.html);
    }

    #[test]
    fn titulo_do_link_e_ignorado() {
        let p = md("[x](https://a.com \"onmouseover=alert(1)\")");
        assert!(!p.html.contains("onmouseover"));
        assert!(!p.html.contains("title"));
    }

    // ---------- 4. Imagens ----------

    #[test]
    fn imagem_do_cofre_vira_img_e_o_resto_vira_texto() {
        let p = md(
            "![legenda](foto.png) ![d](img/dentro.png) ![e](../fora.png) ![r](https://x.com/y.png) ![](data:image/png;base64,AAAA) ![[foto.png]]",
        );
        assert_eq!(p.html.matches("<img").count(), 3, "{}", p.html);
        assert!(p.html.contains("alt=\"legenda\""));
        assert!(p.html.contains("imagem: fora.png"));
        assert!(p.html.contains("imagem: y.png"));
        // `data:` escrito pela nota nunca vira src.
        assert!(!p.html.contains("src=\"data:image/png;base64,AAAA\"\n"));
        assert_eq!(
            p.html.matches("src=\"data:image/png;base64,AAAA\"").count(),
            3
        );
        so_marcacao_permitida(&p.html);
    }

    #[test]
    fn imagem_remota_ou_com_esquema_nunca_e_pedida_ao_cofre() {
        struct Espiao;
        impl Mundo for Espiao {
            fn nota(&self, _: &str, _: &str) -> Option<String> {
                None
            }
            fn imagem(&self, origem: &str, _: &str) -> Option<String> {
                panic!("pediu a imagem {origem:?}");
            }
        }
        let p = renderizar(
            "![a](https://x/y.png) ![b](file:///etc/x.png) ![c](javascript:x)",
            "n.md",
            &Espiao,
        );
        assert!(!p.html.contains("<img"));
        assert!(p.html.contains("imagem: y.png"));
    }

    // ---------- 5. Tarefas ----------

    #[test]
    fn tarefas_guardam_o_lugar_do_colchete() {
        let texto = "# Lista\n\n- [ ] comprar pão\n- [x] pagar conta\n  - [X] sub tarefa\n- item comum [ ] não é tarefa\n\n```\n- [ ] dentro de código\n```\n";
        let p = md(texto);
        assert_eq!(p.tarefas.len(), 3);
        assert_eq!(
            p.tarefas.iter().map(|t| t.marcada).collect::<Vec<_>>(),
            [false, true, true]
        );
        for t in &p.tarefas {
            let trecho = &texto[t.byte..t.byte + 3];
            assert!(["[ ]", "[x]", "[X]"].contains(&trecho), "{trecho:?}");
        }
        assert!(p.html.contains("aria-checked=\"false\" data-t=\"0\""));
        assert!(p.html.contains("aria-checked=\"true\" data-t=\"1\""));
        so_marcacao_permitida(&p.html);
    }

    #[test]
    fn alternar_troca_so_o_par_de_colchetes_daquela_linha() {
        let texto = "- [ ] a\n- [x] b\n- [ ] c [ ] igual\n";
        let p = md(texto);
        let t0 = &p.tarefas[0];
        let novo = alternar_tarefa(texto, t0.byte).unwrap();
        assert_eq!(novo, "- [x] a\n- [x] b\n- [ ] c [ ] igual\n");
        let t1 = &p.tarefas[1];
        assert_eq!(
            alternar_tarefa(texto, t1.byte).unwrap(),
            "- [ ] a\n- [ ] b\n- [ ] c [ ] igual\n"
        );
        // Só mudou um byte.
        let diferentes = texto
            .bytes()
            .zip(novo.bytes())
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(diferentes, 1);
        assert_eq!(novo.len(), texto.len());
    }

    #[test]
    fn alternar_funciona_com_crlf_acentos_e_numeradas() {
        let texto = "ação ✓\r\n\r\n1. [ ] pão\r\n2. [x] café\r\n";
        let p = md(texto);
        assert_eq!(p.tarefas.len(), 2);
        let novo = alternar_tarefa(texto, p.tarefas[0].byte).unwrap();
        assert_eq!(novo, "ação ✓\r\n\r\n1. [x] pão\r\n2. [x] café\r\n");
        let novo = alternar_tarefa(&novo, p.tarefas[1].byte).unwrap();
        assert_eq!(novo, "ação ✓\r\n\r\n1. [x] pão\r\n2. [ ] café\r\n");
    }

    #[test]
    fn alternar_recusa_quando_o_texto_nao_bate() {
        assert_eq!(alternar_tarefa("- [ ] a", 0), None);
        assert_eq!(alternar_tarefa("- [-] a", 2), None);
        assert_eq!(alternar_tarefa("- [ ] a", 99), None);
        assert_eq!(alternar_tarefa("", 0), None);
        // Meio de um caractere de vários bytes: não entra em pânico.
        assert_eq!(alternar_tarefa("ç[ ]", 1), None);
        assert_eq!(alternar_tarefa("- [ ] a", 3), None);
    }

    // ---------- 6. O resto do Markdown ----------

    #[test]
    fn titulo_lista_codigo_e_citacao() {
        let p = md(
            "# Título\n\n## Sub\n\n- um\n- dois\n\n3. três\n4. quatro\n\n> citação\n\n`código` e **forte** e *itálico* e ~~riscado~~\n\n```rust\nlet a = 1 < 2;\n```\n\n---\n",
        );
        for esperado in [
            "<h1>Título</h1>",
            "<h2>Sub</h2>",
            "<ul>",
            "<ol start=\"3\">",
            "<blockquote>",
            "<code>código</code>",
            "<strong>forte</strong>",
            "<em>itálico</em>",
            "<del>riscado</del>",
            "<code class=\"lang-rust\">let a = 1 &lt; 2;\n</code>",
            "<hr>",
        ] {
            assert!(
                p.html.contains(esperado),
                "faltou {esperado:?} em {}",
                p.html
            );
        }
        so_marcacao_permitida(&p.html);
    }

    #[test]
    fn tabela_com_alinhamento() {
        let p =
            md("| Item | Qtd | Preço |\n|:--|:-:|--:|\n| pão | 2 | 5,00 |\n| café | 1 | 12,50 |\n");
        for esperado in [
            "<div class=\"tabela\"><table>",
            "<thead><tr><th class=\"a-e\">Item</th><th class=\"a-c\">Qtd</th><th class=\"a-d\">Preço</th></tr></thead>",
            "<td class=\"a-e\">pão</td><td class=\"a-c\">2</td><td class=\"a-d\">5,00</td>",
            "</tbody></table></div>",
        ] {
            assert!(
                p.html.contains(esperado),
                "faltou {esperado:?} em {}",
                p.html
            );
        }
        so_marcacao_permitida(&p.html);
    }

    #[test]
    fn notas_de_rodape_vao_para_o_fim_em_ordem() {
        let p = md("Primeira[^b] e segunda[^a].\n\n[^a]: Texto A\n[^b]: Texto B\n");
        assert!(
            p.html
                .contains("Primeira<sup class=\"nota-rodape\">1</sup>"),
            "{}",
            p.html
        );
        assert!(p.html.contains("segunda<sup class=\"nota-rodape\">2</sup>"));
        let b = p.html.find("Texto B").unwrap();
        let a = p.html.find("Texto A").unwrap();
        assert!(b < a, "ordem de referência, não de definição: {}", p.html);
        assert!(p.html.find("rodape-notas").unwrap() < b);
        assert!(p.html.contains("<li value=\"1\">"));
        so_marcacao_permitida(&p.html);
    }

    #[test]
    fn propriedades_do_inicio_ficam_recolhidas() {
        let p = md("---\ntags: [a, b]\ntitulo: <script>x</script>\n---\n\nCorpo\n");
        assert!(
            p.html.contains("<details class=\"propriedades\">"),
            "{}",
            p.html
        );
        assert!(p.html.contains("tags: [a, b]"));
        assert!(!p.html.contains("<script"));
        assert!(p.html.contains("<p>Corpo</p>"));
        so_marcacao_permitida(&p.html);
    }

    #[test]
    fn quebra_de_linha_dura_e_vazio() {
        assert!(md("a  \nb").html.contains("a<br>\nb"));
        assert_eq!(md("").html, "");
        assert_eq!(md("   \n\n").html, "");
    }

    #[test]
    fn nota_com_acentos_e_emoji_passa_inteira() {
        let p = md("ação, pão e açúcar 😀\n");
        assert!(p.html.contains("ação, pão e açúcar 😀"));
    }

    #[test]
    fn entrada_hostil_nunca_quebra_a_marcacao() {
        let hostil = [
            "[[".to_string(),
            "]]".to_string(),
            "[[a|b|c]]".to_string(),
            "![[".to_string(),
            "[^".to_string(),
            "[^x]: ".to_string(),
            "- [".to_string(),
            "- [ ]".to_string(),
            "|a|\n|-|\n|<|".to_string(),
            "<".repeat(500),
            "[a](".repeat(200),
            "> ".repeat(300) + "x",
            "\u{0}\u{1}[a](java\u{0}script:x)".to_string(),
        ];
        for h in hostil {
            let p = md(&h);
            so_marcacao_permitida(&p.html);
            assert!(!p.html.contains("<script"));
        }
    }

    #[test]
    fn decodificar_percentual_basico() {
        assert_eq!(decodificar_percentual("a%20b"), "a b");
        assert_eq!(decodificar_percentual("%C3%A7"), "ç");
        assert_eq!(decodificar_percentual("100%"), "100%");
        assert_eq!(decodificar_percentual("%zz"), "%zz");
        assert_eq!(decodificar_percentual("%"), "%");
    }
}
