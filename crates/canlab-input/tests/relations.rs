//! The comparison has to survive the reader.
//!
//! `build_expr` refuses a comparison on purpose: a relation is not a value. The
//! equation reader only knows `=`. If neither path keeps `<` or `>`, then the
//! solver receives `left − right` and cannot tell an inequality from its
//! opposite — these tests pin the reader that does keep it.

use canlab_core::Comparator;
use canlab_input::MathField;

fn field(latex: &str) -> MathField {
    // Typing, not `from_latex`: `|` and `<` are the keys a user presses, and the
    // printer's `\left|…\right|` form is what `from_latex` reads.
    let mut field = MathField::new();
    field.type_str(latex);
    field
}

#[test]
fn less_and_greater_are_different_relations_with_the_same_sides() {
    let less = field("x<2")
        .build_relation_with(&canlab_input::Definitions::default())
        .expect("readable")
        .expect("an inequality");
    let greater = field("x>2")
        .build_relation_with(&canlab_input::Definitions::default())
        .expect("readable")
        .expect("an inequality");

    assert_eq!(less.comparator, Comparator::Less);
    assert_eq!(greater.comparator, Comparator::Greater);
    assert_eq!(less.left, greater.left);
    assert_eq!(less.right, greater.right);
}

#[test]
fn the_command_spellings_are_the_non_strict_comparators() {
    for (text, comparator) in [
        ("x\\le2", Comparator::LessOrEqual),
        ("x\\leq2", Comparator::LessOrEqual),
        ("x\\ge2", Comparator::GreaterOrEqual),
        ("x\\geq2", Comparator::GreaterOrEqual),
    ] {
        let relation = field(text)
            .build_relation_with(&canlab_input::Definitions::default())
            .unwrap_or_else(|error| panic!("{text} did not read: {error}"))
            .unwrap_or_else(|| panic!("{text} did not read as an inequality"));
        assert_eq!(relation.comparator, comparator, "{text}");
    }
}

#[test]
fn an_absolute_value_keeps_its_bars() {
    // The bars are the function. Reading `|x − 1|` as `(x − 1)` would answer a
    // different question with the same bounds, which is the worst kind of bug.
    let relation = field("|x-1|<2")
        .build_relation_with(&canlab_input::Definitions::default())
        .expect("readable")
        .expect("an inequality");
    assert_eq!(relation.left.func_name().as_deref(), Some("abs"));
    assert_eq!(relation.left.to_string(), "abs(-1 + x)");
    assert_eq!(relation.right.to_string(), "2");
    assert_eq!(relation.comparator, Comparator::Less);
}

#[test]
fn a_plain_equation_is_not_an_inequality() {
    assert_eq!(
        field("x=1")
            .build_relation_with(&canlab_input::Definitions::default())
            .expect("readable"),
        None
    );
    // A comparison in a second line is a page of questions, not one relation;
    // this reader declines it rather than picking one.
    let mut document = field("x<2");
    document.new_line();
    document.type_str("x>3");
    assert_eq!(
        document
            .build_relation_with(&canlab_input::Definitions::default())
            .expect("readable"),
        None
    );
}
