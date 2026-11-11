//! Running rows, and the run report the CLI prints and writes.
//!
//! One row, one outcome. A row either passes or fails, and a *load error* is
//! neither: it is a row that never entered the run because the file is wrong.
//! Keeping the three apart is the whole point of the exit codes — 1 means the
//! tool answered differently, 2 means the run cannot be trusted.

use std::time::Duration;

use canlab_solver::Solver;

use crate::batch::{self, ExecCommand};
use crate::compare;
use crate::document::{self, Answer, DocError, RunError};
use crate::row::{Expect, LoadError, Row};

/// What happened to one row.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// The row matched its expectation.
    Pass {
        /// What the run produced.
        got: String,
        /// A physics unit was recorded but not checked. The row is still a
        /// pass, but the report must not pretend the unit was verified.
        unit_unchecked: bool,
    },
    /// The row did not match.
    Fail {
        /// What the run produced instead.
        got: String,
        /// Why it is not the expectation, when the answer alone does not say.
        reason: Option<String>,
    },
}

impl Outcome {
    /// `true` for a pass.
    pub fn is_pass(&self) -> bool {
        matches!(self, Outcome::Pass { .. })
    }

    /// The produced answer, for the console and the report.
    pub fn got(&self) -> &str {
        match self {
            Outcome::Pass { got, .. } | Outcome::Fail { got, .. } => got,
        }
    }

    fn pass(got: String, unit_unchecked: bool) -> Outcome {
        Outcome::Pass {
            got,
            unit_unchecked,
        }
    }

    fn fail(got: impl Into<String>, reason: impl Into<String>) -> Outcome {
        Outcome::Fail {
            got: got.into(),
            reason: Some(reason.into()),
        }
    }

    fn fail_plain(got: impl Into<String>) -> Outcome {
        Outcome::Fail {
            got: got.into(),
            reason: None,
        }
    }
}

/// Run one row and decide whether it matched.
pub fn run_row(row: &Row, solver: &dyn Solver) -> Outcome {
    match &row.expect {
        Expect::Refused(wanted) => run_refusal(row, solver, wanted),
        Expect::Exact(wanted) => match answer(row, solver) {
            Ok(answer) => {
                let got = answer.text();
                if compare::exact_matches(wanted, &got) {
                    Outcome::pass(got, false)
                } else {
                    Outcome::fail_plain(got)
                }
            }
            Err(outcome) => outcome,
        },
        Expect::Numeric {
            value,
            tolerance,
            unit,
            digits,
        } => match answer(row, solver) {
            Ok(answer) => match answer.number() {
                Ok(number) => {
                    if compare::numeric_matches(*value, number, *tolerance, *digits) {
                        // The unit is recorded, never checked: W15 owns that
                        // check, and claiming it now would be a lie in a report
                        // whose only job is to be trusted.
                        Outcome::pass(format!("{number}"), unit.is_some())
                    } else {
                        Outcome::fail_plain(format!("{number}"))
                    }
                }
                Err(why) => Outcome::fail(format!("not a number: {why}"), why),
            },
            Err(outcome) => outcome,
        },
    }
}

/// A refusal row: the run must decline, for the named reason.
fn run_refusal(row: &Row, solver: &dyn Solver, wanted: &str) -> Outcome {
    match document::compile(&row.document) {
        Err(DocError::Refused(reason)) => refusal(reason, wanted),
        Err(DocError::Load(reason)) => Outcome::fail(
            format!("load error: {reason}"),
            "the document is not a question",
        ),
        Ok(document) => match document::ask(&document, solver) {
            Err(RunError::Refused(reason)) => refusal(reason, wanted),
            Err(RunError::Failed(reason)) => Outcome::fail(
                format!("error: {reason}"),
                "the backend failed, which is not a refusal",
            ),
            Ok(answer) => Outcome::fail(
                answer.text(),
                "the run answered; a refusal was expected",
            ),
        },
    }
}

fn refusal(reason: String, wanted: &str) -> Outcome {
    if reason_names(&reason, wanted) {
        Outcome::pass(format!("refused: {reason}"), false)
    } else {
        Outcome::fail(
            format!("refused: {reason}"),
            format!("refused for a different reason than {wanted:?}"),
        )
    }
}

/// The format's matching rule: case-insensitive, whitespace collapsed.
pub fn reason_names(reason: &str, wanted: &str) -> bool {
    compare::collapse(reason)
        .to_lowercase()
        .contains(&compare::collapse(wanted).to_lowercase())
}

/// Compile and ask, turning a compile-time refusal into the same shape as a
/// solver refusal.
fn answer(row: &Row, solver: &dyn Solver) -> Result<Answer, Outcome> {
    match document::compile(&row.document) {
        Ok(document) => match document::ask(&document, solver) {
            Ok(answer) => Ok(answer),
            Err(RunError::Refused(reason)) => Err(Outcome::fail(
                format!("declined: {reason}"),
                "the backend declined",
            )),
            Err(RunError::Failed(reason)) => {
                Err(Outcome::fail(format!("error: {reason}"), "the backend failed"))
            }
        },
        Err(DocError::Refused(reason)) => Err(Outcome::fail(
            format!("refused: {reason}"),
            "the document could not be read",
        )),
        Err(DocError::Load(reason)) => Err(Outcome::fail(
            format!("load error: {reason}"),
            "the document is not a question",
        )),
    }
}

/// Run every row in this process. The CLI uses [`run_batched`]; this is what
/// `canlab corpus exec` calls inside a child, and what the unit tests drive.
pub fn run_in_process(rows: &[Row], solver: &dyn Solver) -> Vec<Outcome> {
    rows.iter().map(|row| run_row(row, solver)).collect()
}

/// One row's place in a batched run.
#[derive(Clone, Debug)]
pub enum RowResult {
    /// The row ran and produced an answer.
    Outcome(Outcome),
    /// The child found the file no longer loads. That is a load error, and the
    /// run cannot be trusted.
    LoadError(String),
}

/// Run every row through child processes, `batch` rows at a time.
///
/// A batch whose child crashes, exits non-zero or misses its deadline loses
/// exactly its own rows; the next batch still runs. That is the difference
/// between one bad row and a dead run.
pub fn run_batched(
    rows: &[Row],
    command: &ExecCommand,
    batch: usize,
    deadline: Duration,
) -> Vec<RowResult> {
    let batch = batch.max(1);
    let mut outcomes: Vec<Option<RowResult>> = vec![None; rows.len()];

    for (chunk_index, chunk) in rows.chunks(batch).enumerate() {
        let start = chunk_index * batch;
        let results = batch::run_batch(command, chunk, deadline);
        for (offset, result) in results.into_iter().enumerate() {
            outcomes[start + offset] = Some(match result {
                Ok(exec) => match exec.status {
                    batch::ExecStatus::Pass => RowResult::Outcome(Outcome::pass(
                        exec.got,
                        exec.unit_unchecked,
                    )),
                    batch::ExecStatus::Fail => RowResult::Outcome(Outcome::Fail {
                        got: exec.got,
                        reason: exec.reason,
                    }),
                    batch::ExecStatus::LoadError => RowResult::LoadError(
                        exec.reason
                            .unwrap_or_else(|| String::from("the child could not load the row")),
                    ),
                    batch::ExecStatus::Error => RowResult::Outcome(Outcome::fail(
                        exec.got,
                        exec.reason
                            .unwrap_or_else(|| String::from("the child could not run the row")),
                    )),
                },
                Err(lost) => RowResult::Outcome(Outcome::fail("no result", lost)),
            });
        }
    }

    outcomes
        .into_iter()
        .map(|result| {
            result.unwrap_or_else(|| {
                RowResult::Outcome(Outcome::fail("no result", "the row was never run"))
            })
        })
        .collect()
}

/// How far `--update` got with the provenance check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Provenance {
    /// No network was asked for; the report says so.
    Unchecked,
    /// Every `hf:` row was checked against its upstream answer.
    Checked,
    /// The check was asked for and could not be completed. The run is untrusted.
    Failed(String),
}

/// Everything one invocation of `canlab corpus run` produced.
#[derive(Clone, Debug)]
pub struct RunReport {
    /// The rows that ran, in file order.
    pub rows: Vec<(Row, Outcome)>,
    /// The rows that could not be loaded at all.
    pub load_errors: Vec<LoadError>,
    /// Which backend answered, for the report.
    pub backend: String,
    /// The `--tag` filter, when one was given.
    pub tag: Option<String>,
    /// The provenance check's state.
    pub provenance: Provenance,
}

impl RunReport {
    /// `true` when at least one row passed.
    pub fn passes(&self) -> usize {
        self.rows.iter().filter(|(_, outcome)| outcome.is_pass()).count()
    }

    /// `true` when at least one row failed.
    pub fn failures(&self) -> usize {
        self.rows.iter().filter(|(_, outcome)| !outcome.is_pass()).count()
    }

    /// How many selected rows expect a refusal.
    pub fn refusals(&self) -> usize {
        self.rows.iter().filter(|(row, _)| row.is_refusal()).count()
    }

    /// The exit code the contract names.
    ///
    /// `2` is not "two failed": it is "this run cannot be trusted", whether
    /// because a file is wrong, because no row matched, or because the
    /// provenance check could not finish.
    pub fn exit_code(&self) -> u8 {
        if matches!(self.provenance, Provenance::Failed(_)) || !self.load_errors.is_empty() {
            return 2;
        }
        if self.rows.is_empty() {
            return 2;
        }
        if self.failures() > 0 { 1 } else { 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::row::Subject;
    use canlab_solver::NativeSolver;
    use std::path::PathBuf;

    fn row(id: &str, document: &str, expect: Expect) -> Row {
        Row {
            path: PathBuf::from(format!("tests/cases/math/{id}.toml")),
            id: id.to_string(),
            subject: Subject::Math,
            capability: String::from("solve.linear"),
            source: String::from("https://example.test/paper — 2024, uppgift 1"),
            document: document.to_string(),
            expect,
            note: None,
        }
    }

    #[test]
    fn a_matching_exact_answer_passes() {
        let outcome = run_row(
            &row(
                "a",
                "x + 1 = 3",
                Expect::Exact("\\left\\{2\\right\\}".into()),
            ),
            &NativeSolver,
        );
        assert!(outcome.is_pass(), "{outcome:?}");
        assert_eq!(outcome.got(), "\\left\\{2\\right\\}");
    }

    #[test]
    fn a_wrong_exact_answer_fails_and_keeps_the_answer() {
        let outcome = run_row(&row("a", "x + 1 = 3", Expect::Exact("3".into())), &NativeSolver);
        assert!(!outcome.is_pass());
        assert_eq!(outcome.got(), "\\left\\{2\\right\\}");
    }

    #[test]
    fn the_quadratic_set_passes_regardless_of_root_order() {
        let outcome = run_row(
            &row("a", "x^2 = 8x", Expect::Exact("\\left\\{0, 8\\right\\}".into())),
            &NativeSolver,
        );
        assert!(outcome.is_pass(), "{outcome:?}");
    }

    #[test]
    fn a_numeric_row_with_a_unit_passes_but_says_the_unit_is_unchecked() {
        let outcome = run_row(
            &row(
                "a",
                "\\sqrt{\\frac{2 \\cdot 4^2}{6}}",
                Expect::Numeric {
                    value: 2.3,
                    tolerance: 0.05,
                    unit: Some(String::from("m")),
                    digits: Some(2),
                },
            ),
            &NativeSolver,
        );
        match outcome {
            Outcome::Pass {
                unit_unchecked, ..
            } => assert!(unit_unchecked),
            other => panic!("expected a pass, got {other:?}"),
        }
    }

    #[test]
    fn a_numeric_row_outside_the_tolerance_fails() {
        let outcome = run_row(
            &row(
                "a",
                "2.31",
                Expect::Numeric {
                    value: 2.3,
                    tolerance: 0.001,
                    unit: None,
                    digits: None,
                },
            ),
            &NativeSolver,
        );
        assert!(!outcome.is_pass());
    }

    #[test]
    fn a_refusal_row_passes_on_a_solver_decline_too() {
        // The refusal need not come from the reader: a backend that declines
        // is the same answer, and the row expects the decline.
        let outcome = run_row(
            &row("a", "x^3 = 2", Expect::Refused("declined".into())),
            &NativeSolver,
        );
        assert!(outcome.is_pass(), "{outcome:?}");
        assert!(outcome.got().contains("declined"), "{outcome:?}");
    }

    #[test]
    fn a_refusal_passes_on_its_reason_and_fails_on_another() {
        let matching = run_row(
            &row("a", "\\frac{1}{0}", Expect::Refused("1/0".into())),
            &NativeSolver,
        );
        assert!(matching.is_pass(), "{matching:?}");

        let mismatch = run_row(
            &row("a", "\\frac{1}{0}", Expect::Refused("division by zero".into())),
            &NativeSolver,
        );
        assert!(!mismatch.is_pass());
        assert!(mismatch.got().contains("1/0"));
    }

    #[test]
    fn a_refusal_row_fails_when_the_tool_answers() {
        let outcome = run_row(
            &row("a", "x + 1 = 3", Expect::Refused("division by zero".into())),
            &NativeSolver,
        );
        assert!(!outcome.is_pass());
        assert!(outcome.got().contains("\\left\\{2\\right\\}"));
    }

    #[test]
    fn a_declined_equation_is_a_failure_not_a_refusal_pass() {
        // A cubic is outside the native backend, so an `exact` row fails with
        // the decline visible; it is not silently a pass.
        let outcome = run_row(
            &row("a", "x^3 = 2", Expect::Exact("\\sqrt[3]{2}".into())),
            &NativeSolver,
        );
        assert!(!outcome.is_pass());
        assert!(outcome.got().contains("declined"), "{outcome:?}");
    }

    #[test]
    fn exit_codes_are_zero_one_and_two() {
        let pass = RunReport {
            rows: vec![(
                row(
                    "a",
                    "x + 1 = 3",
                    Expect::Exact("\\left\\{2\\right\\}".into()),
                ),
                run_row(
                    &row(
                        "a",
                        "x + 1 = 3",
                        Expect::Exact("\\left\\{2\\right\\}".into()),
                    ),
                    &NativeSolver,
                ),
            )],
            load_errors: Vec::new(),
            backend: String::from("native"),
            tag: None,
            provenance: Provenance::Unchecked,
        };
        assert_eq!(pass.exit_code(), 0);

        let failure = RunReport {
            rows: vec![(
                row("a", "x + 1 = 3", Expect::Exact("3".into())),
                run_row(&row("a", "x + 1 = 3", Expect::Exact("3".into())), &NativeSolver),
            )],
            ..pass.clone()
        };
        assert_eq!(failure.exit_code(), 1);

        let untrusted = RunReport {
            load_errors: vec![LoadError {
                path: PathBuf::from("tests/cases/math/bad.toml"),
                reason: String::from("broken"),
            }],
            ..pass.clone()
        };
        assert_eq!(untrusted.exit_code(), 2);

        let empty = RunReport {
            rows: Vec::new(),
            ..pass.clone()
        };
        assert_eq!(empty.exit_code(), 2);

        let unchecked = RunReport {
            provenance: Provenance::Failed(String::from("network unavailable")),
            ..pass
        };
        assert_eq!(unchecked.exit_code(), 2);
    }

    #[test]
    fn reason_matching_collapses_case_and_whitespace() {
        assert!(reason_names("Division  by\tZERO", "division by zero"));
        assert!(reason_names("the field has an empty slot", "EMPTY SLOT"));
        assert!(!reason_names("division by zero", "subscript"));
    }
}
