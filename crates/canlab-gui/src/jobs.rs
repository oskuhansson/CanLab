//! The background solver.
//!
//! A solver call is allowed to take as long as it likes; the UI thread is not.
//! One long-lived worker owns the [`MockSolver`] behind an `Arc<dyn Solver>`
//! and receives jobs over a channel. The frame only ever polls the result
//! channel with [`std::sync::mpsc::Receiver::try_recv`], so a slow backend
//! cannot stall the window.
//!
//! The worker deliberately does not know about egui: it turns a [`Job`] into a
//! plain [`JobResult`], and the app decides how that result looks.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

use canlab_core::{Expr, SymbolId};
use canlab_solver::{NativeSolver, Solver, SolverError};

/// What kind of operation produced a result. The app uses it to decide whether
/// a solve result belongs in the Variables panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind {
    Simplify,
    Integrate,
    Solve,
}

impl JobKind {
    pub fn is_solve(self) -> bool {
        matches!(self, Self::Solve)
    }
}

/// A unit of work for the solver worker.
pub enum JobOp {
    Simplify(Expr),
    Integrate(Expr, SymbolId),
    Solve(Expr, SymbolId),
}

/// A named request queued for the worker.
pub struct Job {
    pub label: String,
    pub op: JobOp,
    /// The editor generation that produced this request. A result carrying an
    /// older generation is discarded instead of overwriting a newer parse.
    pub generation: u64,
}

/// What the backend said, normalised into the four cases the UI understands.
pub enum JobOutcome {
    /// A single expression (simplify, integrate).
    Value(Expr),
    /// All solutions of an equation (solve).
    Solutions(Vec<Expr>),
    /// The backend understood the request and declined, with a line saying what
    /// it can do instead. Not an error.
    Declined(String),
    /// A real failure: timeout, dead worker, broken protocol.
    Failed(String),
}

/// One completed request, tagged with enough context for the UI.
pub struct JobResult {
    pub label: String,
    pub kind: JobKind,
    pub var: Option<SymbolId>,
    pub outcome: JobOutcome,
    pub generation: u64,
}

/// The app-side handle to the worker thread.
pub struct Jobs {
    tx: Option<Sender<Job>>,
    rx: Receiver<JobResult>,
    in_flight: usize,
    running: Option<String>,
}

impl Jobs {
    /// Spawn the worker. A failed spawn leaves the app usable; solver buttons
    /// then report that no backend is available instead of hanging.
    pub fn spawn() -> Self {
        let (tx, job_rx) = mpsc::channel::<Job>();
        let (result_tx, rx) = mpsc::channel::<JobResult>();
        let spawned = std::thread::Builder::new()
            .name(String::from("canlab-solver"))
            .spawn(move || {
                let solver: Arc<dyn Solver> = Arc::new(NativeSolver);
                while let Ok(job) = job_rx.recv() {
                    if result_tx.send(run(&*solver, job)).is_err() {
                        break;
                    }
                }
            });
        let tx = if spawned.is_ok() { Some(tx) } else { None };
        Self {
            tx,
            rx,
            in_flight: 0,
            running: None,
        }
    }

    /// Queue `job`. Returns `false` when no worker is alive.
    pub fn submit(&mut self, job: Job) -> bool {
        let Some(tx) = self.tx.as_ref() else {
            return false;
        };
        let label = job.label.clone();
        if tx.send(job).is_err() {
            self.tx = None;
            return false;
        }
        self.in_flight += 1;
        self.running = Some(label);
        true
    }

    /// Collect every finished job without blocking.
    pub fn poll(&mut self) -> Vec<JobResult> {
        let mut results = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(result) => {
                    self.in_flight = self.in_flight.saturating_sub(1);
                    results.push(result);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.tx = None;
                    self.in_flight = 0;
                    self.running = None;
                    break;
                }
            }
        }
        if self.in_flight == 0 {
            self.running = None;
        }
        results
    }

    pub fn in_flight(&self) -> usize {
        self.in_flight
    }

    pub fn running_label(&self) -> Option<&str> {
        self.running.as_deref()
    }

    pub fn available(&self) -> bool {
        self.tx.is_some()
    }
}

fn run(solver: &dyn Solver, job: Job) -> JobResult {
    let Job {
        label,
        op,
        generation,
    } = job;
    let (kind, var, outcome) = match op {
        JobOp::Simplify(expr) => (
            JobKind::Simplify,
            None,
            single(solver.simplify(&expr), JobKind::Simplify),
        ),
        JobOp::Integrate(expr, var) => (
            JobKind::Integrate,
            Some(var),
            single(solver.integrate(&expr, var), JobKind::Integrate),
        ),
        JobOp::Solve(expr, var) => {
            let outcome = match solver.solve(&expr, var) {
                Ok(solutions) => JobOutcome::Solutions(solutions),
                Err(err) => declined_or_failed(err, JobKind::Solve),
            };
            (JobKind::Solve, Some(var), outcome)
        }
    };
    JobResult {
        label,
        kind,
        var,
        outcome,
        generation,
    }
}

fn single(result: Result<Expr, SolverError>, kind: JobKind) -> JobOutcome {
    match result {
        Ok(expr) => JobOutcome::Value(expr),
        Err(err) => declined_or_failed(err, kind),
    }
}

/// A backend declining is a real answer, and the panel has to be able to say
/// *what* it declined and what the backend can do instead. "No solver backend"
/// is the one thing it is not: the backend answered.
fn declined_or_failed(err: SolverError, kind: JobKind) -> JobOutcome {
    if err.is_declined() {
        JobOutcome::Declined(repertoire(kind))
    } else {
        JobOutcome::Failed(err.to_string())
    }
}

/// What this backend is for, in one line, phrased for a declined request.
fn repertoire(kind: JobKind) -> String {
    match kind {
        JobKind::Solve => String::from(
            "This backend solves one equation in one variable, of degree 1 or 2. \
             A second variable, a higher power, or a function of the variable is beyond it.",
        ),
        JobKind::Integrate => String::from(
            "This backend integrates sums of integer powers of one variable.",
        ),
        JobKind::Simplify => String::from(
            "This backend has no rewriting rules; the expression is already in the only form it knows.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canlab_core::{int, pow, sym};

    fn job(op: JobOp) -> Job {
        Job {
            label: String::from("test"),
            op,
            generation: 9,
        }
    }

    #[test]
    fn integrating_x_squared_returns_the_antiderivative() {
        let result = run(
            &NativeSolver,
            job(JobOp::Integrate(
                pow(sym("x"), int(2)),
                SymbolId::intern("x"),
            )),
        );
        assert_eq!(result.kind, JobKind::Integrate);
        assert_eq!(result.generation, 9);
        assert_eq!(result.var, Some(SymbolId::intern("x")));
        match result.outcome {
            JobOutcome::Value(value) => assert_eq!(value.to_string(), "1/3*x^3"),
            other => assert!(matches!(other, JobOutcome::Value(_))),
        }
    }

    #[test]
    fn solving_a_linear_equation_reaches_the_panel() {
        // 2x - 6 = 0 has exactly one real solution, and the native backend
        // knows it. This is the path the Variables panel reads.
        let equation = canlab_core::add(vec![
            canlab_core::mul(vec![int(2), sym("x")]),
            int(-6),
        ]);
        let result = run(&NativeSolver, job(JobOp::Solve(equation, SymbolId::intern("x"))));
        match result.outcome {
            JobOutcome::Solutions(values) => {
                assert_eq!(values.len(), 1);
                assert_eq!(values[0].to_string(), "3");
            }
            other => panic!("expected solutions, got {}", describe(&other)),
        }
    }

    #[test]
    fn a_declined_solve_is_not_reported_as_a_failure() {
        // A cubic is outside the native backend's repertoire, and declining is a
        // real answer: the UI must show "declined", not an error.
        let result = run(
            &NativeSolver,
            job(JobOp::Solve(pow(sym("x"), int(3)), SymbolId::intern("x"))),
        );
        assert!(result.kind.is_solve());
        match result.outcome {
            JobOutcome::Declined(_) => {}
            other => assert!(matches!(other, JobOutcome::Declined(_))),
        }
    }

    fn describe(outcome: &JobOutcome) -> &'static str {
        match outcome {
            JobOutcome::Value(_) => "a value",
            JobOutcome::Solutions(_) => "solutions",
            JobOutcome::Declined(_) => "declined",
            JobOutcome::Failed(_) => "a failure",
        }
    }
}
