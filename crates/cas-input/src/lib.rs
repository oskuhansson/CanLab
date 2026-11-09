//! `cas-input` — the structural input model that a text box cannot express.
//!
//! Abitti's formula editor is MathQuill: a WYSIWYG field in which typing builds a
//! *tree* and the cursor lives *inside* it. `1/2x` is `\frac{1}{2x}`, not
//! `\frac{1}{2}x`; `x_1^2` is `x_{1^2}`, because after `_` the cursor is inside
//! the subscript. Reproducing that needs a model in which an empty argument slot
//! is a legal, editable object — something [`cas_core::Expr`] deliberately cannot
//! represent.
//!
//! The behaviour implemented here is specified, with evidence tags, in
//! `docs/mathquill-interaction.md`. The short version:
//!
//! * [`Node`] is a small tree; every argument of every construct is a [`Row`],
//!   and an empty `Row` *is* an empty slot (`\frac{}{}`).
//! * [`Cursor`] is a path plus an offset, so it can sit inside a denominator.
//!   Left/Right walk a flattened depth-first order over the tree, which is exactly
//!   MathQuill's behaviour (see [`MathField::move_right`]).
//! * `/ ^ _` and `Backspace` are *structural*: they move content between slots or
//!   dissolve a construct rather than editing characters.
//! * [`MathField::to_latex`] emits Abitti-flavoured LaTeX (empty slots print as
//!   `{ }`, every bracket is `\left…\right`, `\le` grows a space before a letter).
//! * [`MathField::to_expr`] converts to [`cas_core::Expr`] and refuses, with a
//!   reason, whenever the field is not a well-formed expression.
//!
//! Nothing here has GUI dependencies; it is pure data plus pure functions.
//!
//! # Example
//!
//! ```
//! use cas_input::MathField;
//!
//! let mut f = MathField::new();
//! f.type_str("1/2x");
//! assert_eq!(f.to_latex(), "\\frac{1}{2x}");
//!
//! // The cursor is inside the denominator, so a further keypress lands there.
//! f.type_str("+1");
//! assert_eq!(f.to_latex(), "\\frac{1}{2x+1}");
//! ```

mod cursor;
mod expr;
mod field;
mod latex;
mod template;
mod tree;

pub use cursor::{Cursor, PathStep, Selection};
pub use expr::ToExprError;
pub use field::{EscapeOutcome, MathField};
pub use template::{Template, TemplateError};
pub use tree::{AccentKind, Delim, Node, Row, Slot};

/// Re-export so callers can write `cas_input::Expr` without also depending on
/// `cas-core`.
pub use cas_core::Expr;
