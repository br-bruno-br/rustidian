//! Tela dos quadros `.canvas` (R14): a lógica fica em `rustidian::canvas`; aqui só o desenho e o
//! ponteiro. O quadro vive no texto do documento (o JSON), então salvar, autosave e o portão do
//! sync são os mesmos de qualquer nota: toda mudança vira texto novo em `tab.document.text`
//! mais `tab.edited()`.
//!
//! Enquanto se arrasta, o quadro em memória muda a cada quadro de vídeo e o texto do documento
//! só é regravado quando o gesto termina (um desfazer por gesto, nada de JSON a 60 Hz).

use crate::{
    canvas_library::{self, Library},
    canvas_notes::{Content, Notes},
    theme,
};
use eframe::egui::{
    self, Align2, Color32, FontFamily, FontId, Key, Pos2, Sense, Stroke, StrokeKind, Vec2,
    epaint::{CubicBezierShape, text::LayoutJob},
    text::TextFormat,
};
use rustidian::{
    board_list::{self, ItemKind},
    canvas::{
        self, BlockKind, Canvas, GROUP_LABEL_HEIGHT, Handle, Node, NodeKind, Side, color_rgb,
    },
    graph_layout::Camera,
    tab::{Tab, View},
};
use std::{cell::RefCell, collections::HashMap, path::Path, sync::Arc};

/// Distância, em pixels de tela, para pegar uma alça, um ponto de ligação ou uma aresta.
const GRAB: f32 = 9.0;
const LINK_DOT_OFFSET: f32 = 16.0;
const UNDO_LIMIT: usize = 200;
/// Faixa de cima reservada à barra de ferramentas ao ajustar o quadro à tela.
const TOOLBAR_ROOM: f32 = 70.0;
const ZOOM_RANGE: (f32, f32) = (0.1, 3.0);

#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    Node(String),
    Edge(String),
}

#[derive(Clone, Debug)]
enum Edit {
    /// O texto de um cartão de texto.
    Text(String),
    /// O texto da nota de um cartão de nota (H1): escreve direto no `Document` da nota.
    Note(String),
    /// O rótulo de um grupo.
    Group(String),
    /// O rótulo de uma aresta.
    Edge(String),
}

struct Editing {
    what: Edit,
    buffer: String,
    /// Criado agora pelo duplo clique: se terminar vazio, o cartão some.
    fresh: bool,
    focused: bool,
}

enum Drag {
    Pan {
        start: [f32; 2],
    },
    Move {
        /// Posição de cada nó (e dos nós dentro de grupos) quando o gesto começou.
        origins: Vec<(String, f64, f64)>,
        total: (f64, f64),
    },
    Resize {
        id: String,
        handle: Handle,
        original: canvas::Rect,
        total: (f64, f64),
    },
    Link {
        from: String,
        side: Side,
        pointer: Pos2,
    },
}

enum Action {
    Color(Option<&'static str>),
    Delete,
    EditLabel,
    Arrow {
        start: bool,
        on: bool,
    },
    /// Abrir a nota do cartão inteira no outro painel.
    Aside,
    /// Recolher (só o título) ou expandir o cartão de nota.
    Collapse,
    /// Transformar o cartão de texto numa nota do cofre (H2b).
    ToNote,
}

/// A rolagem de um cartão de nota (H1): onde está e até onde vai (medido ao desenhar).
#[derive(Clone, Copy, Default)]
struct CardView {
    scroll: f32,
    max: f32,
}

pub struct CanvasState {
    canvas: Canvas,
    /// O texto do documento de onde `canvas` veio (ou que acabou de gravar).
    source: String,
    error: Option<String>,
    camera: Camera,
    fitted: bool,
    /// Cartão a centralizar e selecionar no próximo desenho (H2a: vindo do painel de links).
    focus: Option<String>,
    selected: Vec<String>,
    selected_edge: Option<String>,
    drag: Option<Drag>,
    editing: Option<Editing>,
    undo: Vec<String>,
    redo: Vec<String>,
    menu: Option<Target>,
    add_note: Option<String>,
    add_link: Option<String>,
    /// O retângulo da tela onde o quadro foi desenhado pela última vez.
    area: egui::Rect,
    /// A rolagem de cada cartão de nota, por id do nó.
    card_view: RefCell<HashMap<String, CardView>>,
    /// A biblioteca de notas, quando aberta.
    library: Option<Library>,
}

impl CanvasState {
    fn load(text: &str) -> Self {
        let (canvas, error) = match Canvas::parse(text) {
            Ok(canvas) => (canvas, None),
            Err(error) => (Canvas::default(), Some(error)),
        };
        Self {
            canvas,
            source: text.to_owned(),
            error,
            camera: Camera::default(),
            fitted: false,
            focus: None,
            selected: Vec::new(),
            selected_edge: None,
            drag: None,
            editing: None,
            undo: Vec::new(),
            redo: Vec::new(),
            menu: None,
            add_note: None,
            add_link: None,
            area: egui::Rect::NOTHING,
            card_view: RefCell::default(),
            library: None,
        }
    }

    /// O documento mudou por fora (sync, disco, modo código-fonte): lê de novo e mantém a
    /// seleção do que ainda existe.
    fn reload(&mut self, text: &str) {
        let camera = self.camera;
        let fitted = self.fitted;
        let focus = self.focus.take();
        let selected = std::mem::take(&mut self.selected);
        let card_view = std::mem::take(&mut self.card_view);
        let library = self.library.take();
        *self = Self::load(text);
        self.camera = camera;
        self.fitted = fitted;
        self.focus = focus;
        self.card_view = card_view;
        self.library = library;
        self.selected = selected
            .into_iter()
            .filter(|id| self.canvas.node(id).is_some())
            .collect();
    }

    #[cfg(test)]
    pub fn canvas(&self) -> &Canvas {
        &self.canvas
    }

    #[cfg(test)]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    #[cfg(test)]
    pub fn selected(&self) -> &[String] {
        &self.selected
    }

    #[cfg(test)]
    pub fn area_for_tests(&self) -> egui::Rect {
        self.area
    }

    #[cfg(test)]
    pub fn zoom(&self) -> f32 {
        self.camera.zoom
    }

    pub fn is_editing(&self) -> bool {
        self.editing.is_some()
    }

    /// O cartão de nota sob o ponteiro que tem mais texto do que cabe: a roda rola ele.
    fn scrollable_card_at(&self, pointer: Pos2) -> Option<String> {
        let world = self.to_world(pointer);
        let node = self.canvas.node_at(world, 0.0)?;
        if !node.is_note_card() || node.is_collapsed() {
            return None;
        }
        let view = self.card_view.borrow().get(&node.id).copied()?;
        (view.max > 0.5).then(|| node.id.clone())
    }

    /// Rola um cartão de nota; `wheel` é o movimento da roda (positivo = para cima).
    fn scroll_card(&self, id: &str, wheel: f32) {
        let mut views = self.card_view.borrow_mut();
        if let Some(view) = views.get_mut(id) {
            view.scroll = (view.scroll - wheel).clamp(0.0, view.max);
        }
    }

    /// O cartão de nota em edição, se for ele (para os testes de GUI).
    #[cfg(test)]
    pub fn editing_note(&self) -> Option<&str> {
        match self.editing.as_ref().map(|editing| &editing.what) {
            Some(Edit::Note(id)) => Some(id),
            _ => None,
        }
    }

    /// Quanto o cartão está rolado e até onde vai (para os testes de GUI).
    #[cfg(test)]
    pub fn card_scroll(&self, id: &str) -> Option<(f32, f32)> {
        self.card_view
            .borrow()
            .get(id)
            .map(|view| (view.scroll, view.max))
    }

    /// Onde ficam os botões do cabeçalho de um cartão de nota: recolher e abrir ao lado.
    #[cfg(test)]
    pub fn note_buttons(&self, id: &str) -> Option<(egui::Rect, egui::Rect)> {
        let rect = self.node_screen_rect(id)?;
        let buttons = header_buttons(rect, self.camera.zoom);
        Some((buttons.collapse, buttons.aside))
    }

    #[cfg(test)]
    pub fn library(&self) -> Option<&Library> {
        self.library.as_ref()
    }

    /// Os caminhos das notas em edição num cartão: o app mantém um `Document` aberto para elas.
    fn editing_note_path(&self) -> Option<std::path::PathBuf> {
        let Some(Edit::Note(id)) = self.editing.as_ref().map(|editing| &editing.what) else {
            return None;
        };
        self.canvas
            .node(id)
            .and_then(|node| node.file.as_deref())
            .map(std::path::PathBuf::from)
    }

    fn origin(&self) -> Pos2 {
        self.area.center() + Vec2::new(self.camera.pan[0], self.camera.pan[1])
    }

    fn to_screen(&self, point: (f64, f64)) -> Pos2 {
        let origin = self.origin();
        Pos2::new(
            origin.x + point.0 as f32 * self.camera.zoom,
            origin.y + point.1 as f32 * self.camera.zoom,
        )
    }

    /// Onde um ponto do quadro cai na tela (para os testes de GUI).
    #[cfg(test)]
    pub fn screen_of(&self, point: (f64, f64)) -> Pos2 {
        self.to_screen(point)
    }

    fn to_world(&self, point: Pos2) -> (f64, f64) {
        let origin = self.origin();
        (
            f64::from((point.x - origin.x) / self.camera.zoom),
            f64::from((point.y - origin.y) / self.camera.zoom),
        )
    }

    fn screen_rect(&self, rect: canvas::Rect) -> egui::Rect {
        egui::Rect::from_min_max(
            self.to_screen((rect.x, rect.y)),
            self.to_screen((rect.x + rect.w, rect.y + rect.h)),
        )
    }

    /// Onde um nó está na tela (para os testes de GUI e para o ponteiro).
    pub fn node_screen_rect(&self, id: &str) -> Option<egui::Rect> {
        self.canvas
            .node(id)
            .map(|node| self.screen_rect(node.rect()))
    }

    /// Centraliza a câmera no cartão `id` e o seleciona. `false` se o cartão não existe.
    fn center_on(&mut self, id: &str) -> bool {
        let Some(rect) = self.canvas.node(id).map(|node| node.rect()) else {
            return false;
        };
        self.camera.pan = [
            -((rect.x + rect.w / 2.0) as f32) * self.camera.zoom,
            -((rect.y + rect.h / 2.0) as f32) * self.camera.zoom + TOOLBAR_ROOM / 2.0,
        ];
        self.selected = vec![id.to_owned()];
        self.selected_edge = None;
        true
    }

    fn fit(&mut self) {
        if let Some(bounds) = self.canvas.bounds() {
            self.camera.fit(
                [
                    [bounds.x as f32, bounds.y as f32],
                    [(bounds.x + bounds.w) as f32, (bounds.y + bounds.h) as f32],
                ],
                [
                    self.area.width(),
                    (self.area.height() - TOOLBAR_ROOM).max(1.0),
                ],
            );
            self.camera.zoom = self.camera.zoom.min(1.0);
            self.camera.pan = [
                -((bounds.x + bounds.w / 2.0) as f32) * self.camera.zoom,
                -((bounds.y + bounds.h / 2.0) as f32) * self.camera.zoom + TOOLBAR_ROOM / 2.0,
            ];
        } else {
            self.camera = Camera::default();
        }
    }

    fn clear_selection(&mut self) {
        self.selected.clear();
        self.selected_edge = None;
    }

    fn select_only(&mut self, id: &str) {
        self.selected = vec![id.to_owned()];
        self.selected_edge = None;
    }

    /// Grava o quadro em memória no documento (um passo de desfazer). Sem mudança, não faz nada.
    fn commit(&mut self, tab: &mut Tab) -> bool {
        // Um gesto que acabou onde começou (ou um cartão novo que ficou vazio) não reescreve o
        // arquivo, nem muda a formatação dele.
        if Canvas::parse(&tab.document.text).is_ok_and(|current| current == self.canvas) {
            return false;
        }
        let text = self.canvas.to_json();
        self.undo
            .push(std::mem::replace(&mut tab.document.text, text.clone()));
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.source = text;
        tab.edited();
        true
    }

    fn step_history(&mut self, tab: &mut Tab, forward: bool) -> bool {
        let Some(text) = (if forward {
            self.redo.pop()
        } else {
            self.undo.pop()
        }) else {
            return false;
        };
        let current = std::mem::replace(&mut tab.document.text, text.clone());
        if forward {
            self.undo.push(current);
        } else {
            self.redo.push(current);
        }
        tab.edited();
        let (undo, redo) = (
            std::mem::take(&mut self.undo),
            std::mem::take(&mut self.redo),
        );
        self.reload(&text);
        self.undo = undo;
        self.redo = redo;
        true
    }

    /// Onde um cartão novo cabe: no centro da tela, deslocado se já houver um ali.
    fn free_spot(&self, size: (f64, f64)) -> (f64, f64) {
        let center = self.to_world(self.area.center());
        let (mut x, mut y) = (center.0 - size.0 / 2.0, center.1 - size.1 / 2.0);
        while self
            .canvas
            .nodes
            .iter()
            .any(|node| (node.x - x).abs() < 1.0 && (node.y - y).abs() < 1.0)
        {
            x += 24.0;
            y += 24.0;
        }
        (x.round(), y.round())
    }
}

#[derive(Default)]
pub struct Manager {
    states: HashMap<u64, CanvasState>,
}

#[derive(Default)]
pub struct Outcome {
    /// Um destino para o app abrir: nota do cofre ou endereço da web.
    pub follow: Option<String>,
    /// Uma nota para abrir inteira no outro painel (cria a divisão à direita se não houver).
    pub aside: Option<String>,
    /// Um aviso para o app mostrar (nota que não abriu para edição, por exemplo).
    pub error: Option<String>,
    /// "Transformar em nota" no menu de um cartão de texto (H2b): o app cria a nota e devolve o
    /// caminho por `Manager::convert_to_note`.
    pub to_note: Option<ToNote>,
    /// O botão "Exportar como nota" da Lista foi apertado (H2b).
    pub export_list: bool,
}

/// O pedido de transformar um cartão de texto em nota.
pub struct ToNote {
    pub node_id: String,
    pub text: String,
}

pub fn is_canvas_tab(tab: &Tab) -> bool {
    rustidian::canvas::is_canvas(&tab.document.path)
        && !tab.document.is_blank()
        && tab.view != View::Source
        && tab.view != View::Graph
}

impl Manager {
    #[cfg(test)]
    pub fn state(&self, tab_id: u64) -> Option<&CanvasState> {
        self.states.get(&tab_id)
    }

    /// Pede que o quadro da aba centralize e selecione o cartão `node_id` quando for desenhado.
    pub fn focar_no(&mut self, tab: &Tab, node_id: &str) {
        let state = self
            .states
            .entry(tab.id())
            .or_insert_with(|| CanvasState::load(&tab.document.text));
        state.focus = Some(node_id.to_owned());
    }

    /// Abre ou fecha a biblioteca de notas do quadro desta aba. `false` se a aba não tem quadro
    /// desenhado.
    pub fn toggle_library(&mut self, tab_id: u64) -> bool {
        let Some(state) = self.states.get_mut(&tab_id) else {
            return false;
        };
        state.library = match state.library.take() {
            Some(_) => None,
            None => Some(Library::opened()),
        };
        true
    }

    /// As notas que algum quadro está editando agora (o app não larga o `Document` delas).
    pub fn editing_notes(&self, live: impl Fn(u64) -> bool) -> Vec<std::path::PathBuf> {
        self.states
            .iter()
            .filter(|(id, _)| live(**id))
            .filter_map(|(_, state)| state.editing_note_path())
            .collect()
    }

    pub fn show(&mut self, ui: &mut egui::Ui, tab: &mut Tab, notes: &mut Notes) -> Outcome {
        let id = tab.id();
        let state = self
            .states
            .entry(id)
            .or_insert_with(|| CanvasState::load(&tab.document.text));
        if state.source != tab.document.text {
            state.reload(&tab.document.text);
        }
        let mut outcome = Outcome::default();
        if let Some(message) = state.error.clone() {
            show_error(ui, tab, &message);
            return outcome;
        }
        if tab.board_list {
            show_list(ui, state, tab, notes, &mut outcome);
        } else {
            show_canvas(ui, state, tab, notes, &mut outcome);
        }
        outcome
    }

    /// Troca o cartão de texto `node_id` pelo cartão vivo da nota `path` (que o app acabou de
    /// criar com o texto do cartão): mesma posição, tamanho, cor e ligações, e um passo de
    /// desfazer do quadro. `false` se o cartão já não é de texto.
    pub fn convert_to_note(&mut self, tab: &mut Tab, node_id: &str, path: &str) -> bool {
        let Some(state) = self.states.get_mut(&tab.id()) else {
            return false;
        };
        if !state.canvas.text_to_file(node_id, path) {
            return false;
        }
        state.select_only(node_id);
        state.commit(tab)
    }
}

fn show_error(ui: &mut egui::Ui, tab: &mut Tab, message: &str) {
    ui.add_space(theme::space::XXL + theme::space::L);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new("Não foi possível abrir este quadro").strong());
        ui.add_space(theme::space::XS);
        ui.label(message);
        ui.label("O arquivo não foi alterado.");
        ui.add_space(theme::space::S);
        if ui.button("Abrir como código-fonte").clicked() {
            tab.set_view(View::Source);
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Lista (H2b)
// ---------------------------------------------------------------------------------------------

/// O começo do texto de uma nota para o trecho da Lista (`None` se ela não existe).
fn note_head(notes: &Notes, path: &str) -> Option<String> {
    match notes.content(Path::new(path)) {
        Content::Text(text) => Some(text.chars().take(2000).collect()),
        Content::Missing => None,
    }
}

/// A visão Lista do quadro da aba como texto Markdown (para "Exportar como nota").
pub fn list_markdown(tab: &Tab, notes: &Notes) -> Result<String, String> {
    let canvas = Canvas::parse(&tab.document.text)?;
    let list = board_list::build(&canvas, |path| note_head(notes, path));
    Ok(board_list::to_markdown(
        &rustidian::board_note::board_title(&tab.document.path),
        &list,
    ))
}

/// O quadro lido como uma nota: grupos viram títulos, cartões viram itens, ligações viram linhas.
/// Não é arquivo nenhum: sai do quadro (e do começo das notas) a cada desenho, então nunca
/// diverge dele. Clicar num item volta ao quadro com o cartão centralizado e selecionado.
fn show_list(
    ui: &mut egui::Ui,
    state: &mut CanvasState,
    tab: &mut Tab,
    notes: &Notes,
    outcome: &mut Outcome,
) {
    let list = board_list::build(&state.canvas, |path| note_head(notes, path));
    let weak = ui.visuals().weak_text_color();
    let accent = theme::accent(ui);
    let mut go_to: Option<String> = None;
    let mut back = false;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(theme::space::XL);
                ui.vertical(|ui| {
                    ui.set_max_width((ui.available_width() - 24.0).clamp(200.0, 780.0));
                    ui.add_space(theme::space::L);
                    // O botão fica no topo, à direita, longe do título; o modo se escolhe no
                    // seletor do cabeçalho.
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Lista do quadro").size(20.0).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new("Exportar como nota (cópia)")
                                        .frame(true)
                                        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                                        .min_size(egui::vec2(0.0, theme::space::CONTROL)),
                                )
                                .on_hover_text(
                                    "Cria uma nota nova com esta lista. É uma cópia: não acompanha o quadro.",
                                )
                                .clicked()
                            {
                                outcome.export_list = true;
                            }
                        });
                    });
                    ui.label(
                        egui::RichText::new(
                            "O quadro lido como uma nota. Não é um arquivo: muda quando o quadro muda.",
                        )
                        .color(weak),
                    );
                    ui.add_space(theme::space::M);
                    if list.items.is_empty() {
                        ui.label(egui::RichText::new("Quadro vazio.").color(weak));
                    }
                    for item in &list.items {
                        ui.horizontal(|ui| {
                            ui.add_space(item.depth as f32 * theme::space::XL);
                            ui.vertical(|ui| match &item.kind {
                                ItemKind::Heading { level } => {
                                    ui.add_space(theme::space::S);
                                    let size = match level {
                                        1 => 20.0,
                                        2 => 17.0,
                                        _ => 15.0,
                                    };
                                    let title = egui::RichText::new(&item.title)
                                        .size(size)
                                        .strong()
                                        .color(accent);
                                    if ui
                                        .add(egui::Label::new(title).sense(Sense::click()))
                                        .clicked()
                                    {
                                        go_to = Some(item.node_id.clone());
                                    }
                                }
                                ItemKind::Note {
                                    path,
                                    snippet,
                                    missing,
                                } => {
                                    // O título abre a nota; o trecho mostra o cartão no quadro.
                                    let title = egui::RichText::new(&item.title).strong();
                                    if *missing {
                                        if ui
                                            .link(title)
                                            .on_hover_text("Mostrar no quadro")
                                            .clicked()
                                        {
                                            go_to = Some(item.node_id.clone());
                                        }
                                        ui.label(
                                            egui::RichText::new("Nota não encontrada no cofre.")
                                                .color(weak),
                                        );
                                    } else {
                                        if ui.link(title).on_hover_text("Abrir a nota").clicked() {
                                            outcome.follow = Some(path.clone());
                                        }
                                        if !snippet.is_empty()
                                            && ui
                                                .add(
                                                    egui::Label::new(
                                                        egui::RichText::new(snippet).color(weak),
                                                    )
                                                    .sense(Sense::click()),
                                                )
                                                .on_hover_text("Mostrar no quadro")
                                                .clicked()
                                        {
                                            go_to = Some(item.node_id.clone());
                                        }
                                    }
                                }
                                ItemKind::File { path } => {
                                    if ui.link(&item.title).on_hover_text("Abrir o arquivo").clicked() {
                                        outcome.follow = Some(path.clone());
                                    }
                                }
                                ItemKind::Text { text } => {
                                    egui::Frame::NONE
                                        .fill(ui.visuals().faint_bg_color)
                                        .corner_radius(4.0)
                                        .inner_margin(theme::space::margin(
                                            theme::space::S,
                                            theme::space::XS,
                                        ))
                                        .show(ui, |ui| {
                                            if ui
                                                .add(
                                                    egui::Label::new(text.trim())
                                                        .sense(Sense::click()),
                                                )
                                                .clicked()
                                            {
                                                go_to = Some(item.node_id.clone());
                                            }
                                        });
                                }
                                ItemKind::Link { url } => {
                                    if ui.link(url).on_hover_text("Abrir o link").clicked() {
                                        outcome.follow = Some(url.clone());
                                    }
                                }
                                ItemKind::Other => {
                                    if ui.link(&item.title).clicked() {
                                        go_to = Some(item.node_id.clone());
                                    }
                                }
                            });
                        });
                        ui.add_space(theme::space::XS);
                    }
                    if !list.relations.is_empty() {
                        ui.add_space(theme::space::M);
                        ui.label(egui::RichText::new("Ligações").size(17.0).strong().color(accent));
                        for relation in &list.relations {
                            if ui
                                .add(egui::Label::new(&relation.text).sense(Sense::click()))
                                .on_hover_text("Mostrar no quadro")
                                .clicked()
                            {
                                go_to = Some(relation.from.clone());
                            }
                        }
                    }
                    ui.add_space(theme::space::XXL);
                });
            });
        });
    if let Some(id) = go_to {
        state.focus = Some(id);
        back = true;
    }
    if back {
        tab.board_list = false;
        ui.ctx().request_repaint();
    }
}

// ---------------------------------------------------------------------------------------------
// Quadro
// ---------------------------------------------------------------------------------------------

fn show_canvas(
    ui: &mut egui::Ui,
    state: &mut CanvasState,
    tab: &mut Tab,
    notes: &mut Notes,
    outcome: &mut Outcome,
) {
    let rect = ui.available_rect_before_wrap();
    state.area = rect;
    if !state.fitted && rect.width() > 1.0 {
        state.fit();
        state.fitted = true;
    }
    if rect.width() > 1.0
        && let Some(id) = state.focus.take()
    {
        state.center_on(&id);
    }
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let mut changed_text = false;
    let editing = state.editing.is_some();

    // --- Zoom e pan por roda / pinça ---------------------------------------------------------
    if response.hovered() && !editing {
        let (wheel, pinch, pointer) = ui.input(|input| {
            (
                input.smooth_scroll_delta.y,
                input.zoom_delta(),
                input.pointer.hover_pos(),
            )
        });
        // Sobre um cartão de nota que não cabe, a roda rola o cartão; Ctrl+roda e a pinça
        // seguem dando zoom no quadro.
        let card = pointer.and_then(|pointer| state.scrollable_card_at(pointer));
        if let Some(card) = card
            && wheel != 0.0
            && (pinch - 1.0).abs() < 1e-4
        {
            state.scroll_card(&card, wheel);
            ui.input_mut(|input| input.smooth_scroll_delta = Vec2::ZERO);
        } else {
            let factor = (wheel * 0.003).exp() * pinch;
            if (factor - 1.0).abs() > 1e-4
                && let Some(pointer) = pointer
            {
                zoom_at(state, factor, pointer);
                ui.input_mut(|input| input.smooth_scroll_delta = Vec2::ZERO);
            }
        }
    }

    // --- Ponteiro ----------------------------------------------------------------------------
    let shift = ui.input(|input| input.modifiers.shift);
    if response.drag_started_by(egui::PointerButton::Primary)
        && !editing
        && let Some(origin) = ui.input(|input| input.pointer.press_origin())
    {
        begin_drag(state, origin, shift);
    }
    if response.dragged_by(egui::PointerButton::Primary)
        && let Some(drag) = state.drag.take()
    {
        state.drag = Some(continue_drag(state, drag, &response, ui));
    }
    if response.drag_stopped()
        && let Some(drag) = state.drag.take()
    {
        changed_text |= finish_drag(state, drag, &response, tab);
    }
    if response.secondary_clicked()
        && let Some(position) = response.interact_pointer_pos()
    {
        let world = state.to_world(position);
        let tolerance = f64::from(GRAB / state.camera.zoom);
        state.menu = state
            .canvas
            .node_at(world, tolerance)
            .map(|node| Target::Node(node.id.clone()))
            .or_else(|| {
                state
                    .canvas
                    .edge_at(world, tolerance)
                    .map(|edge| Target::Edge(edge.id.clone()))
            });
        match &state.menu {
            Some(Target::Node(id)) if !state.selected.contains(id) => {
                let id = id.clone();
                state.select_only(&id);
            }
            Some(Target::Edge(id)) => {
                state.selected_edge = Some(id.clone());
                state.selected.clear();
            }
            _ => {}
        }
    }
    let double = response.double_clicked_by(egui::PointerButton::Primary);
    let single = response.clicked_by(egui::PointerButton::Primary);
    if (double || single)
        && !editing
        && let Some(position) = response.interact_pointer_pos()
    {
        let aside = ui.input(|input| {
            input.events.iter().any(|event| {
                matches!(event, egui::Event::PointerButton {
                    pressed: false,
                    modifiers,
                    ..
                } if modifiers.command && modifiers.alt)
            })
        });
        changed_text |= handle_click(
            state,
            tab,
            Click {
                position,
                double,
                shift,
                aside,
            },
            notes,
            outcome,
        );
    }

    // --- Teclado -----------------------------------------------------------------------------
    let keyboard_free = !ui.ctx().egui_wants_keyboard_input()
        && (response.hovered() || ui.rect_contains_pointer(rect))
        && state.add_note.is_none()
        && state.add_link.is_none();
    if keyboard_free {
        changed_text |= handle_keys(ui, state, tab, notes);
    }

    // --- Desenho -----------------------------------------------------------------------------
    paint_background(ui, &painter, rect, state);
    paint_groups_and_nodes(ui, &painter, state, notes);
    paint_edges(ui, &painter, state);
    paint_selection_tools(&painter, state, ui);
    if let Some(Drag::Link {
        from,
        side,
        pointer,
    }) = &state.drag
        && let Some(node) = state.canvas.node(from)
    {
        let start = state.to_screen(node.anchor(*side));
        painter.line_segment([start, *pointer], Stroke::new(2.0, theme::accent(ui)));
        painter.circle_filled(*pointer, 4.0, theme::accent(ui));
    }

    // --- Edição de texto no lugar -----------------------------------------------------------
    changed_text |= show_editor(ui, state, tab, notes, outcome);

    // --- Menu de contexto --------------------------------------------------------------------
    if let Some(action) = show_context_menu(&response, state) {
        changed_text |= apply_action(state, action, tab, outcome);
    }

    // --- Botão do cartão de texto selecionado -------------------------------------------------
    show_text_card_button(ui, state, outcome, response.context_menu_opened());

    // --- Barra de ferramentas e janelas -----------------------------------------------------
    changed_text |= show_toolbar(ui, state, tab, rect);
    changed_text |= show_dialogs(ui, state, tab, notes.vault);
    changed_text |= show_library(ui, state, tab, notes, rect);
    changed_text |= take_drops(ui, state, tab, notes, rect);

    if state.canvas.nodes.is_empty() && state.drag.is_none() && !state.is_editing() {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Quadro vazio. Clique duas vezes para criar um cartão de texto.",
            FontId::proportional(14.0),
            ui.visuals().weak_text_color(),
        );
    }
    if only_its_own_note(state, tab) && state.drag.is_none() && !state.is_editing() {
        paint_empty_board_hint(ui, rect);
    }
    if changed_text || state.drag.is_some() {
        ui.ctx().request_repaint();
    }
}

/// O quadro gerado de uma nota sem links: o único cartão é o da própria nota (`X.md` no quadro
/// `X.canvas`). Com qualquer outro cartão, a dica some.
fn only_its_own_note(state: &CanvasState, tab: &Tab) -> bool {
    let [node] = state.canvas.nodes.as_slice() else {
        return false;
    };
    node.node_kind() == NodeKind::File
        && node.file.as_deref().is_some_and(|file| {
            rustidian::canvas_index::key_of(Path::new(file))
                == rustidian::canvas_index::key_of(&rustidian::board_note::note_of(
                    &tab.document.path,
                ))
        })
}

/// A dica discreta do quadro que só tem a nota: embaixo, numa faixa de fundo suave, que não
/// pega cliques (o quadro continua mexível por baixo).
fn paint_empty_board_hint(ui: &egui::Ui, rect: egui::Rect) {
    if !rect.is_finite() || rect.width() < 1.0 {
        return;
    }
    let width = (rect.width() - 48.0).clamp(80.0, 560.0);
    let fill = ui.visuals().extreme_bg_color.gamma_multiply(0.92);
    egui::Area::new(egui::Id::new("canvas-empty-hint"))
        .order(egui::Order::Foreground)
        .interactable(false)
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(Pos2::new(rect.center().x, rect.bottom() - 16.0))
        .show(ui.ctx(), |ui| {
            egui::Frame::NONE
                .fill(fill)
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 7))
                .show(ui, |ui| {
                    ui.set_max_width(width);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(
                                "Esta nota ainda não tem links. Escreva [[outra nota]] na nota para ela aparecer aqui.",
                            )
                            .size(13.0)
                            .color(ui.visuals().weak_text_color()),
                        )
                        .wrap(),
                    );
                });
        });
}

/// "Transformar em nota" à vista: um botão logo abaixo do cartão de texto selecionado (ou acima,
/// se não couber), com o mesmo efeito do item do menu do cartão.
fn show_text_card_button(
    ui: &egui::Ui,
    state: &CanvasState,
    outcome: &mut Outcome,
    menu_open: bool,
) {
    // Com o menu do cartão aberto, o mesmo comando já está nele.
    if menu_open || state.selected.len() != 1 || state.selected_edge.is_some() || state.is_editing()
    {
        return;
    }
    if state.drag.is_some() {
        return;
    }
    let id = &state.selected[0];
    let Some(node) = state.canvas.node(id) else {
        return;
    };
    if node.node_kind() != NodeKind::Text {
        return;
    }
    let Some(card) = state.node_screen_rect(id) else {
        return;
    };
    // Num quadro desenhado fora de tela (primeiro quadro, aba escondida) a área não é finita.
    if !card.is_finite() || !state.area.is_finite() || state.area.width() < 1.0 {
        return;
    }
    const HEIGHT: f32 = 32.0;
    // Abaixo dos pontos de ligação (16 px do cartão) e das alças.
    const GAP: f32 = LINK_DOT_OFFSET + 10.0;
    let below = card.bottom() + GAP;
    let top = if below + HEIGHT <= state.area.bottom() - 4.0 {
        below
    } else {
        (card.top() - GAP - HEIGHT).max(state.area.top() + 4.0)
    };
    let left = card.left().clamp(
        state.area.left() + 4.0,
        (state.area.right() - 190.0).max(state.area.left()),
    );
    let node_id = id.clone();
    let text = node.text.clone().unwrap_or_default();
    egui::Area::new(egui::Id::new(("canvas-text-card-button", node_id.as_str())))
        .order(egui::Order::Foreground)
        .fixed_pos(Pos2::new(left, top))
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style())
                .inner_margin(egui::Margin::same(2))
                .show(ui, |ui| {
                    let button = ui
                        .add(
                            egui::Button::new("Transformar em nota")
                                .min_size(Vec2::new(0.0, HEIGHT - 4.0)),
                        )
                        .on_hover_text("Cria uma nota com este texto e troca o cartão por ela");
                    crate::ui_probe::record(ui.ctx(), "canvas-to-note", button.rect);
                    if button.clicked() {
                        outcome.to_note = Some(ToNote { node_id, text });
                    }
                });
        });
}

fn zoom_at(state: &mut CanvasState, factor: f32, pointer: Pos2) {
    let before = state.camera.zoom;
    let target = (before * factor).clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
    state.camera.zoom_at(
        target / before,
        [
            pointer.x - state.area.center().x,
            pointer.y - state.area.center().y,
        ],
    );
}

// ---------------------------------------------------------------------------------------------
// Gestos
// ---------------------------------------------------------------------------------------------

fn handle_positions(rect: egui::Rect) -> [(Handle, Pos2); 8] {
    Handle::ALL.map(|handle| {
        let (x, y) = handle.position();
        (
            handle,
            Pos2::new(
                rect.min.x + rect.width() * x as f32,
                rect.min.y + rect.height() * y as f32,
            ),
        )
    })
}

fn link_dots(rect: egui::Rect) -> [(Side, Pos2); 4] {
    Side::ALL.map(|side| {
        let (nx, ny) = side.normal();
        let anchor = match side {
            Side::Top => rect.center_top(),
            Side::Right => rect.right_center(),
            Side::Bottom => rect.center_bottom(),
            Side::Left => rect.left_center(),
        };
        (
            side,
            anchor + Vec2::new(nx as f32, ny as f32) * LINK_DOT_OFFSET,
        )
    })
}

fn begin_drag(state: &mut CanvasState, origin: Pos2, shift: bool) {
    // Ferramentas do nó selecionado (um só): alças de tamanho e pontos de ligação.
    if state.selected.len() == 1 && state.selected_edge.is_none() {
        let id = state.selected[0].clone();
        if let Some(node) = state.canvas.node(&id) {
            let rect = state.screen_rect(node.rect());
            for (handle, position) in handle_positions(rect) {
                if position.distance(origin) <= GRAB {
                    state.drag = Some(Drag::Resize {
                        id,
                        handle,
                        original: node.rect(),
                        total: (0.0, 0.0),
                    });
                    return;
                }
            }
            for (side, position) in link_dots(rect) {
                if position.distance(origin) <= GRAB {
                    state.drag = Some(Drag::Link {
                        from: id,
                        side,
                        pointer: origin,
                    });
                    return;
                }
            }
        }
    }
    let world = state.to_world(origin);
    let tolerance = f64::from(GRAB / state.camera.zoom);
    if let Some(node) = state.canvas.node_at(world, tolerance) {
        let id = node.id.clone();
        if shift {
            if !state.selected.contains(&id) {
                state.selected.push(id.clone());
            }
        } else if !state.selected.contains(&id) {
            state.select_only(&id);
        }
        state.selected_edge = None;
        let ids = state.canvas.with_group_members(&state.selected);
        let origins = ids
            .iter()
            .filter_map(|id| {
                state
                    .canvas
                    .node(id)
                    .map(|node| (id.clone(), node.x, node.y))
            })
            .collect();
        state.drag = Some(Drag::Move {
            origins,
            total: (0.0, 0.0),
        });
        return;
    }
    state.drag = Some(Drag::Pan {
        start: state.camera.pan,
    });
}

fn continue_drag(
    state: &mut CanvasState,
    drag: Drag,
    response: &egui::Response,
    ui: &egui::Ui,
) -> Drag {
    let zoom = f64::from(state.camera.zoom);
    let delta = response.drag_delta();
    match drag {
        Drag::Pan { start } => {
            state.camera.pan = [state.camera.pan[0] + delta.x, state.camera.pan[1] + delta.y];
            Drag::Pan { start }
        }
        Drag::Move { origins, total } => {
            let total = (
                total.0 + f64::from(delta.x) / zoom,
                total.1 + f64::from(delta.y) / zoom,
            );
            for (id, x, y) in &origins {
                if let Some(node) = state.canvas.node_mut(id) {
                    node.x = (x + total.0).round();
                    node.y = (y + total.1).round();
                }
            }
            Drag::Move { origins, total }
        }
        Drag::Resize {
            id,
            handle,
            original,
            total,
        } => {
            let total = (
                total.0 + f64::from(delta.x) / zoom,
                total.1 + f64::from(delta.y) / zoom,
            );
            if let Some(node) = state.canvas.node_mut(&id) {
                let mut next = canvas::resized(original, handle, total.0, total.1);
                next.x = next.x.round();
                next.y = next.y.round();
                next.w = next.w.round();
                next.h = next.h.round();
                node.set_rect(next);
            }
            Drag::Resize {
                id,
                handle,
                original,
                total,
            }
        }
        Drag::Link {
            from,
            side,
            pointer,
        } => Drag::Link {
            from,
            side,
            pointer: ui
                .input(|input| input.pointer.hover_pos())
                .unwrap_or(pointer),
        },
    }
}

fn finish_drag(
    state: &mut CanvasState,
    drag: Drag,
    response: &egui::Response,
    tab: &mut Tab,
) -> bool {
    match drag {
        Drag::Pan { .. } => false,
        Drag::Move { .. } | Drag::Resize { .. } => state.commit(tab),
        Drag::Link {
            from,
            side,
            pointer,
        } => {
            let position = response.hover_pos().unwrap_or(pointer);
            let world = state.to_world(position);
            let tolerance = f64::from(GRAB / state.camera.zoom);
            let target = state
                .canvas
                .node_at(world, tolerance)
                .filter(|node| node.id != from)
                .map(|node| node.id.clone());
            let Some(target) = target else { return false };
            match state.canvas.connect(&from, &target, Some(side), None) {
                Some(edge) => {
                    state.selected_edge = Some(edge);
                    state.selected.clear();
                    state.commit(tab)
                }
                None => false,
            }
        }
    }
}

/// Um clique no quadro: onde, e com que gesto.
struct Click {
    position: Pos2,
    double: bool,
    shift: bool,
    /// Ctrl+Alt: abrir a nota no outro painel (o mesmo gesto do R15 para links).
    aside: bool,
}

fn handle_click(
    state: &mut CanvasState,
    tab: &mut Tab,
    click: Click,
    notes: &Notes,
    outcome: &mut Outcome,
) -> bool {
    let Click {
        position,
        double,
        shift,
        aside,
    } = click;
    let world = state.to_world(position);
    let tolerance = f64::from(GRAB / state.camera.zoom);
    if let Some(node) = state.canvas.node_at(world, tolerance).cloned() {
        if shift && !double {
            if let Some(index) = state.selected.iter().position(|id| *id == node.id) {
                state.selected.remove(index);
            } else {
                state.selected.push(node.id.clone());
            }
            state.selected_edge = None;
            return false;
        }
        state.select_only(&node.id);
        if node.is_note_card()
            && let Some(file) = node.file.clone()
        {
            let rect = state.screen_rect(node.rect());
            let zoom = state.camera.zoom;
            if buttons_visible(zoom) {
                let buttons = header_buttons(rect, zoom);
                if buttons.collapse.expand(2.0).contains(position) {
                    state.canvas.toggle_collapsed(&node.id);
                    return state.commit(tab);
                }
                if buttons.aside.expand(2.0).contains(position) {
                    outcome.aside = Some(file);
                    return false;
                }
            }
            if aside {
                outcome.aside = Some(file);
                return false;
            }
            let in_header = world.1 - node.y <= canvas::NOTE_HEADER;
            if in_header {
                // O título abre a nota (como no R14).
                outcome.follow = Some(file);
            } else if double && notes.exists(Path::new(&file)) {
                state.editing = Some(Editing {
                    what: Edit::Note(node.id.clone()),
                    buffer: String::new(),
                    fresh: false,
                    focused: false,
                });
            }
            return false;
        }
        let in_header = world.1 - node.y <= GROUP_LABEL_HEIGHT.max(24.0);
        match node.node_kind() {
            NodeKind::Text if double => {
                state.editing = Some(Editing {
                    buffer: node.text.clone().unwrap_or_default(),
                    what: Edit::Text(node.id.clone()),
                    fresh: false,
                    focused: false,
                });
            }
            NodeKind::Group if double => {
                state.editing = Some(Editing {
                    buffer: node.label.clone().unwrap_or_default(),
                    what: Edit::Group(node.id.clone()),
                    fresh: false,
                    focused: false,
                });
            }
            NodeKind::File if double || in_header => {
                if let Some(file) = &node.file {
                    outcome.follow = Some(file.clone());
                }
            }
            NodeKind::Link if double || in_header => {
                if let Some(url) = &node.url {
                    outcome.follow = Some(url.clone());
                }
            }
            _ => {}
        }
        return false;
    }
    if let Some(edge) = state.canvas.edge_at(world, tolerance).cloned() {
        state.selected.clear();
        state.selected_edge = Some(edge.id.clone());
        if double {
            state.editing = Some(Editing {
                buffer: edge.label.clone().unwrap_or_default(),
                what: Edit::Edge(edge.id),
                fresh: false,
                focused: false,
            });
        }
        return false;
    }
    if !shift {
        state.clear_selection();
    }
    if double {
        let id = state
            .canvas
            .add_text((world.0 - 125.0).round(), (world.1 - 30.0).round());
        state.select_only(&id);
        state.editing = Some(Editing {
            what: Edit::Text(id),
            buffer: String::new(),
            fresh: true,
            focused: false,
        });
        // O cartão novo só entra no arquivo quando o texto for confirmado.
    }
    false
}

fn handle_keys(ui: &egui::Ui, state: &mut CanvasState, tab: &mut Tab, notes: &Notes) -> bool {
    let (delete, escape, enter, undo, redo, all, fit) = ui.input(|input| {
        let command = input.modifiers.command;
        (
            input.key_pressed(Key::Delete) || input.key_pressed(Key::Backspace),
            input.key_pressed(Key::Escape),
            input.key_pressed(Key::Enter) && !command,
            command && !input.modifiers.shift && input.key_pressed(Key::Z),
            command
                && (input.key_pressed(Key::Y)
                    || (input.modifiers.shift && input.key_pressed(Key::Z))),
            command && input.key_pressed(Key::A),
            input.key_pressed(Key::Num0) && !command,
        )
    });
    let mut changed = false;
    if delete {
        changed |= delete_selection(state, tab);
    }
    if escape {
        state.clear_selection();
        state.drag = None;
    }
    if enter
        && state.selected.len() == 1
        && let Some(node) = state.canvas.node(&state.selected[0]).cloned()
    {
        if node.node_kind() == NodeKind::Text {
            state.editing = Some(Editing {
                buffer: node.text.unwrap_or_default(),
                what: Edit::Text(node.id),
                fresh: false,
                focused: false,
            });
        } else if node.is_note_card()
            && !node.is_collapsed()
            && node
                .file
                .as_deref()
                .is_some_and(|file| notes.exists(Path::new(file)))
        {
            state.editing = Some(Editing {
                buffer: String::new(),
                what: Edit::Note(node.id),
                fresh: false,
                focused: false,
            });
        }
    }
    if undo {
        changed |= state.step_history(tab, false);
    }
    if redo {
        changed |= state.step_history(tab, true);
    }
    if all {
        state.selected = state
            .canvas
            .nodes
            .iter()
            .map(|node| node.id.clone())
            .collect();
        state.selected_edge = None;
    }
    if fit {
        state.fit();
    }
    changed
}

fn delete_selection(state: &mut CanvasState, tab: &mut Tab) -> bool {
    let mut removed = false;
    if let Some(edge) = state.selected_edge.take() {
        removed |= state.canvas.remove_edge(&edge);
    }
    if !state.selected.is_empty() {
        let ids = std::mem::take(&mut state.selected);
        removed |= state.canvas.remove_nodes(&ids) > 0;
    }
    removed && state.commit(tab)
}

// ---------------------------------------------------------------------------------------------
// Edição no lugar
// ---------------------------------------------------------------------------------------------

fn show_editor(
    ui: &mut egui::Ui,
    state: &mut CanvasState,
    tab: &mut Tab,
    notes: &mut Notes,
    outcome: &mut Outcome,
) -> bool {
    if matches!(
        state.editing.as_ref().map(|editing| &editing.what),
        Some(Edit::Note(_))
    ) {
        show_note_editor(ui, state, notes, outcome);
        return false;
    }
    let Some(mut editing) = state.editing.take() else {
        return false;
    };
    let zoom = state.camera.zoom;
    let area = match &editing.what {
        Edit::Text(id) => state
            .canvas
            .node(id)
            .map(|node| state.screen_rect(node.rect()).shrink(6.0 * zoom.min(1.0))),
        Edit::Group(id) => state.canvas.node(id).map(|node| {
            let rect = state.screen_rect(node.rect());
            egui::Rect::from_min_size(
                rect.min + Vec2::splat(4.0),
                Vec2::new(
                    (rect.width() - 8.0).max(80.0),
                    GROUP_LABEL_HEIGHT as f32 * zoom,
                ),
            )
        }),
        Edit::Edge(id) => state
            .canvas
            .edge(id)
            .and_then(|edge| state.canvas.curve(edge))
            .map(|curve| {
                let middle = state.to_screen(curve.at(0.5));
                egui::Rect::from_center_size(middle, Vec2::new(140.0, 24.0))
            }),
        Edit::Note(_) => None,
    };
    let Some(area) = area else {
        return false;
    };
    let multiline = matches!(editing.what, Edit::Text(_));
    let size = (15.0 * zoom).clamp(11.0, 40.0);
    let id = ui.scope_id().with(("canvas-edit", &editing.what_id()));
    let mut escape = false;
    let response = ui
        .scope_builder(egui::UiBuilder::new().max_rect(area), |ui| {
            let edit = if multiline {
                egui::TextEdit::multiline(&mut editing.buffer)
                    .desired_rows(1)
                    .desired_width(area.width())
            } else {
                egui::TextEdit::singleline(&mut editing.buffer).desired_width(area.width())
            };
            let output = edit
                .id(id)
                .font(FontId::new(size, theme::fonts::text("regular")))
                .frame(
                    egui::Frame::NONE
                        .inner_margin(theme::space::XS)
                        .fill(ui.visuals().extreme_bg_color)
                        .stroke(Stroke::new(1.5, theme::accent(ui))),
                )
                .show(ui);
            output.response
        })
        .inner;
    if !editing.focused {
        response.request_focus();
        editing.focused = true;
    }
    ui.input(|input| {
        escape = input.key_pressed(Key::Escape);
    });
    let finished = response.lost_focus() || escape;
    if !finished {
        state.editing = Some(editing);
        return false;
    }
    let mut changed = false;
    match &editing.what {
        Edit::Text(node) => {
            if editing.fresh && editing.buffer.trim().is_empty() {
                state.canvas.remove_nodes(std::slice::from_ref(node));
                state.clear_selection();
            } else {
                state.canvas.set_text(node, &editing.buffer);
            }
        }
        Edit::Group(node) => {
            state.canvas.set_group_label(node, &editing.buffer);
        }
        Edit::Edge(edge) => {
            state.canvas.set_edge_label(edge, &editing.buffer);
        }
        Edit::Note(_) => {}
    }
    changed |= state.commit(tab);
    changed
}

/// Edita a nota de um cartão de nota no lugar: o texto vai direto para o `Document` da nota (o da
/// aba, se ela está aberta; senão o cartão aberto do app), então a aba e o cartão são sempre a
/// mesma coisa, e o autosave e o portão do sync são os de qualquer nota. Esc, clique fora ou
/// perder o foco terminam a edição. Só marca a nota como editada quando o texto mudou.
fn show_note_editor(
    ui: &mut egui::Ui,
    state: &mut CanvasState,
    notes: &mut Notes,
    outcome: &mut Outcome,
) {
    let Some(mut editing) = state.editing.take() else {
        return;
    };
    let Edit::Note(node_id) = &editing.what else {
        return;
    };
    let Some(node) = state.canvas.node(node_id).cloned() else {
        return;
    };
    let Some(file) = node.file.clone() else {
        return;
    };
    let rect = state.screen_rect(node.rect());
    let zoom = state.camera.zoom;
    let body = note_body_rect(rect, zoom);
    if body.width() < 20.0 || body.height() < 20.0 || node.is_collapsed() {
        return;
    }
    let tab = match notes.edit_target(Path::new(&file)) {
        Ok(tab) => tab,
        Err(message) => {
            outcome.error = Some(message);
            return;
        }
    };
    let had_focus_before = editing.focused;
    let size = (14.0 * zoom).clamp(11.0, 40.0);
    let id = ui.scope_id().with(("canvas-note-edit", &node.id));
    let accent = theme::accent(ui);
    let fill = ui.visuals().extreme_bg_color;
    let mut escape = false;
    let (response, changed) = ui
        .scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
            let output = egui::ScrollArea::vertical()
                .id_salt(("canvas-note-scroll", &node.id))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::TextEdit::multiline(&mut tab.document.text)
                        .id(id)
                        .font(FontId::new(size, theme::fonts::text("regular")))
                        .desired_width(body.width())
                        .min_size(body.size())
                        .frame(egui::Frame::NONE.inner_margin(theme::space::XS).fill(fill))
                        .show(ui)
                })
                .inner;
            (output.response.clone(), output.response.changed())
        })
        .inner;
    ui.painter()
        .rect_stroke(body, 2.0, Stroke::new(1.5, accent), StrokeKind::Inside);
    if changed {
        tab.edited();
    }
    if !editing.focused {
        response.request_focus();
        editing.focused = true;
    }
    ui.input(|input| {
        escape = input.key_pressed(Key::Escape);
    });
    // Sair: Esc, clicar fora (o foco vai embora) ou o texto perder o foco depois de tê-lo.
    let finished = escape || (had_focus_before && !response.has_focus());
    ui.ctx().request_repaint();
    if !finished {
        state.editing = Some(editing);
    }
}

/// A área do corpo de um cartão de nota (abaixo do cabeçalho), na tela.
fn note_body_rect(rect: egui::Rect, zoom: f32) -> egui::Rect {
    let pad = 10.0 * zoom;
    egui::Rect::from_min_max(
        Pos2::new(
            rect.min.x + pad,
            rect.min.y + canvas::NOTE_HEADER as f32 * zoom,
        ),
        rect.max - Vec2::splat(pad),
    )
}

/// Os botões do cabeçalho de um cartão de nota, na tela.
struct HeaderButtons {
    collapse: egui::Rect,
    aside: egui::Rect,
}

/// Os botões só aparecem (e só respondem) com zoom de leitura.
fn buttons_visible(zoom: f32) -> bool {
    zoom >= 0.3
}

fn header_buttons(rect: egui::Rect, zoom: f32) -> HeaderButtons {
    let size = (18.0 * zoom).clamp(13.0, 26.0);
    let pad = 8.0 * zoom.max(0.6);
    let top = rect.min.y + (canvas::NOTE_HEADER as f32 * zoom - size) / 2.0;
    let collapse = egui::Rect::from_min_size(
        Pos2::new(rect.max.x - pad - size, top.max(rect.min.y + 2.0)),
        Vec2::splat(size),
    );
    let aside = collapse.translate(Vec2::new(-(size + 4.0 * zoom.max(0.6)), 0.0));
    HeaderButtons { collapse, aside }
}

impl Editing {
    fn what_id(&self) -> String {
        match &self.what {
            Edit::Text(id) | Edit::Group(id) | Edit::Edge(id) | Edit::Note(id) => id.clone(),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Menu de contexto
// ---------------------------------------------------------------------------------------------

const COLOR_NAMES: [(&str, &str); 6] = [
    ("1", "Vermelho"),
    ("2", "Laranja"),
    ("3", "Amarelo"),
    ("4", "Verde"),
    ("5", "Ciano"),
    ("6", "Roxo"),
];

fn show_context_menu(response: &egui::Response, state: &mut CanvasState) -> Option<Action> {
    let target = state.menu.clone()?;
    let mut action = None;
    response.context_menu(|ui| {
        theme::menu_style(ui);
        match &target {
            Target::Node(id) => {
                if let Some(node) = state.canvas.node(id)
                    && node.is_note_card()
                {
                    if ui.button("Abrir ao lado").clicked() {
                        action = Some(Action::Aside);
                        ui.close();
                    }
                    if ui
                        .button(if node.is_collapsed() {
                            "Expandir"
                        } else {
                            "Recolher"
                        })
                        .clicked()
                    {
                        action = Some(Action::Collapse);
                        ui.close();
                    }
                }
                if let Some(node) = state.canvas.node(id)
                    && node.node_kind() == NodeKind::Text
                    && ui.button("Transformar em nota").clicked()
                {
                    action = Some(Action::ToNote);
                    ui.close();
                }
                if let Some(node) = state.canvas.node(id)
                    && matches!(node.node_kind(), NodeKind::Group | NodeKind::Text)
                    && ui
                        .button(if node.node_kind() == NodeKind::Group {
                            "Editar rótulo"
                        } else {
                            "Editar texto"
                        })
                        .clicked()
                {
                    action = Some(Action::EditLabel);
                    ui.close();
                }
            }
            Target::Edge(id) => {
                if ui.button("Editar rótulo").clicked() {
                    action = Some(Action::EditLabel);
                    ui.close();
                }
                if let Some(edge) = state.canvas.edge(id) {
                    let (from, to) = (edge.from_arrow(), edge.to_arrow());
                    if ui
                        .button(if from {
                            "Tirar seta do início"
                        } else {
                            "Seta no início"
                        })
                        .clicked()
                    {
                        action = Some(Action::Arrow {
                            start: true,
                            on: !from,
                        });
                        ui.close();
                    }
                    if ui
                        .button(if to {
                            "Tirar seta do fim"
                        } else {
                            "Seta no fim"
                        })
                        .clicked()
                    {
                        action = Some(Action::Arrow {
                            start: false,
                            on: !to,
                        });
                        ui.close();
                    }
                }
            }
        }
        ui.menu_button("Cor", |ui| {
            if ui.button("Sem cor").clicked() {
                action = Some(Action::Color(None));
                ui.close();
            }
            for (key, name) in COLOR_NAMES {
                let [r, g, b] = color_rgb(key).unwrap_or([128; 3]);
                let label = egui::RichText::new(name).color(Color32::from_rgb(r, g, b));
                if ui.button(label).clicked() {
                    action = Some(Action::Color(Some(key)));
                    ui.close();
                }
            }
        });
        ui.separator();
        if ui.button("Apagar").clicked() {
            action = Some(Action::Delete);
            ui.close();
        }
    });
    action
}

fn apply_action(
    state: &mut CanvasState,
    action: Action,
    tab: &mut Tab,
    outcome: &mut Outcome,
) -> bool {
    let Some(target) = state.menu.clone() else {
        return false;
    };
    match action {
        Action::Delete => {
            match &target {
                Target::Node(_) => state.selected_edge = None,
                Target::Edge(id) => {
                    state.selected = Vec::new();
                    state.selected_edge = Some(id.clone());
                }
            }
            delete_selection(state, tab)
        }
        Action::Color(color) => {
            let ids: Vec<String> = match &target {
                Target::Node(id) if state.selected.contains(id) => state.selected.clone(),
                Target::Node(id) | Target::Edge(id) => vec![id.clone()],
            };
            let mut any = false;
            for id in ids {
                any |= state.canvas.set_color(&id, color);
            }
            any && state.commit(tab)
        }
        Action::EditLabel => {
            match target {
                Target::Node(id) => {
                    if let Some(node) = state.canvas.node(&id).cloned() {
                        let what = if node.node_kind() == NodeKind::Group {
                            Edit::Group(id)
                        } else {
                            Edit::Text(id)
                        };
                        let buffer = if node.node_kind() == NodeKind::Group {
                            node.label.unwrap_or_default()
                        } else {
                            node.text.unwrap_or_default()
                        };
                        state.editing = Some(Editing {
                            what,
                            buffer,
                            fresh: false,
                            focused: false,
                        });
                    }
                }
                Target::Edge(id) => {
                    let buffer = state
                        .canvas
                        .edge(&id)
                        .and_then(|edge| edge.label.clone())
                        .unwrap_or_default();
                    state.editing = Some(Editing {
                        what: Edit::Edge(id),
                        buffer,
                        fresh: false,
                        focused: false,
                    });
                }
            }
            false
        }
        Action::Arrow { start, on } => {
            let Target::Edge(id) = target else {
                return false;
            };
            state.canvas.set_arrow(&id, start, on) && state.commit(tab)
        }
        Action::Aside => {
            if let Target::Node(id) = target
                && let Some(node) = state.canvas.node(&id)
            {
                outcome.aside = node.file.clone();
            }
            false
        }
        Action::Collapse => {
            let Target::Node(id) = target else {
                return false;
            };
            state.canvas.toggle_collapsed(&id) && state.commit(tab)
        }
        Action::ToNote => {
            if let Target::Node(id) = target
                && let Some(node) = state.canvas.node(&id)
                && node.node_kind() == NodeKind::Text
            {
                outcome.to_note = Some(ToNote {
                    text: node.text.clone().unwrap_or_default(),
                    node_id: id,
                });
            }
            false
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Barra de ferramentas e janelas de "adicionar"
// ---------------------------------------------------------------------------------------------

fn show_toolbar(
    ui: &mut egui::Ui,
    state: &mut CanvasState,
    tab: &mut Tab,
    rect: egui::Rect,
) -> bool {
    let bar = egui::Rect::from_min_size(
        rect.min + Vec2::new(10.0, 10.0),
        Vec2::new((rect.width() - 20.0).max(100.0), 34.0),
    );
    let mut changed = false;
    ui.scope_builder(egui::UiBuilder::new().max_rect(bar), |ui| {
        egui::Frame::popup(ui.style())
            .inner_margin(theme::space::margin(theme::space::S, theme::space::XS))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .button("Texto")
                        .on_hover_text("Novo cartão de texto (ou clique duas vezes no fundo)")
                        .clicked()
                    {
                        let (x, y) = state.free_spot(canvas::TEXT_SIZE);
                        let id = state.canvas.add_text(x, y);
                        state.select_only(&id);
                        state.editing = Some(Editing {
                            what: Edit::Text(id),
                            buffer: String::new(),
                            fresh: true,
                            focused: false,
                        });
                    }
                    if ui
                        .button("Nota")
                        .on_hover_text("Adicionar uma nota do cofre")
                        .clicked()
                    {
                        state.add_note = Some(String::new());
                    }
                    if ui
                        .selectable_label(state.library.is_some(), "Biblioteca")
                        .on_hover_text("Lista das notas do cofre para pôr no quadro")
                        .clicked()
                    {
                        state.library = match state.library.take() {
                            Some(_) => None,
                            None => Some(Library::opened()),
                        };
                    }
                    if ui
                        .button("Link")
                        .on_hover_text("Adicionar um endereço da web")
                        .clicked()
                    {
                        state.add_link = Some(String::new());
                    }
                    if ui.button("Grupo").on_hover_text("Novo grupo").clicked() {
                        let (x, y) = state.free_spot(canvas::BOX_SIZE);
                        let id = state.canvas.add_group("Grupo", x, y);
                        state.select_only(&id);
                        changed |= state.commit(tab);
                    }
                    let can_link = state.selected.len() == 2;
                    if ui
                        .add_enabled(can_link, egui::Button::new("Ligar"))
                        .on_hover_text(
                            "Liga os dois cartões selecionados (Shift+clique escolhe o segundo)",
                        )
                        .clicked()
                    {
                        let (from, to) = (state.selected[0].clone(), state.selected[1].clone());
                        if let Some(edge) = state.canvas.connect(&from, &to, None, None) {
                            state.selected.clear();
                            state.selected_edge = Some(edge);
                            changed |= state.commit(tab);
                        }
                    }
                    ui.separator();
                    if ui.button("−").on_hover_text("Diminuir o zoom").clicked() {
                        let center = state.area.center();
                        zoom_at(state, 1.0 / 1.25, center);
                    }
                    if ui.button("+").on_hover_text("Aumentar o zoom").clicked() {
                        let center = state.area.center();
                        zoom_at(state, 1.25, center);
                    }
                    if ui
                        .button("Ajustar")
                        .on_hover_text("Mostrar o quadro inteiro (0)")
                        .clicked()
                    {
                        state.fit();
                    }
                    ui.label(format!("{:.0}%", state.camera.zoom * 100.0));
                    ui.separator();
                    if ui
                        .add_enabled(!state.undo.is_empty(), egui::Button::new("Desfazer"))
                        .clicked()
                    {
                        changed |= state.step_history(tab, false);
                    }
                    if ui
                        .add_enabled(!state.redo.is_empty(), egui::Button::new("Refazer"))
                        .clicked()
                    {
                        changed |= state.step_history(tab, true);
                    }
                });
            });
    });
    changed
}

fn show_dialogs(
    ui: &mut egui::Ui,
    state: &mut CanvasState,
    tab: &mut Tab,
    vault: Option<&rustidian::vault::Vault>,
) -> bool {
    let mut changed = false;
    if let Some(mut query) = state.add_note.take() {
        let mut keep = true;
        let mut chosen = None;
        egui::Window::new("Adicionar nota ao quadro")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ui.ctx(), |ui| {
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut query)
                        .hint_text("Nome da nota")
                        .desired_width(320.0),
                );
                if !edit.has_focus() && query.is_empty() {
                    edit.request_focus();
                }
                let needle = query.to_lowercase();
                let matches: Vec<String> = vault
                    .map(|vault| {
                        vault
                            .notes
                            .iter()
                            .map(|note| note.path.to_string_lossy().replace('\\', "/"))
                            .filter(|path| {
                                needle.is_empty() || path.to_lowercase().contains(&needle)
                            })
                            .take(8)
                            .collect()
                    })
                    .unwrap_or_default();
                for path in &matches {
                    if ui.button(path).clicked() {
                        chosen = Some(path.clone());
                    }
                }
                if matches.is_empty() {
                    ui.label("Nenhuma nota encontrada.");
                }
                if edit.lost_focus()
                    && ui.input(|input| input.key_pressed(Key::Enter))
                    && let Some(first) = matches.first()
                {
                    chosen = Some(first.clone());
                }
                if ui.button("Cancelar").clicked() {
                    keep = false;
                }
            });
        if ui.input(|input| input.key_pressed(Key::Escape)) {
            keep = false;
        }
        if let Some(path) = chosen {
            let (x, y) = state.free_spot(canvas::BOX_SIZE);
            let id = state.canvas.add_file(&path, x, y);
            state.select_only(&id);
            changed |= state.commit(tab);
        } else if keep {
            state.add_note = Some(query);
        }
    }
    if let Some(mut url) = state.add_link.take() {
        let mut keep = true;
        let mut done = false;
        egui::Window::new("Adicionar link ao quadro")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ui.ctx(), |ui| {
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut url)
                        .hint_text("https://…")
                        .desired_width(320.0),
                );
                if !edit.has_focus() && url.is_empty() {
                    edit.request_focus();
                }
                let enter = edit.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
                ui.horizontal(|ui| {
                    if (ui.button("Adicionar").clicked() || enter) && !url.trim().is_empty() {
                        done = true;
                    }
                    if ui.button("Cancelar").clicked() {
                        keep = false;
                    }
                });
            });
        if ui.input(|input| input.key_pressed(Key::Escape)) {
            keep = false;
        }
        if done {
            let (x, y) = state.free_spot(canvas::BOX_SIZE);
            let id = state.canvas.add_link(&url, x, y);
            state.select_only(&id);
            changed |= state.commit(tab);
        } else if keep {
            state.add_link = Some(url);
        }
    }
    changed
}

// ---------------------------------------------------------------------------------------------
// Biblioteca e notas soltas no quadro (H1)
// ---------------------------------------------------------------------------------------------

/// O painel da biblioteca, quando aberto: lista as notas do cofre e põe no quadro a que o
/// usuário escolher (o botão + de cada linha; arrastar é tratado em `take_drops`).
fn show_library(
    ui: &mut egui::Ui,
    state: &mut CanvasState,
    tab: &mut Tab,
    notes: &Notes,
    rect: egui::Rect,
) -> bool {
    let Some(mut library) = state.library.take() else {
        return false;
    };
    let paths: Vec<String> = notes
        .vault
        .map(|vault| {
            vault
                .notes
                .iter()
                .filter(|note| rustidian::vault::is_markdown(&note.path))
                .map(|note| note.path.to_string_lossy().replace('\\', "/"))
                .collect()
        })
        .unwrap_or_default();
    let names: Vec<&str> = paths.iter().map(String::as_str).collect();
    let output = canvas_library::show(ui, rect, &mut library, &state.canvas, &names);
    let mut changed = false;
    if let Some(path) = output.add {
        let (x, y) = state.free_spot(canvas::BOX_SIZE);
        let id = state.canvas.add_file(&path, x, y);
        state.select_only(&id);
        changed |= state.commit(tab);
    }
    if !output.close {
        state.library = Some(library);
    }
    changed
}

/// Notas que chegam ao quadro por arrastar: da biblioteca ou do explorador de arquivos. Cada
/// uma vira um cartão com o centro onde o ponteiro foi solto.
fn take_drops(
    ui: &mut egui::Ui,
    state: &mut CanvasState,
    tab: &mut Tab,
    notes: &Notes,
    rect: egui::Rect,
) -> bool {
    let ctx = ui.ctx().clone();
    let panel = state
        .library
        .as_ref()
        .map(|_| canvas_library::panel_rect(rect));
    let outside_panel = |position: Pos2| {
        rect.contains(position) && panel.is_none_or(|panel| !panel.contains(position))
    };
    let pointer = ctx.input(|input| input.pointer.latest_pos());
    let mut changed = false;
    let mut hovering = false;

    // Da biblioteca.
    if let Some(dragged) = egui::DragAndDrop::payload::<canvas_library::Dragged>(&ctx) {
        hovering = pointer.is_some_and(outside_panel);
        if ctx.input(|input| input.pointer.any_released()) {
            if let Some(position) = pointer.filter(|position| outside_panel(*position)) {
                let world = state.to_world(position);
                let ids =
                    state
                        .canvas
                        .add_files_at(std::slice::from_ref(&dragged.0), world.0, world.1);
                state.selected = ids;
                state.selected_edge = None;
                changed |= state.commit(tab);
            }
            egui::DragAndDrop::clear_payload(&ctx);
        }
    }

    // Do explorador de arquivos.
    if let Some(paths) = crate::file_tree::dragged_paths(&ctx)
        && paths.iter().any(|path| notes.accepts(path))
    {
        hovering |= pointer.is_some_and(outside_panel);
    }
    if let Some((paths, position)) = crate::file_tree::released_drag(&ctx)
        && rect.contains(position)
    {
        crate::file_tree::clear_released_drag(&ctx);
        let accepted: Vec<String> = paths
            .iter()
            .filter(|path| notes.accepts(path))
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .collect();
        if outside_panel(position) && !accepted.is_empty() {
            let world = state.to_world(position);
            let ids = state.canvas.add_files_at(&accepted, world.0, world.1);
            state.selected = ids;
            state.selected_edge = None;
            changed |= state.commit(tab);
        }
    }

    if hovering {
        let accent = theme::accent(ui);
        let painter = ui.painter_at(rect);
        painter.rect_stroke(
            rect.shrink(3.0),
            6.0,
            Stroke::new(2.5, accent),
            StrokeKind::Inside,
        );
        painter.text(
            Pos2::new(rect.center().x, rect.min.y + 64.0),
            Align2::CENTER_CENTER,
            "Solte para pôr a nota no quadro",
            FontId::new(14.0, theme::fonts::text("semibold")),
            accent,
        );
        ctx.request_repaint();
    }
    changed
}

// ---------------------------------------------------------------------------------------------
// Desenho
// ---------------------------------------------------------------------------------------------

fn tint(rgb: [u8; 3], alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], alpha)
}

fn node_colors(ui: &egui::Ui, node: &Node, selected: bool) -> (Color32, Stroke) {
    let visuals = ui.visuals();
    let rgb = node.color.as_deref().and_then(color_rgb);
    let (fill, border) = match rgb {
        Some(rgb) => (tint(rgb, 30), Color32::from_rgb(rgb[0], rgb[1], rgb[2])),
        None => (
            visuals.faint_bg_color,
            visuals
                .widgets
                .noninteractive
                .fg_stroke
                .color
                .gamma_multiply(0.45),
        ),
    };
    let stroke = if selected {
        Stroke::new(3.0, theme::accent(ui))
    } else {
        Stroke::new(1.5, border)
    };
    (fill, stroke)
}

fn paint_background(ui: &egui::Ui, painter: &egui::Painter, rect: egui::Rect, state: &CanvasState) {
    painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
    let spacing = 20.0 * state.camera.zoom;
    if spacing < 8.0 {
        return;
    }
    let origin = state.origin();
    let dot = ui
        .visuals()
        .widgets
        .noninteractive
        .bg_stroke
        .color
        .gamma_multiply(0.6);
    let start_x = rect.min.x + (origin.x - rect.min.x).rem_euclid(spacing);
    let start_y = rect.min.y + (origin.y - rect.min.y).rem_euclid(spacing);
    let columns = ((rect.width() / spacing) as usize + 1).min(300);
    let rows = ((rect.height() / spacing) as usize + 1).min(300);
    for row in 0..rows {
        for column in 0..columns {
            painter.circle_filled(
                Pos2::new(
                    start_x + column as f32 * spacing,
                    start_y + row as f32 * spacing,
                ),
                1.0,
                dot,
            );
        }
    }
}

fn paint_groups_and_nodes(
    ui: &egui::Ui,
    painter: &egui::Painter,
    state: &CanvasState,
    notes: &Notes,
) {
    let viewport = state.area.expand(40.0);
    let editing_id = state.editing.as_ref().map(|editing| editing.what_id());
    for pass in [true, false] {
        for node in &state.canvas.nodes {
            if (node.node_kind() == NodeKind::Group) != pass {
                continue;
            }
            let rect = state.screen_rect(node.rect());
            if !rect.intersects(viewport) {
                continue;
            }
            let selected = state.selected.contains(&node.id);
            let (fill, stroke) = node_colors(ui, node, selected);
            let zoom = state.camera.zoom;
            let rounding = (8.0 * zoom).clamp(2.0, 12.0);
            painter.rect_filled(rect, rounding, fill);
            painter.rect_stroke(rect, rounding, stroke, StrokeKind::Inside);
            let editing_this = editing_id.as_deref() == Some(node.id.as_str());
            paint_content(ui, painter, state, node, rect, notes, editing_this);
        }
    }
}

fn paint_content(
    ui: &egui::Ui,
    painter: &egui::Painter,
    state: &CanvasState,
    node: &Node,
    rect: egui::Rect,
    notes: &Notes,
    editing: bool,
) {
    let vault = notes.vault;
    let zoom = state.camera.zoom;
    let text_color = ui.visuals().text_color();
    let weak = ui.visuals().weak_text_color();
    let clip = painter.with_clip_rect(rect.shrink(1.0).intersect(painter.clip_rect()));
    let pad = 10.0 * zoom;
    match node.node_kind() {
        NodeKind::Text => {
            if editing || zoom < 0.2 {
                return;
            }
            paint_markdown(
                &clip,
                rect.shrink(pad),
                node.text.as_deref().unwrap_or(""),
                zoom,
                text_color,
            );
        }
        NodeKind::Group => {
            let label = node.label.as_deref().unwrap_or("");
            if !label.is_empty() && !editing && zoom >= 0.15 {
                let rgb = node.color.as_deref().and_then(color_rgb);
                let color = rgb.map_or(weak, |rgb| Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
                clip.text(
                    rect.min + Vec2::new(10.0, 4.0) * zoom.max(0.6),
                    Align2::LEFT_TOP,
                    label,
                    FontId::new(
                        (14.0 * zoom).clamp(9.0, 30.0),
                        theme::fonts::text("semibold"),
                    ),
                    color,
                );
            }
        }
        NodeKind::File if node.is_note_card() => {
            paint_note_card(ui, painter, state, node, rect, notes, editing);
        }
        NodeKind::File => {
            if zoom < 0.15 {
                return;
            }
            let file = node.file.as_deref().unwrap_or("");
            let title_size = (15.0 * zoom).clamp(8.0, 36.0);
            clip.text(
                rect.min + Vec2::splat(pad),
                Align2::LEFT_TOP,
                node.file_title(),
                FontId::new(title_size, theme::fonts::text("semibold")),
                theme::accent(ui),
            );
            let body = egui::Rect::from_min_max(
                rect.min + Vec2::new(pad, pad + title_size * 1.6),
                rect.max - Vec2::splat(pad),
            );
            if body.height() < 8.0 || zoom < 0.3 {
                return;
            }
            if rustidian::assets::is_image(std::path::Path::new(file)) {
                if let Some(vault) = vault {
                    let uri = format!("file://{}", vault.root.join(file).display());
                    egui::Image::new(uri)
                        .fit_to_exact_size(body.size())
                        .paint_at(ui, body);
                }
                return;
            }
            let preview = vault
                .and_then(|vault| vault.note(std::path::Path::new(file)))
                .map(|note| preview_text(&note.text));
            match preview {
                Some(text) => paint_markdown(&clip, body, &text, zoom * 0.9, weak),
                None => {
                    clip.text(
                        body.min,
                        Align2::LEFT_TOP,
                        if vault.is_some() && file.ends_with(".md") {
                            "Nota não encontrada no cofre"
                        } else {
                            ""
                        },
                        FontId::proportional((13.0 * zoom).clamp(8.0, 28.0)),
                        weak,
                    );
                }
            }
        }
        NodeKind::Link => {
            if zoom < 0.15 {
                return;
            }
            let url = node.url.as_deref().unwrap_or("");
            let size = (14.0 * zoom).clamp(8.0, 32.0);
            clip.text(
                rect.min + Vec2::splat(pad),
                Align2::LEFT_TOP,
                url,
                FontId::new(size, theme::fonts::text("semibold")),
                theme::accent(ui),
            );
            if zoom >= 0.4 && rect.height() > size * 4.0 {
                clip.text(
                    rect.min + Vec2::new(pad, pad + size * 1.8),
                    Align2::LEFT_TOP,
                    "Clique no título para abrir",
                    FontId::proportional((12.0 * zoom).clamp(8.0, 26.0)),
                    weak,
                );
            }
        }
        NodeKind::Other => {
            clip.text(
                rect.min + Vec2::splat(pad),
                Align2::LEFT_TOP,
                format!("Cartão “{}”", node.kind),
                FontId::proportional((13.0 * zoom).clamp(8.0, 28.0)),
                weak,
            );
        }
    }
}

/// O começo do corpo de uma nota, sem o bloco de propriedades do topo.
fn preview_text(text: &str) -> String {
    let body = match text
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---"))
    {
        Some((_, after)) => after.trim_start_matches(['\r', '\n']),
        None => text,
    };
    body.chars().take(700).collect()
}

fn paint_markdown(
    painter: &egui::Painter,
    area: egui::Rect,
    text: &str,
    zoom: f32,
    color: Color32,
) {
    if area.width() < 4.0 || area.height() < 4.0 {
        return;
    }
    let galley = markdown_galley(painter, area.width(), text, zoom, color);
    painter.galley(area.min, galley, color);
}

/// O texto em Markdown simples já disposto na largura dada.
fn markdown_galley(
    painter: &egui::Painter,
    width: f32,
    text: &str,
    zoom: f32,
    color: Color32,
) -> Arc<egui::Galley> {
    let base = 15.0 * zoom;
    let mut job = LayoutJob {
        wrap: egui::text::TextWrapping {
            max_width: width,
            ..Default::default()
        },
        ..Default::default()
    };
    let blocks = canvas::markdown_blocks(text);
    for (index, block) in blocks.iter().enumerate() {
        if index > 0 {
            job.append(
                "\n",
                0.0,
                TextFormat::simple(FontId::proportional(base), color),
            );
        }
        let size = match block.kind {
            BlockKind::Heading(1) => base * 1.6,
            BlockKind::Heading(2) => base * 1.4,
            BlockKind::Heading(3) => base * 1.2,
            BlockKind::Heading(_) => base * 1.1,
            _ => base,
        };
        let prefix = match block.kind {
            BlockKind::Bullet => "•  ",
            BlockKind::Quote => "│ ",
            BlockKind::Rule => "────────────",
            _ => "",
        };
        if !prefix.is_empty() {
            job.append(
                prefix,
                0.0,
                TextFormat::simple(FontId::new(size, FontFamily::Proportional), color),
            );
        }
        let heading = matches!(block.kind, BlockKind::Heading(_));
        for span in &block.spans {
            let family = if span.code {
                FontFamily::Monospace
            } else if span.bold || heading {
                theme::fonts::text("bold")
            } else if span.italic || block.kind == BlockKind::Quote {
                theme::fonts::text("italic")
            } else {
                theme::fonts::text("regular")
            };
            let mut format = TextFormat::simple(FontId::new(size, family), color);
            if span.code {
                format.background = color.gamma_multiply(0.12);
            }
            job.append(&span.text, 0.0, format);
        }
    }
    painter.layout_job(job)
}

/// Um cartão de nota "vivo" (H1): título e botões no cabeçalho e, expandido, o texto da nota
/// em Markdown simples com rolagem própria. O texto vem da aba aberta (com a edição que ainda
/// não foi salva) ou do índice do cofre.
fn paint_note_card(
    ui: &egui::Ui,
    painter: &egui::Painter,
    state: &CanvasState,
    node: &Node,
    rect: egui::Rect,
    notes: &Notes,
    editing: bool,
) {
    let zoom = state.camera.zoom;
    if zoom < 0.15 {
        return;
    }
    let text_color = ui.visuals().text_color();
    let weak = ui.visuals().weak_text_color();
    let accent = theme::accent(ui);
    let pad = 10.0 * zoom;
    let collapsed = node.is_collapsed();
    let header_height = canvas::NOTE_HEADER as f32 * zoom;
    let buttons = header_buttons(rect, zoom);
    let show_buttons = buttons_visible(zoom);
    let title_right = if show_buttons {
        buttons.aside.min.x - 6.0
    } else {
        rect.max.x - pad
    };
    let title_size = (15.0 * zoom).clamp(8.0, 36.0);
    painter
        .with_clip_rect(
            egui::Rect::from_min_max(rect.min, Pos2::new(title_right, rect.max.y))
                .intersect(painter.clip_rect()),
        )
        .text(
            rect.min + Vec2::new(pad, ((header_height - title_size) / 2.0).max(2.0)),
            Align2::LEFT_TOP,
            node.file_title(),
            FontId::new(title_size, theme::fonts::text("semibold")),
            accent,
        );
    if show_buttons {
        let pointer = ui.input(|input| input.pointer.hover_pos());
        let mut label = None;
        for (button, aside) in [(buttons.collapse, false), (buttons.aside, true)] {
            let hot = pointer.is_some_and(|pointer| button.contains(pointer));
            if hot {
                painter.rect_filled(button, 4.0, accent.gamma_multiply(0.22));
                label = Some((
                    button,
                    if aside {
                        "Abrir ao lado"
                    } else if collapsed {
                        "Expandir"
                    } else {
                        "Recolher"
                    },
                ));
            }
            let color = if hot { accent } else { weak };
            if aside {
                let inner = button.shrink(button.width() * 0.2);
                painter.rect_stroke(inner, 2.0, Stroke::new(1.3, color), StrokeKind::Inside);
                let divider = inner.min.x + inner.width() * 0.55;
                painter.line_segment(
                    [
                        Pos2::new(divider, inner.min.y),
                        Pos2::new(divider, inner.max.y),
                    ],
                    Stroke::new(1.3, color),
                );
                painter.rect_filled(
                    egui::Rect::from_min_max(Pos2::new(divider, inner.min.y), inner.max),
                    1.0,
                    color.gamma_multiply(0.5),
                );
            } else {
                let center = button.center();
                let half = button.width() * 0.28;
                let points = if collapsed {
                    vec![
                        center + Vec2::new(-half * 0.6, -half),
                        center + Vec2::new(-half * 0.6, half),
                        center + Vec2::new(half * 0.8, 0.0),
                    ]
                } else {
                    vec![
                        center + Vec2::new(-half, -half * 0.6),
                        center + Vec2::new(half, -half * 0.6),
                        center + Vec2::new(0.0, half * 0.8),
                    ]
                };
                painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
            }
        }
        if let Some((button, text)) = label {
            let galley =
                painter.layout_no_wrap(text.to_owned(), FontId::proportional(11.5), text_color);
            let area = egui::Rect::from_min_size(
                Pos2::new(button.max.x - galley.size().x - 8.0, button.max.y + 4.0),
                galley.size() + Vec2::new(8.0, 4.0),
            );
            painter.rect_filled(area, 4.0, ui.visuals().extreme_bg_color);
            painter.rect_stroke(area, 4.0, Stroke::new(1.0, weak), StrokeKind::Inside);
            painter.galley(area.min + Vec2::new(4.0, 2.0), galley, text_color);
        }
    }
    if collapsed || editing || zoom < 0.3 {
        return;
    }
    let body = note_body_rect(rect, zoom);
    if body.height() < 8.0 || body.width() < 8.0 {
        return;
    }
    painter.line_segment(
        [
            Pos2::new(rect.min.x + pad, rect.min.y + header_height),
            Pos2::new(rect.max.x - pad, rect.min.y + header_height),
        ],
        Stroke::new(1.0, weak.gamma_multiply(0.35)),
    );
    let clip = painter.with_clip_rect(body.intersect(painter.clip_rect()));
    let file = node.file.as_deref().unwrap_or("");
    let note_size = FontId::proportional((13.0 * zoom).clamp(8.0, 28.0));
    match notes.content(Path::new(file)) {
        Content::Missing => {
            clip.text(
                body.min,
                Align2::LEFT_TOP,
                "Nota não encontrada no cofre",
                note_size,
                ui.visuals().warn_fg_color,
            );
        }
        Content::Text(text) => {
            let text = canvas::card_body(text);
            if text.trim().is_empty() {
                clip.text(
                    body.min,
                    Align2::LEFT_TOP,
                    "Nota vazia. Clique duas vezes para escrever.",
                    note_size,
                    weak,
                );
                return;
            }
            let galley = markdown_galley(painter, body.width(), text, zoom * 0.95, text_color);
            let max = (galley.size().y - body.height()).max(0.0);
            let scroll = {
                let mut views = state.card_view.borrow_mut();
                let view = views.entry(node.id.clone()).or_default();
                view.max = max;
                view.scroll = view.scroll.clamp(0.0, max);
                view.scroll
            };
            clip.galley(body.min - Vec2::new(0.0, scroll), galley, text_color);
            if max > 0.0 {
                let track = body.height();
                let thumb = (track * body.height() / (body.height() + max)).max(16.0);
                let top = body.min.y + (track - thumb) * (scroll / max);
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        Pos2::new(rect.max.x - 5.0, top),
                        Vec2::new(3.0, thumb),
                    ),
                    1.5,
                    weak.gamma_multiply(0.6),
                );
            }
        }
    }
}

fn paint_edges(ui: &egui::Ui, painter: &egui::Painter, state: &CanvasState) {
    let zoom = state.camera.zoom;
    let weak = ui
        .visuals()
        .widgets
        .noninteractive
        .fg_stroke
        .color
        .gamma_multiply(0.7);
    for edge in &state.canvas.edges {
        let Some(curve) = state.canvas.curve(edge) else {
            continue;
        };
        let selected = state.selected_edge.as_deref() == Some(edge.id.as_str());
        let rgb = edge.color.as_deref().and_then(color_rgb);
        let color = if selected {
            theme::accent(ui)
        } else {
            rgb.map_or(weak, |rgb| Color32::from_rgb(rgb[0], rgb[1], rgb[2]))
        };
        let width = ((if selected { 3.0 } else { 2.0 }) * zoom).clamp(1.0, 5.0);
        let stroke = Stroke::new(width, color);
        let points = curve.points.map(|point| state.to_screen(point));
        painter.add(CubicBezierShape::from_points_stroke(
            points,
            false,
            Color32::TRANSPARENT,
            stroke,
        ));
        let size = (11.0 * zoom).clamp(7.0, 20.0);
        if edge.to_arrow() {
            arrow_head(painter, points[3], curve.tangent(1.0), size, color);
        }
        if edge.from_arrow() {
            let (tx, ty) = curve.tangent(0.0);
            arrow_head(painter, points[0], (-tx, -ty), size, color);
        }
        if let Some(label) = edge.label.as_deref().filter(|label| !label.is_empty())
            && zoom >= 0.3
            && state
                .editing
                .as_ref()
                .is_none_or(|editing| !matches!(&editing.what, Edit::Edge(id) if *id == edge.id))
        {
            let middle = state.to_screen(curve.at(0.5));
            let galley = painter.layout_no_wrap(
                label.to_owned(),
                FontId::new(
                    (13.0 * zoom).clamp(9.0, 24.0),
                    theme::fonts::text("regular"),
                ),
                ui.visuals().text_color(),
            );
            let area = egui::Rect::from_center_size(middle, galley.size() + Vec2::new(10.0, 4.0));
            painter.rect_filled(area, 4.0, ui.visuals().extreme_bg_color);
            painter.rect_stroke(area, 4.0, Stroke::new(1.0, color), StrokeKind::Inside);
            painter.galley(
                area.center() - galley.size() / 2.0,
                galley,
                ui.visuals().text_color(),
            );
        }
    }
}

fn arrow_head(
    painter: &egui::Painter,
    tip: Pos2,
    direction: (f64, f64),
    size: f32,
    color: Color32,
) {
    let direction = Vec2::new(direction.0 as f32, direction.1 as f32);
    let back = tip - direction * size;
    let side = Vec2::new(-direction.y, direction.x) * size * 0.5;
    painter.add(egui::Shape::convex_polygon(
        vec![tip, back + side, back - side],
        color,
        Stroke::NONE,
    ));
}

fn paint_selection_tools(painter: &egui::Painter, state: &CanvasState, ui: &egui::Ui) {
    if state.selected.len() != 1 || state.editing.is_some() {
        return;
    }
    let Some(rect) = state.node_screen_rect(&state.selected[0]) else {
        return;
    };
    let accent = theme::accent(ui);
    let fill = ui.visuals().extreme_bg_color;
    for (_, position) in handle_positions(rect) {
        let handle = egui::Rect::from_center_size(position, Vec2::splat(8.0));
        painter.rect_filled(handle, 2.0, fill);
        painter.rect_stroke(handle, 2.0, Stroke::new(1.5, accent), StrokeKind::Inside);
    }
    for (_, position) in link_dots(rect) {
        painter.circle_filled(position, 5.0, accent);
        painter.circle_stroke(position, 5.0, Stroke::new(1.5, fill));
    }
}
