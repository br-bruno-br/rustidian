//! H2b: trocar entre nota e quadro sem perder nada. Aqui fica só a lógica, sem tela:
//!
//! - o par de uma nota (`X.md` e `X.canvas` na mesma pasta, ligados só pelo nome);
//! - gerar o quadro de uma nota: um cartão vivo da própria nota no centro e, em anel, um cartão
//!   vivo para cada nota ligada por `[[link]]` ou `![[embed]]`. O quadro só guarda **caminhos**;
//!   nenhum texto da nota é copiado para dentro do `.canvas` (uma fonte de verdade só);
//! - o nome do arquivo de uma nota nascida de um cartão de texto.

use crate::canvas::{Canvas, Rect};
use std::path::{Path, PathBuf};

/// Tamanho do cartão da própria nota, no centro do quadro gerado.
pub const CENTER_SIZE: (f64, f64) = (460.0, 520.0);
/// Tamanho dos cartões das notas ligadas.
pub const SATELLITE_SIZE: (f64, f64) = (320.0, 260.0);
/// Folga mínima entre dois cartões do quadro gerado.
const MARGIN: f64 = 40.0;
/// Quantos cartões cabem num anel antes de abrir o próximo.
const PER_RING: usize = 10;

/// O quadro par de uma nota: `X.md` vira `X.canvas`, na mesma pasta.
pub fn board_of(note: &Path) -> PathBuf {
    note.with_extension("canvas")
}

/// A nota par de um quadro: `X.canvas` vira `X.md`, na mesma pasta.
pub fn note_of(board: &Path) -> PathBuf {
    board.with_extension("md")
}

fn slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Os arquivos que a nota liga por `[[link]]` e `![[embed]]`, na ordem em que aparecem, sem
/// repetir e sem a própria nota. `resolve(alvo)` diz onde o alvo está no cofre (caminho relativo
/// à raiz); alvo que não existe é pulado (não se cria cartão para link quebrado).
pub fn linked_files(
    note: &Path,
    text: &str,
    resolve: impl Fn(&str) -> Option<PathBuf>,
) -> Vec<String> {
    let own = crate::canvas_index::key_of(note);
    let mut found: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for target in crate::vault::wiki_links(text) {
        let Some(path) = resolve(&target) else {
            continue;
        };
        let key = crate::canvas_index::key_of(&path);
        if key.is_empty() || key == own || !seen.insert(key) {
            continue;
        }
        found.push(slash(&path));
    }
    found
}

/// Onde caem os cartões em volta do central: o canto de cima à esquerda de cada um, na ordem,
/// começando no alto e seguindo no sentido horário. Cada anel recebe até `PER_RING` cartões e
/// abre no menor raio em que ninguém encosta em ninguém (nem no cartão central).
pub fn ring_layout(count: usize, center: (f64, f64), satellite: (f64, f64)) -> Vec<(f64, f64)> {
    let middle = Rect {
        x: -center.0 / 2.0,
        y: -center.1 / 2.0,
        w: center.0,
        h: center.1,
    };
    let mut placed: Vec<Rect> = vec![middle];
    let mut positions = Vec::with_capacity(count);
    let mut radius = center.0.hypot(center.1) / 2.0;
    let rings = count.div_ceil(PER_RING).max(1);
    let mut left = count;
    for ring in 0..rings {
        let size = left.min(PER_RING);
        // Anéis seguintes giram meio passo, para os cartões não ficarem em fila.
        let offset = if ring % 2 == 1 {
            std::f64::consts::PI / size as f64
        } else {
            0.0
        };
        radius += 120.0;
        let mut guard = 0;
        let spots = loop {
            let spots: Vec<Rect> = (0..size)
                .map(|index| {
                    let angle = -std::f64::consts::FRAC_PI_2
                        + offset
                        + std::f64::consts::TAU * index as f64 / size as f64;
                    Rect {
                        x: (radius * angle.cos() - satellite.0 / 2.0).round(),
                        y: (radius * angle.sin() - satellite.1 / 2.0).round(),
                        w: satellite.0,
                        h: satellite.1,
                    }
                })
                .collect();
            let apart = |a: &Rect, b: &Rect| {
                let wide = |rect: &Rect| Rect {
                    x: rect.x - MARGIN / 2.0,
                    y: rect.y - MARGIN / 2.0,
                    w: rect.w + MARGIN,
                    h: rect.h + MARGIN,
                };
                !wide(a).intersects(&wide(b))
            };
            let clear = spots.iter().enumerate().all(|(index, spot)| {
                placed.iter().all(|other| apart(spot, other))
                    && spots[index + 1..].iter().all(|other| apart(spot, other))
            });
            guard += 1;
            if clear || guard > 4000 {
                break spots;
            }
            radius += 20.0;
        };
        for spot in &spots {
            positions.push((spot.x, spot.y));
        }
        placed.extend(spots);
        left -= size;
    }
    positions
}

/// O quadro de uma nota: no centro um cartão vivo da própria nota (`note`, caminho relativo ao
/// cofre) e, em volta, um cartão vivo para cada arquivo de `linked`, cada um ligado ao central
/// por uma seta. Só caminhos entram no quadro: o texto da nota nunca é copiado.
pub fn board_from_note(note: &Path, linked: &[String]) -> Canvas {
    let mut board = Canvas::default();
    let (width, height) = CENTER_SIZE;
    let center = board.add_file(&slash(note), -width / 2.0, -height / 2.0);
    if let Some(node) = board.node_mut(&center) {
        node.width = width;
        node.height = height;
    }
    for (path, (x, y)) in linked
        .iter()
        .zip(ring_layout(linked.len(), CENTER_SIZE, SATELLITE_SIZE))
    {
        let id = board.add_file(path, x, y);
        if let Some(node) = board.node_mut(&id) {
            node.width = SATELLITE_SIZE.0;
            node.height = SATELLITE_SIZE.1;
        }
        board.connect(&center, &id, None, None);
    }
    board
}

/// Nome de arquivo (sem extensão) tirado da primeira linha útil do texto, sem marcas de
/// Markdown nem caracteres que o Windows ou os links não aceitam. `None` se não sobra nada.
pub fn name_from_text(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    let line = line.trim_start_matches(['#', '>', '-', '*', '+', ' ', '\t']);
    let cleaned: String = line
        .chars()
        .map(|character| match character {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '[' | ']' | '#' | '^' | '`'
            | '_' | '~' | '=' => ' ',
            other if other.is_control() => ' ',
            other => other,
        })
        .collect();
    let joined = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut name: String = joined.chars().take(60).collect();
    name = name.trim_matches(['.', ' ']).to_owned();
    if name.is_empty() {
        return None;
    }
    const RESERVED: [&str; 8] = ["CON", "PRN", "AUX", "NUL", "COM1", "COM2", "LPT1", "LPT2"];
    if RESERVED
        .iter()
        .any(|reserved| name.eq_ignore_ascii_case(reserved))
    {
        name.push_str(" nota");
    }
    Some(name)
}

/// O nome do arquivo `.md` de uma nota nascida de um cartão de texto: a primeira linha, ou
/// "Nota 1", "Nota 2"... se não houver. Nunca repete um nome que `taken` diz que já existe
/// (acrescenta " 1", " 2"... ao nome).
pub fn note_file_name(text: &str, taken: impl Fn(&str) -> bool) -> String {
    match name_from_text(text) {
        Some(base) => {
            let first = format!("{base}.md");
            if !taken(&first) {
                return first;
            }
            (1u32..)
                .map(|number| format!("{base} {number}.md"))
                .find(|name| !taken(name))
                .expect("há sempre um número livre")
        }
        None => (1u32..)
            .map(|number| format!("Nota {number}.md"))
            .find(|name| !taken(name))
            .expect("há sempre um número livre"),
    }
}

/// Um nome livre para a cópia em forma de nota de um quadro ("Quadro (lista).md").
pub fn export_file_name(board: &Path, taken: impl Fn(&str) -> bool) -> String {
    let stem = board
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Quadro".into());
    let base = format!("{stem} (lista)");
    let first = format!("{base}.md");
    if !taken(&first) {
        return first;
    }
    (1u32..)
        .map(|number| format!("{base} {number}.md"))
        .find(|name| !taken(name))
        .expect("há sempre um número livre")
}

/// O nome do quadro (sem pasta e sem extensão), para títulos.
pub fn board_title(board: &Path) -> String {
    board
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::NodeKind;

    fn resolver(known: &[&str]) -> impl Fn(&str) -> Option<PathBuf> {
        let known: Vec<String> = known.iter().map(|name| (*name).to_owned()).collect();
        move |target: &str| {
            let name = target.split(['|', '#']).next().unwrap_or(target).trim();
            known
                .iter()
                .find(|known| {
                    known.trim_end_matches(".md") == name
                        || known.rsplit('/').next().unwrap().trim_end_matches(".md") == name
                })
                .map(PathBuf::from)
        }
    }

    #[test]
    fn o_par_acompanha_o_nome_na_mesma_pasta() {
        assert_eq!(
            board_of(Path::new("Pasta/Ideia.md")),
            Path::new("Pasta/Ideia.canvas")
        );
        assert_eq!(
            note_of(Path::new("Pasta/Ideia.canvas")),
            Path::new("Pasta/Ideia.md")
        );
        assert_eq!(
            board_of(Path::new("Sem pasta.md")),
            Path::new("Sem pasta.canvas")
        );
    }

    #[test]
    fn links_e_embeds_na_ordem_sem_repetir_sem_a_propria_nota_e_sem_quebrados() {
        let text = "# Hub\n\nVeja [[Beta]] e ![[Alpha]].\nDe novo [[Beta|apelido]] e [[Pasta/Gama#Seção]].\n\
                    Uma [[Fantasma]], eu mesma [[Hub]], e `[[Codigo]]` em código.\n";
        let resolve = resolver(&[
            "Hub.md",
            "Alpha.md",
            "Beta.md",
            "Pasta/Gama.md",
            "Codigo.md",
        ]);
        let linked = linked_files(Path::new("Hub.md"), text, resolve);
        assert_eq!(linked, ["Beta.md", "Alpha.md", "Pasta/Gama.md"]);
    }

    fn overlaps(spots: &[(f64, f64)], satellite: (f64, f64), center: (f64, f64)) -> bool {
        let mut rects = vec![Rect {
            x: -center.0 / 2.0,
            y: -center.1 / 2.0,
            w: center.0,
            h: center.1,
        }];
        rects.extend(spots.iter().map(|(x, y)| Rect {
            x: *x,
            y: *y,
            w: satellite.0,
            h: satellite.1,
        }));
        (0..rects.len()).any(|a| (a + 1..rects.len()).any(|b| rects[a].intersects(&rects[b])))
    }

    #[test]
    fn o_layout_em_anel_nunca_sobrepoe_e_o_primeiro_fica_no_alto() {
        for count in [0, 1, 2, 3, 5, 8, 10, 11, 23, 40] {
            let spots = ring_layout(count, CENTER_SIZE, SATELLITE_SIZE);
            assert_eq!(spots.len(), count);
            assert!(
                !overlaps(&spots, SATELLITE_SIZE, CENTER_SIZE),
                "sobreposição com {count} cartões"
            );
        }
        let spots = ring_layout(4, CENTER_SIZE, SATELLITE_SIZE);
        // O primeiro está acima do cartão central, no meio; o segundo, à direita.
        assert!(spots[0].1 + SATELLITE_SIZE.1 <= -CENTER_SIZE.1 / 2.0);
        assert!((spots[0].0 + SATELLITE_SIZE.0 / 2.0).abs() <= 1.0);
        assert!(spots[1].0 >= CENTER_SIZE.0 / 2.0);
    }

    #[test]
    fn o_quadro_gerado_tem_so_caminhos_e_setas_do_centro() {
        let linked: Vec<String> = ["Beta.md", "Alpha.md", "Pasta/Gama.md"]
            .iter()
            .map(|path| (*path).to_owned())
            .collect();
        let board = board_from_note(Path::new("Pasta/Hub.md"), &linked);
        assert_eq!(board.nodes.len(), 4);
        assert_eq!(board.edges.len(), 3);
        let center = &board.nodes[0];
        assert_eq!(center.file.as_deref(), Some("Pasta/Hub.md"));
        assert_eq!(center.node_kind(), NodeKind::File);
        assert_eq!((center.width, center.height), CENTER_SIZE);
        assert!(board.edges.iter().all(|edge| edge.from_node == center.id));
        // Nada de texto: nenhum nó guarda `text`, e o JSON não traz trecho nenhum da nota.
        assert!(board.nodes.iter().all(|node| node.text.is_none()));
        let json = board.to_json();
        assert!(json.contains("\"file\":\"Pasta/Hub.md\""));
        assert_eq!(Canvas::parse(&json).unwrap(), board, "volta igual do JSON");
        assert!(board.has_file("Pasta/Hub.md"));
        // Os cartões em volta não encostam uns nos outros nem no central.
        let rects: Vec<Rect> = board.nodes.iter().map(|node| node.rect()).collect();
        for a in 0..rects.len() {
            for b in a + 1..rects.len() {
                assert!(!rects[a].intersects(&rects[b]), "{a} encosta em {b}");
            }
        }
    }

    #[test]
    fn nota_sem_links_gera_so_o_cartao_central() {
        let board = board_from_note(Path::new("Sozinha.md"), &[]);
        assert_eq!(board.nodes.len(), 1);
        assert!(board.edges.is_empty());
    }

    #[test]
    fn o_nome_da_nota_vem_da_primeira_linha_sem_marcas_nem_caracteres_proibidos() {
        assert_eq!(
            name_from_text("# Plano: **fase 1**\nresto").as_deref(),
            Some("Plano fase 1")
        );
        assert_eq!(
            name_from_text("\n\n  - ideia/boa? \n").as_deref(),
            Some("ideia boa")
        );
        assert_eq!(
            name_from_text("[[Link]] aqui").as_deref(),
            Some("Link aqui")
        );
        assert_eq!(name_from_text("").as_deref(), None);
        assert_eq!(name_from_text("  \n ??? \n").as_deref(), None);
        assert_eq!(name_from_text("NUL").as_deref(), Some("NUL nota"));
        let longa = "a".repeat(200);
        assert_eq!(name_from_text(&longa).unwrap().chars().count(), 60);
    }

    #[test]
    fn o_nome_do_arquivo_e_unico_e_cai_em_nota_n_quando_nao_ha_texto() {
        let taken = |used: &'static [&'static str]| move |name: &str| used.contains(&name);
        assert_eq!(
            note_file_name("Ideia boa\nmais", taken(&[])),
            "Ideia boa.md"
        );
        assert_eq!(
            note_file_name("Ideia boa", taken(&["Ideia boa.md", "Ideia boa 1.md"])),
            "Ideia boa 2.md"
        );
        assert_eq!(note_file_name("", taken(&[])), "Nota 1.md");
        assert_eq!(
            note_file_name("   ", taken(&["Nota 1.md", "Nota 2.md"])),
            "Nota 3.md"
        );
        assert_eq!(
            export_file_name(
                Path::new("Pasta/Plano.canvas"),
                taken(&["Plano (lista).md"])
            ),
            "Plano (lista) 1.md"
        );
    }
}
