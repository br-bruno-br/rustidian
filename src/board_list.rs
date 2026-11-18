//! H2b: a visão "Lista" de um quadro: o quadro lido como uma nota. Não é arquivo: é gerada na
//! hora a partir do quadro (e do começo de cada nota), então nunca diverge dele. Aqui fica só a
//! lógica, sem tela.
//!
//! - Grupos viram títulos, aninhados pela geometria (um grupo dentro de outro é um título
//!   dentro do outro).
//! - Cartões de nota viram itens com o nome e o trecho inicial da nota; cartões de texto trazem
//!   o texto; links trazem o endereço.
//! - Ordem de leitura: de cima para baixo e, na mesma fileira, da esquerda para a direita; vale
//!   dentro de cada grupo e entre os grupos.
//! - As ligações viram linhas "A → B (rótulo)" no fim.

use crate::canvas::{self, Canvas, Node, NodeKind, Rect};
use std::collections::HashMap;

/// Quantos caracteres do começo da nota entram no trecho.
const SNIPPET_CHARS: usize = 200;

#[derive(Clone, Debug, PartialEq)]
pub enum ItemKind {
    /// O título de um grupo (`level` 1 é o grupo de fora).
    Heading {
        level: u8,
    },
    /// Um cartão de nota `.md`; `missing` quando a nota não existe mais.
    Note {
        path: String,
        snippet: String,
        missing: bool,
    },
    /// Um cartão de outro arquivo do cofre (imagem, PDF...).
    File {
        path: String,
    },
    Text {
        text: String,
    },
    Link {
        url: String,
    },
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub node_id: String,
    /// Quantos grupos há em volta (0 fora de qualquer grupo).
    pub depth: usize,
    pub title: String,
    pub kind: ItemKind,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Relation {
    pub from: String,
    pub to: String,
    /// "A → B (rótulo)".
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BoardList {
    pub items: Vec<Item>,
    pub relations: Vec<Relation>,
}

/// A ordem de leitura de uns retângulos: fileiras de cima para baixo (um retângulo entra na
/// fileira do mais alto enquanto o topo dele estiver na metade de cima daquele) e, em cada
/// fileira, da esquerda para a direita. Devolve os índices.
pub fn reading_order(rects: &[Rect]) -> Vec<usize> {
    let mut by_top: Vec<usize> = (0..rects.len()).collect();
    by_top.sort_by(|&a, &b| {
        rects[a]
            .y
            .total_cmp(&rects[b].y)
            .then(rects[a].x.total_cmp(&rects[b].x))
    });
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut anchor = (0.0_f64, 0.0_f64);
    for index in by_top {
        let rect = rects[index];
        match rows.last_mut() {
            Some(row) if rect.y < anchor.0 + anchor.1 * 0.5 => row.push(index),
            _ => {
                anchor = (rect.y, rect.h);
                rows.push(vec![index]);
            }
        }
    }
    rows.into_iter()
        .flat_map(|mut row| {
            row.sort_by(|&a, &b| rects[a].x.total_cmp(&rects[b].x));
            row
        })
        .collect()
}

fn area(rect: &Rect) -> f64 {
    rect.w * rect.h
}

fn first_line(text: &str, most: usize) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    let line = line.trim_start_matches(['#', '>', '-', '*', ' ']);
    let mut shown: String = line.chars().take(most).collect();
    if line.chars().count() > most {
        shown.push('…');
    }
    shown
}

/// O trecho do começo de uma nota: as primeiras linhas que não são título, juntas numa só; se
/// só há títulos, o primeiro.
pub fn snippet(note_text: &str) -> String {
    let body = canvas::card_body(note_text);
    let mut parts: Vec<&str> = Vec::new();
    for line in body.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if line.starts_with('#') {
            continue;
        }
        parts.push(line.trim_start_matches(['>', '-', '*', ' ']));
        if parts.len() >= 3 {
            break;
        }
    }
    let joined = if parts.is_empty() {
        first_line(body, SNIPPET_CHARS)
    } else {
        parts.join(" ")
    };
    let joined = plain_links(&joined);
    if joined.chars().count() > SNIPPET_CHARS {
        let cut: String = joined.chars().take(SNIPPET_CHARS).collect();
        format!("{}…", cut.trim_end())
    } else {
        joined
    }
}

/// `[[Nota]]` e `![[Nota|apelido]]` aparecem só como o nome (ou o apelido), sem os colchetes.
fn plain_links(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        let Some(end) = rest[start + 2..].find("]]") else {
            break;
        };
        let inner = &rest[start + 2..start + 2 + end];
        out.push_str(rest[..start].trim_end_matches('!'));
        let shown = inner.rsplit_once('|').map_or(inner, |(_, alias)| alias);
        out.push_str(shown.split('#').next().unwrap_or(shown));
        rest = &rest[start + 2 + end + 2..];
    }
    out.push_str(rest);
    out
}

fn name_of(node: &Node) -> String {
    match node.node_kind() {
        NodeKind::File => {
            if node.is_note_card() {
                node.file_title()
            } else {
                node.file.clone().unwrap_or_default()
            }
        }
        NodeKind::Text => {
            let line = first_line(node.text.as_deref().unwrap_or(""), 40);
            if line.is_empty() {
                "(cartão vazio)".into()
            } else {
                line
            }
        }
        NodeKind::Link => node.url.clone().unwrap_or_default(),
        NodeKind::Group => match node.label.as_deref().map(str::trim) {
            Some(label) if !label.is_empty() => label.to_owned(),
            _ => "Grupo".into(),
        },
        NodeKind::Other => "Cartão".into(),
    }
}

/// Monta a lista. `note_head(caminho)` devolve o começo do texto da nota (ou `None` se ela não
/// existe); só o começo importa, o trecho sai dele.
pub fn build(canvas: &Canvas, note_head: impl Fn(&str) -> Option<String>) -> BoardList {
    let nodes = &canvas.nodes;
    let rects: Vec<Rect> = nodes.iter().map(Node::rect).collect();
    let is_group = |index: usize| nodes[index].node_kind() == NodeKind::Group;
    // O pai de cada nó: o menor grupo que o contém por inteiro (e é maior que ele, para dois
    // grupos iguais não virarem pai um do outro).
    let parent: Vec<Option<usize>> = (0..nodes.len())
        .map(|index| {
            (0..nodes.len())
                .filter(|&group| group != index && is_group(group))
                .filter(|&group| rects[group].contains_rect(&rects[index]))
                .filter(|&group| !is_group(index) || area(&rects[group]) > area(&rects[index]))
                .min_by(|&a, &b| area(&rects[a]).total_cmp(&area(&rects[b])))
        })
        .collect();
    let mut children: HashMap<Option<usize>, Vec<usize>> = HashMap::new();
    for (index, parent) in parent.iter().enumerate() {
        children.entry(*parent).or_default().push(index);
    }

    let mut list = BoardList::default();
    // Pilha de (pai, posição na ordem de leitura de seus filhos, depth).
    fn emit(parent: Option<usize>, depth: usize, context: &Context, list: &mut BoardList) {
        let Some(kids) = context.children.get(&parent) else {
            return;
        };
        let rects: Vec<Rect> = kids.iter().map(|&kid| context.rects[kid]).collect();
        for position in reading_order(&rects) {
            let index = kids[position];
            let node = &context.nodes[index];
            match node.node_kind() {
                NodeKind::Group => {
                    list.items.push(Item {
                        node_id: node.id.clone(),
                        depth,
                        title: name_of(node),
                        kind: ItemKind::Heading {
                            level: (depth + 1).min(6) as u8,
                        },
                    });
                    emit(Some(index), depth + 1, context, list);
                }
                kind => {
                    let kind = match kind {
                        NodeKind::File if node.is_note_card() => {
                            let path = node.file.clone().unwrap_or_default().replace('\\', "/");
                            let head = (context.note_head)(&path);
                            ItemKind::Note {
                                snippet: head.as_deref().map(snippet).unwrap_or_default(),
                                missing: head.is_none(),
                                path,
                            }
                        }
                        NodeKind::File => ItemKind::File {
                            path: node.file.clone().unwrap_or_default().replace('\\', "/"),
                        },
                        NodeKind::Text => ItemKind::Text {
                            text: node.text.clone().unwrap_or_default(),
                        },
                        NodeKind::Link => ItemKind::Link {
                            url: node.url.clone().unwrap_or_default(),
                        },
                        _ => ItemKind::Other,
                    };
                    list.items.push(Item {
                        node_id: node.id.clone(),
                        depth,
                        title: name_of(node),
                        kind,
                    });
                }
            }
        }
    }
    struct Context<'a> {
        nodes: &'a [Node],
        rects: &'a [Rect],
        children: &'a HashMap<Option<usize>, Vec<usize>>,
        note_head: &'a dyn Fn(&str) -> Option<String>,
    }
    emit(
        None,
        0,
        &Context {
            nodes,
            rects: &rects,
            children: &children,
            note_head: &note_head,
        },
        &mut list,
    );

    // As ligações, na ordem de leitura de quem sai.
    let position_of = |id: &str| nodes.iter().position(|node| node.id == id);
    let mut relations: Vec<(usize, Relation)> = Vec::new();
    for edge in &canvas.edges {
        let (Some(from), Some(to)) = (position_of(&edge.from_node), position_of(&edge.to_node))
        else {
            continue;
        };
        let (a, b) = (name_of(&nodes[from]), name_of(&nodes[to]));
        let arrow_to = edge.to_arrow();
        let arrow_from = edge.from_arrow();
        let (left, right, link, first, last) = match (arrow_from, arrow_to) {
            (true, true) => (a, b, "↔", edge.from_node.clone(), edge.to_node.clone()),
            (true, false) => (b, a, "→", edge.to_node.clone(), edge.from_node.clone()),
            (false, true) => (a, b, "→", edge.from_node.clone(), edge.to_node.clone()),
            (false, false) => (a, b, "—", edge.from_node.clone(), edge.to_node.clone()),
        };
        let label = edge
            .label
            .as_deref()
            .map(str::trim)
            .filter(|label| !label.is_empty());
        let text = match label {
            Some(label) => format!("{left} {link} {right} ({label})"),
            None => format!("{left} {link} {right}"),
        };
        relations.push((
            from,
            Relation {
                from: first,
                to: last,
                text,
            },
        ));
    }
    let order = reading_order(
        &relations
            .iter()
            .map(|(from, _)| rects[*from])
            .collect::<Vec<_>>(),
    );
    list.relations = order
        .into_iter()
        .map(|index| relations[index].1.clone())
        .collect();
    list
}

/// A lista como texto Markdown, para a cópia "Exportar quadro como nota". `title` é o nome do
/// quadro. Notas viram `[[links]]`, então a cópia continua ligada às notas de verdade (só o
/// trecho e os textos dos cartões são cópia, e o aviso do topo diz isso).
pub fn to_markdown(title: &str, list: &BoardList) -> String {
    let mut out = format!("# {title}\n\n");
    out.push_str(&format!(
        "*Cópia da visão em lista do quadro \"{title}\". Não acompanha o quadro: o que mudar lá não muda aqui.*\n\n"
    ));
    for item in &list.items {
        match &item.kind {
            ItemKind::Heading { level } => {
                out.push_str(&format!(
                    "{} {}\n\n",
                    "#".repeat((*level as usize + 1).min(6)),
                    item.title
                ));
            }
            ItemKind::Note {
                path,
                snippet,
                missing,
            } => {
                let target = path.strip_suffix(".md").unwrap_or(path);
                if *missing {
                    out.push_str(&format!("- {} (nota não encontrada)\n", item.title));
                } else if target == item.title {
                    out.push_str(&format!("- [[{target}]]\n"));
                } else {
                    out.push_str(&format!("- [[{target}|{}]]\n", item.title));
                }
                if !snippet.is_empty() {
                    out.push_str(&format!("  > {snippet}\n"));
                }
            }
            ItemKind::File { path } => out.push_str(&format!("- ![[{path}]]\n")),
            ItemKind::Text { text } => {
                let text = text.trim();
                if text.is_empty() {
                    continue;
                }
                if text.contains('\n') {
                    out.push('\n');
                    for line in text.lines() {
                        out.push_str(&format!("> {line}\n"));
                    }
                    out.push('\n');
                } else {
                    out.push_str(&format!("- {text}\n"));
                }
            }
            ItemKind::Link { url } => out.push_str(&format!("- <{url}>\n")),
            ItemKind::Other => out.push_str(&format!("- ({})\n", item.title)),
        }
    }
    if !list.relations.is_empty() {
        out.push_str("\n## Ligações\n\n");
        for relation in &list.relations {
            out.push_str(&format!("- {}\n", relation.text));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(json: &str) -> Canvas {
        Canvas::parse(json).expect("quadro de teste")
    }

    fn head(path: &str) -> Option<String> {
        match path {
            "Home.md" => {
                Some("---\ntags: x\n---\n# Home\n\nPrimeira linha da home.\nSegunda.\n".into())
            }
            "Alpha.md" => Some("# Só título\n".into()),
            _ => None,
        }
    }

    const QUADRO: &str = r##"{
	"nodes":[
		{"id":"fora","type":"group","x":-50,"y":-50,"width":1000,"height":700,"label":"Fase 1"},
		{"id":"dentro","type":"group","x":0,"y":300,"width":500,"height":300,"label":"Detalhes"},
		{"id":"d1","type":"text","x":20,"y":340,"width":200,"height":80,"text":"# Uma ideia\nCom corpo"},
		{"id":"home","type":"file","x":400,"y":10,"width":300,"height":200,"file":"Home.md"},
		{"id":"alpha","type":"file","x":0,"y":0,"width":300,"height":210,"file":"Alpha.md"},
		{"id":"sumiu","type":"file","x":0,"y":900,"width":300,"height":200,"file":"Fantasma.md"},
		{"id":"url","type":"link","x":1100,"y":0,"width":200,"height":100,"url":"https://exemplo.com"},
		{"id":"img","type":"file","x":1100,"y":200,"width":200,"height":100,"file":"fotos/a.png"}
	],
	"edges":[
		{"id":"e1","fromNode":"alpha","toNode":"home","label":"origem"},
		{"id":"e2","fromNode":"d1","toNode":"alpha","fromEnd":"arrow","toEnd":"none"},
		{"id":"e3","fromNode":"home","toNode":"url","toEnd":"none"},
		{"id":"e4","fromNode":"home","toNode":"inexistente"}
	]
}"##;

    #[test]
    fn a_ordem_de_leitura_e_de_cima_para_baixo_e_da_esquerda_para_a_direita() {
        let rect = |x: f64, y: f64| Rect {
            x,
            y,
            w: 100.0,
            h: 100.0,
        };
        // Mesma fileira com alturas um pouco diferentes: vale a esquerda primeiro.
        let rects = [
            rect(300.0, 0.0),
            rect(0.0, 30.0),
            rect(150.0, 10.0),
            rect(0.0, 400.0),
            rect(10.0, 200.0),
        ];
        assert_eq!(reading_order(&rects), [1, 2, 0, 4, 3]);
        assert!(reading_order(&[]).is_empty());
    }

    #[test]
    fn grupos_viram_titulos_aninhados_e_os_itens_seguem_a_ordem_de_leitura() {
        let list = build(&board(QUADRO), head);
        let outline: Vec<(usize, String)> = list
            .items
            .iter()
            .map(|item| (item.depth, item.title.clone()))
            .collect();
        assert_eq!(
            outline,
            [
                (0, "Fase 1".to_owned()),
                // Dentro do grupo de fora: a fileira do alto (Alpha, Home) e depois o grupo de baixo.
                (1, "Alpha".to_owned()),
                (1, "Home".to_owned()),
                (1, "Detalhes".to_owned()),
                (2, "Uma ideia".to_owned()),
                // Fora de qualquer grupo: o link e a imagem (à direita, fileira do alto) e a nota sumida.
                (0, "https://exemplo.com".to_owned()),
                (0, "fotos/a.png".to_owned()),
                (0, "Fantasma".to_owned()),
            ]
        );
        assert_eq!(list.items[0].kind, ItemKind::Heading { level: 1 });
        assert_eq!(list.items[3].kind, ItemKind::Heading { level: 2 });
    }

    #[test]
    fn nota_traz_o_trecho_sem_propriedades_e_sem_titulo_e_nota_sumida_avisa() {
        let list = build(&board(QUADRO), head);
        let by_id = |id: &str| list.items.iter().find(|item| item.node_id == id).unwrap();
        assert_eq!(
            by_id("home").kind,
            ItemKind::Note {
                path: "Home.md".into(),
                snippet: "Primeira linha da home. Segunda.".into(),
                missing: false
            }
        );
        // Só título: o trecho cai para o título.
        match &by_id("alpha").kind {
            ItemKind::Note { snippet, .. } => assert_eq!(snippet, "Só título"),
            other => panic!("{other:?}"),
        }
        match &by_id("sumiu").kind {
            ItemKind::Note {
                missing, snippet, ..
            } => {
                assert!(*missing);
                assert!(snippet.is_empty());
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            by_id("d1").kind,
            ItemKind::Text {
                text: "# Uma ideia\nCom corpo".into()
            }
        );
        assert_eq!(
            by_id("url").kind,
            ItemKind::Link {
                url: "https://exemplo.com".into()
            }
        );
        assert_eq!(
            by_id("img").kind,
            ItemKind::File {
                path: "fotos/a.png".into()
            }
        );
    }

    #[test]
    fn o_trecho_mostra_os_links_so_com_o_nome() {
        assert_eq!(
            snippet("Veja [[Beta]], ![[Alpha|apelido]] e [[Pasta/Gama#Seção]]. Fim [[solto"),
            "Veja Beta, apelido e Pasta/Gama. Fim [[solto"
        );
    }

    #[test]
    fn ligacoes_viram_a_seta_b_com_rotulo_e_respeitam_o_sentido() {
        let list = build(&board(QUADRO), head);
        let texts: Vec<&str> = list
            .relations
            .iter()
            .map(|relation| relation.text.as_str())
            .collect();
        // Na ordem de leitura de quem sai: Alpha (e1), Home (e3) e depois a ideia (e2). A e2 tem
        // a seta no começo da aresta, então a leitura aponta para a ideia.
        assert_eq!(
            texts,
            [
                "Alpha → Home (origem)",
                "Home — https://exemplo.com",
                "Alpha → Uma ideia",
            ]
        );
        // A aresta para um nó que não existe é ignorada.
        assert_eq!(list.relations.len(), 3);
    }

    #[test]
    fn grupos_iguais_nao_viram_pai_um_do_outro_e_quadro_vazio_da_lista_vazia() {
        let list = build(
            &board(
                r#"{"nodes":[
                {"id":"a","type":"group","x":0,"y":0,"width":100,"height":100,"label":"A"},
                {"id":"b","type":"group","x":0,"y":0,"width":100,"height":100,"label":"B"}],"edges":[]}"#,
            ),
            head,
        );
        assert_eq!(list.items.len(), 2);
        assert!(list.items.iter().all(|item| item.depth == 0));
        assert_eq!(build(&Canvas::default(), head), BoardList::default());
    }

    #[test]
    fn o_markdown_e_uma_nota_comum_com_wikilinks_e_aviso_de_copia() {
        let list = build(&board(QUADRO), head);
        let text = to_markdown("Plano", &list);
        assert!(
            text.starts_with("# Plano\n\n*Cópia da visão em lista"),
            "{text}"
        );
        assert!(text.contains("## Fase 1\n"), "{text}");
        assert!(text.contains("### Detalhes\n"), "{text}");
        assert!(
            text.contains("- [[Home]]\n  > Primeira linha da home. Segunda.\n"),
            "{text}"
        );
        assert!(
            text.contains("- Fantasma (nota não encontrada)\n"),
            "{text}"
        );
        assert!(text.contains("> # Uma ideia\n> Com corpo\n"), "{text}");
        assert!(text.contains("- <https://exemplo.com>\n"), "{text}");
        assert!(text.contains("- ![[fotos/a.png]]\n"), "{text}");
        assert!(
            text.contains("\n## Ligações\n\n- Alpha → Home (origem)\n"),
            "{text}"
        );
    }
}
