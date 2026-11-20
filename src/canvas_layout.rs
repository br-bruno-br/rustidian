//! H3: organizar os cartões do quadro. Aqui fica só a lógica, sem tela: alinhar, distribuir,
//! dar o mesmo espaçamento, arrumar em grade e abrir um mapa mental a partir de um cartão.
//!
//! Cada função mexe só em `x` e `y` (e, no mapa mental, nos lados das setas da árvore) e diz se
//! mudou algo; a tela chama uma por comando e grava uma vez, então cada comando é um passo de
//! desfazer. Mover um grupo leva junto o que está dentro dele (como o arrastar). Os valores
//! saem inteiros, como manda a especificação do JSON Canvas.

use crate::canvas::{Canvas, NodeKind, Rect, Side};
use std::collections::{HashMap, HashSet};

/// Vão padrão entre cartões ao arrumar em grade e no "espaçamento igual", em unidades do quadro.
pub const GAP: f64 = 40.0;
/// Vão entre uma coluna do mapa mental e a seguinte.
pub const TREE_COLUMN_GAP: f64 = 90.0;
/// Vão entre dois ramos do mapa mental, na vertical.
pub const TREE_ROW_GAP: f64 = 28.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    CenterH,
    Right,
    Top,
    MiddleV,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

/// Os cartões pedidos que existem no quadro, sem repetir e na ordem do quadro.
fn existing(canvas: &Canvas, ids: &[String]) -> Vec<(String, Rect)> {
    canvas
        .nodes
        .iter()
        .filter(|node| ids.contains(&node.id))
        .map(|node| (node.id.clone(), node.rect()))
        .collect()
}

/// Aplica os deslocamentos. O que está dentro de um grupo movido e não tem deslocamento próprio
/// acompanha o grupo. Devolve se algum cartão saiu do lugar.
fn apply_deltas(canvas: &mut Canvas, deltas: &[(String, f64, f64)]) -> bool {
    let mut all: HashMap<String, (f64, f64)> = HashMap::new();
    for (id, dx, dy) in deltas {
        all.insert(id.clone(), (*dx, *dy));
    }
    for (id, dx, dy) in deltas {
        if (*dx, *dy) == (0.0, 0.0) {
            continue;
        }
        let Some(node) = canvas.node(id) else {
            continue;
        };
        if node.node_kind() != NodeKind::Group {
            continue;
        }
        for member in canvas.with_group_members(std::slice::from_ref(id)) {
            all.entry(member).or_insert((*dx, *dy));
        }
    }
    let mut changed = false;
    for (id, (dx, dy)) in all {
        if (dx, dy) == (0.0, 0.0) {
            continue;
        }
        if let Some(node) = canvas.node_mut(&id) {
            node.x = (node.x + dx).round();
            node.y = (node.y + dy).round();
            changed = true;
        }
    }
    changed
}

/// Alinha os cartões pela borda ou pelo meio da caixa que envolve todos. Precisa de dois ou
/// mais cartões.
pub fn align(canvas: &mut Canvas, ids: &[String], how: Align) -> bool {
    let items = existing(canvas, ids);
    if items.len() < 2 {
        return false;
    }
    let left = items.iter().map(|(_, r)| r.x).fold(f64::INFINITY, f64::min);
    let top = items.iter().map(|(_, r)| r.y).fold(f64::INFINITY, f64::min);
    let right = items
        .iter()
        .map(|(_, r)| r.x + r.w)
        .fold(f64::NEG_INFINITY, f64::max);
    let bottom = items
        .iter()
        .map(|(_, r)| r.y + r.h)
        .fold(f64::NEG_INFINITY, f64::max);
    let deltas: Vec<(String, f64, f64)> = items
        .iter()
        .map(|(id, r)| {
            let (dx, dy) = match how {
                Align::Left => (left - r.x, 0.0),
                Align::CenterH => ((left + right) / 2.0 - (r.x + r.w / 2.0), 0.0),
                Align::Right => (right - (r.x + r.w), 0.0),
                Align::Top => (0.0, top - r.y),
                Align::MiddleV => (0.0, (top + bottom) / 2.0 - (r.y + r.h / 2.0)),
                Align::Bottom => (0.0, bottom - (r.y + r.h)),
            };
            (id.clone(), dx, dy)
        })
        .collect();
    apply_deltas(canvas, &deltas)
}

fn start_of(rect: &Rect, axis: Axis) -> f64 {
    match axis {
        Axis::Horizontal => rect.x,
        Axis::Vertical => rect.y,
    }
}

fn size_of(rect: &Rect, axis: Axis) -> f64 {
    match axis {
        Axis::Horizontal => rect.w,
        Axis::Vertical => rect.h,
    }
}

fn delta_on(axis: Axis, by: f64) -> (f64, f64) {
    match axis {
        Axis::Horizontal => (by, 0.0),
        Axis::Vertical => (0.0, by),
    }
}

/// Os cartões em fila ao longo do eixo (pelo meio de cada um).
fn sorted_along(canvas: &Canvas, ids: &[String], axis: Axis) -> Vec<(String, Rect)> {
    let mut items = existing(canvas, ids);
    items.sort_by(|a, b| {
        let key = |rect: &Rect| start_of(rect, axis) + size_of(rect, axis) / 2.0;
        key(&a.1).total_cmp(&key(&b.1))
    });
    items
}

/// Distribui no eixo: os dois cartões das pontas ficam onde estão e os vãos entre todos ficam
/// iguais. Precisa de três ou mais cartões.
pub fn distribute(canvas: &mut Canvas, ids: &[String], axis: Axis) -> bool {
    let items = sorted_along(canvas, ids, axis);
    if items.len() < 3 {
        return false;
    }
    let first = start_of(&items[0].1, axis);
    let last = items
        .iter()
        .map(|(_, rect)| start_of(rect, axis) + size_of(rect, axis))
        .fold(f64::NEG_INFINITY, f64::max);
    let total: f64 = items.iter().map(|(_, rect)| size_of(rect, axis)).sum();
    let gap = (last - first - total) / (items.len() - 1) as f64;
    let mut cursor = first;
    let mut deltas = Vec::new();
    for (id, rect) in &items {
        let by = cursor.round() - start_of(rect, axis);
        deltas.push((id.clone(), delta_on(axis, by).0, delta_on(axis, by).1));
        cursor += size_of(rect, axis) + gap;
    }
    apply_deltas(canvas, &deltas)
}

/// Espaçamento igual: o primeiro cartão fica e os outros vêm atrás dele, em fila, com o mesmo
/// vão (`gap`) entre um e o seguinte. O eixo é o do lado mais comprido da caixa que envolve a
/// seleção. Precisa de dois ou mais cartões.
pub fn equal_spacing(canvas: &mut Canvas, ids: &[String], gap: f64) -> bool {
    let items = existing(canvas, ids);
    if items.len() < 2 {
        return false;
    }
    let width = items
        .iter()
        .map(|(_, r)| r.x + r.w)
        .fold(f64::NEG_INFINITY, f64::max)
        - items.iter().map(|(_, r)| r.x).fold(f64::INFINITY, f64::min);
    let height = items
        .iter()
        .map(|(_, r)| r.y + r.h)
        .fold(f64::NEG_INFINITY, f64::max)
        - items.iter().map(|(_, r)| r.y).fold(f64::INFINITY, f64::min);
    let axis = if width >= height {
        Axis::Horizontal
    } else {
        Axis::Vertical
    };
    let items = sorted_along(canvas, ids, axis);
    let mut cursor = start_of(&items[0].1, axis);
    let mut deltas = Vec::new();
    for (id, rect) in &items {
        let by = cursor.round() - start_of(rect, axis);
        deltas.push((id.clone(), delta_on(axis, by).0, delta_on(axis, by).1));
        cursor += size_of(rect, axis) + gap;
    }
    apply_deltas(canvas, &deltas)
}

/// Arruma os cartões em grade: o canto de cima à esquerda da seleção é o ponto de partida, as
/// colunas são `ceil(raiz(n))`, a ordem é a de leitura (fileiras de cima para baixo e, dentro de
/// cada uma, da esquerda para a direita) e cada coluna e fileira tem o tamanho do maior cartão
/// dela, com `gap` entre as células. Ninguém encosta em ninguém.
pub fn arrange_grid(canvas: &mut Canvas, ids: &[String], gap: f64) -> bool {
    let mut items = existing(canvas, ids);
    if items.len() < 2 {
        return false;
    }
    let left = items.iter().map(|(_, r)| r.x).fold(f64::INFINITY, f64::min);
    let top = items.iter().map(|(_, r)| r.y).fold(f64::INFINITY, f64::min);
    let columns = (items.len() as f64).sqrt().ceil() as usize;
    items.sort_by(|a, b| {
        (a.1.y + a.1.h / 2.0)
            .total_cmp(&(b.1.y + b.1.h / 2.0))
            .then(a.1.x.total_cmp(&b.1.x))
    });
    let rows: Vec<Vec<(String, Rect)>> = items
        .chunks(columns)
        .map(|row| {
            let mut row = row.to_vec();
            row.sort_by(|a, b| a.1.x.total_cmp(&b.1.x));
            row
        })
        .collect();
    let mut column_width = vec![0.0f64; columns];
    let mut row_height = vec![0.0f64; rows.len()];
    for (r, row) in rows.iter().enumerate() {
        for (c, (_, rect)) in row.iter().enumerate() {
            column_width[c] = column_width[c].max(rect.w);
            row_height[r] = row_height[r].max(rect.h);
        }
    }
    let mut deltas = Vec::new();
    let mut y = top;
    for (r, row) in rows.iter().enumerate() {
        let mut x = left;
        for (c, (id, rect)) in row.iter().enumerate() {
            deltas.push((id.clone(), x.round() - rect.x, y.round() - rect.y));
            x += column_width[c] + gap;
        }
        y += row_height[r] + gap;
    }
    apply_deltas(canvas, &deltas)
}

/// Abre um mapa mental a partir de `root`: os cartões ligados a ele por setas (em qualquer
/// direção, e os ligados a esses, e assim por diante) viram uma árvore em colunas. O cartão
/// escolhido não sai do lugar; os filhos vão para a direita do pai, um ramo embaixo do outro, na
/// ordem em que já estavam de cima para baixo, e o pai fica no meio dos filhos. Um ramo nunca
/// invade a faixa de outro, então ninguém da árvore se sobrepõe. Cada seta da árvore passa a
/// sair do lado direito do pai e entrar no lado esquerdo do filho. Cartões soltos, grupos e
/// ligações que fechariam um ciclo ficam como estão. Devolve quantos cartões foram arrumados
/// (sem contar o escolhido); 0 se ele não tem ligações.
pub fn mind_map(canvas: &mut Canvas, root: &str) -> usize {
    let Some(root_node) = canvas.node(root) else {
        return 0;
    };
    if root_node.node_kind() == NodeKind::Group {
        return 0;
    }
    let is_card = |id: &str| {
        canvas
            .node(id)
            .is_some_and(|node| node.node_kind() != NodeKind::Group)
    };
    // Vizinhos na ordem das setas do arquivo (estável).
    let mut neighbors: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
    for edge in &canvas.edges {
        let (from, to) = (edge.from_node.as_str(), edge.to_node.as_str());
        if from == to || !is_card(from) || !is_card(to) {
            continue;
        }
        neighbors
            .entry(from)
            .or_default()
            .push((to, edge.id.as_str()));
        neighbors
            .entry(to)
            .or_default()
            .push((from, edge.id.as_str()));
    }
    // Árvore geradora por largura a partir do escolhido.
    let mut parent_edge: HashMap<String, (String, String)> = HashMap::new();
    let mut children: HashMap<String, Vec<String>> = HashMap::new();
    let mut seen: HashSet<&str> = HashSet::from([root]);
    let mut queue = std::collections::VecDeque::from([root]);
    while let Some(current) = queue.pop_front() {
        for (next, edge) in neighbors.get(current).into_iter().flatten() {
            if seen.insert(next) {
                parent_edge.insert((*next).to_owned(), (current.to_owned(), (*edge).to_owned()));
                children
                    .entry(current.to_owned())
                    .or_default()
                    .push((*next).to_owned());
                queue.push_back(next);
            }
        }
    }
    if children.is_empty() {
        return 0;
    }
    // Ordem dos irmãos: a de cima para baixo que já estava no quadro.
    for list in children.values_mut() {
        list.sort_by(|a, b| {
            let center = |id: &String| {
                canvas
                    .node(id)
                    .map_or((0.0, 0.0), |node| (node.y + node.height / 2.0, node.x))
            };
            let (a, b) = (center(a), center(b));
            a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1))
        });
    }
    // Altura da faixa de cada ramo.
    fn band(
        id: &str,
        canvas: &Canvas,
        children: &HashMap<String, Vec<String>>,
        memo: &mut HashMap<String, f64>,
    ) -> f64 {
        let own = canvas.node(id).map_or(0.0, |node| node.height);
        let kids = children.get(id).map(Vec::as_slice).unwrap_or(&[]);
        let stacked: f64 = kids
            .iter()
            .map(|kid| band(kid, canvas, children, memo))
            .sum::<f64>()
            + TREE_ROW_GAP * kids.len().saturating_sub(1) as f64;
        let height = own.max(stacked);
        memo.insert(id.to_owned(), height);
        height
    }
    let mut bands = HashMap::new();
    band(root, canvas, &children, &mut bands);

    // Posições: o escolhido fica; cada filho entra na faixa do pai.
    let root_rect = root_node.rect();
    let top = root_rect.y + root_rect.h / 2.0 - bands[root] / 2.0;
    let mut placed: Vec<(String, f64, f64)> = Vec::new();
    let mut stack = vec![(root.to_owned(), root_rect.x, top)];
    while let Some((id, x, band_top)) = stack.pop() {
        let Some(node) = canvas.node(&id) else {
            continue;
        };
        let (width, height) = (node.width, node.height);
        let band_height = bands[&id];
        let y = if id == root {
            root_rect.y
        } else {
            (band_top + band_height / 2.0 - height / 2.0).round()
        };
        placed.push((id.clone(), x, y));
        let kids = children.get(&id).map(Vec::as_slice).unwrap_or(&[]);
        let stacked: f64 = kids.iter().map(|kid| bands[kid]).sum::<f64>()
            + TREE_ROW_GAP * kids.len().saturating_sub(1) as f64;
        let mut cursor = band_top + (band_height - stacked) / 2.0;
        for kid in kids {
            stack.push((kid.clone(), x + width + TREE_COLUMN_GAP, cursor));
            cursor += bands[kid] + TREE_ROW_GAP;
        }
    }
    let mut moved = 0;
    for (id, x, y) in placed {
        if id == root {
            continue;
        }
        if let Some(node) = canvas.node_mut(&id) {
            node.x = x.round();
            node.y = y;
            moved += 1;
        }
    }
    // As setas da árvore saem do lado direito do pai e entram no esquerdo do filho.
    for (child, (parent, edge_id)) in &parent_edge {
        if let Some(edge) = canvas.edge_mut(edge_id) {
            let (parent_side, child_side) = (Side::Right.name(), Side::Left.name());
            if edge.from_node == *parent && edge.to_node == *child {
                edge.from_side = Some(parent_side.to_owned());
                edge.to_side = Some(child_side.to_owned());
            } else {
                edge.from_side = Some(child_side.to_owned());
                edge.to_side = Some(parent_side.to_owned());
            }
        }
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(nodes: &[(&str, f64, f64, f64, f64)], edges: &[(&str, &str)]) -> Canvas {
        let nodes: Vec<String> = nodes
            .iter()
            .map(|(id, x, y, w, h)| {
                format!(
                    r#"{{"id":"{id}","type":"text","x":{x},"y":{y},"width":{w},"height":{h},"text":"{id}"}}"#
                )
            })
            .collect();
        let edges: Vec<String> = edges
            .iter()
            .enumerate()
            .map(|(index, (from, to))| {
                format!(r#"{{"id":"e{index}","fromNode":"{from}","toNode":"{to}"}}"#)
            })
            .collect();
        Canvas::parse(&format!(
            r#"{{"nodes":[{}],"edges":[{}]}}"#,
            nodes.join(","),
            edges.join(",")
        ))
        .unwrap()
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|id| (*id).to_owned()).collect()
    }

    fn rect(canvas: &Canvas, id: &str) -> Rect {
        canvas.node(id).unwrap().rect()
    }

    fn solto() -> Canvas {
        board(
            &[
                ("a", 0.0, 0.0, 100.0, 50.0),
                ("b", 300.0, 80.0, 200.0, 70.0),
                ("c", 90.0, 400.0, 60.0, 30.0),
            ],
            &[],
        )
    }

    #[test]
    fn alinha_pelas_bordas_e_pelo_meio_da_caixa_que_envolve() {
        let todos = ids(&["a", "b", "c"]);
        for (how, check) in [
            (Align::Left, 0),
            (Align::Right, 1),
            (Align::CenterH, 2),
            (Align::Top, 3),
            (Align::Bottom, 4),
            (Align::MiddleV, 5),
        ] {
            let mut canvas = solto();
            assert!(align(&mut canvas, &todos, how), "{how:?}");
            let (a, b, c) = (rect(&canvas, "a"), rect(&canvas, "b"), rect(&canvas, "c"));
            match check {
                0 => assert!(a.x == 0.0 && b.x == 0.0 && c.x == 0.0),
                1 => assert!([a.x + a.w, b.x + b.w, c.x + c.w] == [500.0; 3]),
                2 => assert!([a.x + a.w / 2.0, b.x + b.w / 2.0, c.x + c.w / 2.0] == [250.0; 3]),
                3 => assert!(a.y == 0.0 && b.y == 0.0 && c.y == 0.0),
                4 => assert!([a.y + a.h, b.y + b.h, c.y + c.h] == [430.0; 3]),
                _ => assert!([a.y + a.h / 2.0, b.y + b.h / 2.0, c.y + c.h / 2.0] == [215.0; 3]),
            }
            // Só o eixo pedido muda.
            if check < 3 {
                assert_eq!((a.y, b.y, c.y), (0.0, 80.0, 400.0));
            } else {
                assert_eq!((a.x, b.x, c.x), (0.0, 300.0, 90.0));
            }
        }
    }

    #[test]
    fn alinhar_ja_alinhado_ou_sozinho_nao_muda_nada() {
        let mut canvas = solto();
        assert!(!align(&mut canvas, &ids(&["a"]), Align::Left));
        assert!(align(&mut canvas, &ids(&["a", "b", "c"]), Align::Left));
        assert!(!align(&mut canvas, &ids(&["a", "b", "c"]), Align::Left));
        assert!(!align(&mut canvas, &ids(&["a", "nada"]), Align::Top));
    }

    #[test]
    fn distribuir_mantem_as_pontas_e_iguala_os_vaos() {
        let mut canvas = board(
            &[
                ("a", 0.0, 0.0, 100.0, 50.0),
                ("b", 150.0, 0.0, 50.0, 50.0),
                ("c", 700.0, 0.0, 200.0, 50.0),
                ("d", 260.0, 0.0, 100.0, 50.0),
            ],
            &[],
        );
        assert!(distribute(
            &mut canvas,
            &ids(&["a", "b", "c", "d"]),
            Axis::Horizontal
        ));
        let mut rects: Vec<Rect> = ["a", "b", "d", "c"]
            .iter()
            .map(|id| rect(&canvas, id))
            .collect();
        // Ordem pelo meio: a, b, d, c. As pontas não saem do lugar.
        assert_eq!(rects[0].x, 0.0);
        assert_eq!(rects[3].x + rects[3].w, 900.0);
        rects.sort_by(|a, b| a.x.total_cmp(&b.x));
        let gaps: Vec<f64> = rects
            .windows(2)
            .map(|pair| pair[1].x - (pair[0].x + pair[0].w))
            .collect();
        assert_eq!(gaps, [gaps[0]; 3], "vãos iguais: {gaps:?}");
        assert_eq!(gaps[0], (900.0 - 450.0) / 3.0);
        assert!(
            !distribute(&mut canvas, &ids(&["a", "b"]), Axis::Horizontal),
            "são precisos três"
        );
    }

    #[test]
    fn espacamento_igual_deixa_o_primeiro_e_empilha_com_o_vao_pedido() {
        let mut canvas = board(
            &[
                ("a", 10.0, 0.0, 100.0, 50.0),
                ("b", 500.0, 20.0, 60.0, 50.0),
                ("c", 130.0, 5.0, 80.0, 50.0),
            ],
            &[],
        );
        assert!(equal_spacing(&mut canvas, &ids(&["a", "b", "c"]), 40.0));
        assert_eq!(rect(&canvas, "a").x, 10.0);
        assert_eq!(rect(&canvas, "c").x, 150.0);
        assert_eq!(rect(&canvas, "b").x, 270.0);
        // O eixo cruzado não muda.
        assert_eq!(rect(&canvas, "b").y, 20.0);
        // Mais alto do que largo: empilha na vertical.
        let mut alto = board(
            &[
                ("a", 0.0, 0.0, 50.0, 100.0),
                ("b", 10.0, 900.0, 50.0, 100.0),
            ],
            &[],
        );
        assert!(equal_spacing(&mut alto, &ids(&["a", "b"]), 40.0));
        assert_eq!((rect(&alto, "b").x, rect(&alto, "b").y), (10.0, 140.0));
    }

    #[test]
    fn grade_em_ordem_de_leitura_sem_ninguem_encostar() {
        let mut canvas = board(
            &[
                ("e", 700.0, 300.0, 120.0, 60.0),
                ("a", 0.0, 0.0, 100.0, 50.0),
                ("c", 500.0, 10.0, 200.0, 90.0),
                ("b", 200.0, 0.0, 80.0, 50.0),
                ("d", 20.0, 310.0, 100.0, 60.0),
            ],
            &[],
        );
        assert!(arrange_grid(
            &mut canvas,
            &ids(&["a", "b", "c", "d", "e"]),
            40.0
        ));
        // 5 cartões: 3 colunas. Fileira de cima: a, b, c. Fileira de baixo: d, e.
        let pos = |id: &str| (rect(&canvas, id).x, rect(&canvas, id).y);
        assert_eq!(pos("a"), (0.0, 0.0));
        assert_eq!(pos("b"), (140.0, 0.0));
        assert_eq!(pos("c"), (300.0, 0.0));
        assert_eq!(pos("d"), (0.0, 130.0));
        assert_eq!(pos("e"), (140.0, 130.0));
        for (index, a) in canvas.nodes.iter().enumerate() {
            for b in &canvas.nodes[index + 1..] {
                assert!(!a.rect().intersects(&b.rect()), "{} x {}", a.id, b.id);
            }
        }
        assert!(!arrange_grid(&mut canvas, &ids(&["a"]), 40.0));
        // Já arrumado: repetir não muda nada.
        assert!(!arrange_grid(
            &mut canvas,
            &ids(&["a", "b", "c", "d", "e"]),
            40.0
        ));
    }

    #[test]
    fn mover_um_grupo_leva_o_que_esta_dentro() {
        let mut canvas = Canvas::parse(
            r#"{"nodes":[
            {"id":"g","type":"group","x":0,"y":0,"width":300,"height":300,"label":"G"},
            {"id":"in","type":"text","x":20,"y":50,"width":100,"height":50,"text":"dentro"},
            {"id":"x","type":"text","x":600,"y":50,"width":100,"height":50,"text":"fora"}],"edges":[]}"#,
        )
        .unwrap();
        assert!(align(&mut canvas, &ids(&["g", "x"]), Align::Right));
        // O grupo foi até a borda direita (x = 400) e levou o cartão de dentro.
        assert_eq!(rect(&canvas, "g").x, 400.0);
        assert_eq!(rect(&canvas, "in").x, 420.0);
        assert_eq!(rect(&canvas, "x").x, 600.0);
    }

    fn arvore() -> Canvas {
        // Raiz r ligada a a, b (b → r aponta para a raiz) e a ligada a a1, a2; ciclo a2 → b.
        board(
            &[
                ("r", 0.0, 0.0, 200.0, 80.0),
                ("a", 900.0, 700.0, 150.0, 60.0),
                ("b", 10.0, 10.0, 150.0, 60.0),
                ("a1", 0.0, 800.0, 120.0, 40.0),
                ("a2", 5.0, 900.0, 120.0, 200.0),
                ("solto", 3000.0, 3000.0, 100.0, 100.0),
            ],
            &[
                ("r", "a"),
                ("b", "r"),
                ("a", "a1"),
                ("a", "a2"),
                ("a2", "b"),
            ],
        )
    }

    #[test]
    fn mapa_mental_filhos_a_direita_sem_sobreposicao() {
        let mut canvas = arvore();
        let moved = mind_map(&mut canvas, "r");
        assert_eq!(moved, 4, "a, b, a1, a2 (o solto não entra)");
        assert_eq!(
            rect(&canvas, "r"),
            Rect {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 80.0
            }
        );
        // Filhos da raiz: na coluna à direita dela.
        let column_one = 200.0 + TREE_COLUMN_GAP;
        assert_eq!(rect(&canvas, "a").x, column_one);
        assert_eq!(rect(&canvas, "b").x, column_one);
        // Netos: à direita do pai.
        let column_two = column_one + 150.0 + TREE_COLUMN_GAP;
        assert_eq!(rect(&canvas, "a1").x, column_two);
        assert_eq!(rect(&canvas, "a2").x, column_two);
        // Ordem dos irmãos pela posição de cima para baixo que já tinham (b estava acima de a).
        assert!(rect(&canvas, "b").y < rect(&canvas, "a").y);
        assert!(rect(&canvas, "a1").y < rect(&canvas, "a2").y);
        // O pai fica no meio dos filhos.
        let a = rect(&canvas, "a");
        let (a1, a2) = (rect(&canvas, "a1"), rect(&canvas, "a2"));
        let middle = (a1.y + a2.y + a2.h) / 2.0;
        assert!(
            (a.y + a.h / 2.0 - middle).abs() <= 1.0,
            "{a:?} {a1:?} {a2:?}"
        );
        // Ninguém da árvore se sobrepõe, e o cartão solto não saiu do lugar.
        let tree = ["r", "a", "b", "a1", "a2"];
        for (index, first) in tree.iter().enumerate() {
            for second in &tree[index + 1..] {
                assert!(
                    !rect(&canvas, first).intersects(&rect(&canvas, second)),
                    "{first} x {second}"
                );
            }
        }
        assert_eq!(rect(&canvas, "solto").x, 3000.0);
        // As setas da árvore saem do lado direito do pai e entram no esquerdo do filho.
        let side = |edge: &str| {
            let edge = canvas.edge(edge).unwrap();
            (
                edge.from_side.clone().unwrap(),
                edge.to_side.clone().unwrap(),
            )
        };
        assert_eq!(side("e0"), ("right".into(), "left".into())); // r → a
        assert_eq!(side("e1"), ("left".into(), "right".into())); // b → r (b é filho)
        // A seta que fecharia o ciclo (a2 → b) ficou como estava.
        assert_eq!(canvas.edge("e4").unwrap().from_side, None);
    }

    #[test]
    fn mapa_mental_sem_ligacoes_ou_de_grupo_nao_faz_nada() {
        let mut canvas = arvore();
        assert_eq!(mind_map(&mut canvas, "solto"), 0);
        assert_eq!(mind_map(&mut canvas, "nada"), 0);
        assert_eq!(rect(&canvas, "a").x, 900.0);
        // Repetir dá o mesmo resultado (idempotente).
        mind_map(&mut canvas, "r");
        let once = canvas.clone();
        mind_map(&mut canvas, "r");
        assert_eq!(canvas, once);
    }
}
