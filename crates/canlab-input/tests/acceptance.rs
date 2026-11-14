//! A corpus of documents and what they should do.
//!
//! This file is meant to be **read**, not just run: every case is a line a
//! Finnish student would plausibly type, paired with the result they would
//! expect. The test prints the whole table as Markdown so the expectations can be
//! reviewed against what the app actually does.
//!
//! ```text
//! cargo test -p canlab-input --test acceptance -- --nocapture
//! ```
//!
//! Three layers are being tested, and confusing them was the first mistake this
//! file caught:
//!
//! * `Latex` — what the editor pane shows, which is what was typed;
//! * `Simplified` — what the result card shows, which is what it means;
//! * `Value` — the number it evaluates to.
//!
//! `Reads` and `Refused` are about whether the document is algebra at all.

use canlab_input::MathField;

/// What the case should produce.
enum Expect {
    /// The document reads and prints exactly this LaTeX (the typed form).
    Latex(&'static str),
    /// The document reads, and its *value* prints as this LaTeX.
    Simplified(&'static str),
    /// The document reads as an expression that evaluates to this number.
    Value(f64),
    /// The document reads, and the value is not the point.
    Reads,
    /// The document is refused — it is not an algebraic expression.
    Refused,
    /// The document defines exactly these function names (sorted, comma separated).
    Defined(&'static str),
    /// The document reads with its own definitions, and its last line evaluates
    /// to this number.
    DefinedValue(f64),
}

struct Case {
    /// What the student typed, `|` separating lines of a system.
    document: &'static str,
    /// Why this case exists, in a word or two.
    why: &'static str,
    expect: Expect,
}

fn cases() -> Vec<Case> {
    let case = |document: &'static str, why: &'static str, expect: Expect| Case {
        document,
        why,
        expect,
    };
    vec![
        // ---------------------------------------------------------- arithmetic
        case("1+2*3", "precedence", Expect::Simplified("7")),
        case("2*3", "star is a multiplication sign", Expect::Simplified("6")),
        case("10^6", "an exact integer power", Expect::Simplified("1000000")),
        case(
            "2^100",
            "beyond i128, degrades to a float",
            Expect::Value(2f64.powi(100)),
        ),
        case("2^10", "a modest power", Expect::Simplified("1024")),
        case("2^{-1}", "a negative power is a reciprocal", Expect::Simplified("\\frac{1}{2}")),
        case("(1+2)*3", "bare parentheses group", Expect::Simplified("9")),
        case("-3+5", "a leading minus", Expect::Simplified("2")),
        case("-7", "a negative literal", Expect::Simplified("-7")),
        case("0", "zero", Expect::Simplified("0")),
        case("x--y", "double minus is a plus", Expect::Simplified("x + y")),
        // ----------------------------------------------------------- fractions
        case("\\frac{2}{4}", "reduces to lowest terms", Expect::Simplified("\\frac{1}{2}")),
        case("\\frac{1}{2}+\\frac{1}{3}", "exact rational arithmetic", Expect::Simplified("\\frac{5}{6}")),
        case("\\frac{0}{5}", "zero over anything", Expect::Simplified("0")),
        case("\\frac{1}{3}*3", "the thirds cancel exactly", Expect::Value(1.0)),
        case("1/2x", "the denominator keeps the x (Abitti)", Expect::Latex("\\frac{1}{2x}")),
        case(
            "\\frac{1}{2}x",
            "a coefficient is not part of a fraction",
            Expect::Simplified("\\frac{1}{2}x"),
        ),
        // -------------------------------------------------------------- powers
        case("x^2", "a power", Expect::Simplified("x^{2}")),
        case("-x^2", "a leading minus is a sign, not a factor", Expect::Simplified("-x^{2}")),
        case("e^{-x^2}", "a negative exponent keeps its sign", Expect::Simplified("e^{-x^{2}}")),
        case("x^2y", "a script absorbs what follows it", Expect::Latex("x^{2y}")),
        case("x^2+1", "LaTeX is unambiguous: the script ends at the operator", Expect::Latex("x^2+1")),
        case("\\sqrt{16}", "a square root of a square", Expect::Value(4.0)),
        case("\\sqrt[3]{8}", "a cube root", Expect::Value(2.0)),
        case("\\sqrt{2}", "an irrational radical", Expect::Value(std::f64::consts::SQRT_2)),
        case(
            "\\sqrt{2}+\\sqrt{2}",
            "like surds collect",
            Expect::Value(2.0 * std::f64::consts::SQRT_2),
        ),
        case("2^{0.5}", "a fractional exponent", Expect::Value(std::f64::consts::SQRT_2)),
        // ----------------------------------------------------------- functions
        case("\\sin 0", "sin of zero", Expect::Value(0.0)),
        case("\\cos 0", "cos of zero", Expect::Value(1.0)),
        case("\\ln 1", "ln of one", Expect::Value(0.0)),
        case("\\exp 0", "e to the zero", Expect::Value(1.0)),
        case("\\log_2(8)", "a logarithm with a base", Expect::Value(3.0)),
        case("\\log_2(10)", "and an irrational one", Expect::Value(10.0f64.log2())),
        // --------------------------------------------------- products and runs
        case("2x", "juxtaposition is multiplication", Expect::Simplified("2x")),
        case("2*x", "an explicit sign is the same thing", Expect::Simplified("2x")),
        case("2x+3x", "like terms collect", Expect::Simplified("5x")),
        case("2.5x", "a decimal coefficient", Expect::Simplified("2.5x")),
        case("3xy", "three factors", Expect::Reads),
        case("x2", "a digit after a letter", Expect::Simplified("2x")),
        case("abc", "a run of variables", Expect::Reads),
        // An exponent binds to one base, but the field glues `5x` into one run.
        // Reading the run as the base would make `5x^2` mean `25x^2`, which is
        // what the corpus's quartic row caught: `x^4 - 5x^2 + 4 = 0` was read as
        // `x^4 - 25x^2 + 4 = 0` and had no rational root. The pair below is the
        // whole rule -- the run splits, and parentheses are how a student asks
        // for the other reading.
        case(
            "5x^2",
            "an exponent takes the run's last factor, not the run",
            Expect::Simplified("5x^{2}"),
        ),
        case(
            "(5x)^2",
            "parentheses are how the other reading is written",
            Expect::Simplified("\\left(5x\\right)^{2}"),
        ),
        // ------------------------------------------------------------- units
        case("80°", "a degree marker is notation", Expect::Value(80.0)),
        case("50%", "so is a percent sign", Expect::Value(50.0)),
        // --------------------------------------------------------- not algebra
        case("x=1", "an equation is not an expression", Expect::Refused),
        case("x<y", "neither is a relation", Expect::Refused),
        // `x_1` is one name, not `x` with a decoration: the subscript is part of
        // the symbol, which is what keeps `v_1` and `v_2` two variables. An index
        // *expression* is still refused, and the parser says which shape it reads.
        case("x_1", "a subscript is part of the name", Expect::Reads),
        case("\\bar{x}", "an accent is a decoration", Expect::Refused),
        case("\\vec{v}", "so is a vector arrow", Expect::Refused),
        case("f'", "a derivative needs definitions (roadmap P0)", Expect::Refused),
        // ---------------------------------------------------------- edge shapes
        case("(x+1)^2", "bare parentheses are a group", Expect::Simplified("\\left(1 + x\\right)^{2}")),
        case("\\frac{x}{x}", "policy: never cancelled silently", Expect::Reads),
        case("x/0", "division by zero has no value", Expect::Refused),
        // --------------------------------------------------------- definitions
        // A definition is a *name*, not a curve: `f(x) = x² + 1` on its own line
        // is not an equation and is not an expression.
        case("f(x) = x^2+1", "a definition defines a name", Expect::Defined("f")),
        case("f(x) = x^2+1|f(3)", "a call substitutes", Expect::DefinedValue(10.0)),
        case("f(x) = x^2+1|f(0)", "and the constant term is kept", Expect::DefinedValue(1.0)),
        case("f(x) = x^2|f'(3)", "f' differentiates the body first", Expect::DefinedValue(6.0)),
        case("f(x) = x^2|f''(2)", "f'' does it twice", Expect::DefinedValue(2.0)),
        case("f(x) = x^3|f'''(2)", "and f''' three times", Expect::DefinedValue(6.0)),
        case("g(a,b) = a*b|g(3,4)", "two parameters, in order", Expect::DefinedValue(12.0)),
        case(
            "g(x) = f(x)+1|f(x) = x^2|g(1)",
            "a definition may use one below it",
            Expect::DefinedValue(2.0),
        ),
        case(
            "f(x) = f(x)+1",
            "recursion is refused at definition time",
            Expect::Defined(""),
        ),
        case("x^2+y^2=1", "an equation is not a definition", Expect::Defined("")),
        case("a(x+1) = 0", "a product is not a definition", Expect::Defined("")),
        // ------------------------------------------------- a sign after a product
        // The corpus had `-3+5` and `x--y` but nothing that put a sign *after* a
        // multiplication, which is why `1·-6+2·-3` reading as `1-6+2-3` went
        // unnoticed until a user typed it.
        case("1*-6+2*-3", "a sign after a product multiplies", Expect::Value(-12.0)),
        case("2*-3", "and on its own", Expect::Value(-6.0)),
        case("1*-6", "with a leading unit factor", Expect::Value(-6.0)),
        case("-2*-3", "two negatives multiply to a positive", Expect::Value(6.0)),
        case("2*-3*4", "one negative among three factors", Expect::Value(-24.0)),
        case("3*--2", "a doubled sign after a product", Expect::Value(6.0)),
        case("2*-3+4*-5", "a signed product in each term", Expect::Value(-26.0)),
        case("2/-4", "a sign in a denominator", Expect::Value(-0.5)),
        case("-1/2", "a unary minus before a fraction", Expect::Value(-0.5)),
        case("-3^2", "a unary minus binds looser than a power", Expect::Value(-9.0)),
        case("(-3)^2", "a bracket binds tighter", Expect::Value(9.0)),
        case("2*-3^2", "and after a product too", Expect::Value(-18.0)),
        case("-(2+3)", "a unary minus distributes over a sum", Expect::Value(-5.0)),
        case("1--6", "a doubled minus is still a plus", Expect::Value(7.0)),
        case("3-+2", "a plus after a minus", Expect::Value(1.0)),
    ]
}

/// Render the table, then assert every row. A failing row shows what was
/// expected next to what happened, which is the point of the file.
#[test]
fn the_corpus_behaves_as_documented() {
    println!();
    println!("| Document | Why | Expected | Actual |");
    println!("|---|---|---|---|");
    let mut failures = Vec::new();
    for case in cases() {
        let lines: Vec<String> = case.document.split('|').map(str::to_string).collect();
        let field = match MathField::from_latex_lines(&lines) {
            Ok(field) => field,
            Err(error) => {
                println!(
                    "| `{}` | {} | — | unreadable: {} |",
                    case.document, case.why, error
                );
                failures.push(case.document);
                continue;
            }
        };        let (expected, actual, ok) = check(&field, &case.expect);
        println!(
            "| `{}` | {} | {} | {}{} |",
            case.document,
            case.why,
            expected,
            if ok { "" } else { "**" },
            actual
        );
        if !ok {
            failures.push(case.document);
        }
    }
    println!();
    assert!(
        failures.is_empty(),
        "{} of {} case(s) did not match: {:?}",
        failures.len(),
        cases().len(),
        failures
    );
}

fn check(field: &MathField, expect: &Expect) -> (String, String, bool) {
    let value_latex = || {
        field
            .to_expr()
            .map(|expression| canlab_tex::to_latex(&expression))
    };
    match expect {
        Expect::Latex(want) => {
            let got = field.to_latex();
            let ok = got == *want;
            (format!("`{want}`"), format!("`{got}`"), ok)
        }
        Expect::Simplified(want) => match value_latex() {
            Some(got) => {
                let ok = got == *want;
                (format!("`{want}`"), format!("`{got}`"), ok)
            }
            None => (format!("`{want}`"), String::from("no expression"), false),
        },
        Expect::Value(want) => match field.to_expr() {
            None => (format!("{want}"), String::from("no expression"), false),
            Some(expression) => {
                let Some(program) = canlab_core::Program::compile(&expression, &[]) else {
                    return (format!("{want}"), String::from("not evaluable"), false);
                };
                let got = program.eval(&[]);
                let ok = (got - want).abs() <= 1e-9 * want.abs().max(1.0);
                (format!("{want}"), format!("{got}"), ok)
            }
        },
        Expect::Reads => match value_latex() {
            Some(got) => (String::from("reads"), format!("reads, `{got}`"), true),
            None => (String::from("reads"), String::from("refused"), false),
        },
        Expect::Refused => match value_latex() {
            None => (String::from("refused"), String::from("refused"), true),
            Some(got) => (String::from("refused"), format!("read as `{got}`"), false),
        },
        Expect::Defined(want) => {
            let definitions = field.harvest().definitions;
            let mut names: Vec<String> = definitions.names().map(str::to_string).collect();
            names.sort();
            let got = names.join(", ");
            let ok = got == *want;
            (format!("`{want}`"), format!("`{got}`"), ok)
        }
        Expect::DefinedValue(want) => {
            let definitions = field.harvest().definitions;
            let lines = match field.build_lines_with(&definitions) {
                Ok(lines) => lines,
                Err(error) => return (format!("{want}"), format!("refused: {error}"), false),
            };
            let Some(expression) = lines.last() else {
                return (format!("{want}"), String::from("no expression"), false);
            };
            let Some(program) = canlab_core::Program::compile(expression, &[]) else {
                return (format!("{want}"), String::from("not evaluable"), false);
            };
            let got = program.eval(&[]);
            let ok = (got - want).abs() <= 1e-9 * want.abs().max(1.0);
            (format!("{want}"), format!("{got}"), ok)
        }
    }
}

/// Every single-line case must read back from its own printed LaTeX.
///
/// "LaTeX is the interchange format" is only true if printing and reading agree,
/// and the pane the user copies from is exactly that printed form. A case that
/// prints something the app cannot read again is a case whose LaTeX is a dead end.
///
/// Multi-line documents are skipped: their printed form is a `cases` block, and
/// the reader for that is a separate question.
#[test]
fn every_single_line_case_round_trips_through_its_own_latex() {
    let mut failures = Vec::new();
    for case in cases() {
        let lines: Vec<String> = case.document.split('|').map(str::to_string).collect();
        let Ok(field) = MathField::from_latex_lines(&lines) else {
            continue;
        };
        if lines.len() != 1 {
            continue;
        }
        let printed = field.to_latex();
        if printed.is_empty() {
            continue;
        }
        let Ok(reloaded) = MathField::from_latex(&printed) else {
            failures.push(format!(
                "{}: printed `{}`, which cannot be read back",
                case.document, printed
            ));
            continue;
        };
        let before = field.to_expr().map(|value| canlab_tex::to_latex(&value));
        let after = reloaded.to_expr().map(|value| canlab_tex::to_latex(&value));
        if before != after {
            failures.push(format!(
                "{}: printed `{}`, which reads back as {:?} instead of {:?}",
                case.document, printed, after, before
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} case(s) do not survive their own LaTeX:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
