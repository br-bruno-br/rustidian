//! Biblioteca de notas do quadro (H1): um painel lateral dentro do quadro com todas as notas do
//! cofre, busca, a marca das que já estão neste quadro e o jeito de pôr uma no quadro (arrastar
//! até o fundo ou o botão +). A mesma nota pode estar em vários quadros e segue um arquivo só.

use crate::theme;
use eframe::egui::{self, Align2, FontId, Pos2, Sense, Stroke, StrokeKind, Vec2};
use rustidian::canvas::{Canvas, LibraryEntry, library_entries};

/// Largura do painel e margens dele no quadro, em pontos.
const WIDTH: f32 = 290.0;
const TOP: f32 = 54.0;
const ROW: f32 = 30.0;

/// O estado do painel enquanto está aberto.
#[derive(Default)]
pub struct Library {
    pub query: String,
    focus: bool,
    /// Onde cada linha e o botão + dela foram desenhados (para os testes de GUI).
    #[cfg(test)]
    pub rows: Vec<(String, bool, egui::Rect, egui::Rect)>,
}

impl Library {
    pub fn opened() -> Self {
        Self {
            focus: true,
            ..Self::default()
        }
    }
}

/// A nota arrastada da biblioteca; quem recebe o soltar está no quadro.
pub struct Dragged(pub String);

/// O que o usuário pediu no painel neste quadro de vídeo.
#[derive(Default)]
pub struct Output {
    /// Pôr esta nota no quadro (botão +).
    pub add: Option<String>,
    pub close: bool,
}

/// O retângulo do painel dentro da área do quadro.
pub fn panel_rect(area: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_size(
        area.min + Vec2::new(10.0, TOP),
        Vec2::new(WIDTH, (area.height() - TOP - 10.0).max(160.0)),
    )
}

pub fn show(
    ui: &mut egui::Ui,
    area: egui::Rect,
    library: &mut Library,
    canvas: &Canvas,
    notes: &[&str],
) -> Output {
    let panel = panel_rect(area);
    let mut output = Output::default();
    #[cfg(test)]
    library.rows.clear();
    // O painel come os cliques: o fundo do quadro, por baixo, não vê o ponteiro aqui.
    ui.interact(
        panel,
        ui.scope_id().with("canvas-library-shield"),
        Sense::click_and_drag(),
    );
    let entries = library_entries(notes.iter().copied(), canvas, &library.query);
    let total = notes.len();
    let in_board = canvas
        .file_paths()
        .iter()
        .filter(|path| notes.iter().any(|note| note.replace('\\', "/") == **path))
        .count();
    ui.scope_builder(egui::UiBuilder::new().max_rect(panel), |ui| {
        egui::Frame::popup(ui.style())
            .inner_margin(theme::space::margin(theme::space::S, theme::space::S))
            .show(ui, |ui| {
                ui.set_width(WIDTH - 2.0 * theme::space::S);
                ui.set_min_height(panel.height() - 2.0 * theme::space::S - 2.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Biblioteca").strong());
                    ui.label(
                        egui::RichText::new(format!("{in_board} de {total} no quadro"))
                            .weak()
                            .size(12.0),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button("×")
                            .on_hover_text("Fechar a biblioteca")
                            .clicked()
                        {
                            output.close = true;
                        }
                    });
                });
                let search = ui.add(
                    egui::TextEdit::singleline(&mut library.query)
                        .hint_text("Buscar nota")
                        .desired_width(f32::INFINITY),
                );
                if library.focus {
                    search.request_focus();
                    library.focus = false;
                }
                ui.add_space(theme::space::XS);
                if entries.is_empty() {
                    ui.label(egui::RichText::new("Nenhuma nota encontrada.").weak());
                    return;
                }
                egui::ScrollArea::vertical()
                    .id_salt("canvas-library-list")
                    .auto_shrink([false, false])
                    .show_rows(ui, ROW, entries.len(), |ui, range| {
                        for entry in &entries[range] {
                            let _geometry = row(ui, entry, &mut output);
                            #[cfg(test)]
                            library.rows.push((
                                entry.path.clone(),
                                entry.in_board,
                                _geometry.0,
                                _geometry.1,
                            ));
                        }
                    });
            });
    });
    if egui::DragAndDrop::has_payload_of_type::<Dragged>(ui.ctx())
        && let Some(entry) = egui::DragAndDrop::payload::<Dragged>(ui.ctx())
        && let Some(pointer) = ui.input(|input| input.pointer.latest_pos())
    {
        ghost(ui, &entry.0, pointer);
    }
    output
}

fn row(ui: &mut egui::Ui, entry: &LibraryEntry, output: &mut Output) -> (egui::Rect, egui::Rect) {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), ROW),
        Sense::click_and_drag(),
    );
    let plus = egui::Rect::from_center_size(
        Pos2::new(rect.right() - 16.0, rect.center().y),
        Vec2::splat(22.0),
    );
    let visuals = ui.visuals();
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 6.0, theme::interactive(ui).gamma_multiply(0.14));
    }
    let painter = ui.painter_at(rect);
    let text_color = visuals.text_color();
    let weak = visuals.weak_text_color();
    let accent = theme::accent(ui);
    let mut left = rect.left() + 8.0;
    if entry.in_board {
        painter.circle_filled(Pos2::new(left + 3.0, rect.center().y), 3.5, accent);
    }
    left += 14.0;
    let title = painter.layout_no_wrap(
        entry.title.clone(),
        FontId::new(13.5, theme::fonts::text("semibold")),
        text_color,
    );
    let title_width = title.size().x;
    let max_right = plus.left() - 6.0;
    painter
        .with_clip_rect(egui::Rect::from_min_max(
            Pos2::new(left, rect.top()),
            Pos2::new(max_right, rect.bottom()),
        ))
        .galley(
            Pos2::new(left, rect.center().y - title.size().y / 2.0),
            title,
            text_color,
        );
    let folder = entry
        .path
        .rsplit_once('/')
        .map(|(folder, _)| folder.to_owned())
        .unwrap_or_default();
    if !folder.is_empty() && left + title_width + 10.0 < max_right {
        painter
            .with_clip_rect(egui::Rect::from_min_max(
                Pos2::new(left + title_width + 8.0, rect.top()),
                Pos2::new(max_right, rect.bottom()),
            ))
            .text(
                Pos2::new(left + title_width + 8.0, rect.center().y),
                Align2::LEFT_CENTER,
                folder,
                FontId::proportional(11.5),
                weak,
            );
    }
    // O botão +: dois traços, sem depender de fonte.
    let over_plus = ui
        .input(|input| input.pointer.hover_pos())
        .is_some_and(|pointer| plus.contains(pointer));
    if over_plus {
        painter.rect_filled(plus, 5.0, accent.gamma_multiply(0.22));
    }
    painter.rect_stroke(plus, 5.0, Stroke::new(1.0, weak), StrokeKind::Inside);
    let center = plus.center();
    painter.line_segment(
        [center - Vec2::new(5.0, 0.0), center + Vec2::new(5.0, 0.0)],
        Stroke::new(1.6, accent),
    );
    painter.line_segment(
        [center - Vec2::new(0.0, 5.0), center + Vec2::new(0.0, 5.0)],
        Stroke::new(1.6, accent),
    );
    if response.drag_started_by(egui::PointerButton::Primary) {
        egui::DragAndDrop::set_payload(ui.ctx(), Dragged(entry.path.clone()));
    }
    if response.clicked_by(egui::PointerButton::Primary)
        && response
            .interact_pointer_pos()
            .is_some_and(|pointer| plus.contains(pointer))
    {
        output.add = Some(entry.path.clone());
    }
    if over_plus {
        response.on_hover_text("Pôr no quadro (ou arraste a nota para o quadro)");
    }
    (rect, plus)
}

/// A nota arrastada acompanha o ponteiro.
fn ghost(ui: &egui::Ui, path: &str, pointer: Pos2) {
    let title = path
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.').map(|(stem, _)| stem))
        .unwrap_or(path);
    let painter = ui.ctx().layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::unique("canvas-library-ghost"),
    ));
    let galley = painter.layout_no_wrap(
        title.to_owned(),
        FontId::new(13.0, theme::fonts::text("semibold")),
        ui.visuals().text_color(),
    );
    let rect = egui::Rect::from_min_size(
        pointer + Vec2::new(12.0, 12.0),
        galley.size() + Vec2::new(20.0, 10.0),
    );
    painter.rect_filled(rect, 7.0, ui.visuals().window_fill());
    painter.rect_stroke(
        rect,
        7.0,
        Stroke::new(1.5, theme::accent(ui)),
        StrokeKind::Inside,
    );
    painter.galley(
        rect.min + Vec2::new(10.0, 5.0),
        galley,
        ui.visuals().text_color(),
    );
}
