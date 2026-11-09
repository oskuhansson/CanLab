//! The palette, as data.
//!
//! The `action` strings are copied verbatim from Abitti's
//! `src/app/components/toolbar/math-shortcut-data.ts`; the palette sends them to
//! MathQuill as commands. [`Template::from_palette_action`] is the mapping from
//! that table to this model, and it is deliberately total: an action we cannot
//! model returns [`TemplateError::UnsupportedAction`] instead of silently
//! inserting something else.

use crate::tree::Slot;

/// A structure the palette (or a keyboard shortcut) can insert.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Template {
    /// `\frac`, and the `/` key.
    Frac,
    /// `\binom`
    Binom,
    /// `\sqrt` — no optional index.
    Sqrt,
    /// `\nthroot` — `\sqrt[]{ }`, cursor in the index.
    NthRoot,
    /// `^`
    Power,
    /// `_`
    Subscript,
    /// `(` → `\left(\right)`
    Paren,
    /// `[` → `\left[\right]`
    Bracket,
    /// `|` → `\left|\right|`
    Abs,
    /// `{` → `\left\{\right\}`
    Brace,
    /// `\overrightarrow`
    Vector,
    /// `\overline`
    Overline,
    /// `\underrightarrow`
    Underline,
    /// `\int_{ }^{ }`
    Int,
    /// `\sum_{ }^{ }`
    Sum,
    /// `\lim_{ }`
    Lim,
    /// `\sin` — a bare operator token; there is **no** argument slot.
    Sin,
    /// `\cos`
    Cos,
    /// `\tan`
    Tan,
}

/// Why a template could not be inserted.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TemplateError {
    /// The palette action has no equivalent in this model. `cases`, `matrix`,
    /// `array` and the composite actions are in this bucket; see
    /// `docs/mathquill-interaction.md` §10.
    UnsupportedAction(String),
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TemplateError::UnsupportedAction(a) => {
                write!(f, "palette action {a:?} is not modelled by canlab-input")
            }
        }
    }
}

impl std::error::Error for TemplateError {}

impl Template {
    /// Map a palette `action` string (Abitti's shortcut table) to a template.
    pub fn from_palette_action(action: &str) -> Result<Template, TemplateError> {
        let t = match action {
            "\\frac" => Template::Frac,
            "\\binom" => Template::Binom,
            "\\sqrt" => Template::Sqrt,
            "\\nthroot" => Template::NthRoot,
            "^" => Template::Power,
            "_" => Template::Subscript,
            "(" => Template::Paren,
            "[" => Template::Bracket,
            "|" => Template::Abs,
            "\\overrightarrow" => Template::Vector,
            "\\overline" => Template::Overline,
            "\\underrightarrow" => Template::Underline,
            "\\int" => Template::Int,
            "\\sum" => Template::Sum,
            "\\lim_" => Template::Lim,
            "\\sin" => Template::Sin,
            "\\cos" => Template::Cos,
            "\\tan" => Template::Tan,
            other => return Err(TemplateError::UnsupportedAction(other.to_string())),
        };
        Ok(t)
    }

    /// The slot that receives a selection when the template is inserted.
    ///
    /// `None` means the template has no natural content slot (`\int`, `\sin`, …),
    /// so a selection is simply replaced.
    pub fn content_slot(self) -> Option<Slot> {
        match self {
            Template::Frac | Template::Binom => Some(Slot::Num),
            Template::Sqrt | Template::NthRoot => Some(Slot::Body),
            Template::Power => Some(Slot::Sup),
            Template::Subscript => Some(Slot::Sub),
            Template::Paren
            | Template::Bracket
            | Template::Abs
            | Template::Brace
            | Template::Vector
            | Template::Overline
            | Template::Underline => Some(Slot::Body),
            Template::Int | Template::Sum | Template::Lim | Template::Sin | Template::Cos | Template::Tan => None,
        }
    }
}
