//! Definitions and derivatives, read from a document.
//!
//! `f(x) = x² + 1` makes `f` a name; `f(3)` substitutes, `f'(x)` differentiates
//! the body first and then substitutes. These are the reader-level tests — the
//! GUI hands it the table, but the rules live here.

use canlab_input::define::Definitions;
use canlab_input::build_with;
use canlab_input::MathField;
use canlab_core::SymbolId;

/// A definition whose body is written as LaTeX.
fn define(definitions: &mut Definitions, name: &str, params: &[&str], body: &str) {
    let expression = MathField::from_latex(body)
        .unwrap_or_else(|error| panic!("body {body:?}: {error}"))
        .to_expr()
        .unwrap_or_else(|| panic!("body {body:?} is not an expression"));
    definitions
        .define(name, params.iter().map(|p| SymbolId::intern(p)).collect(), expression)
        .unwrap_or_else(|error| panic!("defining {name}: {error:?}"));
}

/// Read a document line with the given table.
fn read(definitions: &Definitions, document: &str) -> Result<String, String> {
    let field = MathField::from_latex(document)
        .map_err(|error| format!("unreadable: {error}"))?;
    build_with(field.root(), definitions)
        .map(|expression| expression.to_string())
        .map_err(|error| format!("{error}"))
}

#[test]
fn a_call_substitutes_its_argument() {
    let mut definitions = Definitions::default();
    define(&mut definitions, "f", &["x"], "x^2+1");
    assert_eq!(read(&definitions, "f(3)").unwrap(), "10");
    assert_eq!(read(&definitions, "f(0)").unwrap(), "1");
    // The name alone is not a call, and still means the symbol `f`.
    assert_eq!(read(&definitions, "f").unwrap(), "f");
}

#[test]
fn a_second_parameter_is_substituted_in_order() {
    let mut definitions = Definitions::default();
    define(&mut definitions, "g", &["a", "b"], "a*b");
    assert_eq!(read(&definitions, "g(2,3)").unwrap(), "6");
}

#[test]
fn a_first_derivative_differentiates_the_body() {
    let mut definitions = Definitions::default();
    define(&mut definitions, "f", &["x"], "x^2+1");
    // f'(x) = 2x, then x = 5 → 10
    assert_eq!(read(&definitions, "f'(5)").unwrap(), "10");
}

#[test]
fn higher_derivatives_repeat_the_rule() {
    let mut definitions = Definitions::default();
    define(&mut definitions, "f", &["x"], "x^3");
    // f''(x) = 6x → 12 at x = 2; f'''(x) = 6 → 6 anywhere.
    assert_eq!(read(&definitions, "f''(2)").unwrap(), "12");
    assert_eq!(read(&definitions, "f'''(2)").unwrap(), "6");
    assert_eq!(read(&definitions, "f''''(2)").unwrap(), "0");
}

#[test]
fn a_recursive_definition_is_refused() {
    let mut definitions = Definitions::default();
    let body = MathField::from_latex("f").unwrap().to_expr().unwrap();
    assert!(definitions
        .define("f", vec![SymbolId::intern("x")], body)
        .is_err());
}

#[test]
fn the_wrong_number_of_arguments_is_an_error_not_a_product() {
    let mut definitions = Definitions::default();
    define(&mut definitions, "f", &["x"], "x^2");
    let message = read(&definitions, "f(1,2)").unwrap_err();
    assert!(message.contains("takes 1"), "got {message}");
}
