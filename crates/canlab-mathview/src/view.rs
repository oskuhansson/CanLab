//! The egui adapter: measure with the real font, paint the display list, turn a
//! click into a cursor.
//!
//! This layer contains no layout decisions. It exists to (a) provide a
//! [`Measure`] backed by egui's font system and (b) translate [`Item`]s into
//! shapes. Everything hard — and everything worth testing — is in
//! [`crate::layout`].
//!
//! # Coordinates
//!
//! The display list is painted with its top-left at the widget rect's top-left.
//! Text and delimiter positions are baseline-left, and egui anchors text by its
//! top-left, so the painter subtracts `0.8 · height` — the same ascent fraction
//! [`EguiMeasure`] reports. Measurement and painting therefore agree exactly,
//! which is what keeps the rules sitting on the text they belong to.
//!
//! # Interaction
//!
//! Clicking is the only interaction handled here. Arrow keys, `Backspace`, `/`,
//! `^` and the rest are the caller's job: this widget owns no key state, which is
//! what lets the same display list be embedded in a palette preview as easily as
//! in a live editor.

use canlab_input::MathField;
use egui::{Color32, CornerRadius, FontId, Sense, Stroke, StrokeKind};

use crate::layout::{
    DisplayList, Item, Measure, Metrics, Pos2, Style, layout_field, line_at, nearest_in,
};

/// [`Measure`] backed by egui's font stack.
///
/// Widths and heights come from a real galley with no wrapping; vertical metrics
/// are the usual 80 %/20 % approximation of the laid-out height
/// (`ascent ≈ 0.8 · height`), which is exact enough for rules, scripts and
/// delimiters because those only need a consistent baseline, not a font's
/// true ascent table.
#[derive(Clone)]
pub struct EguiMeasure {
    painter: egui::Painter,
}

impl EguiMeasure {
    /// Measure with the fonts and pixels-per-point of `ui`.
    pub fn new(ui: &egui::Ui) -> EguiMeasure {
        EguiMeasure {
            painter: ui.painter().clone(),
        }
    }
}

impl Measure for EguiMeasure {
    fn measure(&self, text: &str, size: f32) -> Metrics {
        // A zero or negative size can only come from a degenerate `Style`; take
        // the smallest size epaint accepts rather than feeding it nonsense.
        let size = size.max(1.0);
        let galley = self.painter.layout_no_wrap(
            text.to_owned(),
            FontId::proportional(size),
            Color32::WHITE,
        );
        let height = galley.size().y;
        Metrics {
            width: galley.size().x,
            ascent: height * 0.8,
            descent: height * 0.2,
        }
    }
}

/// The egui widget.
///
/// A unit struct: all state lives in the [`MathField`] the caller owns, and the
/// cursor is part of that field. `MathView` is only a namespace for `show`.
#[derive(Clone, Copy, Debug, Default)]
pub struct MathView;

impl MathView {
    /// Lay out `field` with `ui`'s fonts, without allocating or painting.
    ///
    /// Useful when the caller needs the size before deciding where the widget
    /// goes (or wants to paint the list itself).
    pub fn layout(ui: &egui::Ui, field: &MathField, style: &Style) -> DisplayList {
        let measure = EguiMeasure::new(ui);
        layout_field(field, &measure, style)
    }

    /// Lay out, paint, and handle clicks.
    ///
    /// Returns the widget's [`egui::Response`]. When the click moved the cursor,
    /// the response is marked changed (`response.changed()`), which is the
    /// caller's cue to [`egui::Response::request_focus`] and start routing keys
    /// into the field.
    ///
    /// The widget is at least `2 × base_size` wide and `1.2 × base_size` tall, so
    /// an empty field is still a visible, clickable target.
    pub fn show(ui: &mut egui::Ui, field: &mut MathField, style: &Style) -> egui::Response {
        let display = Self::layout(ui, field, style);

        let desired = egui::vec2(
            display.size.x.max(style.base_size * 2.0),
            display.size.y.max(style.base_size * 1.2),
        );
        let (rect, mut response) = ui.allocate_exact_size(desired, Sense::click());
        response = response.on_hover_cursor(egui::CursorIcon::Text);

        // Clicking maps a pixel back to one of the field's own positions. In a
        // system it first picks the equation that was clicked, then the caret
        // inside it; the caret list and `positions()` share an order, so the index
        // is the mapping.
        let mut changed = false;
        if response.clicked()
            && let Some(pointer) = response.interact_pointer_pos()
        {
            let at = Pos2::new(pointer.x - rect.min.x, pointer.y - rect.min.y);
            let band = style.base_size * 0.75;
            if let Some(line) = line_at(&display, at) {
                changed |= field.set_line(line);
            }
            let carets = display
                .line_carets
                .get(field.line())
                .map_or(display.carets.as_slice(), |carets| carets.as_slice());
            if let Some(index) = nearest_in(carets, at, band)
                && let Some(target) = field.positions().get(index).cloned()
            {
                changed |= field.set_cursor(target);
            }
        }

        let current = field
            .positions()
            .iter()
            .position(|cursor| cursor == field.cursor());

        let painter = ui.painter().with_clip_rect(rect);
        paint(&painter, &display, rect.min, ui.visuals(), current);

        if changed {
            response.mark_changed();
        }
        response
    }
}

/// Paint one display list with its top-left at `origin`.
fn paint(
    painter: &egui::Painter,
    display: &DisplayList,
    origin: egui::Pos2,
    visuals: &egui::Visuals,
    current_caret: Option<usize>,
) {
    let text_color = visuals.text_color();
    let muted = visuals.weak_text_color();
    // A ghost delimiter is drawn, not hidden: it is half of the bracket pair the
    // user will get, so it has to be visible — just not as loud as the half they
    // actually typed.
    let faint = visuals.weak_text_color().gamma_multiply(0.65);

    for item in &display.items {
        match item {
            Item::Text {
                pos,
                text,
                size,
                italic,
            } => {
                paint_run(painter, origin, pos, text, *size, *italic, text_color);
            }
            Item::Delim {
                pos,
                ch,
                size,
                ghost,
            } => {
                let mut buf = [0u8; 4];
                let text: &str = ch.encode_utf8(&mut buf);
                let color = if *ghost { faint } else { text_color };
                paint_run(painter, origin, pos, text, *size, false, color);
            }
            Item::Rule { rect } => {
                painter.rect_filled(to_egui_rect(*rect, origin), CornerRadius::ZERO, text_color);
            }
            Item::Placeholder { rect } => {
                painter.rect_stroke(
                    to_egui_rect(*rect, origin),
                    CornerRadius::same(2),
                    Stroke::new(1.0, muted),
                    StrokeKind::Inside,
                );
            }
            // Carets are painted separately, once, from the current index.
            Item::Caret { .. } => {}
        }
    }

    if let Some(index) = current_caret
        && let Some(caret) = display.carets.get(index)
    {
        let stroke = visuals.text_cursor.stroke;
        let width = stroke.width.max(1.5);
        let rect = egui::Rect::from_min_size(
            egui::pos2(origin.x + caret.x - width * 0.5, origin.y + caret.y),
            egui::vec2(width, caret.h),
        );
        painter.rect_filled(rect, CornerRadius::ZERO, stroke.color);
    }
}

/// Draw one text run with its baseline-left corner at `pos`.
fn paint_run(
    painter: &egui::Painter,
    origin: egui::Pos2,
    pos: &Pos2,
    text: &str,
    size: f32,
    italic: bool,
    color: Color32,
) {
    if text.is_empty() {
        return;
    }
    let mut job = egui::text::LayoutJob::default();
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: FontId::proportional(size.max(1.0)),
            color,
            italics: italic,
            ..Default::default()
        },
    );
    let galley = painter.layout_job(job);
    let height = galley.size().y;
    let top_left = egui::pos2(origin.x + pos.x, origin.y + pos.y - height * 0.8);
    painter.galley(top_left, galley, color);
}

/// Convert a display-list rect into an egui rect offset by `origin`.
fn to_egui_rect(rect: crate::layout::Rect, origin: egui::Pos2) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(origin.x + rect.x, origin.y + rect.y),
        egui::vec2(rect.w, rect.h),
    )
}
