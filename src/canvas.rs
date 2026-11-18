//! Quadros `.canvas` (R14): o formato aberto JSON Canvas 1.0 (<https://jsoncanvas.org/spec/1.0/>),
//! o mesmo que o Obsidian lê e grava. Aqui fica só a lógica, sem tela: ler e gravar o JSON,
//! as operações sobre nós e arestas, a geometria das setas e o Markdown simples dos cartões.
//!
//! A régua é a especificação. Todo campo que este código não conhece (nos nós, nas arestas e
//! na raiz) é guardado como veio e volta igual na gravação, para o Obsidian abrir o mesmo
//! arquivo sem perder nada. Texto vazio vale como quadro vazio; JSON inválido é erro e nunca é
//! sobrescrito (a tela mostra o aviso e deixa abrir como código-fonte).

use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Map, Value};
use std::path::Path;

/// Texto de um quadro novo, no estilo de escrita do Obsidian (um nó por linha, tabulação).
pub const EMPTY: &str = "{\n\t\"nodes\":[\n\t],\n\t\"edges\":[\n\t]\n}";

/// Tamanhos de um cartão novo, em unidades do quadro.
pub const TEXT_SIZE: (f64, f64) = (250.0, 60.0);
pub const BOX_SIZE: (f64, f64) = (400.0, 400.0);
/// Menor tamanho que o redimensionar aceita.
pub const MIN_SIZE: (f64, f64) = (40.0, 30.0);
/// Altura do cabeçalho (título e botões) de um cartão de nota, em unidades do quadro; é também a
/// altura de um cartão recolhido.
pub const NOTE_HEADER: f64 = 36.0;
/// Campos opcionais nossos (H1). O JSON Canvas aceita campos extras e o Obsidian os ignora; o
/// arquivo continua abrindo lá. Sem eles o cartão é um cartão de nota comum.
pub const COLLAPSED_KEY: &str = "rustidianCollapsed";
pub const EXPANDED_HEIGHT_KEY: &str = "rustidianHeight";

pub fn is_canvas(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("canvas"))
}

// ---------------------------------------------------------------------------------------------
// Dados
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

impl Side {
    pub const ALL: [Side; 4] = [Side::Top, Side::Right, Side::Bottom, Side::Left];

    pub fn name(self) -> &'static str {
        match self {
            Side::Top => "top",
            Side::Right => "right",
            Side::Bottom => "bottom",
            Side::Left => "left",
        }
    }

    pub fn parse(text: &str) -> Option<Side> {
        Self::ALL.into_iter().find(|side| side.name() == text)
    }

    /// Para onde a linha sai deste lado.
    pub fn normal(self) -> (f64, f64) {
        match self {
            Side::Top => (0.0, -1.0),
            Side::Right => (1.0, 0.0),
            Side::Bottom => (0.0, 1.0),
            Side::Left => (-1.0, 0.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Text,
    File,
    Link,
    Group,
    /// Tipo que esta versão não conhece: aparece como cartão cinza e é preservado.
    Other,
}

fn number<S: Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
    // A especificação manda inteiros; só sai decimal se o arquivo já trouxe decimal.
    if value.fract() == 0.0 && value.abs() < 1e15 {
        serializer.serialize_i64(*value as i64)
    } else {
        serializer.serialize_f64(*value)
    }
}

/// Um número JSON que fica inteiro quando o valor é inteiro (a especificação manda inteiros).
fn json_number(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        Value::from(value as i64)
    } else {
        Value::from(value)
    }
}

fn default_width() -> f64 {
    TEXT_SIZE.0
}

fn default_height() -> f64 {
    TEXT_SIZE.1
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default, serialize_with = "number")]
    pub x: f64,
    #[serde(default, serialize_with = "number")]
    pub y: f64,
    #[serde(default = "default_width", serialize_with = "number")]
    pub width: f64,
    #[serde(default = "default_height", serialize_with = "number")]
    pub height: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subpath: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_style: Option<String>,
    /// Tudo o que a especificação (ou o Obsidian, ou um plugin) tem e este código não conhece.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Node {
    pub fn node_kind(&self) -> NodeKind {
        match self.kind.as_str() {
            "text" => NodeKind::Text,
            "file" => NodeKind::File,
            "link" => NodeKind::Link,
            "group" => NodeKind::Group,
            _ => NodeKind::Other,
        }
    }

    fn blank(id: String, kind: &str, x: f64, y: f64, size: (f64, f64)) -> Self {
        Node {
            id,
            kind: kind.to_owned(),
            x,
            y,
            width: size.0,
            height: size.1,
            color: None,
            text: None,
            file: None,
            subpath: None,
            url: None,
            label: None,
            background: None,
            background_style: None,
            extra: Map::new(),
        }
    }

    pub fn rect(&self) -> Rect {
        Rect {
            x: self.x,
            y: self.y,
            w: self.width,
            h: self.height,
        }
    }

    pub fn set_rect(&mut self, rect: Rect) {
        self.x = rect.x;
        self.y = rect.y;
        self.width = rect.w;
        self.height = rect.h;
    }

    /// Ponto do meio do lado.
    pub fn anchor(&self, side: Side) -> (f64, f64) {
        let rect = self.rect();
        match side {
            Side::Top => (rect.x + rect.w / 2.0, rect.y),
            Side::Right => (rect.x + rect.w, rect.y + rect.h / 2.0),
            Side::Bottom => (rect.x + rect.w / 2.0, rect.y + rect.h),
            Side::Left => (rect.x, rect.y + rect.h / 2.0),
        }
    }

    /// Cartão de uma nota Markdown do cofre (o que o H1 desenha "vivo"): `type: file` apontando
    /// para um `.md`.
    pub fn is_note_card(&self) -> bool {
        self.node_kind() == NodeKind::File
            && self.file.as_deref().is_some_and(|file| {
                Path::new(file)
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
            })
    }

    /// O cartão está recolhido (só o título)? Campo opcional nosso; sem ele, está expandido.
    pub fn is_collapsed(&self) -> bool {
        self.extra
            .get(COLLAPSED_KEY)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    /// Nome do arquivo (sem pasta e sem extensão) de um cartão de nota.
    pub fn file_title(&self) -> String {
        let path = self.file.as_deref().unwrap_or("");
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        match name.rsplit_once('.') {
            Some((stem, _)) if !stem.is_empty() => stem.to_owned(),
            _ => name.to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Edge {
    pub id: String,
    #[serde(default)]
    pub from_node: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_side: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_end: Option<String>,
    #[serde(default)]
    pub to_node: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_side: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_end: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Edge {
    /// Por padrão (spec) a ponta de origem é sem seta e a de destino tem seta.
    pub fn from_arrow(&self) -> bool {
        self.from_end.as_deref() == Some("arrow")
    }

    pub fn to_arrow(&self) -> bool {
        self.to_end.as_deref() != Some("none")
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn contains(&self, point: (f64, f64)) -> bool {
        point.0 >= self.x
            && point.0 <= self.x + self.w
            && point.1 >= self.y
            && point.1 <= self.y + self.h
    }

    pub fn contains_rect(&self, other: &Rect) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.x + other.w <= self.x + self.w
            && other.y + other.h <= self.y + self.h
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.w
            && other.x < self.x + self.w
            && self.y < other.y + other.h
            && other.y < self.y + self.h
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Canvas {
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

// ---------------------------------------------------------------------------------------------
// Ler e gravar
// ---------------------------------------------------------------------------------------------

impl Canvas {
    pub fn parse(text: &str) -> Result<Canvas, String> {
        if text.trim().is_empty() {
            return Ok(Canvas::default());
        }
        let canvas: Canvas = serde_json::from_str(text)
            .map_err(|error| format!("Este quadro não é um JSON Canvas válido: {error}"))?;
        Ok(canvas)
    }

    /// O texto do arquivo: um nó e uma aresta por linha, com tabulação, como o Obsidian grava.
    pub fn to_json(&self) -> String {
        fn lines<T: Serialize>(items: &[T]) -> String {
            if items.is_empty() {
                return "[\n\t]".into();
            }
            let body: Vec<String> = items
                .iter()
                .map(|item| {
                    format!(
                        "\t\t{}",
                        serde_json::to_string(item).expect("nó e aresta sempre serializam")
                    )
                })
                .collect();
            format!("[\n{}\n\t]", body.join(",\n"))
        }
        let mut parts = vec![
            format!("\t\"nodes\":{}", lines(&self.nodes)),
            format!("\t\"edges\":{}", lines(&self.edges)),
        ];
        for (key, value) in &self.extra {
            parts.push(format!(
                "\t{}:{}",
                serde_json::to_string(key).expect("chave de texto"),
                serde_json::to_string(value).expect("valor JSON")
            ));
        }
        format!("{{\n{}\n}}", parts.join(",\n"))
    }
}

// ---------------------------------------------------------------------------------------------
// Operações
// ---------------------------------------------------------------------------------------------

/// Alça de redimensionar: qual borda ou canto o ponteiro segura.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    North,
    South,
    East,
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
}

impl Handle {
    pub const ALL: [Handle; 8] = [
        Handle::North,
        Handle::South,
        Handle::East,
        Handle::West,
        Handle::NorthEast,
        Handle::NorthWest,
        Handle::SouthEast,
        Handle::SouthWest,
    ];

    fn moves(self) -> (bool, bool, bool, bool) {
        // (borda esquerda, borda direita, borda de cima, borda de baixo)
        match self {
            Handle::North => (false, false, true, false),
            Handle::South => (false, false, false, true),
            Handle::East => (false, true, false, false),
            Handle::West => (true, false, false, false),
            Handle::NorthEast => (false, true, true, false),
            Handle::NorthWest => (true, false, true, false),
            Handle::SouthEast => (false, true, false, true),
            Handle::SouthWest => (true, false, false, true),
        }
    }

    /// Posição da alça no retângulo (0..1 em cada eixo).
    pub fn position(self) -> (f64, f64) {
        let (left, right, top, bottom) = self.moves();
        let x = if left {
            0.0
        } else if right {
            1.0
        } else {
            0.5
        };
        let y = if top {
            0.0
        } else if bottom {
            1.0
        } else {
            0.5
        };
        (x, y)
    }
}

/// O retângulo `original` depois de arrastar a alça por `(dx, dy)`, sem passar do tamanho mínimo
/// (a borda oposta fica parada).
pub fn resized(original: Rect, handle: Handle, dx: f64, dy: f64) -> Rect {
    let (left, right, top, bottom) = handle.moves();
    let mut rect = original;
    if left {
        let edge = (original.x + dx).min(original.x + original.w - MIN_SIZE.0);
        rect.w = original.x + original.w - edge;
        rect.x = edge;
    }
    if right {
        rect.w = (original.w + dx).max(MIN_SIZE.0);
    }
    if top {
        let edge = (original.y + dy).min(original.y + original.h - MIN_SIZE.1);
        rect.h = original.y + original.h - edge;
        rect.y = edge;
    }
    if bottom {
        rect.h = (original.h + dy).max(MIN_SIZE.1);
    }
    rect
}

impl Canvas {
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|node| node.id == id)
    }

    pub fn node_mut(&mut self, id: &str) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|node| node.id == id)
    }

    pub fn edge(&self, id: &str) -> Option<&Edge> {
        self.edges.iter().find(|edge| edge.id == id)
    }

    pub fn edge_mut(&mut self, id: &str) -> Option<&mut Edge> {
        self.edges.iter_mut().find(|edge| edge.id == id)
    }

    /// Um id de 16 dígitos hexadecimais que ainda não existe no quadro (o formato do Obsidian).
    pub fn fresh_id(&self) -> String {
        loop {
            let id = format!("{:016x}", fastrand::u64(..));
            if self.node(&id).is_none() && self.edge(&id).is_none() {
                return id;
            }
        }
    }

    fn push_node(&mut self, mut node: Node) -> String {
        node.id = self.fresh_id();
        let id = node.id.clone();
        self.nodes.push(node);
        id
    }

    /// Cartão de texto com o canto de cima à esquerda em `(x, y)`.
    pub fn add_text(&mut self, x: f64, y: f64) -> String {
        let mut node = Node::blank(String::new(), "text", x, y, TEXT_SIZE);
        node.text = Some(String::new());
        self.push_node(node)
    }

    /// Cartão de nota (ou de qualquer arquivo do cofre); `path` é relativo à raiz do cofre,
    /// com barras normais.
    pub fn add_file(&mut self, path: &str, x: f64, y: f64) -> String {
        let mut node = Node::blank(String::new(), "file", x, y, BOX_SIZE);
        node.file = Some(path.replace('\\', "/"));
        self.push_node(node)
    }

    /// Se o quadro já tem um cartão para este arquivo (`/` e `\` valem igual).
    pub fn has_file(&self, path: &str) -> bool {
        let path = path.replace('\\', "/");
        self.nodes.iter().any(|node| {
            node.file
                .as_deref()
                .is_some_and(|file| file.replace('\\', "/") == path)
        })
    }

    /// Os caminhos dos cartões de arquivo do quadro, com barras normais.
    pub fn file_paths(&self) -> Vec<String> {
        self.nodes
            .iter()
            .filter_map(|node| node.file.as_deref())
            .map(|file| file.replace('\\', "/"))
            .collect()
    }

    /// Troca um cartão de texto por um cartão de arquivo `path` (H2b: "Transformar em nota"),
    /// na mesma posição e no mesmo tamanho, com o mesmo id (as ligações continuam valendo) e a
    /// mesma cor. O texto sai do quadro: quem chama já o gravou na nota. `false` se o nó não é
    /// um cartão de texto.
    pub fn text_to_file(&mut self, id: &str, path: &str) -> bool {
        let Some(node) = self.node_mut(id) else {
            return false;
        };
        if node.node_kind() != NodeKind::Text {
            return false;
        }
        node.kind = "file".to_owned();
        node.text = None;
        node.file = Some(path.replace('\\', "/"));
        true
    }

    /// Põe vários arquivos no quadro a partir de `(x, y)` (o primeiro com o centro ali), em
    /// grade de três colunas, e devolve os ids. A mesma nota pode entrar mais de uma vez.
    pub fn add_files_at(&mut self, paths: &[String], x: f64, y: f64) -> Vec<String> {
        const COLUMNS: usize = 3;
        const GAP: f64 = 24.0;
        let (width, height) = BOX_SIZE;
        paths
            .iter()
            .enumerate()
            .map(|(index, path)| {
                let column = (index % COLUMNS) as f64;
                let row = (index / COLUMNS) as f64;
                self.add_file(
                    path,
                    (x - width / 2.0 + column * (width + GAP)).round(),
                    (y - height / 2.0 + row * (height + GAP)).round(),
                )
            })
            .collect()
    }

    /// Recolhe (só o título) ou expande um cartão de nota. Recolher guarda a altura num campo
    /// nosso e expandir a devolve. `false` se o nó não é cartão de nota ou já estava assim.
    pub fn set_collapsed(&mut self, id: &str, collapsed: bool) -> bool {
        let Some(node) = self.node_mut(id) else {
            return false;
        };
        if !node.is_note_card() || node.is_collapsed() == collapsed {
            return false;
        }
        if collapsed {
            node.extra
                .insert(EXPANDED_HEIGHT_KEY.to_owned(), json_number(node.height));
            node.extra
                .insert(COLLAPSED_KEY.to_owned(), Value::Bool(true));
            node.height = NOTE_HEADER;
        } else {
            let height = node
                .extra
                .remove(EXPANDED_HEIGHT_KEY)
                .and_then(|value| value.as_f64())
                .filter(|height| *height >= MIN_SIZE.1)
                .unwrap_or(BOX_SIZE.1);
            node.extra.remove(COLLAPSED_KEY);
            node.height = height;
        }
        true
    }

    pub fn toggle_collapsed(&mut self, id: &str) -> bool {
        let Some(node) = self.node(id) else {
            return false;
        };
        let collapsed = node.is_collapsed();
        self.set_collapsed(id, !collapsed)
    }

    pub fn add_link(&mut self, url: &str, x: f64, y: f64) -> String {
        let mut node = Node::blank(String::new(), "link", x, y, BOX_SIZE);
        node.url = Some(url.trim().to_owned());
        self.push_node(node)
    }

    pub fn add_group(&mut self, label: &str, x: f64, y: f64) -> String {
        let mut node = Node::blank(String::new(), "group", x, y, BOX_SIZE);
        node.label = Some(label.to_owned());
        // Grupo vai para o começo: é desenhado e clicado por baixo dos demais cartões.
        node.id = self.fresh_id();
        let id = node.id.clone();
        self.nodes.insert(0, node);
        id
    }

    pub fn set_text(&mut self, id: &str, text: &str) -> bool {
        match self.node_mut(id) {
            Some(node) if node.text.as_deref() != Some(text) => {
                node.text = Some(text.to_owned());
                true
            }
            _ => false,
        }
    }

    pub fn set_group_label(&mut self, id: &str, label: &str) -> bool {
        match self.node_mut(id) {
            Some(node) if node.node_kind() == NodeKind::Group => {
                let label = (!label.is_empty()).then(|| label.to_owned());
                if node.label == label {
                    return false;
                }
                node.label = label;
                true
            }
            _ => false,
        }
    }

    pub fn set_edge_label(&mut self, id: &str, label: &str) -> bool {
        match self.edge_mut(id) {
            Some(edge) => {
                let label = (!label.is_empty()).then(|| label.to_owned());
                if edge.label == label {
                    return false;
                }
                edge.label = label;
                true
            }
            None => false,
        }
    }

    /// Cor de um nó ou aresta: `"1"`..`"6"` ou `#rrggbb`; `None` tira a cor.
    pub fn set_color(&mut self, id: &str, color: Option<&str>) -> bool {
        let color = color.map(str::to_owned);
        if let Some(node) = self.node_mut(id) {
            let changed = node.color != color;
            node.color = color;
            return changed;
        }
        if let Some(edge) = self.edge_mut(id) {
            let changed = edge.color != color;
            edge.color = color;
            return changed;
        }
        false
    }

    /// Liga ou desliga a seta de uma ponta da aresta.
    pub fn set_arrow(&mut self, id: &str, at_start: bool, arrow: bool) -> bool {
        let Some(edge) = self.edge_mut(id) else {
            return false;
        };
        let value = Some(if arrow { "arrow" } else { "none" }.to_owned());
        let slot = if at_start {
            &mut edge.from_end
        } else {
            &mut edge.to_end
        };
        // O padrão da spec (origem sem seta, destino com seta) não precisa ser escrito.
        let default = if at_start { "none" } else { "arrow" };
        let new = (value.as_deref() != Some(default))
            .then_some(value)
            .flatten();
        let changed = *slot != new;
        *slot = new;
        changed
    }

    /// Os nós dentro dos grupos escolhidos, sem repetir: mover um grupo leva o que está nele.
    pub fn with_group_members(&self, ids: &[String]) -> Vec<String> {
        let mut all: Vec<String> = Vec::new();
        for id in ids {
            let Some(node) = self.node(id) else { continue };
            if !all.contains(id) {
                all.push(id.clone());
            }
            if node.node_kind() != NodeKind::Group {
                continue;
            }
            let area = node.rect();
            for other in &self.nodes {
                if other.id != node.id
                    && area.contains_rect(&other.rect())
                    && !all.contains(&other.id)
                {
                    all.push(other.id.clone());
                }
            }
        }
        all
    }

    /// Apaga nós e as arestas que tocavam neles. Devolve quantos nós saíram.
    pub fn remove_nodes(&mut self, ids: &[String]) -> usize {
        let before = self.nodes.len();
        self.nodes.retain(|node| !ids.contains(&node.id));
        self.edges
            .retain(|edge| !ids.contains(&edge.from_node) && !ids.contains(&edge.to_node));
        before - self.nodes.len()
    }

    pub fn remove_edge(&mut self, id: &str) -> bool {
        let before = self.edges.len();
        self.edges.retain(|edge| edge.id != id);
        before != self.edges.len()
    }

    /// Liga dois nós. Sem lado dado, escolhe os lados que se olham. `None` se faltar um nó ou
    /// se forem o mesmo.
    pub fn connect(
        &mut self,
        from: &str,
        to: &str,
        from_side: Option<Side>,
        to_side: Option<Side>,
    ) -> Option<String> {
        if from == to {
            return None;
        }
        let (auto_from, auto_to) = facing_sides(&self.node(from)?.rect(), &self.node(to)?.rect());
        let id = self.fresh_id();
        self.edges.push(Edge {
            id: id.clone(),
            from_node: from.to_owned(),
            from_side: Some(from_side.unwrap_or(auto_from).name().to_owned()),
            from_end: None,
            to_node: to.to_owned(),
            to_side: Some(to_side.unwrap_or(auto_to).name().to_owned()),
            to_end: None,
            color: None,
            label: None,
            extra: Map::new(),
        });
        Some(id)
    }

    /// Caixa que envolve todos os nós, ou `None` num quadro sem nós.
    pub fn bounds(&self) -> Option<Rect> {
        let mut nodes = self.nodes.iter();
        let first = nodes.next()?.rect();
        let (mut left, mut top) = (first.x, first.y);
        let (mut right, mut bottom) = (first.x + first.w, first.y + first.h);
        for node in nodes {
            let rect = node.rect();
            left = left.min(rect.x);
            top = top.min(rect.y);
            right = right.max(rect.x + rect.w);
            bottom = bottom.max(rect.y + rect.h);
        }
        Some(Rect {
            x: left,
            y: top,
            w: right - left,
            h: bottom - top,
        })
    }

    /// O cartão de cima no ponto. Grupos só respondem na faixa do rótulo e na borda, para o
    /// miolo vazio de um grupo não "engolir" o clique de fundo.
    pub fn node_at(&self, point: (f64, f64), border: f64) -> Option<&Node> {
        self.nodes.iter().rev().find(|node| {
            let rect = node.rect();
            if node.node_kind() != NodeKind::Group {
                return rect.contains(point);
            }
            let inner = Rect {
                x: rect.x + border,
                y: rect.y + GROUP_LABEL_HEIGHT.max(border),
                w: rect.w - 2.0 * border,
                h: rect.h - GROUP_LABEL_HEIGHT.max(border) - border,
            };
            rect.contains(point) && !(inner.w > 0.0 && inner.h > 0.0 && inner.contains(point))
        })
    }

    /// A aresta mais perto do ponto, se estiver a menos de `tolerance` unidades.
    pub fn edge_at(&self, point: (f64, f64), tolerance: f64) -> Option<&Edge> {
        self.edges
            .iter()
            .filter_map(|edge| {
                let curve = self.curve(edge)?;
                Some((edge, curve.distance_to(point)))
            })
            .filter(|(_, distance)| *distance <= tolerance)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(edge, _)| edge)
    }

    /// Os lados efetivos de uma aresta (os que faltam no arquivo saem da posição dos nós).
    pub fn edge_sides(&self, edge: &Edge) -> Option<(Side, Side)> {
        let from = self.node(&edge.from_node)?;
        let to = self.node(&edge.to_node)?;
        let (auto_from, auto_to) = facing_sides(&from.rect(), &to.rect());
        Some((
            edge.from_side
                .as_deref()
                .and_then(Side::parse)
                .unwrap_or(auto_from),
            edge.to_side
                .as_deref()
                .and_then(Side::parse)
                .unwrap_or(auto_to),
        ))
    }

    /// A curva da aresta, ou `None` se ela aponta para um nó que não existe.
    pub fn curve(&self, edge: &Edge) -> Option<Curve> {
        let (from_side, to_side) = self.edge_sides(edge)?;
        let start = self.node(&edge.from_node)?.anchor(from_side);
        let end = self.node(&edge.to_node)?.anchor(to_side);
        Some(Curve::between(start, from_side, end, to_side))
    }
}

/// Altura da faixa do rótulo de um grupo, em unidades do quadro.
pub const GROUP_LABEL_HEIGHT: f64 = 28.0;

/// Os lados que se olham: o de `a` voltado para `b` e o de `b` voltado para `a`.
pub fn facing_sides(a: &Rect, b: &Rect) -> (Side, Side) {
    let dx = (b.x + b.w / 2.0) - (a.x + a.w / 2.0);
    let dy = (b.y + b.h / 2.0) - (a.y + a.h / 2.0);
    if dx.abs() / (a.w + b.w).max(1.0) >= dy.abs() / (a.h + b.h).max(1.0) {
        if dx >= 0.0 {
            (Side::Right, Side::Left)
        } else {
            (Side::Left, Side::Right)
        }
    } else if dy >= 0.0 {
        (Side::Bottom, Side::Top)
    } else {
        (Side::Top, Side::Bottom)
    }
}

/// Curva de Bézier cúbica de uma aresta: sai do lado de origem, entra no lado de destino.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Curve {
    pub points: [(f64, f64); 4],
}

impl Curve {
    pub fn between(start: (f64, f64), from: Side, end: (f64, f64), to: Side) -> Curve {
        let distance = ((end.0 - start.0).hypot(end.1 - start.1) * 0.5).clamp(30.0, 200.0);
        let (fx, fy) = from.normal();
        let (tx, ty) = to.normal();
        Curve {
            points: [
                start,
                (start.0 + fx * distance, start.1 + fy * distance),
                (end.0 + tx * distance, end.1 + ty * distance),
                end,
            ],
        }
    }

    pub fn at(&self, t: f64) -> (f64, f64) {
        let [p0, p1, p2, p3] = self.points;
        let u = 1.0 - t;
        let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
        (
            a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0,
            a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1,
        )
    }

    /// Direção (unitária) da curva em `t`.
    pub fn tangent(&self, t: f64) -> (f64, f64) {
        let [p0, p1, p2, p3] = self.points;
        let u = 1.0 - t;
        let (a, b, c) = (3.0 * u * u, 6.0 * u * t, 3.0 * t * t);
        let dx = a * (p1.0 - p0.0) + b * (p2.0 - p1.0) + c * (p3.0 - p2.0);
        let dy = a * (p1.1 - p0.1) + b * (p2.1 - p1.1) + c * (p3.1 - p2.1);
        let length = dx.hypot(dy);
        if length < 1e-9 {
            (1.0, 0.0)
        } else {
            (dx / length, dy / length)
        }
    }

    /// A distância do ponto à curva, por amostragem (vale para clicar numa aresta).
    pub fn distance_to(&self, point: (f64, f64)) -> f64 {
        const STEPS: usize = 32;
        let mut best = f64::INFINITY;
        let mut previous = self.at(0.0);
        for step in 1..=STEPS {
            let next = self.at(step as f64 / STEPS as f64);
            best = best.min(segment_distance(point, previous, next));
            previous = next;
        }
        best
    }
}

fn segment_distance(point: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let length_squared = abx * abx + aby * aby;
    let t = if length_squared < 1e-12 {
        0.0
    } else {
        (((point.0 - a.0) * abx + (point.1 - a.1) * aby) / length_squared).clamp(0.0, 1.0)
    };
    (point.0 - (a.0 + t * abx)).hypot(point.1 - (a.1 + t * aby))
}

// ---------------------------------------------------------------------------------------------
// Cor
// ---------------------------------------------------------------------------------------------

/// As seis cores predefinidas do JSON Canvas (`"1"`..`"6"`: vermelho, laranja, amarelo, verde,
/// ciano e roxo) ou um `#rgb`/`#rrggbb`. Texto que não é cor dá `None` (o nó fica sem cor).
pub fn color_rgb(color: &str) -> Option<[u8; 3]> {
    match color {
        "1" => Some([251, 70, 76]),
        "2" => Some([233, 151, 63]),
        "3" => Some([224, 190, 60]),
        "4" => Some([68, 207, 110]),
        "5" => Some([83, 223, 221]),
        "6" => Some([168, 130, 255]),
        hex => {
            let digits = hex.strip_prefix('#')?;
            if !digits.is_ascii() {
                return None;
            }
            let value = |text: &str| u8::from_str_radix(text, 16).ok();
            match digits.len() {
                6 => Some([
                    value(&digits[0..2])?,
                    value(&digits[2..4])?,
                    value(&digits[4..6])?,
                ]),
                3 => {
                    let one = |index: usize| value(&digits[index..=index]).map(|part| part * 17);
                    Some([one(0)?, one(1)?, one(2)?])
                }
                _ => None,
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Markdown simples dos cartões de texto
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Paragraph,
    Heading(u8),
    Bullet,
    Quote,
    Rule,
    Blank,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    pub spans: Vec<Span>,
}

/// Divide o texto de um cartão em blocos (título, item, citação, parágrafo) com negrito,
/// itálico e código em linha. Só o que um cartão precisa; o resto aparece como texto.
pub fn markdown_blocks(text: &str) -> Vec<Block> {
    text.lines().map(block_of).collect()
}

fn block_of(line: &str) -> Block {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return Block {
            kind: BlockKind::Blank,
            spans: Vec::new(),
        };
    }
    let hashes = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
        return Block {
            kind: BlockKind::Heading(hashes as u8),
            spans: inline_spans(trimmed[hashes..].trim()),
        };
    }
    if matches!(trimmed, "---" | "***" | "___") {
        return Block {
            kind: BlockKind::Rule,
            spans: Vec::new(),
        };
    }
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(marker) {
            return Block {
                kind: BlockKind::Bullet,
                spans: inline_spans(rest),
            };
        }
    }
    if let Some(rest) = trimmed.strip_prefix("> ") {
        return Block {
            kind: BlockKind::Quote,
            spans: inline_spans(rest),
        };
    }
    Block {
        kind: BlockKind::Paragraph,
        spans: inline_spans(trimmed),
    }
}

fn inline_spans(line: &str) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    let (mut bold, mut italic, mut code) = (false, false, false);
    let mut current = String::new();
    let flush = |spans: &mut Vec<Span>, current: &mut String, bold, italic, code| {
        if !current.is_empty() {
            spans.push(Span {
                text: std::mem::take(current),
                bold,
                italic,
                code,
            });
        }
    };
    let mut rest = line;
    while let Some(character) = rest.chars().next() {
        if code {
            if character == '`' {
                flush(&mut spans, &mut current, bold, italic, true);
                code = false;
            } else {
                current.push(character);
            }
            rest = &rest[character.len_utf8()..];
            continue;
        }
        if let Some(after) = rest.strip_prefix("**")
            && (bold || after.contains("**"))
        {
            flush(&mut spans, &mut current, bold, italic, false);
            bold = !bold;
            rest = after;
            continue;
        }
        if character == '`' && rest[1..].contains('`') {
            flush(&mut spans, &mut current, bold, italic, false);
            code = true;
            rest = &rest[1..];
            continue;
        }
        if (character == '*' || character == '_')
            && !rest.starts_with("**")
            && (italic || rest[1..].contains(character))
            && (character == '*'
                || italic
                || current.chars().last().is_none_or(|c| !c.is_alphanumeric()))
        {
            flush(&mut spans, &mut current, bold, italic, false);
            italic = !italic;
            rest = &rest[1..];
            continue;
        }
        if let Some(after) = rest.strip_prefix("[[")
            && let Some(end) = after.find("]]")
        {
            let inside = &after[..end];
            let shown = inside.rsplit_once('|').map_or(inside, |(_, alias)| alias);
            current.push_str(shown);
            rest = &after[end + 2..];
            continue;
        }
        if character == '['
            && let Some(close) = rest.find("](")
            && let Some(end) = rest[close..].find(')')
        {
            current.push_str(&rest[1..close]);
            rest = &rest[close + end + 1..];
            continue;
        }
        current.push(character);
        rest = &rest[character.len_utf8()..];
    }
    flush(&mut spans, &mut current, bold, italic, code);
    spans
}

// ---------------------------------------------------------------------------------------------
// Biblioteca de notas (H1)
// ---------------------------------------------------------------------------------------------

/// Uma linha da biblioteca: uma nota do cofre e se ela já tem cartão neste quadro.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryEntry {
    /// Caminho relativo à raiz do cofre, com barras normais.
    pub path: String,
    /// Nome da nota (sem pasta e sem extensão).
    pub title: String,
    pub in_board: bool,
}

/// As notas da biblioteca que casam com a busca: cada palavra da busca precisa aparecer no
/// caminho (sem diferença de maiúsculas nem de acento). As que o nome começa pela busca vêm
/// primeiro; o resto segue a ordem alfabética do caminho.
pub fn library_entries<'a>(
    notes: impl IntoIterator<Item = &'a str>,
    canvas: &Canvas,
    query: &str,
) -> Vec<LibraryEntry> {
    let terms: Vec<String> = query.split_whitespace().map(fold).collect();
    let in_board: std::collections::HashSet<String> = canvas.file_paths().into_iter().collect();
    let mut entries: Vec<(bool, String, LibraryEntry)> = notes
        .into_iter()
        .map(|path| path.replace('\\', "/"))
        .filter_map(|path| {
            let folded = fold(&path);
            if !terms.iter().all(|term| folded.contains(term.as_str())) {
                return None;
            }
            let name = path.rsplit('/').next().unwrap_or(&path);
            let title = match name.rsplit_once('.') {
                Some((stem, _)) if !stem.is_empty() => stem.to_owned(),
                _ => name.to_owned(),
            };
            let starts = terms
                .first()
                .is_some_and(|term| fold(&title).starts_with(term.as_str()));
            let entry = LibraryEntry {
                in_board: in_board.contains(&path),
                title,
                path,
            };
            Some((!starts, folded, entry))
        })
        .collect();
    entries.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    entries.into_iter().map(|(_, _, entry)| entry).collect()
}

/// Minúsculas e sem acento do português, para a busca da biblioteca.
fn fold(text: &str) -> String {
    text.chars()
        .map(
            |character| match character.to_lowercase().next().unwrap_or(character) {
                'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
                'é' | 'è' | 'ê' | 'ë' => 'e',
                'í' | 'ì' | 'î' | 'ï' => 'i',
                'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
                'ú' | 'ù' | 'û' | 'ü' => 'u',
                'ç' => 'c',
                other => other,
            },
        )
        .collect()
}

/// O texto de uma nota como o cartão mostra: sem o bloco de propriedades do topo e limitado
/// (a rolagem do cartão nunca monta um texto gigante de uma vez).
pub fn card_body(text: &str) -> &str {
    const MOST: usize = 20_000;
    let after_properties = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
        .and_then(|rest| rest.split_once("\n---"))
        .map(|(_, after)| after.trim_start_matches([' ', '\t']));
    let body = match after_properties {
        Some(after) => after.trim_start_matches(['\r', '\n']),
        None => text,
    };
    match body.char_indices().nth(MOST) {
        Some((end, _)) => &body[..end],
        None => body,
    }
}

// ---------------------------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Um quadro escrito à mão seguindo a especificação 1.0, com um campo desconhecido em cada
    /// nível (raiz, nó, aresta) e um tipo de nó que não existe na spec.
    pub(crate) const SAMPLE: &str = r##"{
	"nodes":[
		{"id":"g1","type":"group","x":-40,"y":-40,"width":560,"height":300,"label":"Fase 1","color":"4","backgroundStyle":"cover","gerado-por":"plugin x"},
		{"id":"n1","type":"text","x":0,"y":0,"width":250,"height":60,"text":"# Ideia\nUma **boa** ideia","color":"1","extra":{"a":[1,2,{"b":null}]}},
		{"id":"n2","type":"file","x":300,"y":0,"width":200,"height":120,"file":"Notas/Home.md","subpath":"#Resumo"},
		{"id":"n3","type":"link","x":0,"y":400,"width":300,"height":200,"url":"https://jsoncanvas.org"},
		{"id":"n4","type":"futuro","x":700,"y":0,"width":100,"height":100,"campo":"preservado"}
	],
	"edges":[
		{"id":"e1","fromNode":"n1","fromSide":"right","toNode":"n2","toSide":"left","label":"liga","color":"#ff8800"},
		{"id":"e2","fromNode":"n1","fromSide":"bottom","fromEnd":"arrow","toNode":"n3","toSide":"top","toEnd":"none","peso":3}
	],
	"metadata":{"version":"1.0-1.0","frontmatter":{}}
}"##;

    fn value(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn ida_e_volta_preserva_cada_campo_inclusive_os_desconhecidos() {
        let canvas = Canvas::parse(SAMPLE).unwrap();
        assert_eq!(canvas.nodes.len(), 5);
        assert_eq!(canvas.edges.len(), 2);
        let back = canvas.to_json();
        // O mesmo JSON, campo a campo (a ordem das chaves não conta, o valor sim).
        assert_eq!(value(&back), value(SAMPLE));
        // E ler de novo dá o mesmo quadro.
        assert_eq!(Canvas::parse(&back).unwrap(), canvas);
        // Os campos que o código não conhece estão mesmo guardados, não só "passaram".
        assert_eq!(canvas.extra["metadata"]["version"], "1.0-1.0");
        assert_eq!(canvas.node("g1").unwrap().extra["gerado-por"], "plugin x");
        assert_eq!(canvas.node("n4").unwrap().extra["campo"], "preservado");
        assert_eq!(canvas.edge("e2").unwrap().extra["peso"], 3);
    }

    #[test]
    fn a_gravacao_segue_o_estilo_do_obsidian_um_no_por_linha() {
        let text = Canvas::parse(SAMPLE).unwrap().to_json();
        assert!(
            text.starts_with("{\n\t\"nodes\":[\n\t\t{\"id\":\"g1\""),
            "{text}"
        );
        assert_eq!(
            text.lines()
                .filter(|line| line.contains("\"fromNode\""))
                .count(),
            2
        );
        assert!(text.ends_with("\n}"));
    }

    #[test]
    fn numeros_inteiros_continuam_inteiros() {
        let canvas = Canvas::parse(SAMPLE).unwrap();
        let text = canvas.to_json();
        assert!(text.contains("\"x\":-40,"), "{text}");
        assert!(!text.contains("-40.0"), "{text}");
        let decimal = Canvas::parse(
            r#"{"nodes":[{"id":"a","type":"text","x":1.5,"y":2,"width":3,"height":4}]}"#,
        )
        .unwrap();
        assert!(decimal.to_json().contains("\"x\":1.5"));
    }

    #[test]
    fn texto_vazio_e_quadro_vazio_e_lixo_e_erro() {
        assert_eq!(Canvas::parse("").unwrap(), Canvas::default());
        assert_eq!(Canvas::parse("  \n").unwrap(), Canvas::default());
        assert_eq!(Canvas::parse("{}").unwrap(), Canvas::default());
        assert!(Canvas::parse("{ nao é json").is_err());
        assert!(Canvas::parse("[1,2]").is_err());
        let novo = Canvas::parse(EMPTY).unwrap();
        assert!(novo.nodes.is_empty() && novo.edges.is_empty());
        assert_eq!(
            value(&Canvas::default().to_json()),
            value(r#"{"nodes":[],"edges":[]}"#)
        );
    }

    #[test]
    fn nos_novos_tem_id_unico_tipo_e_tamanho_certos() {
        let mut canvas = Canvas::default();
        let text = canvas.add_text(10.0, 20.0);
        let file = canvas.add_file("Pasta\\Nota.md", 0.0, 0.0);
        let link = canvas.add_link("  https://exemplo.com ", 0.0, 0.0);
        let group = canvas.add_group("Meu grupo", 0.0, 0.0);
        let ids = [&text, &file, &link, &group];
        for (index, id) in ids.iter().enumerate() {
            assert_eq!(id.len(), 16);
            assert!(ids[index + 1..].iter().all(|other| other != id));
        }
        assert_eq!(
            canvas.node(&text).unwrap().rect(),
            Rect {
                x: 10.0,
                y: 20.0,
                w: 250.0,
                h: 60.0
            }
        );
        assert_eq!(
            canvas.node(&file).unwrap().file.as_deref(),
            Some("Pasta/Nota.md")
        );
        assert_eq!(
            canvas.node(&link).unwrap().url.as_deref(),
            Some("https://exemplo.com")
        );
        assert_eq!(canvas.node(&group).unwrap().node_kind(), NodeKind::Group);
        // O grupo fica por baixo: primeiro da lista.
        assert_eq!(canvas.nodes[0].id, group);
        // Tudo que foi criado volta igual depois de gravar e ler.
        assert_eq!(Canvas::parse(&canvas.to_json()).unwrap(), canvas);
    }

    #[test]
    fn ligar_escolhe_os_lados_que_se_olham_e_recusa_o_proprio_no() {
        let mut canvas = Canvas::parse(SAMPLE).unwrap();
        let id = canvas.connect("n2", "n1", None, None).unwrap();
        let edge = canvas.edge(&id).unwrap();
        assert_eq!(edge.from_side.as_deref(), Some("left"));
        assert_eq!(edge.to_side.as_deref(), Some("right"));
        let vertical = canvas.connect("n1", "n3", None, None).unwrap();
        assert_eq!(
            canvas.edge(&vertical).unwrap().from_side.as_deref(),
            Some("bottom")
        );
        assert_eq!(
            canvas.edge(&vertical).unwrap().to_side.as_deref(),
            Some("top")
        );
        let forced = canvas
            .connect("n1", "n2", Some(Side::Top), Some(Side::Bottom))
            .unwrap();
        assert_eq!(
            canvas.edge(&forced).unwrap().from_side.as_deref(),
            Some("top")
        );
        assert!(canvas.connect("n1", "n1", None, None).is_none());
        assert!(canvas.connect("n1", "nada", None, None).is_none());
    }

    #[test]
    fn apagar_no_leva_as_arestas_dele() {
        let mut canvas = Canvas::parse(SAMPLE).unwrap();
        assert_eq!(canvas.remove_nodes(&["n3".to_owned()]), 1);
        assert!(canvas.node("n3").is_none());
        assert!(canvas.edge("e2").is_none());
        assert!(canvas.edge("e1").is_some());
        assert!(canvas.remove_edge("e1"));
        assert!(!canvas.remove_edge("e1"));
        assert_eq!(canvas.remove_nodes(&["inexistente".to_owned()]), 0);
    }

    #[test]
    fn mover_um_grupo_leva_o_que_esta_dentro() {
        let canvas = Canvas::parse(SAMPLE).unwrap();
        // g1 cobre n1 e n2 (n2 vai até x=500 < 520) mas não n3 nem n4.
        let moved = canvas.with_group_members(&["g1".to_owned()]);
        assert_eq!(moved, ["g1", "n1", "n2"]);
        // Escolher o grupo e um filho junto não repete o filho.
        let both = canvas.with_group_members(&["n1".to_owned(), "g1".to_owned()]);
        assert_eq!(both.iter().filter(|id| *id == "n1").count(), 1);
    }

    #[test]
    fn redimensionar_respeita_o_tamanho_minimo_e_a_borda_oposta() {
        let original = Rect {
            x: 100.0,
            y: 100.0,
            w: 200.0,
            h: 100.0,
        };
        let east = resized(original, Handle::East, 50.0, 999.0);
        assert_eq!(
            east,
            Rect {
                x: 100.0,
                y: 100.0,
                w: 250.0,
                h: 100.0
            }
        );
        let west = resized(original, Handle::West, 30.0, 0.0);
        assert_eq!(
            west,
            Rect {
                x: 130.0,
                y: 100.0,
                w: 170.0,
                h: 100.0
            }
        );
        // Esmagar passa do mínimo: a largura trava e a borda direita não anda.
        let squash = resized(original, Handle::West, 5000.0, 0.0);
        assert_eq!(squash.w, MIN_SIZE.0);
        assert_eq!(squash.x + squash.w, 300.0);
        let corner = resized(original, Handle::NorthWest, -10.0, -20.0);
        assert_eq!(
            corner,
            Rect {
                x: 90.0,
                y: 80.0,
                w: 210.0,
                h: 120.0
            }
        );
        let small = resized(original, Handle::SouthEast, -9999.0, -9999.0);
        assert_eq!((small.w, small.h), MIN_SIZE);
    }

    #[test]
    fn clique_acha_o_cartao_de_cima_e_o_miolo_do_grupo_deixa_passar() {
        let canvas = Canvas::parse(SAMPLE).unwrap();
        // Em cima de n1 (que está dentro do grupo) vale n1.
        assert_eq!(canvas.node_at((10.0, 10.0), 8.0).unwrap().id, "n1");
        // No miolo vazio do grupo, nada.
        assert!(canvas.node_at((200.0, 200.0), 8.0).is_none());
        // Na faixa do rótulo do grupo, o grupo.
        assert_eq!(canvas.node_at((100.0, -30.0), 8.0).unwrap().id, "g1");
        // Na borda do grupo, o grupo.
        assert_eq!(canvas.node_at((-36.0, 200.0), 8.0).unwrap().id, "g1");
        assert!(canvas.node_at((5000.0, 5000.0), 8.0).is_none());
    }

    #[test]
    fn a_curva_vai_de_um_lado_ao_outro_e_o_clique_na_aresta_acerta() {
        let canvas = Canvas::parse(SAMPLE).unwrap();
        let edge = canvas.edge("e1").unwrap();
        let curve = canvas.curve(edge).unwrap();
        assert_eq!(curve.points[0], (250.0, 30.0)); // lado direito de n1
        assert_eq!(curve.points[3], (300.0, 60.0)); // lado esquerdo de n2
        assert_eq!(curve.at(0.0), curve.points[0]);
        assert_eq!(curve.at(1.0), curve.points[3]);
        let middle = curve.at(0.5);
        assert!(canvas.edge_at(middle, 4.0).is_some());
        assert!(canvas.edge_at((middle.0, middle.1 + 300.0), 4.0).is_none());
        // A seta chega pelo lado esquerdo de n2: a tangente final aponta para a direita.
        let (tx, _) = curve.tangent(1.0);
        assert!(tx > 0.5);
        // Aresta para nó que não existe é ignorada, não derruba nada.
        let mut broken = canvas.clone();
        broken.edges[0].to_node = "fantasma".into();
        assert!(broken.curve(&broken.edges[0]).is_none());
        assert_eq!(
            broken.to_json().matches("fantasma").count(),
            1,
            "o dado continua no arquivo"
        );
    }

    #[test]
    fn setas_seguem_o_padrao_da_spec_e_so_escrevem_o_que_difere() {
        let mut canvas = Canvas::parse(SAMPLE).unwrap();
        assert!(!canvas.edge("e1").unwrap().from_arrow());
        assert!(canvas.edge("e1").unwrap().to_arrow());
        assert!(canvas.edge("e2").unwrap().from_arrow());
        assert!(!canvas.edge("e2").unwrap().to_arrow());
        assert!(canvas.set_arrow("e1", true, true));
        assert_eq!(
            canvas.edge("e1").unwrap().from_end.as_deref(),
            Some("arrow")
        );
        // Voltar ao padrão apaga o campo em vez de escrever "none".
        assert!(canvas.set_arrow("e1", true, false));
        assert_eq!(canvas.edge("e1").unwrap().from_end, None);
        assert!(canvas.set_arrow("e1", false, false));
        assert_eq!(canvas.edge("e1").unwrap().to_end.as_deref(), Some("none"));
        assert!(!canvas.set_arrow("fantasma", false, true));
    }

    #[test]
    fn edicoes_de_texto_rotulo_e_cor_dizem_se_mudaram_algo() {
        let mut canvas = Canvas::parse(SAMPLE).unwrap();
        assert!(canvas.set_text("n1", "outro"));
        assert!(!canvas.set_text("n1", "outro"));
        assert!(!canvas.set_text("nada", "x"));
        assert!(canvas.set_edge_label("e1", "novo"));
        assert!(canvas.set_edge_label("e1", ""));
        assert_eq!(canvas.edge("e1").unwrap().label, None);
        assert!(canvas.set_group_label("g1", "Fase 2"));
        assert!(!canvas.set_group_label("n1", "x"), "só grupo tem rótulo");
        assert!(canvas.set_color("n1", Some("#112233")));
        assert!(canvas.set_color("e2", Some("5")));
        assert!(canvas.set_color("n1", None));
        assert_eq!(canvas.node("n1").unwrap().color, None);
    }

    #[test]
    fn caixa_envolvente_do_quadro() {
        assert_eq!(Canvas::default().bounds(), None);
        let canvas = Canvas::parse(SAMPLE).unwrap();
        let bounds = canvas.bounds().unwrap();
        assert_eq!((bounds.x, bounds.y), (-40.0, -40.0));
        assert_eq!((bounds.x + bounds.w, bounds.y + bounds.h), (800.0, 600.0));
    }

    #[test]
    fn cores_predefinidas_e_hexadecimais() {
        assert_eq!(color_rgb("1"), Some([251, 70, 76]));
        assert_eq!(color_rgb("#ff8800"), Some([255, 136, 0]));
        assert_eq!(color_rgb("#f80"), Some([255, 136, 0]));
        assert_eq!(color_rgb("#12"), None);
        assert_eq!(color_rgb("azul"), None);
        assert_eq!(color_rgb("#éééé"), None);
        assert_eq!(color_rgb("7"), None);
    }

    #[test]
    fn titulo_do_cartao_de_nota() {
        let canvas = Canvas::parse(SAMPLE).unwrap();
        assert_eq!(canvas.node("n2").unwrap().file_title(), "Home");
        let mut node = canvas.node("n2").unwrap().clone();
        node.file = Some("Pasta/.oculto".into());
        assert_eq!(node.file_title(), ".oculto");
    }

    fn plain(blocks: &[Block]) -> Vec<String> {
        blocks
            .iter()
            .map(|block| {
                block
                    .spans
                    .iter()
                    .map(|span| span.text.as_str())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn markdown_simples_titulos_itens_e_estilos() {
        let blocks = markdown_blocks(
            "# Título\n- item **forte** e *leve*\n> citação\n\ntexto com `código` e [[Nota|alias]]\n---",
        );
        assert_eq!(blocks[0].kind, BlockKind::Heading(1));
        assert_eq!(blocks[1].kind, BlockKind::Bullet);
        assert_eq!(blocks[2].kind, BlockKind::Quote);
        assert_eq!(blocks[3].kind, BlockKind::Blank);
        assert_eq!(blocks[4].kind, BlockKind::Paragraph);
        assert_eq!(blocks[5].kind, BlockKind::Rule);
        assert_eq!(
            plain(&blocks),
            [
                "Título",
                "item forte e leve",
                "citação",
                "",
                "texto com código e alias",
                ""
            ]
        );
        let item = &blocks[1].spans;
        assert!(item.iter().any(|span| span.bold && span.text == "forte"));
        assert!(item.iter().any(|span| span.italic && span.text == "leve"));
        assert!(
            blocks[4]
                .spans
                .iter()
                .any(|span| span.code && span.text == "código")
        );
    }

    #[test]
    fn marcador_sem_par_fica_como_texto() {
        let blocks = markdown_blocks("2 * 3 = 6\nsnake_case_nome\n**aberto\n`solto\n#sem espaço");
        assert_eq!(
            plain(&blocks),
            [
                "2 * 3 = 6",
                "snake_case_nome",
                "**aberto",
                "`solto",
                "#sem espaço"
            ]
        );
        assert!(
            blocks
                .iter()
                .all(|block| block.kind == BlockKind::Paragraph)
        );
    }

    #[test]
    fn links_em_markdown_mostram_so_o_texto() {
        let blocks = markdown_blocks("veja [o site](https://x.com/a) agora");
        assert_eq!(plain(&blocks), ["veja o site agora"]);
    }

    #[test]
    fn recolher_guarda_a_altura_num_campo_nosso_e_expandir_devolve() {
        let mut canvas = Canvas::parse(SAMPLE).unwrap();
        let before = canvas.node("n2").unwrap().rect();
        assert!(canvas.node("n2").unwrap().is_note_card());
        assert!(!canvas.node("n2").unwrap().is_collapsed());
        assert!(canvas.set_collapsed("n2", true));
        let node = canvas.node("n2").unwrap();
        assert!(node.is_collapsed());
        assert_eq!(node.height, NOTE_HEADER);
        assert_eq!((node.x, node.width), (before.x, before.w));
        // O arquivo continua JSON Canvas: campos extras com inteiros e o resto preservado.
        let text = canvas.to_json();
        assert!(text.contains("\"rustidianCollapsed\":true"), "{text}");
        assert!(text.contains("\"rustidianHeight\":120"), "{text}");
        assert!(text.contains("\"subpath\":\"#Resumo\""), "{text}");
        let again = Canvas::parse(&text).unwrap();
        assert!(again.node("n2").unwrap().is_collapsed());
        // Recolher de novo não muda nada; expandir devolve a altura e tira os campos.
        assert!(!canvas.set_collapsed("n2", true));
        assert!(canvas.set_collapsed("n2", false));
        assert_eq!(canvas.node("n2").unwrap().rect(), before);
        assert!(!canvas.to_json().contains("rustidian"));
        assert_eq!(value(&canvas.to_json()), value(SAMPLE));
    }

    #[test]
    fn so_cartao_de_nota_recolhe() {
        let mut canvas = Canvas::parse(SAMPLE).unwrap();
        for id in ["n1", "n3", "n4", "g1", "nao-existe"] {
            assert!(!canvas.set_collapsed(id, true), "{id}");
        }
        let image = canvas.add_file("fotos/a.png", 0.0, 0.0);
        assert!(!canvas.node(&image).unwrap().is_note_card());
        assert!(!canvas.set_collapsed(&image, true));
        // Um cartão recolhido sem a altura guardada volta ao tamanho padrão.
        let note = canvas.add_file("A.MD", 0.0, 0.0);
        assert!(canvas.node(&note).unwrap().is_note_card());
        canvas.set_collapsed(&note, true);
        canvas
            .node_mut(&note)
            .unwrap()
            .extra
            .remove(EXPANDED_HEIGHT_KEY);
        canvas.set_collapsed(&note, false);
        assert_eq!(canvas.node(&note).unwrap().height, BOX_SIZE.1);
    }

    #[test]
    fn alternar_recolhimento_vai_e_volta() {
        let mut canvas = Canvas::parse(SAMPLE).unwrap();
        assert!(canvas.toggle_collapsed("n2"));
        assert!(canvas.node("n2").unwrap().is_collapsed());
        assert!(canvas.toggle_collapsed("n2"));
        assert!(!canvas.node("n2").unwrap().is_collapsed());
        assert!(!canvas.toggle_collapsed("n1"));
    }

    #[test]
    fn varias_notas_entram_em_grade_com_a_primeira_no_ponto() {
        let mut canvas = Canvas::default();
        let paths: Vec<String> = (0..4).map(|n| format!("Pasta\\Nota {n}.md")).collect();
        let ids = canvas.add_files_at(&paths, 1000.0, 500.0);
        assert_eq!(ids.len(), 4);
        let first = canvas.node(&ids[0]).unwrap();
        assert_eq!(first.file.as_deref(), Some("Pasta/Nota 0.md"));
        assert_eq!(
            (first.x + first.width / 2.0, first.y + first.height / 2.0),
            (1000.0, 500.0)
        );
        // A quarta nota abre a segunda fileira, sem cobrir as outras.
        let rects: Vec<Rect> = ids
            .iter()
            .map(|id| canvas.node(id).unwrap().rect())
            .collect();
        for (index, rect) in rects.iter().enumerate() {
            for other in &rects[index + 1..] {
                assert!(!rect.intersects(other));
            }
        }
        assert!(canvas.has_file("Pasta/Nota 3.md"));
        assert!(canvas.has_file("Pasta\\Nota 3.md"));
        assert!(!canvas.has_file("Pasta/Nota 4.md"));
        assert_eq!(canvas.file_paths().len(), 4);
    }

    #[test]
    fn a_biblioteca_filtra_marca_e_ordena() {
        let mut canvas = Canvas::default();
        canvas.add_file("Projetos/Aurora.md", 0.0, 0.0);
        let notes = [
            "Reuniões/Reunião de segunda.md",
            "Projetos/Aurora.md",
            "Projetos\\Plano da Aurora.md",
            "Diário.md",
        ];
        let all = library_entries(notes, &canvas, "");
        assert_eq!(all.len(), 4);
        assert!(
            all.iter()
                .any(|entry| entry.path == "Projetos/Aurora.md" && entry.in_board)
        );
        assert_eq!(all.iter().filter(|entry| entry.in_board).count(), 1);
        // Sem acento e sem maiúsculas; o nome que começa pela busca vem antes.
        let found = library_entries(notes, &canvas, "aurora");
        let paths: Vec<&str> = found.iter().map(|entry| entry.path.as_str()).collect();
        assert_eq!(paths, ["Projetos/Aurora.md", "Projetos/Plano da Aurora.md"]);
        assert_eq!(found[0].title, "Aurora");
        let accent = library_entries(notes, &canvas, "reuniao seg");
        assert_eq!(accent.len(), 1);
        assert_eq!(accent[0].title, "Reunião de segunda");
        assert!(library_entries(notes, &canvas, "zebra").is_empty());
        assert_eq!(library_entries(notes, &canvas, "diario")[0].title, "Diário");
    }

    #[test]
    fn o_corpo_do_cartao_pula_as_propriedades_e_limita_o_tamanho() {
        assert_eq!(card_body("# T\ntexto"), "# T\ntexto");
        assert_eq!(card_body("---\ntags: [a]\n---\n# T\n"), "# T\n");
        assert_eq!(card_body("---\r\nx: 1\r\n---\r\ncorpo"), "corpo");
        // Sem fechar o bloco, o texto inteiro aparece (não some conteúdo).
        assert_eq!(card_body("---\nsem fim"), "---\nsem fim");
        let long = "ã".repeat(30_000);
        assert_eq!(card_body(&long).chars().count(), 20_000);
    }

    #[test]
    fn texto_vira_cartao_de_arquivo_na_mesma_posicao_tamanho_cor_e_ligacoes() {
        let mut canvas = Canvas::parse(
            r#"{"nodes":[
                {"id":"t","type":"text","x":40,"y":-20,"width":250,"height":90,"color":"3","text":"Ideia\nsolta","extra":1},
                {"id":"o","type":"text","x":400,"y":0,"width":100,"height":50,"text":"outro"}],
              "edges":[{"id":"e","fromNode":"t","toNode":"o"}]}"#,
        )
        .unwrap();
        assert!(canvas.text_to_file("t", "Notas\\Ideia.md"));
        let node = canvas.node("t").unwrap();
        assert_eq!(node.node_kind(), NodeKind::File);
        assert_eq!(node.file.as_deref(), Some("Notas/Ideia.md"));
        assert_eq!(node.text, None);
        assert_eq!(
            node.rect(),
            Rect {
                x: 40.0,
                y: -20.0,
                w: 250.0,
                h: 90.0
            }
        );
        assert_eq!(node.color.as_deref(), Some("3"));
        assert_eq!(node.extra.get("extra"), Some(&Value::from(1)));
        assert!(node.is_note_card());
        // A aresta continua ligada ao mesmo id; só cartão de texto se transforma.
        assert_eq!(canvas.edges[0].from_node, "t");
        assert!(!canvas.text_to_file("t", "Outra.md"));
        assert!(!canvas.text_to_file("nada", "Outra.md"));
        assert_eq!(Canvas::parse(&canvas.to_json()).unwrap(), canvas);
    }

    #[test]
    fn reconhece_a_extensao() {
        assert!(is_canvas(Path::new("a/Quadro.canvas")));
        assert!(is_canvas(Path::new("Q.CANVAS")));
        assert!(!is_canvas(Path::new("Q.md")));
        assert!(!is_canvas(Path::new("canvas")));
    }
}
