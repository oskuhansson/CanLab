//! Reading the printer's own output back into a field.
//!
//! The result card typesets a solver's answer by handing the *LaTeX string* to
//! the field, so the two can never disagree about what was produced: the picture
//! and the code beside it come from the same text. These tests are the guard on
//! that: anything `canlab-tex` writes for an ordinary expression must read back
//! into a field, and that field must build the same expression again.

use canlab_core::{add, int, mul, pow, ratio, sym};
use canlab_input::MathField;
use canlab_tex::to_latex;

fn round_trip(expression: canlab_core::Expr) {
    let latex = to_latex(&expression);
    let field = MathField::from_latex(&latex)
        .unwrap_or_else(|error| panic!("could not read {latex:?}: {error}"));
    let back = field
        .build_expr()
        .unwrap_or_else(|error| panic!("could not build {latex:?}: {error}"));
    assert_eq!(back, expression, "via {latex:?}");
}

#[test]
fn simple_expressions_survive_the_round_trip() {
    for expression in [
        sym("x"),
        int(2),
        ratio(1, 2),
        add(vec![sym("x"), int(1)]),
        mul(vec![int(2), sym("x")]),
        pow(sym("x"), int(3)),
        mul(vec![ratio(1, 3), pow(sym("x"), int(3))]),
        add(vec![sym("x"), mul(vec![int(-1), sym("y")])]),
        pow(add(vec![sym("x"), int(1)]), int(2)),
        mul(vec![int(2), int(3)]),
    ] {
        round_trip(expression);
    }
}

#[test]
fn the_printer_output_for_a_result_is_readable() {
    // The exact shape an integral or a solve tends to hand back.
    let expression = mul(vec![ratio(1, 3), pow(sym("x"), int(3))]);
    assert_eq!(to_latex(&expression), "\\frac{1}{3}x^{3}");
    round_trip(expression);
}

#[test]
fn a_function_call_survives_the_round_trip() {
    let expression = canlab_core::expr::ExprNode::fn_raw(
        canlab_core::SymbolId::intern("sin"),
        vec![sym("x")],
    );
    round_trip(expression);
}

#[test]
fn a_reciprocal_is_displayed_as_a_fraction() {
    let expression = pow(sym("x"), int(-1));
    assert_eq!(to_latex(&expression), "\\frac{1}{x}");
    round_trip(expression);
}

#[test]
fn a_root_is_displayed_as_a_radical() {
    let expression = pow(sym("x"), ratio(1, 2));
    assert_eq!(to_latex(&expression), "\\sqrt{x}");
    round_trip(expression);
}

#[test]
fn a_system_display_string_is_refused_rather_than_guessed() {
    // `cases` is outside the display subset; the card falls back to the source.
    assert!(MathField::from_latex("\\begin{cases} x=1 \\\\ y=2 \\end{cases}").is_err());
}

#[test]
fn a_list_of_solutions_reads_as_one_row() {
    // `Solve` joins several answers with `,\quad`.
    let field = MathField::from_latex("1,\\quad 2").expect("a solution list is displayable");
    assert!(!field.to_latex().is_empty());
}

#[test]
fn bare_parentheses_read_as_a_group() {
    // The printer writes `\left(…\right)`, but a hand-written `(a+b)^2` and
    // anything a solver hands back must read too.
    let field = MathField::from_latex("x^2+(y-1)^2=1").expect("an equation with brackets");
    assert!(field.to_latex().contains("\\left("), "got {:?}", field.to_latex());
    assert!(MathField::from_latex("(a+b)^2").is_ok());
}
