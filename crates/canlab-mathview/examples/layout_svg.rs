//! Render the math layout to SVG, so it can be looked at without a window.
//!
//! `cargo run -p canlab-mathview --example layout_svg -- out.svg`
//!
//! This exists because a GPU-composited window cannot be screenshotted in a
//! remote session (`PrintWindow` returns a blank bitmap), but the part that
//! actually has to look right — the maths — is pure geometry that turns into SVG
//! in a few dozen lines. The font is a deterministic stand-in, so the metrics are
//! not the real ones; the structure, spacing, ghost delimiters and caret
//! positions are.

use canlab_input::MathField;
use canlab_mathview::{DisplayList, Item, Measure, Metrics, Style, layout_field};

/// A deterministic font: every character `0.6 em` wide, 80 % above the baseline.
struct Fixed;

impl Measure for Fixed {
    fn measure(&self, text: &str, size: f32) -> Metrics {
        Metrics {
            width: 0.6 * size * text.chars().count() as f32,
            ascent: 0.8 * size,
            descent: 0.2 * size,
        }
    }
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "layout.svg".into());
    let json_out = std::env::args().nth(2);
    let style = Style {
        base_size: 30.0,
        ..Style::default()
    };

    let cases = cases();
    let mut body = String::new();
    let mut scenes = Vec::new();
    let mut y = 46.0;
    for (label, field) in &cases {
        let list = layout_field(field, &Fixed, &style);
        body.push_str(&format!(
            r#"<text class="label" x="30" y="{label_y}">{label}</text>"#,
            label_y = y - 10.0
        ));
        body.push_str(&format!(
            r#"<g transform="translate(30, {y})">{}"#,
            draw(&list)
        ));
        body.push_str("</g>");
        let height = list.size.y;
        scenes.push((label.to_string(), y, list));
        y += height + 46.0;
    }

    let height = y + 20.0;
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="900" height="{height:.0}" viewBox="0 0 900 {height:.0}">
<rect width="900" height="{height:.0}" fill="#1b1b1f"/>
<style>
  text {{ font-family: "DejaVu Sans", "Segoe UI", sans-serif; fill: #e8e8ea; }}
  .label {{ font-size: 13px; fill: #9a9aa2; font-family: "Segoe UI", sans-serif; }}
  .ghost {{ fill: #6a6a72; }}
  .caret {{ fill: #ff9f43; opacity: 0.35; }}
</style>
{body}
</svg>
"##
    );

    std::fs::write(&out, svg).expect("write svg");
    println!("wrote {out} ({} cases)", cases.len());

    if let Some(path) = json_out {
        let mut json = String::from("[");
        for (index, (label, top, list)) in scenes.iter().enumerate() {
            if index > 0 {
                json.push(',');
            }
            json.push_str(&format!(
                r#"{{"label":{},"top":{:.2},"height":{:.2},"items":["#,
                json_string(label),
                top,
                list.size.y
            ));
            for (item_index, item) in list.items.iter().enumerate() {
                if item_index > 0 {
                    json.push(',');
                }
                json.push_str(&json_item(item));
            }
            json.push_str("],\"carets\":[");
            for (caret_index, caret) in list.carets.iter().enumerate() {
                if caret_index > 0 {
                    json.push(',');
                }
                json.push_str(&format!(
                    "[{:.2},{:.2},{:.2}]",
                    caret.x, caret.y, caret.h
                ));
            }
            json.push_str("]}");
        }
        json.push(']');
        std::fs::write(&path, json).expect("write json");
        println!("wrote {path}");
    }
}

fn json_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn json_item(item: &Item) -> String {
    match item {
        Item::Text {
            pos,
            text,
            size,
            italic,
        } => format!(
            r#"{{"k":"t","x":{:.2},"y":{:.2},"s":{:.2},"v":{},"i":{}}}"#,
            pos.x,
            pos.y,
            size,
            json_string(text),
            italic
        ),
        Item::Delim {
            pos,
            ch,
            size,
            ghost,
        } => format!(
            r#"{{"k":"t","x":{:.2},"y":{:.2},"s":{:.2},"v":{},"g":{}}}"#,
            pos.x,
            pos.y,
            size,
            json_string(&ch.to_string()),
            ghost
        ),
        Item::Rule { rect } => format!(
            r#"{{"k":"r","x":{:.2},"y":{:.2},"w":{:.2},"h":{:.2}}}"#,
            rect.x, rect.y, rect.w, rect.h
        ),
        Item::Placeholder { rect } => format!(
            r#"{{"k":"p","x":{:.2},"y":{:.2},"w":{:.2},"h":{:.2}}}"#,
            rect.x, rect.y, rect.w, rect.h
        ),
        Item::Caret { .. } => String::from(r#"{"k":"c"}"#),
    }
}

fn cases() -> Vec<(&'static str, MathField)> {
    let mut plain = MathField::new();
    plain.type_str("1/2x + \\bar{x} + \\vec{v}");
    plain.move_left();

    let mut open = MathField::new();
    open.type_str("(x+1");

    let mut stray = MathField::new();
    stray.type_str("x+1)");

    let mut accents = MathField::new();
    accents.type_str("\\hat{a} + \\tilde{n} + \\dot{x} + \\underline{y} + \\overline{ab}");

    let mut system = MathField::new();
    system.type_str("x+y=3");
    system.new_line();
    system.type_str("2x-y=0");

    let mut page = MathField::new();
    page.type_str("x+y=3");
    page.new_line();
    page.type_str("2x-y=0");
    page.new_system();
    page.type_str("a+b=1");

    let mut slot = MathField::new();
    slot.type_str("\\frac{1}{");

    let mut snippet = MathField::new();
    snippet.apply_snippet(r"\int_{0}^{1}").unwrap();

    vec![
        ("1/2x + \\bar{x} + \\vec{v}   (caret after the first left press)", plain),
        ("(x+1  — the closing paren is a ghost, drawn faint", open),
        ("x+1)  — a stray closer wraps the block; the opening paren is a ghost", stray),
        ("accents typed from the keyboard", accents),
        ("a system: Enter starts the second equation", system),
        ("a page: two systems, the first with two equations", page),
        ("\\frac{1}{  — an empty slot is a legal, reachable state", slot),
        ("palette snippet: \\int_{0}^{1}", snippet),
    ]
}

fn draw(list: &DisplayList) -> String {
    let mut out = String::new();
    for item in &list.items {
        match item {
            Item::Text {
                pos,
                text,
                size,
                italic,
            } => out.push_str(&text_item(pos.x, pos.y, text, *size, *italic, false)),
            Item::Delim {
                pos,
                ch,
                size,
                ghost,
            } => out.push_str(&text_item(
                pos.x,
                pos.y,
                &ch.to_string(),
                *size,
                false,
                *ghost,
            )),
            Item::Rule { rect } => out.push_str(&format!(
                r##"<rect x="{}" y="{}" width="{}" height="{}" fill="#e8e8ea"/>"##,
                rect.x,
                rect.y,
                rect.w,
                rect.h.max(1.0)
            )),
            Item::Placeholder { rect } => out.push_str(&format!(
                r##"<rect x="{}" y="{}" width="{}" height="{}" fill="none" stroke="#6a6a72" stroke-width="1"/>"##,
                rect.x, rect.y, rect.w, rect.h
            )),
            Item::Caret { .. } => {}
        }
    }
    // Every caret position, faint: this is what "the caret sits between
    // characters" means, drawn.
    for caret in &list.carets {
        out.push_str(&format!(
            r#"<rect class="caret" x="{}" y="{}" width="1" height="{}"/>"#,
            caret.x, caret.y, caret.h
        ));
    }
    out
}

fn text_item(x: f32, y: f32, text: &str, size: f32, italic: bool, ghost: bool) -> String {
    let escaped = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        r#"<text x="{x}" y="{y}" font-size="{size}"{style}{class}>{escaped}</text>"#,
        style = if italic { r#" font-style="italic""# } else { "" },
        class = if ghost { r#" class="ghost""# } else { "" },
    )
}
