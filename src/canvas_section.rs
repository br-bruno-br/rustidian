//! H2a: cartão de seção. Um cartão de nota com `subpath` (`#Cabeçalho`) mostra e edita só um
//! trecho da nota: do cabeçalho até o próximo de nível igual ou maior (número menor ou igual).
//! Aqui fica só a lógica, sem tela: achar os cabeçalhos, achar a seção e trocar o trecho no texto
//! da nota inteira (o cartão escreve no mesmo `Document` da nota, não numa cópia).
//!
//! Regras:
//! - cabeçalho é a linha ATX (`#` a `######` e um espaço); linhas dentro de bloco de código
//!   (``` ou ~~~) e o bloco de propriedades do topo não contam;
//! - `#A#B` é a seção B dentro da seção A (como no Obsidian);
//! - `#^bloco` (referência de bloco) não é uma seção: o cartão segue mostrando a nota inteira;
//! - o nome do cabeçalho casa sem diferença de maiúsculas nem de espaços repetidos.

use std::ops::Range;

/// Um cabeçalho da nota.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heading {
    /// 1 a 6.
    pub level: u8,
    pub title: String,
    /// Onde a linha do cabeçalho começa (bytes).
    pub start: usize,
}

/// Onde a seção está na nota.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Section {
    /// O trecho, em bytes, do começo da linha do cabeçalho até o próximo cabeçalho de nível
    /// igual ou maior (ou o fim da nota).
    Found(Range<usize>),
    /// O cabeçalho não existe (foi renomeado ou apagado).
    Missing,
    /// O `subpath` não é de cabeçalho (vazio ou `#^bloco`): vale a nota inteira.
    NotHeading,
}

/// Quantos bytes o bloco de propriedades do topo ocupa (0 se não há).
fn properties_len(text: &str) -> usize {
    let rest = match text
        .strip_prefix("---\r\n")
        .or_else(|| text.strip_prefix("---\n"))
    {
        Some(rest) => rest,
        None => return 0,
    };
    let opening = text.len() - rest.len();
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']).trim_end() == "---" {
            return opening + offset + line.len();
        }
        offset += line.len();
    }
    0
}

/// Os cabeçalhos da nota, na ordem em que aparecem.
pub fn headings(text: &str) -> Vec<Heading> {
    let mut found = Vec::new();
    let mut fence: Option<char> = None;
    let mut offset = properties_len(text);
    for line in text[offset..].split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let content = line.trim_end_matches(['\r', '\n']);
        let indent = content.len() - content.trim_start_matches(' ').len();
        let trimmed = &content[indent..];
        if indent <= 3
            && let Some(marker) = ['`', '~']
                .into_iter()
                .find(|marker| trimmed.starts_with(&marker.to_string().repeat(3)))
        {
            match fence {
                None => fence = Some(marker),
                Some(open) if open == marker => fence = None,
                Some(_) => {}
            }
            continue;
        }
        if fence.is_some() || indent > 3 {
            continue;
        }
        let level = trimmed.chars().take_while(|c| *c == '#').count();
        if !(1..=6).contains(&level) {
            continue;
        }
        let after = &trimmed[level..];
        if !(after.is_empty() || after.starts_with([' ', '\t'])) {
            continue;
        }
        let title = after
            .trim()
            .trim_end_matches('#')
            .trim_end()
            .trim()
            .to_owned();
        found.push(Heading {
            level: level as u8,
            title,
            start,
        });
    }
    found
}

/// O nome do cabeçalho para comparar: minúsculas e espaços repetidos viram um.
fn norm(title: &str) -> String {
    title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Os pedaços de um `subpath` de cabeçalho (`#A#B` dá `["A", "B"]`), ou `None` se não é de
/// cabeçalho.
pub fn parse_subpath(subpath: &str) -> Option<Vec<String>> {
    let rest = subpath.trim().strip_prefix('#')?;
    let parts: Vec<String> = rest.split('#').map(|part| part.trim().to_owned()).collect();
    if parts
        .iter()
        .any(|part| part.is_empty() || part.starts_with('^'))
    {
        return None;
    }
    Some(parts)
}

/// O `subpath` de um cabeçalho.
pub fn subpath_for(title: &str) -> String {
    format!("#{}", title.trim())
}

/// O `subpath` do cabeçalho `index` da lista: só o nome, se for o primeiro com esse nome; senão o
/// caminho pelos cabeçalhos maiores que o contêm (`#A#B`), para achar o certo entre repetidos.
pub fn subpath_at(headings: &[Heading], index: usize) -> String {
    let target = &headings[index];
    let name = norm(&target.title);
    let first = headings
        .iter()
        .position(|heading| norm(&heading.title) == name)
        .unwrap_or(index);
    if first == index {
        return subpath_for(&target.title);
    }
    let mut chain = vec![target.title.trim().to_owned()];
    let mut level = target.level;
    for heading in headings[..index].iter().rev() {
        if heading.level < level {
            chain.push(heading.title.trim().to_owned());
            level = heading.level;
        }
    }
    chain.reverse();
    format!("#{}", chain.join("#"))
}

/// Troca o último pedaço do `subpath` (`#A#B` com `C` vira `#A#C`).
pub fn renamed(subpath: &str, title: &str) -> String {
    match parse_subpath(subpath) {
        Some(mut parts) => {
            if let Some(last) = parts.last_mut() {
                *last = title.trim().to_owned();
            }
            format!("#{}", parts.join("#"))
        }
        None => subpath_for(title),
    }
}

/// Acha a seção do `subpath` no texto da nota.
pub fn find_section(text: &str, subpath: &str) -> Section {
    let Some(parts) = parse_subpath(subpath) else {
        return Section::NotHeading;
    };
    let all = headings(text);
    let mut window = 0..text.len();
    let mut min_level = 0u8;
    let mut result = None;
    for part in &parts {
        let wanted = norm(part);
        let Some(index) = all.iter().position(|heading| {
            heading.start >= window.start
                && heading.start < window.end
                && heading.level > min_level
                && norm(&heading.title) == wanted
        }) else {
            return Section::Missing;
        };
        let heading = &all[index];
        let end = all[index + 1..]
            .iter()
            .find(|next| next.level <= heading.level)
            .map_or(text.len(), |next| next.start)
            .min(window.end);
        window = heading.start..end;
        min_level = heading.level;
        result = Some(window.clone());
    }
    result.map_or(Section::NotHeading, Section::Found)
}

/// A nota com o trecho `range` trocado por `new`.
pub fn replace_range(text: &str, range: Range<usize>, new: &str) -> String {
    let mut out = String::with_capacity(text.len() - range.len() + new.len());
    out.push_str(&text[..range.start]);
    out.push_str(new);
    out.push_str(&text[range.end..]);
    out
}

/// O título do cabeçalho se a primeira linha do trecho for um cabeçalho (depois de editar a
/// seção, é o nome novo do `subpath`).
pub fn first_heading_title(section: &str) -> Option<String> {
    headings(section)
        .into_iter()
        .next()
        .filter(|heading| heading.start == 0)
        .map(|heading| heading.title)
        .filter(|title| !title.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTA: &str = "---\ntitulo: x\n# nao e cabecalho\n---\n# Home\nabertura\n\n## Resumo\nlinha 1\n```\n# dentro do codigo\n```\n### Detalhe\nfino\n## Fim\nfinal\n# Outro\nresto\n";

    fn trecho<'a>(text: &'a str, subpath: &str) -> &'a str {
        match find_section(text, subpath) {
            Section::Found(range) => &text[range],
            other => panic!("esperava seção, veio {other:?}"),
        }
    }

    #[test]
    fn lista_so_os_cabecalhos_de_verdade() {
        let titles: Vec<(u8, String)> = headings(NOTA)
            .into_iter()
            .map(|heading| (heading.level, heading.title))
            .collect();
        assert_eq!(
            titles,
            [
                (1, "Home".to_owned()),
                (2, "Resumo".to_owned()),
                (3, "Detalhe".to_owned()),
                (2, "Fim".to_owned()),
                (1, "Outro".to_owned()),
            ]
        );
        assert_eq!(headings("#semespaco\n####### sete\ntexto # nao\n"), []);
        assert_eq!(headings("## Título ##\n")[0].title, "Título");
    }

    #[test]
    fn a_secao_vai_ate_o_proximo_de_nivel_igual_ou_maior() {
        assert_eq!(
            trecho(NOTA, "#Resumo"),
            "## Resumo\nlinha 1\n```\n# dentro do codigo\n```\n### Detalhe\nfino\n"
        );
        assert_eq!(trecho(NOTA, "#Detalhe"), "### Detalhe\nfino\n");
        assert_eq!(trecho(NOTA, "#Fim"), "## Fim\nfinal\n");
        // O de nível 1 leva os de nível 2 e 3 que vêm depois dele.
        assert!(trecho(NOTA, "#Home").ends_with("final\n"));
        assert_eq!(trecho(NOTA, "#Outro"), "# Outro\nresto\n");
    }

    #[test]
    fn caminho_aninhado_e_nome_sem_diferenca_de_maiusculas() {
        assert_eq!(trecho(NOTA, "#resumo"), trecho(NOTA, "#Resumo"));
        assert_eq!(trecho(NOTA, "#Resumo#Detalhe"), "### Detalhe\nfino\n");
        assert_eq!(trecho(NOTA, "#Home#Fim"), "## Fim\nfinal\n");
        // Detalhe não está dentro de Fim.
        assert_eq!(find_section(NOTA, "#Fim#Detalhe"), Section::Missing);
    }

    #[test]
    fn sumido_e_nao_cabecalho() {
        assert_eq!(find_section(NOTA, "#Inexistente"), Section::Missing);
        assert_eq!(find_section(NOTA, "#^abc123"), Section::NotHeading);
        assert_eq!(find_section(NOTA, ""), Section::NotHeading);
        assert_eq!(find_section(NOTA, "Resumo"), Section::NotHeading);
        // O que está nas propriedades do topo não é cabeçalho.
        assert_eq!(find_section(NOTA, "#nao e cabecalho"), Section::Missing);
    }

    #[test]
    fn trocar_o_trecho_deixa_o_resto_igual() {
        let Section::Found(range) = find_section(NOTA, "#Fim") else {
            panic!("seção")
        };
        let novo = replace_range(NOTA, range, "## Fim\nmudou\n");
        assert!(novo.contains("## Fim\nmudou\n# Outro"));
        assert!(novo.starts_with(&NOTA[..NOTA.find("## Fim").unwrap()]));
        assert!(novo.ends_with("# Outro\nresto\n"));
        // Trocar pelo mesmo texto devolve a nota byte a byte.
        let Section::Found(range) = find_section(NOTA, "#Resumo") else {
            panic!("seção")
        };
        let mesmo = replace_range(NOTA, range.clone(), &NOTA[range]);
        assert_eq!(mesmo, NOTA);
    }

    #[test]
    fn renomear_o_cabecalho_no_cartao_atualiza_o_subpath() {
        assert_eq!(
            first_heading_title("## Novo nome\ncorpo"),
            Some("Novo nome".into())
        );
        assert_eq!(first_heading_title("sem cabeçalho\n## Depois"), None);
        assert_eq!(renamed("#Resumo", "Visão"), "#Visão");
        assert_eq!(renamed("#A#B", "C"), "#A#C");
        assert_eq!(subpath_for(" Fim "), "#Fim");
    }

    #[test]
    fn subpath_do_cabecalho_escolhido_desambigua_nomes_repetidos() {
        let texto = "# A\n## Notas\nx\n# B\n## Notas\ny\n### Fundo\n";
        let lista = headings(texto);
        assert_eq!(subpath_at(&lista, 1), "#Notas");
        assert_eq!(subpath_at(&lista, 2), "#B");
        assert_eq!(subpath_at(&lista, 3), "#B#Notas");
        assert_eq!(
            trecho(texto, &subpath_at(&lista, 3)),
            "## Notas\ny\n### Fundo\n"
        );
        assert_eq!(trecho(texto, &subpath_at(&lista, 1)), "## Notas\nx\n");
    }

    #[test]
    fn fim_de_linha_do_windows_e_cabecalho_no_fim_da_nota() {
        let texto = "# A\r\ncorpo\r\n## B\r\nfim";
        assert_eq!(trecho(texto, "#B"), "## B\r\nfim");
        assert_eq!(headings(texto)[1].title, "B");
    }
}
