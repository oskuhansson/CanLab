//! A corpus of documents and what they should do.
//!
//! This file is meant to be **read**, not just run: every case is a line a
//! Finnish student would plausibly type, paired with the result they would
//! expect. The test prints the whole table as Markdown so the expectations can be
//! reviewed against what the app actually does.
//!
//! Run it with the table visible:
//!
//! ```text
//! cargo test -p canlab-input --test acceptance -- --nocapture
//! ```
//!
//! Two layers are being tested, and confusing them was the first mistake this
//! file caught:
//!
//! * `Latex` — what the editor *pane* shows, which is what was typed;
//! * `Simplified` — what the *result card* shows, which is what it means.

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
}

struct Case {
    /// What the student typed, `|` separating lines of a system.
    document: &'static str,
    /// Why this case exists, in a word or two.
    why: &'static str,
    expect: Expect,
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            document: "2x+3x",
            why: "like terms collect",
            expect: Expect::Simplified("5x"),
        },
        Case {
            document: "\\frac{1}{2}+\\frac{1}{3}",
            why: "exact rational arithmetic",
            expect: Expect::Simplified("\\frac{5}{6}"),
        },
        Case {
            document: "\\sqrt{2}",
            why: "a radical is a value",
            expect: Expect::Value(std::f64::consts::SQRT_2),
        },
        Case {
            document: "2*3",
            why: "star is a multiplication sign",
            expect: Expect::Simplified("6"),
        },
        Case {
            document: "2x",
            why: "juxtaposition is multiplication",
            expect: Expect::Reads,
        },
        Case {
            document: "80°",
            why: "a unit marker is notation, not an operator",
            expect: Expect::Value(80.0),
        },
        Case {
            document: "\\log_2(10)",
            why: "a subscript on a function is its base",
            expect: Expect::Value(10.0f64.log2()),
        },
        Case {
            document: "e^{-x^2}",
            why: "a negative exponent keeps its sign",
            expect: Expect::Simplified("e^{-x^{2}}"),
        },
        Case {
            document: "-x^2",
            why: "a leading minus is a sign, not a factor",
            expect: Expect::Simplified("-x^{2}"),
        },
        Case {
            document: "\\bar{x}",
            why: "an accent is a decoration, not a scalar",
            expect: Expect::Refused,
        },
        Case {
            document: "(x+1)^2",
            why: "bare parentheses are a group",
            expect: Expect::Simplified("\\left(1 + x\\right)^{2}"),
        },
        Case {
            document: "x^2+y^2=1",
            why: "an equation is not an expression",
            expect: Expect::Refused,
        },
        Case {
            document: "f'(x)",
            why: "derivatives need function definitions (roadmap P0)",
            expect: Expect::Refused,
        },
        Case {
            document: "x_1",
            why: "a subscript on a variable is not algebraic",
            expect: Expect::Refused,
        },
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
        };
        let (expected, actual, ok) = check(&field, &case.expect);
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
        "{} case(s) did not match: {:?}",
        failures.len(),
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
                let ok = (got - want).abs() < 1e-9;
                (format!("{want}"), format!("{got}"), ok)
            }
        },
        Expect::Reads => match value_latex() {
            Some(got) => (String::from("reads"), format!("reads, `{got}`"), true),
            None => (String::from("reads"), String::from("refused"), false),
        },
        Expect::Refused => match value_latex() {
            None => (String::from("refused"), String::from("refused"), true),
            Some(got) => (
                String::from("refused"),
                format!("read as `{got}`"),
                false,
            ),
        },
    }
}
