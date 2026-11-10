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

// ------------------------------------------------------------ document harvest

/// Read a whole document, definitions and all.
fn document(lines: &[&str]) -> MathField {
    let owned: Vec<String> = lines.iter().map(|line| line.to_string()).collect();
    MathField::from_latex_lines(&owned).unwrap_or_else(|error| panic!("{error}"))
}

#[test]
fn a_document_defines_and_uses_its_own_function() {
    let field = document(&["f(x) = x^2+1", "f(3)"]);
    let harvest = field.harvest();
    assert!(harvest.problems.is_empty(), "{:?}", harvest.problems);
    assert_eq!(harvest.definitions.len(), 1);
    assert!(harvest.definitions.get("f").is_some());

    // The call is resolved because the table came from the same document.
    let lines = field.build_lines_with(&harvest.definitions).expect("readable");
    assert_eq!(lines.len(), 1, "the definition line is not a value");
    assert_eq!(lines[0].to_string(), "10");

    // And without the table it is a product, which is what it looks like.
    assert_ne!(read(&Definitions::default(), "f(3)").unwrap(), "10");
}

#[test]
fn a_definition_may_come_after_the_line_that_uses_it() {
    // Document-wide, not forward-only: the same document read top-down would
    // still be a product on line 1.
    let field = document(&["f(3)", "f(x) = x^2+1"]);
    let harvest = field.harvest();
    assert!(harvest.problems.is_empty(), "{:?}", harvest.problems);
    let lines = field.build_lines_with(&harvest.definitions).expect("readable");
    assert_eq!(lines[0].to_string(), "10");
}

#[test]
fn a_definition_line_is_not_an_equation() {
    // `f(x) = x² + 1` has an `=` in it but is not a set of points; plotting it
    // as one would draw the curve `f·x - x² - 1 = 0`, which means nothing.
    let field = document(&["f(x) = x^2+1", "x + y = 3"]);
    let equations = field.build_equations().expect("readable");
    assert_eq!(equations.len(), 1, "only the equation");
    assert!(equations[0].1.is_some());
    let rows: Vec<&canlab_input::Row> = field.rows().collect();
    assert_eq!(rows.len(), 2);
    assert!(field.is_definition_row(rows[0]));
    assert!(!field.is_definition_row(rows[1]));
}

#[test]
fn one_broken_definition_does_not_take_the_others_with_it() {
    // `g(x) =` has no body. `f` must still be `f`.
    let field = document(&["f(x) = x^2", "g(x) =", "f(2)"]);
    let harvest = field.harvest();
    assert!(harvest.definitions.get("f").is_some());
    assert!(harvest.definitions.get("g").is_none());
    assert_eq!(harvest.problems.len(), 1);
    assert_eq!(harvest.problems[0].0, "g");
    let lines = field.build_lines_with(&harvest.definitions).expect("readable");
    assert_eq!(lines[0].to_string(), "4");
}

#[test]
fn a_recursive_definition_is_refused_by_the_document() {
    let field = document(&["f(x) = f(x)+1"]);
    let harvest = field.harvest();
    assert!(harvest.definitions.get("f").is_none());
    assert_eq!(harvest.problems.len(), 1);
    assert!(
        matches!(&harvest.problems[0].1, canlab_input::DefineError::Recursive(name) if name == "f"),
        "{:?}",
        harvest.problems
    );
}

#[test]
fn a_two_parameter_definition_is_recognised_and_called() {
    let field = document(&["g(a,b) = a*b", "g(3,4)"]);
    let harvest = field.harvest();
    assert!(harvest.problems.is_empty(), "{:?}", harvest.problems);
    assert_eq!(harvest.definitions.get("g").expect("g").params.len(), 2);
    let lines = field.build_lines_with(&harvest.definitions).expect("readable");
    assert_eq!(lines[0].to_string(), "12");
}

#[test]
fn a_derivative_of_a_document_definition_reads() {
    let field = document(&["f(x) = x^2", "f'(3)"]);
    let harvest = field.harvest();
    let lines = field.build_lines_with(&harvest.definitions).expect("readable");
    assert_eq!(lines[0].to_string(), "6");
}

#[test]
fn an_equation_is_not_mistaken_for_a_definition() {
    for line in ["x^2+y^2=1", "2x+6=0", "a(x+1)=0", "x = 3"] {
        let field = MathField::from_latex(line).unwrap_or_else(|error| panic!("{line}: {error}"));
        assert!(
            !field.is_definition_row(field.root()),
            "{line} was read as a definition"
        );
        assert_eq!(field.harvest().definitions.len(), 0, "{line}");
    }
}

#[test]
fn a_lone_definition_is_not_an_expression() {
    let field = MathField::from_latex("f(x) = x^2+1").expect("a definition");
    assert_eq!(
        field.build_expr_with(&field.definitions()),
        Err(canlab_input::ToExprError::Definition)
    );
}

// ------------------------------------------------------- what a call substitutes

#[test]
fn a_built_in_function_in_a_body_is_substituted() {
    // `f(x) = x + sin x` called with 2 is `2 + sin 2`. Leaving the argument alone
    // would be silently wrong algebra, not a refusal.
    let field = document(&["f(x) = x+\\sin(x)", "f(2)"]);
    let harvest = field.harvest();
    assert!(harvest.problems.is_empty(), "{:?}", harvest.problems);
    let lines = field.build_lines_with(&harvest.definitions).expect("readable");
    let printed = canlab_tex::to_latex(&lines[0]);
    assert!(!printed.contains('x'), "x survived substitution: {printed}");
    assert!(printed.contains("\\sin"), "{printed}");
}

#[test]
fn a_logarithm_in_a_body_is_substituted() {
    let field = document(&["f(x) = \\ln(x)", "f(3)"]);
    let harvest = field.harvest();
    let lines = field.build_lines_with(&harvest.definitions).expect("readable");
    let printed = canlab_tex::to_latex(&lines[0]);
    assert!(!printed.contains('x'), "x survived substitution: {printed}");
}

#[test]
fn a_definition_that_depends_on_a_broken_one_is_reported_not_guessed() {
    // `f` cannot be made, so `g(x) = f(x) + 1` must be *absent* rather than
    // built against a table without `f` — which would read as `f·x + 1`.
    let field = document(&["f(x) = x+", "g(x) = f(x)+1"]);
    let harvest = field.harvest();
    assert!(harvest.definitions.get("f").is_none());
    assert!(
        harvest.definitions.get("g").is_none(),
        "g was built against a missing f: {:?}",
        harvest.definitions.get("g")
    );
    let names: Vec<&String> = harvest.problems.iter().map(|(name, _)| name).collect();
    assert!(names.contains(&&String::from("g")), "{:?}", harvest.problems);
}

#[test]
fn a_space_before_the_bracket_is_still_a_call() {
    // Spaces are layout. `a(x) = b (x)` above `b` must resolve to `b`, not to the
    // product `b·x`.
    let field = document(&["a(x) = b (x)", "b(x) = x^2", "a(3)"]);
    let harvest = field.harvest();
    assert!(harvest.problems.is_empty(), "{:?}", harvest.problems);
    let lines = field.build_lines_with(&harvest.definitions).expect("readable");
    assert_eq!(lines[0].to_string(), "9");
}

#[test]
fn only_the_names_in_a_cycle_are_called_recursive() {
    // `d` merely uses a cycle; telling the user "d cannot be defined in terms of
    // itself" would be a false statement about their document.
    let field = document(&["a(x) = b(x)", "b(x) = a(x)", "d(x) = a(x)"]);
    let harvest = field.harvest();
    let why = |name: &str| {
        harvest
            .problems
            .iter()
            .find(|(problem, _)| problem == name)
            .map(|(_, error)| error.clone())
    };
    assert!(matches!(why("a"), Some(canlab_input::DefineError::Recursive(_))));
    assert!(matches!(why("b"), Some(canlab_input::DefineError::Recursive(_))));
    assert!(
        matches!(why("d"), Some(canlab_input::DefineError::Unreadable(_))),
        "{:?}",
        harvest.problems
    );
}

#[test]
fn a_blank_line_does_not_break_reading() {
    let field = document(&["x+1", ""]);
    let lines = field.build_lines_with(&canlab_input::Definitions::default());
    assert_eq!(lines.map(|lines| lines.len()), Ok(1));
}

