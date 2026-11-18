//! H2a: em quais quadros `.canvas` cada nota aparece ("Onde esta nota aparece").
//!
//! O índice guarda, por quadro, os cartões `file` e o nome do grupo que os contém, e responde
//! pela nota de destino. Um cartão que aponta para outro `.canvas` conta igual: assim um quadro
//! sabe em quais quadros ele está. A atualização é incremental: `sync` confere o carimbo
//! (tamanho e hora) de cada quadro e só relê o que mudou; quadro apagado sai, quadro renomeado é
//! um sai e um entra, e um quadro inválido fica sem ocorrências (nunca quebra nada).

use crate::canvas::{Canvas, NodeKind};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Quadros maiores que isto não são lidos (o app não abre um JSON desse tamanho com conforto).
const MAX_BOARD_BYTES: u64 = 16 * 1024 * 1024;

/// Um cartão de nota (ou de quadro) dentro de um quadro.
#[derive(Clone, Debug, PartialEq)]
pub struct Occurrence {
    /// O quadro, como caminho relativo ao cofre.
    pub board: PathBuf,
    pub node_id: String,
    pub x: f64,
    pub y: f64,
    /// O rótulo do menor grupo com nome que contém o cartão.
    pub group: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Disk {
        len: u64,
        modified: Option<SystemTime>,
    },
    Draft(u64),
}

#[derive(Debug)]
struct Board {
    source: Source,
    /// As chaves de destino que este quadro alimenta em `by_target`.
    targets: Vec<String>,
}

#[derive(Debug, Default)]
pub struct CanvasIndex {
    boards: HashMap<PathBuf, Board>,
    by_target: HashMap<String, Vec<Occurrence>>,
}

/// Caminho do cofre em forma comparável: `/`, sem `.` nem `//`, `..` resolvido, acentos em NFC.
pub fn key(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    compose(&parts.join("/"))
}

pub fn key_of(path: &Path) -> String {
    key(&path.to_string_lossy())
}

/// NFC para as letras latinas com acento comum (o que um nome de arquivo em português usa):
/// letra + marca combinante vira a letra única. O resto passa como está.
fn compose(text: &str) -> String {
    const TABLE: &[(char, u32, u32)] = &[
        ('A', 0x300, 0xC0),
        ('E', 0x300, 0xC8),
        ('I', 0x300, 0xCC),
        ('O', 0x300, 0xD2),
        ('U', 0x300, 0xD9),
        ('N', 0x300, 0x1F8),
        ('Y', 0x300, 0x1EF2),
        ('a', 0x300, 0xE0),
        ('e', 0x300, 0xE8),
        ('i', 0x300, 0xEC),
        ('o', 0x300, 0xF2),
        ('u', 0x300, 0xF9),
        ('n', 0x300, 0x1F9),
        ('y', 0x300, 0x1EF3),
        ('A', 0x301, 0xC1),
        ('E', 0x301, 0xC9),
        ('I', 0x301, 0xCD),
        ('O', 0x301, 0xD3),
        ('U', 0x301, 0xDA),
        ('C', 0x301, 0x106),
        ('N', 0x301, 0x143),
        ('Y', 0x301, 0xDD),
        ('a', 0x301, 0xE1),
        ('e', 0x301, 0xE9),
        ('i', 0x301, 0xED),
        ('o', 0x301, 0xF3),
        ('u', 0x301, 0xFA),
        ('c', 0x301, 0x107),
        ('n', 0x301, 0x144),
        ('y', 0x301, 0xFD),
        ('A', 0x302, 0xC2),
        ('E', 0x302, 0xCA),
        ('I', 0x302, 0xCE),
        ('O', 0x302, 0xD4),
        ('U', 0x302, 0xDB),
        ('C', 0x302, 0x108),
        ('Y', 0x302, 0x176),
        ('a', 0x302, 0xE2),
        ('e', 0x302, 0xEA),
        ('i', 0x302, 0xEE),
        ('o', 0x302, 0xF4),
        ('u', 0x302, 0xFB),
        ('c', 0x302, 0x109),
        ('y', 0x302, 0x177),
        ('A', 0x303, 0xC3),
        ('E', 0x303, 0x1EBC),
        ('I', 0x303, 0x128),
        ('O', 0x303, 0xD5),
        ('U', 0x303, 0x168),
        ('N', 0x303, 0xD1),
        ('Y', 0x303, 0x1EF8),
        ('a', 0x303, 0xE3),
        ('e', 0x303, 0x1EBD),
        ('i', 0x303, 0x129),
        ('o', 0x303, 0xF5),
        ('u', 0x303, 0x169),
        ('n', 0x303, 0xF1),
        ('y', 0x303, 0x1EF9),
        ('A', 0x308, 0xC4),
        ('E', 0x308, 0xCB),
        ('I', 0x308, 0xCF),
        ('O', 0x308, 0xD6),
        ('U', 0x308, 0xDC),
        ('Y', 0x308, 0x178),
        ('a', 0x308, 0xE4),
        ('e', 0x308, 0xEB),
        ('i', 0x308, 0xEF),
        ('o', 0x308, 0xF6),
        ('u', 0x308, 0xFC),
        ('y', 0x308, 0xFF),
        ('E', 0x327, 0x228),
        ('C', 0x327, 0xC7),
        ('N', 0x327, 0x145),
        ('e', 0x327, 0x229),
        ('c', 0x327, 0xE7),
        ('n', 0x327, 0x146),
    ];
    if text.is_ascii() {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    for letter in text.chars() {
        let composed = out.chars().next_back().and_then(|base| {
            TABLE
                .iter()
                .find(|(b, mark, _)| *b == base && *mark == letter as u32)
                .and_then(|(_, _, whole)| char::from_u32(*whole))
        });
        match composed {
            Some(whole) => {
                out.pop();
                out.push(whole);
            }
            None => out.push(letter),
        }
    }
    out
}

fn hash(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// Os cartões de nota do quadro, com o grupo de cada um.
fn occurrences_of(board: &Path, canvas: &Canvas) -> Vec<(String, Occurrence)> {
    let groups: Vec<_> = canvas
        .nodes
        .iter()
        .filter(|node| node.node_kind() == NodeKind::Group)
        .filter(|node| {
            node.label
                .as_deref()
                .is_some_and(|label| !label.trim().is_empty())
        })
        .collect();
    let mut found = Vec::new();
    for node in &canvas.nodes {
        if node.node_kind() != NodeKind::File {
            continue;
        }
        let Some(file) = node.file.as_deref().map(key).filter(|key| !key.is_empty()) else {
            continue;
        };
        let rect = node.rect();
        let group = groups
            .iter()
            .filter(|group| group.rect().contains_rect(&rect))
            .min_by(|a, b| (a.width * a.height).total_cmp(&(b.width * b.height)))
            .and_then(|group| group.label.clone());
        found.push((
            file,
            Occurrence {
                board: board.to_owned(),
                node_id: node.id.clone(),
                x: node.x,
                y: node.y,
                group,
            },
        ));
    }
    found
}

impl CanvasIndex {
    /// Onde a nota aparece: um item por cartão, por quadro e depois pela posição.
    pub fn occurrences(&self, note: &Path) -> Vec<&Occurrence> {
        let mut found: Vec<&Occurrence> = self
            .by_target
            .get(&key_of(note))
            .map(|list| list.iter().collect())
            .unwrap_or_default();
        found.sort_by(|a, b| {
            a.board
                .cmp(&b.board)
                .then(a.y.total_cmp(&b.y))
                .then(a.x.total_cmp(&b.x))
                .then_with(|| a.node_id.cmp(&b.node_id))
        });
        found
    }

    pub fn board_count(&self) -> usize {
        self.boards.len()
    }

    fn forget(&mut self, board: &Path) {
        let Some(old) = self.boards.remove(board) else {
            return;
        };
        for target in old.targets {
            if let Some(list) = self.by_target.get_mut(&target) {
                list.retain(|occurrence| occurrence.board != board);
                if list.is_empty() {
                    self.by_target.remove(&target);
                }
            }
        }
    }

    fn store(&mut self, board: &Path, text: &str, source: Source) {
        self.forget(board);
        // Texto que não é um quadro válido: guardado sem ocorrências, para não ser relido.
        let found = Canvas::parse(text)
            .map(|canvas| occurrences_of(board, &canvas))
            .unwrap_or_default();
        let mut targets = Vec::new();
        for (target, occurrence) in found {
            if !targets.contains(&target) {
                targets.push(target.clone());
            }
            self.by_target.entry(target).or_default().push(occurrence);
        }
        self.boards
            .insert(board.to_owned(), Board { source, targets });
    }

    /// O quadro mudou (ou apareceu) com este texto; `None` o tira do índice.
    pub fn update_board(&mut self, board: &Path, text: Option<&str>) {
        match text {
            Some(text) => self.store(board, text, Source::Draft(hash(text))),
            None => self.forget(board),
        }
    }

    /// Põe o índice em dia com `boards` (os `.canvas` do cofre, relativos a `root`). `open` são
    /// os textos das abas abertas, que valem mais que o disco. Só relê o que mudou; devolve se
    /// algo mudou.
    pub fn sync(&mut self, root: &Path, boards: &[PathBuf], open: &[(&Path, &str)]) -> bool {
        let mut changed = false;
        let gone: Vec<PathBuf> = self
            .boards
            .keys()
            .filter(|known| !boards.contains(known))
            .cloned()
            .collect();
        for board in gone {
            self.forget(&board);
            changed = true;
        }
        for board in boards {
            let draft = open
                .iter()
                .find(|(path, _)| *path == board.as_path())
                .map(|(_, text)| *text);
            if let Some(text) = draft {
                let source = Source::Draft(hash(text));
                if self
                    .boards
                    .get(board)
                    .is_none_or(|known| known.source != source)
                {
                    self.store(board, text, source);
                    changed = true;
                }
                continue;
            }
            let absolute = root.join(board);
            let Ok(metadata) = std::fs::metadata(&absolute) else {
                changed |= self.boards.contains_key(board);
                self.forget(board);
                continue;
            };
            let source = Source::Disk {
                len: metadata.len(),
                modified: metadata.modified().ok(),
            };
            if self
                .boards
                .get(board)
                .is_some_and(|known| known.source == source)
            {
                continue;
            }
            let text = if metadata.len() > MAX_BOARD_BYTES {
                String::new()
            } else {
                std::fs::read_to_string(&absolute).unwrap_or_default()
            };
            self.store(board, &text, source);
            changed = true;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type GroupSpec<'a> = (&'a str, &'a str, f64, f64, f64, f64);

    fn board(files: &[(&str, f64, f64)], groups: &[GroupSpec]) -> String {
        let mut nodes = Vec::new();
        for (label, id, x, y, w, h) in groups {
            nodes.push(format!(
                r#"{{"id":"{id}","type":"group","x":{x},"y":{y},"width":{w},"height":{h},"label":"{label}"}}"#
            ));
        }
        for (index, (file, x, y)) in files.iter().enumerate() {
            let file = file.replace('\\', "\\\\");
            nodes.push(format!(
                r#"{{"id":"c{index}","type":"file","x":{x},"y":{y},"width":100,"height":50,"file":"{file}"}}"#
            ));
        }
        format!(r#"{{"nodes":[{}],"edges":[]}}"#, nodes.join(","))
    }

    #[test]
    fn nota_em_dois_quadros_com_posicao_e_grupo() {
        let mut index = CanvasIndex::default();
        let a = board(
            &[("Alpha.md", 10.0, 20.0)],
            &[("Fase 1", "g1", 0.0, 0.0, 300.0, 300.0)],
        );
        index.update_board(Path::new("A.canvas"), Some(&a));
        index.update_board(
            Path::new("pasta/B.canvas"),
            Some(&board(&[("./Alpha.md", 500.0, 5.0)], &[])),
        );
        let found = index.occurrences(Path::new("Alpha.md"));
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].board, Path::new("A.canvas"));
        assert_eq!(found[0].node_id, "c0");
        assert_eq!((found[0].x, found[0].y), (10.0, 20.0));
        assert_eq!(found[0].group.as_deref(), Some("Fase 1"));
        assert_eq!(found[1].group, None);
        assert!(index.occurrences(Path::new("Outra.md")).is_empty());
    }

    #[test]
    fn grupo_mais_interno_vence() {
        let mut index = CanvasIndex::default();
        let text = board(
            &[("N.md", 20.0, 20.0)],
            &[
                ("Fora", "g1", 0.0, 0.0, 1000.0, 1000.0),
                ("Dentro", "g2", 10.0, 10.0, 200.0, 200.0),
            ],
        );
        index.update_board(Path::new("Q.canvas"), Some(&text));
        let found = index.occurrences(Path::new("N.md"));
        assert_eq!(found[0].group.as_deref(), Some("Dentro"));
    }

    #[test]
    fn quadro_dentro_de_quadro_conta() {
        let mut index = CanvasIndex::default();
        index.update_board(
            Path::new("Mapa.canvas"),
            Some(&board(&[("Sub/Filho.canvas", 0.0, 0.0)], &[])),
        );
        let found = index.occurrences(Path::new("Sub/Filho.canvas"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].board, Path::new("Mapa.canvas"));
    }

    #[test]
    fn subpasta_barras_invertidas_e_acento_nfc() {
        let mut index = CanvasIndex::default();
        // O cartão grava o nome decomposto (e com barra invertida); a nota é a forma composta.
        let decomposed = "Notas\\Cafe\u{301} Ac\u{327}ao.md";
        index.update_board(
            Path::new("Q.canvas"),
            Some(&board(&[(decomposed, 0.0, 0.0)], &[])),
        );
        let found = index.occurrences(Path::new("Notas/Caf\u{e9} A\u{e7}ao.md"));
        assert_eq!(found.len(), 1, "NFC e separadores devem casar");
        assert_eq!(key("a/../b//./c.md"), "b/c.md");
    }

    #[test]
    fn quadro_invalido_e_ignorado() {
        let mut index = CanvasIndex::default();
        index.update_board(Path::new("Ruim.canvas"), Some("{ isto não é json"));
        index.update_board(
            Path::new("Bom.canvas"),
            Some(&board(&[("Alpha.md", 0.0, 0.0)], &[])),
        );
        assert_eq!(index.occurrences(Path::new("Alpha.md")).len(), 1);
    }

    #[test]
    fn apagar_e_renomear_quadro() {
        let mut index = CanvasIndex::default();
        let text = board(&[("Alpha.md", 0.0, 0.0)], &[]);
        index.update_board(Path::new("Velho.canvas"), Some(&text));
        // Renomear: sai o nome velho, entra o novo.
        index.update_board(Path::new("Velho.canvas"), None);
        index.update_board(Path::new("Novo.canvas"), Some(&text));
        let found = index.occurrences(Path::new("Alpha.md"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].board, Path::new("Novo.canvas"));
        index.update_board(Path::new("Novo.canvas"), None);
        assert!(index.occurrences(Path::new("Alpha.md")).is_empty());
        assert_eq!(index.board_count(), 0);
    }

    #[test]
    fn sync_so_rele_o_que_mudou_e_acompanha_o_disco() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path();
        std::fs::write(root.join("A.canvas"), board(&[("Alpha.md", 0.0, 0.0)], &[])).unwrap();
        std::fs::write(root.join("B.canvas"), "lixo").unwrap();
        let both = vec![PathBuf::from("A.canvas"), PathBuf::from("B.canvas")];
        let mut index = CanvasIndex::default();
        assert!(index.sync(root, &both, &[]));
        assert_eq!(index.occurrences(Path::new("Alpha.md")).len(), 1);
        assert!(!index.sync(root, &both, &[]), "nada mudou, nada relido");
        // Um rascunho aberto vale mais que o disco.
        let draft = board(&[("Alpha.md", 1.0, 1.0), ("Alpha.md", 2.0, 2.0)], &[]);
        assert!(index.sync(root, &both, &[(Path::new("A.canvas"), draft.as_str())]));
        assert_eq!(index.occurrences(Path::new("Alpha.md")).len(), 2);
        // Fechou a aba: volta ao disco.
        assert!(index.sync(root, &both, &[]));
        assert_eq!(index.occurrences(Path::new("Alpha.md")).len(), 1);
        // Quadro apagado do cofre.
        assert!(index.sync(root, &both[1..], &[]));
        assert!(index.occurrences(Path::new("Alpha.md")).is_empty());
    }
}
