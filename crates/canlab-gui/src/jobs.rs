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

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};

use canlab_core::{Expr, Relation, SymbolId};
use canlab_solver::{NativeSolver, Solver, SolverError};

/// Which backend the worker thread is talking to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// The in-process backend: exact rationals, linear and quadratic equations,
    /// polynomial calculus, Gaussian elimination for linear systems.
    Native,
    /// SymPy in a child process: the real thing, when Python is installed.
    Sympy,
}

impl Backend {
    /// What the Variables panel says it is.
    pub fn label(self) -> &'static str {
        match self {
            Backend::Native => "native · exact rationals",
            Backend::Sympy => "sympy · full symbolic",
        }
    }

    /// What this backend can do, phrased for a request it declined.
    fn repertoire(self, kind: JobKind) -> String {
        match (self, kind) {
            (Backend::Sympy, _) => String::from(
                "SymPy declined this. The expression is probably outside elementary \
                 closed forms, or needs assumptions this backend cannot guess.",
            ),
            (Backend::Native, JobKind::Solve) => String::from(
                "This backend solves one equation in one variable: exactly when it is of \
                 degree 1 or 2, numerically when a real root lies in [-32, 32], and as a \
                 linear system of any size. A root outside that window, or a non-linear \
                 system, is beyond it. An inequality is charted when both sides are \
                 polynomials, a ratio of polynomials, or absolute values of those.",
            ),
            (Backend::Native, JobKind::Integrate) => String::from(
                "This backend integrates sums of integer powers of one variable.",
            ),
            (Backend::Native, JobKind::Simplify) => String::from(
                "This backend has no rewriting rules; the expression is already in the only \
                 form it knows.",
            ),
        }
    }
}

/// What the worker thread decided to use, and why it did not use SymPy.
///
/// The decision is made *on the worker thread*, because finding a Python means
/// starting processes and that is not something a frame may wait for.
#[derive(Clone, Debug, Default)]
pub struct BackendStatus {
    /// `None` until the thread has chosen.
    pub backend: Option<Backend>,
    /// Why SymPy is not in use, when it was asked for and is missing.
    pub note: Option<String>,
}

/// Choose the backend: SymPy when it is there, native otherwise.
///
/// `CANLAB_SOLVER=native` forces the native backend even if Python is installed,
/// which is what a user comparing the two would want.
fn choose_backend() -> (Arc<dyn Solver>, Backend, Option<String>) {
    let forced_native = std::env::var("CANLAB_SOLVER")
        .map(|value| value.eq_ignore_ascii_case("native"))
        .unwrap_or(false);
    // Tests pin the backend: a discovery pass starts processes, and a test that
    // depended on whether the developer has Python installed would be a test of
    // the machine rather than of the app. `canlab-solver` itself tests discovery.
    if forced_native || cfg!(test) {
        return (Arc::new(NativeSolver), Backend::Native, None);
    }
    match canlab_solver::sympy::open() {
        Ok(worker) => (Arc::new(worker), Backend::Sympy, None),
        Err(why) => (Arc::new(NativeSolver), Backend::Native, Some(why)),
    }
}

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
#[derive(Clone)]
pub enum JobOp {
    Simplify(Expr),
    Integrate(Expr, SymbolId),
    Solve(Expr, SymbolId),
    /// A comparison the user typed, with both sides and the comparator intact.
    ///
    /// Beside [`JobOp::Solve`] rather than inside it on purpose: folding the
    /// sides into a difference first makes `<` and `>` the same expression, so
    /// the comparator has to be part of the request.
    SolveRelation(Relation, SymbolId),
    /// The whole document's equations, solved together for all its variables.
    SolveSystem(Vec<Expr>, Vec<SymbolId>),
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
    /// Every kind of solution of an equation (solve): none, all, values, a
    /// family, or an interval.
    Solutions(canlab_solver::Solutions),
    /// One value per variable, from solving a system (solve).
    Assignments(Vec<(SymbolId, Expr)>),
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
    /// True when the failure is one the backend cannot recover from. The worker
    /// retires SymPy only for these: `BadOutput` is one malformed line from a
    /// worker that is still alive, not a dead pipe.
    pub fatal: bool,
}

/// The app-side handle to the worker thread.
pub struct Jobs {
    tx: Option<Sender<Job>>,
    /// Behind a lock only because `Receiver` is not `Sync`, and Bevy's resources
    /// must be. Nothing contends for it: one thread polls, and only the UI does.
    rx: Mutex<Receiver<JobResult>>,
    in_flight: usize,
    running: Option<String>,
    /// Filled in by the worker thread once it has chosen a backend.
    status: Arc<Mutex<BackendStatus>>,
}

impl Jobs {
    /// Spawn the worker. A failed spawn leaves the app usable; solver buttons
    /// then report that no backend is available instead of hanging.
    pub fn spawn() -> Self {
        let (tx, job_rx) = mpsc::channel::<Job>();
        let (result_tx, rx) = mpsc::channel::<JobResult>();
        let status = Arc::new(Mutex::new(BackendStatus::default()));
        let shared = Arc::clone(&status);
        let spawned = std::thread::Builder::new()
            .name(String::from("canlab-solver"))
            .spawn(move || {
                // Choosing may start a process (finding Python), which is exactly
                // why it happens here and not in a frame.
                let (mut solver, mut backend, note) = choose_backend();
                {
                    let mut guard = shared
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    guard.backend = Some(backend);
                    guard.note = note;
                }
                while let Ok(job) = job_rx.recv() {
                    // A *fatal* answer means the backend is gone for good — a
                    // dead worker, a timeout, a Python without SymPy. Retry the
                    // same request on the native backend once, and say so, rather
                    // than leaving every later request to fail against a dead
                    // pipe. A `BadOutput` is a malformed line, not a dead backend,
                    // and must not retire a worker that is still alive.
                    let retry = job.op.clone();
                    let mut result = run(&*solver, backend, job);
                    if should_retire_backend(result.fatal, backend) {
                        solver = Arc::new(NativeSolver);
                        backend = Backend::Native;
                        let mut guard = shared
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        guard.backend = Some(backend);
                        guard.note =
                            Some(String::from("SymPy failed; using the native backend"));
                        drop(guard);
                        result = run(
                            &*solver,
                            backend,
                            Job {
                                label: result.label.clone(),
                                op: retry,
                                generation: result.generation,
                            },
                        );
                    }
                    if result_tx.send(result).is_err() {
                        break;
                    }
                }
            });
        let tx = if spawned.is_ok() { Some(tx) } else { None };
        Self {
            tx,
            rx: Mutex::new(rx),
            in_flight: 0,
            running: None,
            status,
        }
    }

    /// What the worker is using, and why it is not using SymPy (if it is not).
    pub fn backend(&self) -> BackendStatus {
        self.status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// A one-line description of the backend for a panel header.
    pub fn backend_label(&self) -> String {
        match self.backend().backend {
            Some(backend) => backend.label().to_string(),
            None => String::from("starting…"),
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
        let rx = self.rx.lock().expect("job receiver lock");
        loop {
            match rx.try_recv() {
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

/// Should a failed answer retire the backend for the rest of the session?
///
/// Only a fatal error means the backend is gone: `BadOutput` is one malformed
/// line from a worker that is still alive, and a decline is an answer. Retiring
/// on every `Failed` silently ended SymPy for one protocol hiccup and blamed
/// SymPy in the note.
fn should_retire_backend(fatal: bool, backend: Backend) -> bool {
    fatal && backend != Backend::Native
}

fn run(solver: &dyn Solver, backend: Backend, job: Job) -> JobResult {
    let Job {
        label,
        op,
        generation,
    } = job;
    let (kind, var, outcome, fatal) = match op {
        JobOp::Simplify(expr) => {
            let (outcome, fatal) = single(solver.simplify(&expr), JobKind::Simplify, backend);
            (JobKind::Simplify, None, outcome, fatal)
        }
        JobOp::Integrate(expr, var) => {
            let (outcome, fatal) = single(solver.integrate(&expr, var), JobKind::Integrate, backend);
            (JobKind::Integrate, Some(var), outcome, fatal)
        }
        JobOp::Solve(expr, var) => {
            // `solve_all` rather than `solve`: the panel can show none, all, a
            // family and an interval, and a plain list of values would throw
            // three of those away before the answer reached the screen.
            let (outcome, fatal) = match solver.solve_all(&expr, var) {
                Ok(solutions) => (JobOutcome::Solutions(solutions), false),
                Err(err) => declined_or_failed(err, JobKind::Solve, backend),
            };
            (JobKind::Solve, Some(var), outcome, fatal)
        }
        JobOp::SolveRelation(relation, var) => {
            // The answer type is the same one an equation uses: `x < 2` is a
            // union of intervals, which is one more kind of `Solutions`.
            let (outcome, fatal) = match solver.solve_relation(&relation, var) {
                Ok(solutions) => (JobOutcome::Solutions(solutions), false),
                // SymPy has no relation solver behind this trait, while the
                // native sign chart answers the school cases exactly. As with a
                // system, trying the other backend beats declining.
                Err(err) if err.is_declined() && backend != Backend::Native => {
                    match NativeSolver.solve_relation(&relation, var) {
                        Ok(solutions) => (JobOutcome::Solutions(solutions), false),
                        Err(native) => declined_or_failed(native, JobKind::Solve, Backend::Native),
                    }
                }
                Err(err) => declined_or_failed(err, JobKind::Solve, backend),
            };
            (JobKind::Solve, Some(var), outcome, fatal)
        }
        JobOp::SolveSystem(equations, vars) => {
            let (outcome, fatal) = match solver.solve_system(&equations, &vars) {
                Ok(assignments) => (JobOutcome::Assignments(assignments), false),
                // SymPy has no system solver behind this trait, while the native
                // matrix elimination solves exactly these. A system is the one
                // case where trying the other backend beats declining.
                Err(err) if err.is_declined() && backend != Backend::Native => {
                    match NativeSolver.solve_system(&equations, &vars) {
                        Ok(assignments) => (JobOutcome::Assignments(assignments), false),
                        Err(native) => declined_or_failed(native, JobKind::Solve, Backend::Native),
                    }
                }
                Err(err) => declined_or_failed(err, JobKind::Solve, backend),
            };
            (JobKind::Solve, vars.first().copied(), outcome, fatal)
        }
    };
    JobResult {
        label,
        kind,
        var,
        outcome,
        generation,
        fatal,
    }
}

fn single(result: Result<Expr, SolverError>, kind: JobKind, backend: Backend) -> (JobOutcome, bool) {
    match result {
        Ok(expr) => (JobOutcome::Value(expr), false),
        Err(err) => declined_or_failed(err, kind, backend),
    }
}

/// A backend declining is a real answer, and the panel has to be able to say
/// *what* it declined and what the backend can do instead. "No solver backend"
/// is the one thing it is not: the backend answered.
///
/// The bool is whether the error is fatal to the backend. The worker uses it to
/// decide whether to retire SymPy; `JobOutcome` stays a plain answer for the
/// panel.
fn declined_or_failed(err: SolverError, kind: JobKind, backend: Backend) -> (JobOutcome, bool) {
    let fatal = err.is_fatal();
    if err.is_declined() {
        (JobOutcome::Declined(backend.repertoire(kind)), false)
    } else {
        (JobOutcome::Failed(err.to_string()), fatal)
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use canlab_core::{int, pow, sym};
    use canlab_solver::MockSolver;

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
            Backend::Native,
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
        let result = run(&NativeSolver, Backend::Native, job(JobOp::Solve(equation, SymbolId::intern("x"))));
        match result.outcome {
            JobOutcome::Solutions(canlab_solver::Solutions::Values(values)) => {
                assert_eq!(values.len(), 1);
                assert_eq!(values[0].to_string(), "3");
            }
            other => panic!("expected a set of values, got {}", describe(&other)),
        }
    }

    #[test]
    fn a_declined_solve_is_not_reported_as_a_failure() {
        // `sin x − x² − 5` is below −4 everywhere: no closed form, and no root
        // for the numeric scan to find. Declining is a real answer, and the UI
        // must show "declined", not an error. (`x³ − 2` used to be the example
        // here; the numeric fallback now answers it approximately, which is a
        // different behaviour and a different test.)
        let equation = canlab_core::add(vec![
            canlab_core::ExprNode::fn_raw(SymbolId::intern("sin"), vec![sym("x")]),
            canlab_core::mul(vec![int(-1), pow(sym("x"), int(2))]),
            int(-5),
        ]);
        let result = run(
            &NativeSolver,
            Backend::Native,
            job(JobOp::Solve(equation, SymbolId::intern("x"))),
        );
        assert!(result.kind.is_solve());
        match result.outcome {
            JobOutcome::Declined(_) => {}
            other => assert!(matches!(other, JobOutcome::Declined(_))),
        }
    }

    #[test]
    fn a_declining_backend_falls_back_to_native_system_elimination() {
        // MockSolver leaves `solve_system` at the trait default, so it stands in
        // for a backend that cannot do systems. Naming a non-native backend must
        // make the worker retry on NativeSolver, not report the decline.
        let equations = vec![
            canlab_core::add(vec![sym("x"), sym("y"), int(-3)]),
            canlab_core::add(vec![
                canlab_core::mul(vec![int(2), sym("x")]),
                canlab_core::mul(vec![int(-1), sym("y")]),
            ]),
        ];
        let vars = vec![SymbolId::intern("x"), SymbolId::intern("y")];

        let fallback = run(
            &MockSolver,
            Backend::Sympy,
            job(JobOp::SolveSystem(equations.clone(), vars.clone())),
        );
        assert_x_is_one_and_y_is_two(fallback);

        // The ordinary native path returns the same answer, without a retry.
        let native = run(
            &NativeSolver,
            Backend::Native,
            job(JobOp::SolveSystem(equations, vars)),
        );
        assert_x_is_one_and_y_is_two(native);
    }

    /// Check a system answer against `x = 1, y = 2`, in any assignment order.
    fn assert_x_is_one_and_y_is_two(result: JobResult) {
        match result.outcome {
            JobOutcome::Assignments(assignments) => {
                assert_eq!(assignments.len(), 2, "{assignments:?}");
                for (variable, value) in &assignments {
                    let expected = match variable.as_str().as_str() {
                        "x" => "1",
                        "y" => "2",
                        other => panic!("unexpected variable {other}"),
                    };
                    assert_eq!(value.to_string(), expected, "{variable:?}");
                }
            }
            other => panic!("expected assignments, got {}", describe(&other)),
        }
    }

    /// A backend whose every operation fails with the same error.
    struct Fails(SolverError);

    impl Solver for Fails {
        fn simplify(&self, _: &Expr) -> Result<Expr, SolverError> {
            Err(self.0.clone())
        }
        fn diff(&self, _: &Expr, _: SymbolId) -> Result<Expr, SolverError> {
            Err(self.0.clone())
        }
        fn integrate(&self, _: &Expr, _: SymbolId) -> Result<Expr, SolverError> {
            Err(self.0.clone())
        }
        fn solve(&self, _: &Expr, _: SymbolId) -> Result<Vec<Expr>, SolverError> {
            Err(self.0.clone())
        }
        fn limit(&self, _: &Expr, _: SymbolId, _: &Expr) -> Result<Expr, SolverError> {
            Err(self.0.clone())
        }
        fn series(&self, _: &Expr, _: SymbolId, _: &Expr, _: usize) -> Result<Expr, SolverError> {
            Err(self.0.clone())
        }
    }

    #[test]
    fn only_a_fatal_error_retires_sympy() {
        let failed = |error: SolverError| {
            run(
                &Fails(error),
                Backend::Sympy,
                job(JobOp::Simplify(sym("x"))),
            )
        };

        // A malformed line came from a worker that is still alive: keep it.
        let bad_output = failed(SolverError::BadOutput(String::from("not a result")));
        assert!(matches!(bad_output.outcome, JobOutcome::Failed(_)));
        assert!(!bad_output.fatal);
        assert!(!should_retire_backend(bad_output.fatal, Backend::Sympy));

        // A timeout or a dead worker is fatal: retire SymPy and retry natively.
        for error in [
            SolverError::Timeout,
            SolverError::Backend(String::from("worker exited")),
        ] {
            let result = failed(error);
            assert!(matches!(result.outcome, JobOutcome::Failed(_)));
            assert!(result.fatal);
            assert!(should_retire_backend(result.fatal, Backend::Sympy));
        }

        // A decline is an answer, not a failure.
        let declined = run(
            &MockSolver,
            Backend::Sympy,
            job(JobOp::Simplify(sym("x"))),
        );
        assert!(matches!(declined.outcome, JobOutcome::Declined(_)));
        assert!(!declined.fatal);
        assert!(!should_retire_backend(declined.fatal, Backend::Sympy));

        // Native is never retired in favour of itself.
        assert!(!should_retire_backend(true, Backend::Native));
    }

    fn describe(outcome: &JobOutcome) -> &'static str {
        match outcome {
            JobOutcome::Value(_) => "a value",
            JobOutcome::Solutions(_) => "solutions",
            JobOutcome::Assignments(_) => "assignments",
            JobOutcome::Declined(_) => "declined",
            JobOutcome::Failed(_) => "a failure",
        }
    }
}
