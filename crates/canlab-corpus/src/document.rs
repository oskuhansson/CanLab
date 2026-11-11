//! Turning a row's `document` into a question, and a question into an answer.
//!
//! The document is what the editor's field code serialises: `|` between
//! questions, `||` between the equations of one system, and definitions as
//! ordinary lines. Reading it back through [`canlab_input::MathField`] rather
//! than through a second parser is deliberate — the corpus must mean exactly
//! what the field means, or a row would measure a different tool.
//!
//! The rule is the one in `docs/corpus.md`: definitions resolve first, the last
//! non-definition line decides. If that line is in a system of several
//! equations, the system is solved as a system; a single equation is solved for
//! its one unknown; a plain expression is evaluated as it stands.

use canlab_core::{Expr, ExprNode, Program, SymbolId, add, int, mul};
use canlab_input::MathField;
use canlab_solver::{NativeSolver, Solutions, Solver, variables_of};

/// Why a document is not a question, or why the tool declined it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocError {
    /// The row is wrong: no question, two unknowns in one equation, a system
    /// line without `=`, a definition that cannot be made.
    Load(String),
    /// The tool refused a construct. This is an answer a `refused` row may
    /// expect, not a load error.
    Refused(String),
}

impl std::fmt::Display for DocError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DocError::Load(reason) => write!(f, "{reason}"),
            DocError::Refused(reason) => write!(f, "{reason}"),
        }
    }
}

impl std::error::Error for DocError {}

/// What the last non-definition line asks for.
#[derive(Clone, Debug)]
pub enum Question {
    /// One equation in one unknown.
    Equation {
        /// `left − right`, the form every solver takes.
        equation: Expr,
        /// The unknown.
        var: SymbolId,
    },
    /// A system's equations, with every unknown they mention.
    System {
        /// `left − right` for each equation.
        equations: Vec<Expr>,
        /// The unknowns, sorted by name so a report is stable.
        vars: Vec<SymbolId>,
    },
    /// A plain expression: evaluated, or normalized as the answer.
    Expression {
        /// The expression itself.
        expression: Expr,
    },
}

/// A compiled document.
#[derive(Clone, Debug)]
pub struct Document {
    /// What the document asks.
    pub question: Question,
}

/// Split a document into systems of lines.
///
/// `||` joins two lines of one system; `|` starts a new one. A line with no
/// content is dropped: a blank line is a carriage return, not an expression.
///
/// A pipe that belongs to `\left|` or `\right|` is part of the LaTeX, not a
/// separator. That is the one escape the format needs and `docs/corpus.md` does
/// not spell out; without it an absolute value would silently become two
/// questions.
pub fn split_systems(document: &str) -> Vec<Vec<String>> {
    let mut systems: Vec<Vec<String>> = Vec::new();
    let mut system: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut characters = document.chars().peekable();

    while let Some(character) = characters.peek().copied() {
        if character == '|' && pipe_is_separator(&line) {
            characters.next();
            let double = characters.peek() == Some(&'|');
            if double {
                characters.next();
            }
            let text = line.trim();
            if !text.is_empty() {
                system.push(text.to_string());
            }
            line.clear();
            if !double && !system.is_empty() {
                systems.push(std::mem::take(&mut system));
            }
            continue;
        }
        line.push(character);
        characters.next();
    }

    let text = line.trim();
    if !text.is_empty() {
        system.push(text.to_string());
    }
    if !system.is_empty() {
        systems.push(system);
    }
    systems
}

/// A `|` is a separator unless it closes a `\left|…\right|` pair or is escaped.
fn pipe_is_separator(line: &str) -> bool {
    let trimmed = line.trim_end();
    if trimmed.ends_with('\\') {
        return false;
    }
    !(trimmed.ends_with("\\left") || trimmed.ends_with("\\right"))
}

/// Compile a document, or say why it is not a question.
pub fn compile(document: &str) -> Result<Document, DocError> {
    let systems = split_systems(document);
    if systems.is_empty() {
        return Err(DocError::Load(String::from(
            "the document has no line to answer",
        )));
    }

    let field = MathField::from_systems(&systems)
        .map_err(|error| DocError::Refused(error.to_string()))?;

    // Definitions are document-wide, so their problems are document-wide too:
    // a name that silently did not get defined would change what the question
    // means, and no run may pretend otherwise.
    let harvest = field.harvest();
    if let Some((name, error)) = harvest.problems.first() {
        return Err(DocError::Load(format!(
            "the definition `{name}` cannot be made: {error}"
        )));
    }
    let definitions = harvest.definitions;

    let mut last: Option<(usize, usize)> = None;
    for (system_index, lines) in field.systems().iter().enumerate() {
        for (line_index, row) in lines.iter().enumerate() {
            if row.is_empty() || field.is_definition_row(row) {
                continue;
            }
            last = Some((system_index, line_index));
        }
    }
    let Some((system_index, line_index)) = last else {
        return Err(DocError::Load(String::from(
            "the document has no question: every line is a definition or empty",
        )));
    };

    let meaningful = field.systems()[system_index]
        .iter()
        .filter(|row| !row.is_empty() && !field.is_definition_row(row))
        .count();

    if meaningful > 1 {
        return system_question(&field, system_index, &definitions);
    }

    let equations = field
        .build_system_equations_with(system_index, &definitions)
        .map_err(|error| DocError::Refused(error.to_string()))?;
    let Some((left, right)) = equations.first() else {
        return Err(DocError::Load(String::from(
            "the document has no line to answer",
        )));
    };

    match right {
        Some(right) => {
            let equation = difference(left, right);
            let vars = free_variables(&equation);
            if vars.len() != 1 {
                return Err(DocError::Load(match vars.len() {
                    0 => String::from(
                        "the equation names no unknown; a single equation must name one",
                    ),
                    count => format!(
                        "the equation names {count} unknowns; a single equation must name one, \
                         or be written as a `||` system"
                    ),
                }));
            }
            Ok(Document {
                question: Question::Equation {
                    equation,
                    var: vars[0],
                },
            })
        }
        None => {
            let row = &field.systems()[system_index][line_index];
            let expression = canlab_input::build_with(row, &definitions)
                .map_err(|error| DocError::Refused(error.to_string()))?;
            Ok(Document {
                question: Question::Expression { expression },
            })
        }
    }
}

fn system_question(
    field: &MathField,
    system_index: usize,
    definitions: &canlab_input::Definitions,
) -> Result<Document, DocError> {
    let equations = field
        .build_system_equations_with(system_index, definitions)
        .map_err(|error| DocError::Refused(error.to_string()))?;

    let mut expressions = Vec::new();
    for (index, (left, right)) in equations.iter().enumerate() {
        let Some(right) = right else {
            return Err(DocError::Load(format!(
                "equation {} of the system has no `=`; a system is a list of equations",
                index + 1
            )));
        };
        expressions.push(difference(left, right));
    }

    let mut vars: Vec<SymbolId> = Vec::new();
    for equation in &expressions {
        for variable in free_variables(equation) {
            if !vars.contains(&variable) {
                vars.push(variable);
            }
        }
    }
    vars.sort_by(|a, b| a.name().as_ref().cmp(b.name().as_ref()));
    if vars.is_empty() {
        return Err(DocError::Load(String::from(
            "the system names no unknown; there is nothing to solve for",
        )));
    }

    Ok(Document {
        question: Question::System {
            equations: expressions,
            vars,
        },
    })
}

/// `left − right`, the form all the solver operations take.
fn difference(left: &Expr, right: &Expr) -> Expr {
    add([left.clone(), mul([int(-1), right.clone()])])
}

/// Every symbol that is an unknown, in name order.
///
/// `pi`, `e` and `tau` are constants in this CAS, not unknowns; treating them
/// as variables would turn `2x = pi` into a two-unknown system, which is a load
/// error and not what the row means.
pub fn free_variables(expression: &Expr) -> Vec<SymbolId> {
    let mut vars: Vec<SymbolId> = variables_of(expression)
        .into_iter()
        .filter(|symbol| !matches!(symbol.name().as_ref(), "pi" | "e" | "tau"))
        .collect();
    vars.sort_by(|a, b| a.name().as_ref().cmp(b.name().as_ref()));
    vars
}

/// What the run produced.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    /// The solution set of an equation.
    Solutions(Solutions),
    /// One assignment per unknown, for a system.
    System(Vec<(SymbolId, Expr)>),
    /// A plain expression.
    Expression(Expr),
}

impl Answer {
    /// The answer in the format's canonical LaTeX.
    pub fn text(&self) -> String {
        match self {
            Answer::Solutions(solutions) => solutions.to_latex(),
            Answer::Expression(expression) => canlab_tex::to_latex(expression),
            Answer::System(assignments) if assignments.is_empty() => String::from("\\varnothing"),
            Answer::System(assignments) => {
                let mut sorted = assignments.clone();
                sorted.sort_by(|a, b| a.0.name().as_ref().cmp(b.0.name().as_ref()));
                sorted
                    .iter()
                    .map(|(variable, value)| {
                        format!(
                            "{} = {}",
                            canlab_tex::to_latex(&ExprNode::sym(*variable)),
                            canlab_tex::to_latex(value)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ")
            }
        }
    }

    /// The answer as one f64, when it is one.
    pub fn number(&self) -> Result<f64, String> {
        match self {
            Answer::Expression(expression) => {
                let vars = free_variables(expression);
                if !vars.is_empty() {
                    let names = vars
                        .iter()
                        .map(|symbol| symbol.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(format!("the expression still has unknowns: {names}"));
                }
                let program = Program::compile(expression, &[])
                    .ok_or("the expression cannot be evaluated numerically")?;
                let value = program.eval(&[]);
                if value.is_finite() {
                    Ok(value)
                } else {
                    Err(format!("the expression evaluated to {value}"))
                }
            }
            Answer::Solutions(_) => Err(String::from("a solution set is not a single number")),
            Answer::System(_) => Err(String::from("a system's answer is not a single number")),
        }
    }
}

/// Why a question produced no answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunError {
    /// The backend declined. This is an answer, and a refusal row may expect it.
    Refused(String),
    /// The backend failed. This is never a pass.
    Failed(String),
}

/// Ask `solver` the document's question.
pub fn ask(document: &Document, solver: &dyn Solver) -> Result<Answer, RunError> {
    match &document.question {
        Question::Equation { equation, var } => match solver.solve_all(equation, *var) {
            Ok(solutions) => Ok(Answer::Solutions(solutions)),
            Err(error) => Err(classify(error)),
        },
        Question::System { equations, vars } => match solver.solve_system(equations, vars) {
            Ok(assignments) => Ok(Answer::System(assignments)),
            // The GUI tries the native elimination when the chosen backend
            // declines a system, because the matrix path covers exactly these
            // and a system is the one case where the second backend is not a
            // second answer but the same one.
            Err(error) if error.is_declined() => match NativeSolver.solve_system(equations, vars) {
                Ok(assignments) => Ok(Answer::System(assignments)),
                Err(native) => Err(classify(native)),
            },
            Err(error) => Err(classify(error)),
        },
        Question::Expression { expression } => Ok(Answer::Expression(expression.clone())),
    }
}

fn classify(error: canlab_solver::SolverError) -> RunError {
    if error.is_declined() {
        RunError::Refused(error.to_string())
    } else {
        RunError::Failed(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canlab_core::sym;

    #[test]
    fn a_single_line_is_one_system_of_one_line() {
        let systems = split_systems("x + 1 = 3");
        assert_eq!(systems, vec![vec![String::from("x + 1 = 3")]]);
    }

    #[test]
    fn a_double_pipe_joins_one_system() {
        let systems = split_systems("x + y = 3 || x - y = 1");
        assert_eq!(
            systems,
            vec![vec![
                String::from("x + y = 3"),
                String::from("x - y = 1")
            ]]
        );
    }

    #[test]
    fn a_single_pipe_starts_a_new_system() {
        let systems = split_systems("f(x) = x^2 + 1 | f(3)");
        assert_eq!(systems.len(), 2);
        assert_eq!(systems[1], vec![String::from("f(3)")]);
    }

    #[test]
    fn empty_lines_and_stray_separators_are_dropped() {
        assert_eq!(
            split_systems("| x = 1 ||  y = 2  |"),
            vec![
                vec![String::from("x = 1"), String::from("y = 2")]
            ]
        );
        assert!(split_systems("   |  |  ").is_empty());
    }

    #[test]
    fn an_absolute_value_pipe_is_not_a_separator() {
        let systems = split_systems("\\left|x\\right| = 2");
        assert_eq!(systems.len(), 1);
        assert_eq!(systems[0][0], "\\left|x\\right| = 2");
    }

    #[test]
    fn the_last_non_definition_line_decides() {
        let document = compile("f(x) = x^2 + 1 | 1 + 2").expect("compiled");
        assert!(matches!(document.question, Question::Expression { .. }));
    }

    #[test]
    fn a_later_expression_question_wins_over_an_earlier_system() {
        let document = compile("x + y = 3 || x - y = 1 | 5").expect("compiled");
        match document.question {
            Question::Expression { expression } => {
                assert_eq!(canlab_tex::to_latex(&expression), "5")
            }
            other => panic!("expected the last line, got {other:?}"),
        }
    }

    #[test]
    fn definitions_apply_across_systems_and_below_the_question() {
        // The definition is the last line and the question is above it; the
        // table is document-wide and order-independent.
        let document = compile("f(3) | f(x) = x^2").expect("compiled");
        let Answer::Expression(expression) = ask(&document, &NativeSolver).expect("asked") else {
            panic!("expected an expression");
        };
        assert_eq!(canlab_tex::to_latex(&expression), "9");
    }

    #[test]
    fn definitions_are_substituted_before_the_question() {
        let document = compile("f(x) = x^2 + 1 | f(3)").expect("compiled");
        let Answer::Expression(expression) = ask(&document, &NativeSolver).expect("asked") else {
            panic!("expected an expression");
        };
        assert_eq!(canlab_tex::to_latex(&expression), "10");
    }

    #[test]
    fn a_single_equation_is_solved_for_its_one_unknown() {
        let document = compile("x + 1 = 3").expect("compiled");
        match &document.question {
            Question::Equation { var, .. } => {
                assert_eq!(*var, SymbolId::intern("x"));
            }
            other => panic!("expected an equation, got {other:?}"),
        }
        let Answer::Solutions(solutions) = ask(&document, &NativeSolver).expect("asked") else {
            panic!("expected solutions");
        };
        assert_eq!(solutions.to_latex(), "\\left\\{2\\right\\}");
    }

    #[test]
    fn a_quartic_document_is_solved_for_its_one_unknown() {
        // `x^4 - 5x^2 + 4 = (x^2-1)(x^2-4)`. The coefficient in front of `x^2`
        // is the point: the field stores `5x` as one run, and reading the run as
        // the base of the exponent turned this into `x^4 - 25x^2 + 4`, which has
        // no rational root at all and made the whole row decline.
        let document = compile("x^4 - 5x^2 + 4 = 0").expect("compiled");
        let Answer::Solutions(solutions) = ask(&document, &NativeSolver).expect("asked") else {
            panic!("expected solutions");
        };
        let Solutions::Values(values) = solutions else {
            panic!("expected four values");
        };
        let mut roots: Vec<f64> = values
            .iter()
            .filter_map(canlab_core::approx::value_of)
            .collect();
        roots.sort_by(f64::total_cmp);
        assert_eq!(roots, vec![-2.0, -1.0, 1.0, 2.0]);
    }

    #[test]
    fn a_system_is_solved_as_a_system() {
        let document = compile("x + y = 8 || 2x - y = 1").expect("compiled");
        let Answer::System(assignments) = ask(&document, &NativeSolver).expect("asked") else {
            panic!("expected a system answer");
        };
        assert_eq!(
            Answer::System(assignments).text(),
            "x = 3; y = 5".to_string()
        );
    }

    #[test]
    fn constants_are_not_unknowns() {
        // `pi` is a constant, so this is one unknown and solvable, not a
        // two-unknown load error. The backslash matters: a bare `pi` in the
        // field is `p·i`, because that is what two letters mean there.
        let document = compile("2x = \\pi").expect("compiled");
        assert!(matches!(document.question, Question::Equation { .. }));
    }

    #[test]
    fn two_unknowns_in_one_equation_is_a_load_error() {
        let error = compile("x + y = 3").expect_err("must refuse");
        assert!(matches!(error, DocError::Load(_)), "got {error:?}");
        assert!(error.to_string().contains("2 unknowns"));
    }

    #[test]
    fn a_definition_only_document_has_no_question() {
        let error = compile("f(x) = x + 1").expect_err("must refuse");
        assert!(matches!(error, DocError::Load(_)));
        assert!(error.to_string().contains("no question"));
    }

    #[test]
    fn a_broken_definition_is_a_load_error() {
        let error = compile("f(x) = f(x) + 1 | f(2)").expect_err("must refuse");
        assert!(error.to_string().contains("definition"), "got {error:?}");
    }

    #[test]
    fn a_construct_the_tool_declines_is_a_refusal() {
        let error = compile("\\frac{1}{0}").expect_err("must decline");
        assert!(matches!(error, DocError::Refused(_)), "got {error:?}");
        assert!(error.to_string().contains("1/0"), "got {error:?}");
    }

    #[test]
    fn an_unknown_command_is_a_refusal_with_a_reason() {
        let error = compile("\\int 3x^2 dx").expect_err("must decline");
        assert!(matches!(error, DocError::Refused(_)));
        assert!(error.to_string().contains("\\int"), "got {error:?}");
    }

    #[test]
    fn a_numeric_answer_is_one_number() {
        let document = compile("\\sqrt{\\frac{2 \\cdot 4^2}{6}}").expect("compiled");
        let answer = ask(&document, &NativeSolver).expect("asked");
        let value = answer.number().expect("a number");
        assert!((value - 2.309_401).abs() < 1e-6, "got {value}");
    }

    #[test]
    fn an_expression_with_an_unknown_has_no_number() {
        let document = compile("x + 1").expect("compiled");
        let answer = ask(&document, &NativeSolver).expect("asked");
        let error = answer.number().expect_err("not a number");
        assert!(error.contains("unknowns"), "got {error:?}");
    }

    #[test]
    fn a_set_answer_prints_as_a_set_in_backend_order() {
        let answer = Answer::Solutions(Solutions::Values(vec![sym("x"), sym("y")]));
        assert_eq!(answer.text(), "\\left\\{x, y\\right\\}");
        assert_eq!(
            Answer::Solutions(Solutions::None).text(),
            "\\varnothing"
        );
    }
}
