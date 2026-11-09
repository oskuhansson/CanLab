//! `cas-mathview` — turning a [`cas_input::MathField`] into pixels, in two layers.
//!
//! * [`layout`] is **pure**: a field plus a [`Measure`](layout::Measure) becomes a
//!   [`DisplayList`](layout::DisplayList). No window, no egui, fully testable.
//! * [`view`] is a thin egui adapter that measures with the real font, paints the
//!   list and turns a click into a cursor.
//!
//! The split is deliberate. A math layout is the part that is easy to get subtly
//! wrong (and expensive to verify by eye), so it lives in a layer where a fake
//! font makes every number assertable; the widget above it only translates
//! `f32`s into shapes.
//!
//! # Quick start
//!
//! ```
//! use cas_input::MathField;
//! use cas_mathview::{layout_field, Measure, Metrics, Style};
//!
//! /// A deterministic font: every character is 0.6 em wide.
//! struct Fixed;
//!
//! impl Measure for Fixed {
//!     fn measure(&self, text: &str, size: f32) -> Metrics {
//!         Metrics {
//!             width: 0.6 * size * text.chars().count() as f32,
//!             ascent: 0.8 * size,
//!             descent: 0.2 * size,
//!         }
//!     }
//! }
//!
//! let mut field = MathField::new();
//! field.type_str("1/2");
//!
//! let list = layout_field(&field, &Fixed, &Style::default());
//!
//! assert!(list.size.x > 0.0 && list.size.y > 0.0);
//! // The caret contract: one rect per position, in the field's own order.
//! assert_eq!(list.carets.len(), field.positions().len());
//! ```
//!
//! In a real application the widget form is one call:
//!
//! ```no_run
//! # use cas_input::MathField;
//! # use cas_mathview::{MathView, Style};
//! # fn demo(ui: &mut egui::Ui, field: &mut MathField) {
//! let response = MathView::show(ui, field, &Style::default());
//! if response.changed() {
//!     response.request_focus();
//! }
//! # }
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod layout;
pub mod view;

pub use layout::{
    layout_field, nearest_caret, DisplayList, Item, Measure, Metrics, Pos2, Rect, Style, Vec2,
};
pub use view::{EguiMeasure, MathView};
