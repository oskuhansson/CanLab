//! Signs, in every position a student can put them.
//!
//! `1·-6 + 2·-3` was read as `1 - 6 + 2 - 3` — a sign after a multiplication was
//! taken for a *binary* minus, so the app answered `-6` instead of `-12`. The
//! corpus had `x--y` and `-3+5` but nothing that put a sign after an operator,
//! which is why nothing caught it.
//!
//! So this file is a table of positions rather than a handful of examples: unary
//! at the start of a line, after `·`, after `+`/`-`, inside a fraction, inside a
//! superscript, in front of a bracket, doubled, and mixed. Every row is checked
//! three ways where it can be:
//!
//! * the value it evaluates to;
//! * the LaTeX the canonicaliser prints, for the rows where the form is the point;
//! * the same document **typed** and **loaded** — they go through different code
//!   (`type_str` builds nodes character by character, `from_latex` parses a
//!   string), and a reader agreement bug hid in that gap before.

use canlab_input::MathField;

/// A document, the value it must evaluate to, and why the row exists.
struct Row {
    document: &'static str,
    value: f64,
    why: &'static str,
}

fn rows() -> Vec<Row> {
    let row = |document: &'static str, value: f64, why: &'static str| Row {
        document,
        value,
        why,
    };
    vec![
        // ------------------------------------------------- a sign after a product
        row("1*-6+2*-3", -12.0, "a sign after a product multiplies"),
        row("2*-3", -6.0, "and on its own"),
        row("1*-6", -6.0, "with a leading unit factor"),
        row("-2*-3", 6.0, "two negatives multiply to a positive"),
        row("2*-3*4", -24.0, "one negative among three factors"),
        row("2*-3*-4", 24.0, "two negatives among four factors"),
        row("2*-3+4*-5", -26.0, "a sign after a product in each term"),
        row("3*--2", 6.0, "a doubled sign after a product"),
        row("1*-6+2", -4.0, "a signed product before a plain term"),
        // ------------------------------------------------------ signs as operators
        row("1-6", -5.0, "a binary minus is still a subtraction"),
        row("1+-6", -5.0, "a unary minus after a binary plus"),
        row("1--6", 7.0, "a doubled minus is a plus"),
        row("3+-2", 1.0, "the same, with the sign on the number"),
        row("3-+2", 1.0, "a plus after a minus"),
        row("+5", 5.0, "a leading plus"),
        row("--5", 5.0, "a doubled leading minus"),
        row("-3+5", 2.0, "a negative literal in a sum"),
        // --------------------------------------------------------- signs and powers
        row("-3^2", -9.0, "a unary minus binds looser than a power"),
        row("(-3)^2", 9.0, "a bracket binds tighter"),
        row("2*-3^2", -18.0, "and after a product too"),
        row("-2^2", -4.0, "the same shape without a product"),
        // ------------------------------------------------------------ fractions
        row("2/-4", -0.5, "a sign in a denominator"),
        row("-2/4", -0.5, "a sign in a numerator"),
        row("-1/2", -0.5, "a unary minus before a fraction"),
        // ---------------------------------------------------------- brackets
        row("-(3)", -3.0, "a unary minus before a bracket"),
        row("-(-3)", 3.0, "a doubled minus before a bracket"),
        row("2*(-3)", -6.0, "a sign inside a product's bracket"),
        row("-(2+3)", -5.0, "a unary minus distributes over a sum"),
    ]
}

/// Evaluate a document at no variables, or `None` when it does not read.
fn value(field: &MathField) -> Option<f64> {
    let expression = field.to_expr()?;
    let program = canlab_core::Program::compile(&expression, &[])?;
    let value = program.eval(&[]);
    value.is_finite().then_some(value)
}

#[test]
fn every_position_of_a_sign_reads_as_the_arithmetic_says() {
    for row in rows() {
        let field = MathField::from_latex(row.document)
            .unwrap_or_else(|error| panic!("{}: {error}", row.document));
        let got = value(&field);
        match got {
            Some(got) => assert!(
                (got - row.value).abs() <= 1e-9 * row.value.abs().max(1.0),
                "{}: expected {} ({}), got {got}",
                row.document,
                row.value,
                row.why
            ),
            None => panic!(
                "{}: reads to nothing, expected {} ({})",
                row.document, row.value, row.why
            ),
        }
    }
}

#[test]
fn typing_a_document_and_loading_it_agree() {
    // The two paths are different code. `typed` goes character by character
    // through the snippet reader's structural rules; `from_latex` parses the whole
    // string. A disagreement is a bug in one of them, and this is the cheapest
    // place to notice.
    for row in rows() {
        let loaded = MathField::from_latex(row.document)
            .unwrap_or_else(|error| panic!("{}: {error}", row.document));
        let mut typed = MathField::new();
        // `*` is the typed multiplication sign; the loaded form uses `\cdot`.
        typed.type_str(&row.document.replace("\\cdot", "*"));
        assert_eq!(
            typed.to_latex(),
            loaded.to_latex(),
            "typing and loading {} disagree",
            row.document
        );
        assert_eq!(
            value(&typed),
            value(&loaded),
            "typing and loading {} evaluate differently",
            row.document
        );
    }
}

#[test]
fn a_sign_before_a_fraction_stays_outside_it() {
    // The `/` key takes the run to its left as the numerator, so a leading minus
    // belongs to the whole fraction, not to the numerator. `-1/2x` is
    // `−1/(2x)`, not `(−1)/2 · x`.
    let field = MathField::from_latex("-1/2x").expect("readable");
    assert_eq!(field.to_latex(), "-\\frac{1}{2x}");
    let mut typed = MathField::new();
    typed.type_str("-1/2x");
    assert_eq!(typed.to_latex(), field.to_latex(), "typing and loading disagree");
}

#[test]
fn a_sign_after_a_product_keeps_its_precedence_in_a_system() {
    // The same reader, as part of an equation: `x = 1·-6` is `x = -6`, not
    // `x = 1 - 6` (which happens to be -5 and would be just as wrong).
    let mut field = MathField::from_latex("x=1*-6").expect("a readable equation");
    let equations = field.build_equations().expect("readable");
    assert_eq!(equations.len(), 1);
    let (left, right) = &equations[0];
    let right = right.as_ref().expect("an equation has two sides");
    let program = canlab_core::Program::compile(right, &[]).expect("no variables");
    assert_eq!(program.eval(&[]), -6.0);
    // And the left side is the variable, unchanged.
    assert_eq!(left.to_string(), "x");
    field.new_line();
    field.type_str("y=2*-3");
    assert_eq!(field.harvest().problems.len(), 0);
}
