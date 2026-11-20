//! H2b: quadro dentro de quadro. Um cartão `file` que aponta para outro `.canvas` mostra uma
//! miniatura desse quadro: os cartões dele como retângulos com título, sem interação. Aqui fica
//! só a lógica, sem tela: resumir o quadro menor e ajustar a caixa dele ao retângulo do cartão.
//!
//! Guarda contra ciclo (A contém B contém A): a miniatura vai só até a profundidade 1. Dentro
//! dela, um cartão de quadro é só um retângulo com o nome (nunca se abre outra miniatura), e um
//! cartão de quadro que aponta para o próprio quadro aberto é marcado `cycle` para a tela
//! escrever "quadro já aberto acima". O mesmo vale para um cartão do quadro aberto que aponta
//! para ele mesmo.

use crate::canvas::{Canvas, Node, NodeKind, Rect};

/// Um cartão do quadro menor.
#[derive(Clone, Debug, PartialEq)]
pub struct MiniItem {
    /// Posição e tamanho no quadro menor.
    pub rect: Rect,
    pub title: String,
    pub color: Option<String>,
    pub kind: NodeKind,
    /// O cartão é um quadro que já está aberto (ou que é o próprio quadro menor).
    pub cycle: bool,
}

/// O quadro menor pronto para desenhar.
#[derive(Clone, Debug, PartialEq)]
pub struct Mini {
    /// Grupos primeiro, depois os demais, como no quadro de verdade.
    pub items: Vec<MiniItem>,
    /// As ligações, de centro a centro.
    pub links: Vec<((f64, f64), (f64, f64))>,
    /// A caixa que envolve tudo.
    pub bounds: Rect,
}

/// O texto curto que identifica um cartão na miniatura.
fn title_of(node: &Node) -> String {
    let text = match node.node_kind() {
        NodeKind::File => node.file_title(),
        NodeKind::Text => node
            .text
            .as_deref()
            .unwrap_or("")
            .lines()
            .map(|line| line.trim_start_matches(['#', '-', '*', '>', ' ']).trim())
            .find(|line| !line.is_empty())
            .unwrap_or("")
            .to_owned(),
        NodeKind::Link => node.url.clone().unwrap_or_default(),
        NodeKind::Group => node.label.clone().unwrap_or_default(),
        NodeKind::Other => node.kind.clone(),
    };
    text.chars().take(48).collect()
}

/// Resume o quadro menor. `open_key` é o caminho (em forma de chave) do quadro que está aberto
/// agora; um cartão de quadro que aponta para ele sai marcado `cycle`. `None` se o quadro menor
/// não tem nenhum cartão.
pub fn summarize(canvas: &Canvas, open_key: &str) -> Option<Mini> {
    let bounds = canvas.bounds()?;
    let mut items = Vec::new();
    for groups_first in [true, false] {
        for node in &canvas.nodes {
            if (node.node_kind() == NodeKind::Group) != groups_first {
                continue;
            }
            let cycle = node.is_board_card()
                && !open_key.is_empty()
                && node
                    .file
                    .as_deref()
                    .is_some_and(|file| crate::canvas_index::key(file) == open_key);
            items.push(MiniItem {
                rect: node.rect(),
                title: title_of(node),
                color: node.color.clone(),
                kind: node.node_kind(),
                cycle,
            });
        }
    }
    let center = |id: &str| {
        canvas.node(id).map(|node| {
            let rect = node.rect();
            (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
        })
    };
    let links = canvas
        .edges
        .iter()
        .filter_map(|edge| Some((center(&edge.from_node)?, center(&edge.to_node)?)))
        .collect();
    Some(Mini {
        items,
        links,
        bounds,
    })
}

/// Como levar a caixa `bounds` do quadro menor para uma área de `width` x `height`: a escala (sem
/// passar de 1) e o deslocamento que centraliza. Um ponto `p` do quadro menor cai em
/// `p * scale + offset`.
pub fn fit(bounds: &Rect, width: f64, height: f64) -> (f64, (f64, f64)) {
    let scale = (width / bounds.w.max(1.0))
        .min(height / bounds.h.max(1.0))
        .clamp(0.0, 1.0);
    let offset = (
        (width - bounds.w * scale) / 2.0 - bounds.x * scale,
        (height - bounds.h * scale) / 2.0 - bounds.y * scale,
    );
    (scale, offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MENOR: &str = r##"{"nodes":[
        {"id":"g","type":"group","x":-20,"y":-20,"width":600,"height":300,"label":"Fase"},
        {"id":"t","type":"text","x":0,"y":0,"width":200,"height":80,"text":"\n# Ideia boa\nresto"},
        {"id":"n","type":"file","x":300,"y":0,"width":200,"height":100,"file":"Pasta/Alpha.md","color":"4"},
        {"id":"b","type":"file","x":300,"y":150,"width":200,"height":100,"file":"./Mapa.canvas"},
        {"id":"o","type":"file","x":0,"y":150,"width":200,"height":100,"file":"Outro.canvas"}],
        "edges":[{"id":"e","fromNode":"t","toNode":"n"},{"id":"x","fromNode":"t","toNode":"nada"}]}"##;

    #[test]
    fn resume_titulos_cores_e_ligacoes() {
        let canvas = Canvas::parse(MENOR).unwrap();
        let mini = summarize(&canvas, "Mapa.canvas").unwrap();
        let titles: Vec<&str> = mini.items.iter().map(|item| item.title.as_str()).collect();
        // O grupo vem primeiro; o cartão de texto usa a primeira linha sem os marcadores.
        assert_eq!(titles, ["Fase", "Ideia boa", "Alpha", "Mapa", "Outro"]);
        assert_eq!(mini.items[2].color.as_deref(), Some("4"));
        assert_eq!(mini.links, [((100.0, 40.0), (400.0, 50.0))]);
        assert_eq!(
            mini.bounds,
            Rect {
                x: -20.0,
                y: -20.0,
                w: 600.0,
                h: 300.0
            }
        );
    }

    #[test]
    fn so_o_cartao_que_aponta_para_o_quadro_aberto_e_ciclo() {
        let canvas = Canvas::parse(MENOR).unwrap();
        let mini = summarize(&canvas, "Mapa.canvas").unwrap();
        let cycles: Vec<bool> = mini.items.iter().map(|item| item.cycle).collect();
        assert_eq!(cycles, [false, false, false, true, false]);
        let other = summarize(&canvas, "Nenhum.canvas").unwrap();
        assert!(other.items.iter().all(|item| !item.cycle));
        let blank = summarize(&canvas, "").unwrap();
        assert!(blank.items.iter().all(|item| !item.cycle));
    }

    #[test]
    fn quadro_vazio_nao_tem_miniatura() {
        assert!(summarize(&Canvas::default(), "A.canvas").is_none());
    }

    #[test]
    fn ajusta_a_caixa_na_area_sem_ampliar_e_centraliza() {
        let bounds = Rect {
            x: 100.0,
            y: 100.0,
            w: 800.0,
            h: 400.0,
        };
        let (scale, offset) = fit(&bounds, 400.0, 400.0);
        assert_eq!(scale, 0.5);
        // O canto de cima à esquerda cai a 100 de cima (centralizado na vertical) e colado à esquerda.
        assert_eq!(
            (100.0 * scale + offset.0, 100.0 * scale + offset.1),
            (0.0, 100.0)
        );
        // Um quadro menor que a área não é ampliado.
        let small = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        };
        let (scale, offset) = fit(&small, 400.0, 400.0);
        assert_eq!(scale, 1.0);
        assert_eq!(offset, (150.0, 175.0));
    }
}
