//! Layout contract tests — the pure layer, with no window and a fake font.

use canlab_input::{AccentKind, Delim, MathField, Node};
use canlab_mathview::{
    DisplayList, Item, Measure, Metrics, Pos2, Rect, Style, layout_field, nearest_caret,
};

/// A deterministic font: every character is `0.6 em` wide, and every run's box
/// is 80 % above and 20 % below the baseline.
struct FakeMeasure;

impl Measure for FakeMeasure {
    fn measure(&self, text: &str, size: f32) -> Metrics {
        Metrics {
            width: 0.6 * size * text.chars().count() as f32,
            ascent: 0.8 * size,
            descent: 0.2 * size,
        }
    }
}

const EPS: f32 = 1e-3;

fn layout(field: &MathField) -> DisplayList {
    layout_field(field, &FakeMeasure, &Style::default())
}

/// One `Item::Text` with its fields named, so assertions read clearly.
#[derive(Clone, Debug)]
struct TextItem {
    text: String,
    pos: Pos2,
    size: f32,
    italic: bool,
}

fn text_items(list: &DisplayList) -> Vec<TextItem> {
    list.items
        .iter()
        .filter_map(|item| match item {
            Item::Text {
                pos,
                text,
                size,
                italic,
            } => Some(TextItem {
                text: text.clone(),
                pos: *pos,
                size: *size,
                italic: *italic,
            }),
            _ => None,
        })
        .collect()
}

/// The first text item whose content is exactly `needle`.
fn text(list: &DisplayList, needle: &str) -> TextItem {
    text_items(list)
        .into_iter()
        .find(|item| item.text == needle)
        .unwrap_or_else(|| panic!("no text item {needle:?} in {list:#?}"))
}

fn rules(list: &DisplayList) -> Vec<Rect> {
    list.items
        .iter()
        .filter_map(|item| match item {
            Item::Rule { rect } => Some(*rect),
            _ => None,
        })
        .collect()
}

fn placeholders(list: &DisplayList) -> Vec<Rect> {
    list.items
        .iter()
        .filter_map(|item| match item {
            Item::Placeholder { rect } => Some(*rect),
            _ => None,
        })
        .collect()
}

fn delimiters(list: &DisplayList) -> Vec<(char, Pos2, f32)> {
    list.items
        .iter()
        .filter_map(|item| match item {
            Item::Delim { pos, ch, size } => Some((*ch, *pos, *size)),
            _ => None,
        })
        .collect()
}

fn caret_items(list: &DisplayList) -> Vec<Rect> {
    list.items
        .iter()
        .filter_map(|item| match item {
            Item::Caret { rect } => Some(*rect),
            _ => None,
        })
        .collect()
}

fn frac(num: Vec<Node>, den: Vec<Node>) -> Node {
    Node::Frac { num, den }
}

/// A field that exercises every node kind at once, including nesting.
fn kitchen_sink() -> MathField {
    MathField::from_row(vec![
        Node::text("x"),
        Node::Sup(vec![Node::text("2")]),
        Node::text("+"),
        frac(
            vec![Node::Sqrt {
                index: Some(vec![Node::text("3")]),
                body: vec![Node::text("y")],
            }],
            vec![Node::Group {
                delim: Delim::Bracket,
                body: vec![Node::text("z"), Node::Sub(vec![Node::text("1")])],
            }],
        ),
        Node::Binom {
            top: vec![Node::text("n")],
            bottom: vec![Node::text("k")],
        },
        Node::Accent {
            kind: AccentKind::Overline,
            body: vec![Node::text("v")],
        },
    ])
}

// --------------------------------------------------------------- the contract

#[test]
fn caret_count_equals_positions_for_x() {
    let mut field = MathField::new();
    field.type_str("x");
    let list = layout(&field);
    assert_eq!(list.carets.len(), field.positions().len());
    assert_eq!(list.carets.len(), 2, "before and after a single atom");
}

#[test]
fn caret_count_equals_positions_for_superscript() {
    let mut field = MathField::new();
    field.type_str("x^2");
    let list = layout(&field);
    assert_eq!(list.carets.len(), field.positions().len());
    // before `x`, inside the script, after the script's text, after the atom
    assert_eq!(list.carets.len(), 4);
}

#[test]
fn caret_count_equals_positions_for_fraction() {
    let mut field = MathField::new();
    field.type_str("1/2");
    let list = layout(&field);
    assert_eq!(list.carets.len(), field.positions().len());
    // root[0], num[0], num[1], den[0], den[1], root[1]
    assert_eq!(list.carets.len(), 6);
}

#[test]
fn caret_count_equals_positions_for_nested_fractions() {
    let field = MathField::from_row(vec![frac(
        vec![frac(vec![Node::text("1")], vec![Node::text("2")])],
        vec![Node::text("3")],
    )]);
    let list = layout(&field);
    assert_eq!(list.carets.len(), field.positions().len());
    assert_eq!(list.carets.len(), 1 + 6 + 2 + 1);
}

#[test]
fn caret_count_equals_positions_for_every_node_kind() {
    let field = kitchen_sink();
    let list = layout(&field);
    assert_eq!(list.carets.len(), field.positions().len());
    assert!(list.carets.len() > 10, "the sink should be position-rich");
}

#[test]
fn caret_items_mirror_the_caret_vector_in_order() {
    let field = kitchen_sink();
    let list = layout(&field);
    assert_eq!(caret_items(&list), list.carets);
    assert_eq!(caret_items(&list).len(), field.positions().len());
}

#[test]
fn empty_field_lays_out_without_a_base_and_has_one_caret() {
    let field = MathField::new();
    let list = layout(&field);
    assert_eq!(list.carets.len(), field.positions().len());
    assert_eq!(list.carets.len(), 1);
    assert!(list.size.x > 0.0 && list.size.y > 0.0);
    assert_eq!(
        placeholders(&list).len(),
        1,
        "an empty field shows one empty slot"
    );
}

// ---------------------------------------------------------------- fractions

#[test]
fn fraction_rule_width_is_the_wider_argument_plus_padding() {
    let style = Style::default();
    let field = MathField::from_row(vec![frac(vec![Node::text("1")], vec![Node::text("22")])]);
    let list = layout(&field);

    let rule = rules(&list);
    assert_eq!(rule.len(), 1);

    let num_width = 0.6 * style.base_size * 1.0;
    let den_width = 0.6 * style.base_size * 2.0;
    let expected = num_width.max(den_width) + 2.0 * style.pad;
    assert!(
        (rule[0].w - expected).abs() < EPS,
        "rule width {} != {expected}",
        rule[0].w
    );
}

#[test]
fn numerator_is_above_the_rule_and_denominator_below() {
    let field = MathField::from_row(vec![frac(vec![Node::text("1")], vec![Node::text("22")])]);
    let list = layout(&field);
    let rule = rules(&list)[0];
    let numerator = text(&list, "1").pos;
    let denominator = text(&list, "22").pos;

    assert!(
        numerator.y < rule.y,
        "numerator baseline must be above the bar"
    );
    assert!(
        denominator.y > rule.bottom(),
        "denominator baseline must be below the bar"
    );
}

#[test]
fn a_fraction_makes_the_row_taller_than_plain_text() {
    let plain = MathField::from_row(vec![Node::text("1")]);
    let tall = MathField::from_row(vec![frac(vec![Node::text("1")], vec![Node::text("2")])]);
    assert!(layout(&tall).size.y > layout(&plain).size.y);
}

#[test]
fn an_empty_fraction_gets_two_placeholder_boxes() {
    let field = MathField::from_row(vec![frac(Vec::new(), Vec::new())]);
    let list = layout(&field);
    let boxes = placeholders(&list);
    assert_eq!(boxes.len(), 2);
    let style = Style::default();
    for rect in boxes {
        assert!((rect.w - style.empty_box(style.base_size)).abs() < EPS);
        assert!((rect.h - style.empty_box(style.base_size)).abs() < EPS);
    }
}

// ------------------------------------------------------------------- scripts

#[test]
fn superscript_is_smaller_and_raised() {
    let style = Style::default();
    let mut field = MathField::new();
    field.type_str("x^2");
    let list = layout(&field);

    let base = text(&list, "x");
    let script = text(&list, "2");

    assert!(
        script.size < base.size,
        "the script must be smaller than its base"
    );
    assert!(
        (script.size - style.script_size(style.base_size)).abs() < EPS,
        "script size should be base * script_scale"
    );
    assert!(
        script.pos.y < base.pos.y,
        "the script baseline must be raised"
    );
}

#[test]
fn script_size_never_goes_below_the_minimum() {
    let style = Style::default();
    // 22 -> 15.4 -> 10.78 -> floored at 10.
    let deep = Node::Sup(vec![
        Node::text("y"),
        Node::Sup(vec![Node::text("z"), Node::Sup(vec![Node::text("w")])]),
    ]);
    let field = MathField::from_row(vec![Node::text("x"), deep]);
    let list = layout(&field);

    assert!((text(&list, "y").size - 15.4).abs() < EPS);
    assert!((text(&list, "z").size - 10.78).abs() < EPS);
    assert!((text(&list, "w").size - style.min_script_size).abs() < EPS);
}

#[test]
fn a_baseless_superscript_lays_out_and_does_not_panic() {
    let field = MathField::from_row(vec![Node::Sup(vec![Node::text("2")])]);
    let list = layout(&field);

    assert!(list.size.x > 0.0 && list.size.y > 0.0);
    let item = text(&list, "2");
    assert!(item.size < Style::default().base_size);
    assert!(
        item.pos.y < list.baseline,
        "a base-less script is still raised"
    );
    assert_eq!(list.carets.len(), field.positions().len());
}

#[test]
fn a_baseless_subscript_lays_out_and_does_not_panic() {
    let field = MathField::from_row(vec![Node::Sub(vec![Node::text("1")])]);
    let list = layout(&field);

    let item = text(&list, "1");
    assert!(item.size < Style::default().base_size);
    assert!(item.pos.y > list.baseline, "a subscript is lowered");
    assert_eq!(list.carets.len(), field.positions().len());
}

#[test]
fn adjacent_sub_and_superscript_share_one_column() {
    let field = MathField::from_row(vec![
        Node::text("x"),
        Node::Sub(vec![Node::text("1")]),
        Node::Sup(vec![Node::text("2")]),
    ]);
    let list = layout(&field);
    let sub = text(&list, "1");
    let sup = text(&list, "2");

    assert!(
        (sub.pos.x - sup.pos.x).abs() < EPS,
        "scripts stack on the same x"
    );
    assert!(
        sub.pos.y > sup.pos.y,
        "the subscript sits below the superscript"
    );
}

// ---------------------------------------------------------------------- roots

#[test]
fn sqrt_overline_spans_the_body() {
    let style = Style::default();
    let field = MathField::from_row(vec![Node::sqrt(vec![Node::text("xyz")])]);
    let list = layout(&field);

    let rule = rules(&list);
    assert_eq!(rule.len(), 1, "one overline");
    let body_width = 0.6 * style.base_size * 3.0;
    assert!(
        rule[0].w >= body_width - EPS,
        "the overline ({}) must span the body ({body_width})",
        rule[0].w
    );

    let body = text(&list, "xyz");
    assert!(rule[0].y < body.pos.y, "the overline is above the radicand");

    let radical = text(&list, "√");
    assert!(radical.size > style.base_size, "the radical is stretched");
    assert!(radical.pos.x < body.pos.x);
}

#[test]
fn an_indexed_root_puts_a_small_raised_index_left_of_the_radical() {
    let style = Style::default();
    let field = MathField::from_row(vec![Node::Sqrt {
        index: Some(vec![Node::text("3")]),
        body: vec![Node::text("x")],
    }]);
    let list = layout(&field);

    let index = text(&list, "3");
    let radical = text(&list, "√");

    assert!(
        index.pos.x < radical.pos.x,
        "the index is left of the radical"
    );
    assert!(index.size < style.base_size, "the index is set small");
    assert!(index.pos.y < list.baseline, "the index is raised");
}

// --------------------------------------------------------------------- groups

#[test]
fn group_delimiters_are_scaled_to_tall_content() {
    let style = Style::default();
    let field = MathField::from_row(vec![Node::Group {
        delim: Delim::Paren,
        body: vec![frac(vec![Node::text("1")], vec![Node::text("2")])],
    }]);
    let list = layout(&field);

    let delims = delimiters(&list);
    assert_eq!(delims.len(), 2);
    assert_eq!(delims[0].0, '(');
    assert_eq!(delims[1].0, ')');
    assert!(
        delims[0].2 > style.base_size,
        "a delimiter around a fraction must grow"
    );
    assert!(
        delims[1].1.x >= text(&list, "2").pos.x,
        "the closing delimiter is right of the content"
    );
}

#[test]
fn every_delimiter_kind_produces_its_own_glyphs() {
    for (delim, open, close) in [
        (Delim::Paren, '(', ')'),
        (Delim::Bracket, '[', ']'),
        (Delim::Abs, '|', '|'),
        (Delim::Brace, '{', '}'),
    ] {
        let field = MathField::from_row(vec![Node::Group {
            delim,
            body: vec![Node::text("x")],
        }]);
        let list = layout(&field);
        let chars: Vec<char> = delimiters(&list).into_iter().map(|(ch, _, _)| ch).collect();
        assert_eq!(chars, vec![open, close]);
    }
}

// -------------------------------------------------------------------- accents

#[test]
fn a_vector_accent_is_drawn_above_the_base() {
    let field = MathField::from_row(vec![Node::Accent {
        kind: AccentKind::Vector,
        body: vec![Node::text("v")],
    }]);
    let list = layout(&field);
    let arrow = text(&list, "→");
    let base = text(&list, "v");
    assert!(arrow.pos.y < base.pos.y, "the arrow is above the base");
}

#[test]
fn an_overline_accent_is_a_rule_above_the_base() {
    let field = MathField::from_row(vec![Node::Accent {
        kind: AccentKind::Overline,
        body: vec![Node::text("v")],
    }]);
    let list = layout(&field);
    let rule = rules(&list);
    assert_eq!(rule.len(), 1);
    assert!(rule[0].y < text(&list, "v").pos.y);
    assert!(rule[0].w > 0.0);
}

#[test]
fn an_underline_accent_is_drawn_below_the_base() {
    let field = MathField::from_row(vec![Node::Accent {
        kind: AccentKind::Underline,
        body: vec![Node::text("v")],
    }]);
    let list = layout(&field);
    let arrow = text(&list, "→");
    let base = text(&list, "v");
    assert!(arrow.pos.y > base.pos.y, "the arrow is below the base");
}

// ------------------------------------------------------------------- binomial

#[test]
fn a_binomial_stacks_two_rows_between_parens() {
    let field = MathField::from_row(vec![Node::Binom {
        top: vec![Node::text("n")],
        bottom: vec![Node::text("k")],
    }]);
    let list = layout(&field);

    let top = text(&list, "n").pos;
    let bottom = text(&list, "k").pos;
    assert!(top.y < bottom.y, "the top row is above the bottom row");

    let delims = delimiters(&list);
    assert_eq!(delims.len(), 2);
    assert!(delims[0].2 > Style::default().base_size);
}

// -------------------------------------------------------------- text and syms

#[test]
fn width_grows_when_text_is_appended() {
    let mut field = MathField::new();
    field.type_str("x");
    let narrow = layout(&field).size.x;
    field.type_str("y");
    let wide = layout(&field).size.x;
    assert!(wide > narrow, "{wide} should be wider than {narrow}");
}

#[test]
fn a_letter_is_italic_and_a_function_name_is_upright() {
    let mut letters = MathField::new();
    letters.type_str("ab");
    let list = layout(&letters);
    assert!(text(&list, "ab").italic, "a variable run is italic");

    let mut function = MathField::new();
    function.insert_sym("\\sin");
    let list = layout(&function);
    let sin = text(&list, "sin");
    assert!(!sin.italic, "function names are upright");
}

#[test]
fn an_unknown_command_is_shown_as_its_name_rather_than_vanishing() {
    let mut field = MathField::new();
    field.insert_sym("\\foo");
    let list = layout(&field);
    assert!(text_items(&list).iter().any(|item| item.text == "foo"));
}

// ------------------------------------------------------------------ invariants

#[test]
fn every_item_lies_inside_the_display_list() {
    let fields = [
        MathField::new(),
        MathField::from_row(vec![Node::text("x")]),
        MathField::from_row(vec![Node::Sup(vec![Node::text("2")])]),
        MathField::from_row(vec![Node::sqrt(vec![Node::text("xyz")])]),
        MathField::from_row(vec![Node::Group {
            delim: Delim::Brace,
            body: vec![frac(vec![Node::text("1")], vec![Node::text("2")])],
        }]),
        kitchen_sink(),
    ];

    for field in &fields {
        let list = layout(field);
        assert!(list.size.x > 0.0, "a field is never zero-width");
        assert!(list.size.y > 0.0, "a field is never zero-height");
        for item in &list.items {
            match item {
                Item::Text { pos, .. } | Item::Delim { pos, .. } => {
                    assert!(
                        pos.x >= -EPS && pos.x <= list.size.x + EPS,
                        "text at {pos:?} outside {:?}",
                        list.size
                    );
                    assert!(
                        pos.y >= -EPS && pos.y <= list.size.y + EPS,
                        "text baseline at {pos:?} outside {:?}",
                        list.size
                    );
                }
                Item::Rule { rect } | Item::Placeholder { rect } | Item::Caret { rect } => {
                    assert!(
                        rect.x >= -EPS && rect.right() <= list.size.x + EPS,
                        "rect {rect:?} outside {:?}",
                        list.size
                    );
                    assert!(
                        rect.y >= -EPS && rect.bottom() <= list.size.y + EPS,
                        "rect {rect:?} outside {:?}",
                        list.size
                    );
                }
            }
        }
    }
}

#[test]
fn caret_x_is_monotonic_in_a_plain_row() {
    // Separate nodes: `canlab-input` merges `abc` into one `Text` node, which has a
    // position only at each end.
    let field = MathField::from_row(vec![Node::text("a"), Node::text("b"), Node::text("c")]);
    let list = layout(&field);
    assert_eq!(list.carets.len(), field.positions().len());
    assert_eq!(list.carets.len(), 4);
    for pair in list.carets.windows(2) {
        assert!(pair[0].x <= pair[1].x + EPS);
    }
}

#[test]
fn nearest_caret_is_the_closest_position_in_the_row() {
    let field = MathField::from_row(vec![Node::text("a"), Node::text("b"), Node::text("c")]);
    let list = layout(&field);

    // Carets at x = 0, 13.2, 26.4, 39.6; all at the same height.
    assert_eq!(nearest_caret(&list, Pos2::new(1.0, 10.0), 16.5), Some(0));
    assert_eq!(nearest_caret(&list, Pos2::new(14.0, 10.0), 16.5), Some(1));
    assert_eq!(nearest_caret(&list, Pos2::new(25.0, 10.0), 16.5), Some(2));
    assert_eq!(
        nearest_caret(&list, Pos2::new(10.0, 500.0), 16.5),
        None,
        "a click far outside the band selects nothing"
    );
}

#[test]
fn layout_is_deterministic() {
    let field = kitchen_sink();
    assert_eq!(layout(&field), layout(&field));
}

// -------------------------------------------------------------- egui surface

#[test]
fn egui_integration_exists_without_a_window() {
    fn assert_measure<T: Measure>() {}
    assert_measure::<canlab_mathview::EguiMeasure>();

    // A compile-time check that the widget's entry point keeps its signature.
    let _show: fn(&mut egui::Ui, &mut MathField, &Style) -> egui::Response =
        canlab_mathview::MathView::show;
}
