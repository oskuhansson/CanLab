//! LaTeX emission in Abitti's dialect.
//!
//! The rules here are copied from the live editor's LaTeX pane, not from any
//! textbook (see `docs/mathquill-interaction.md` §8):
//!
//! * an empty argument prints as a single space inside braces — `\frac{ }{ }`,
//!   `\sqrt{ }`, `x^{ }` — except inside a bracket group, where it prints nothing
//!   (`\left(\right)`);
//! * a superscript/subscript argument of exactly one character loses its braces
//!   (`x^2`), anything longer keeps them (`x^{2y}`);
//! * every delimiter is `\left…\right`;
//! * a space is `\ ` and a comma is `{,}`;
//! * after a command token, a following letter grows a space, because `\ley`
//!   would be a different command (`x\le y` but `2\le3`, `\sin x` but `\sin2`).

use crate::tree::{Node, Row};

/// Render a row as LaTeX.
pub fn emit(row: &Row) -> String {
    let mut e = Emitter::default();
    e.row(row);
    e.out
}

#[derive(Default)]
struct Emitter {
    out: String,
    /// The previous operand was a command token, so a following letter needs a
    /// separating space.
    prev_sym: bool,
}

impl Emitter {
    fn row(&mut self, row: &Row) {
        for node in row {
            self.node(node);
        }
    }

    fn node(&mut self, node: &Node) {
        match node {
            Node::Text(s) => {
                let t = escape_text(s);
                self.operand(&t, false);
            }
            Node::Sym(s) => self.operand(s, true),
            Node::Frac { num, den } => {
                let s = format!("\\frac{{{}}}{{{}}}", braced(num), braced(den));
                self.operand(&s, false);
            }
            Node::Binom { top, bottom } => {
                let s = format!("\\binom{{{}}}{{{}}}", braced(top), braced(bottom));
                self.operand(&s, false);
            }
            Node::Sqrt { index, body } => {
                let s = match index {
                    None => format!("\\sqrt{{{}}}", braced(body)),
                    Some(i) => format!("\\sqrt[{}]{{{}}}", emit(i), braced(body)),
                };
                self.operand(&s, false);
            }
            Node::Sup(row) => {
                let s = script('^', row);
                self.operand(&s, false);
            }
            Node::Sub(row) => {
                let s = script('_', row);
                self.operand(&s, false);
            }
            Node::Group { delim, body, .. } => {
                let s = format!("{}{}{}", delim.open(), emit(body), delim.close());
                self.operand(&s, false);
            }
            Node::Accent { kind, body } => {
                let s = format!("{}{{{}}}", kind.command(), braced(body));
                self.operand(&s, false);
            }
        }
    }

    fn operand(&mut self, s: &str, is_sym: bool) {
        if s.is_empty() {
            return;
        }
        if self.prev_sym && s.starts_with(|c: char| c.is_ascii_alphabetic()) {
            self.out.push(' ');
        }
        self.out.push_str(s);
        // Only a *bare* command needs protecting from the next letter: `\le y`
        // but `\le3`, and `\text{ddot}x` with no space at all, because the brace
        // already ends the command. [OBSERVED]
        self.prev_sym = is_sym && s.ends_with(|c: char| c.is_ascii_alphabetic());
    }
}

/// The body of a required argument: an empty one prints as `{ }`.
fn braced(row: &Row) -> String {
    let inner = emit(row);
    if inner.is_empty() {
        " ".to_string()
    } else {
        inner
    }
}

/// `^x` for a one-character argument, `^{2y}` otherwise, `^{ }` when empty.
fn script(marker: char, row: &Row) -> String {
    let inner = emit(row);
    if inner.is_empty() {
        format!("{marker}{{ }}")
    } else if inner.chars().count() == 1 {
        format!("{marker}{inner}")
    } else {
        format!("{marker}{{{inner}}}")
    }
}

fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            ' ' => out.push_str("\\ "),
            ',' => out.push_str("{,}"),
            '%' | '#' | '&' | '$' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}
