//! The solver bridge: one trait, one wire format, several backends.
//!
//! A computer algebra system that only ever evaluates what it can prove is a
//! small system. The interesting operations — symbolic integration, solving,
//! limits, series — are the ones where the honest answer is often "I do not
//! know". This crate is organised around that fact:
//!
//! * [`SolverError::NotImplemented`] is a **normal result**, not a failure. The
//!   UI renders an unevaluated [`canlab_core::Hold::Integral`] node (or the
//!   original equation) instead of inventing an answer. Only
//!   [`SolverError::Timeout`], [`SolverError::Backend`] and
//!   [`SolverError::BadOutput`] are real errors.
//! * [`Solver`] is object-safe and `Send + Sync`, so a UI thread can hold an
//!   `Arc<dyn Solver>` and swap implementations without knowing anything about
//!   processes or pipes.
//! * The heavy lifting lives behind a *process* ([`sympy::SympyWorker`]), never
//!   in-process. A symbolic backend that segfaults, leaks memory or hangs must
//!   not take the editor down with it; a subprocess boundary turns those into
//!   recoverable errors.
//!
//! # The protocol
//!
//! Requests and responses are single-line JSON objects, because JSON is the one
//! format every candidate backend (Python, Julia, a Rust service) parses
//! natively, and because a line is a natural unit for "one answer, or none".
//! [`session::Session`] is generic over `BufRead`/`Write`, which is what makes
//! the protocol testable without a backend at all: the tests drive it from
//! [`std::io::Cursor`].
//!
//! # Deliberately out of scope
//!
//! There is no caching and no retry policy here. Both are policy decisions for
//! the caller; bolting a cache into the bridge would hide the backend's
//! cost model from the layer that can actually reason about it.

#![warn(missing_docs)]

use canlab_core::{Expr, Relation, SymbolId};

pub mod exponential;
pub mod inequality;
pub mod logarithmic;
pub mod json;
pub mod mock;
pub mod native;
pub mod nonlinear;
pub mod parametric;
pub mod solutions;
pub mod system;
pub mod session;
pub mod trig;
pub mod sympy;

pub use json::{decode, decode_array, decode_solutions, encode, encode_solutions};
pub use mock::MockSolver;
pub use native::NativeSolver;
pub use solutions::{Bound, Point, Solutions, SystemAnswer};
pub use system::{solve_system, substitute, variables_of};
pub use session::Session;
pub use sympy::{Discovery, Interpreter, SympyWorker, discover, discovery, probe};

/// Everything that can go wrong on the way to a backend — plus the one thing
/// that is not wrong at all.
///
/// `NotImplemented` deliberately shares the `Result` channel with the failures.
/// The alternative, an `Option` inside an `Option`, forces every caller to
/// remember two error paths. One enum, one `match`, one rendering decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SolverError {
    /// The backend understood the request and declined.
    ///
    /// This is a *legitimate answer*: the caller should keep the expression
    /// unevaluated (for integrals, as a [`canlab_core::Hold::Integral`] node) and
    /// show it to the user. It must never be presented as a crash.
    NotImplemented,
    /// The backend did not answer within the configured deadline and was
    /// killed by [`sympy::SympyWorker`]. The worker has to be restarted before
    /// the next request.
    Timeout,
    /// The backend failed: the worker process died, could not be spawned, or
    /// reported an error of its own.
    Backend(String),
    /// The backend answered, but not in the protocol. Treat this as a bug in
    /// the worker, never as a user error.
    BadOutput(String),
}

impl SolverError {
    /// `true` for [`SolverError::NotImplemented`] only.
    ///
    /// UIs call this to decide between "show the unevaluated expression" and
    /// "show an error banner".
    pub fn is_declined(&self) -> bool {
        matches!(self, SolverError::NotImplemented)
    }

    /// `true` when the worker is no longer usable and must be respawned.
    pub fn is_fatal(&self) -> bool {
        matches!(self, SolverError::Timeout | SolverError::Backend(_))
    }
}

impl std::fmt::Display for SolverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SolverError::NotImplemented => {
                write!(f, "the backend declined to evaluate this expression")
            }
            SolverError::Timeout => write!(f, "the backend did not answer before the deadline"),
            SolverError::Backend(message) => write!(f, "solver backend failed: {message}"),
            SolverError::BadOutput(message) => {
                write!(f, "solver backend broke the protocol: {message}")
            }
        }
    }
}

impl std::error::Error for SolverError {}

/// A symbolic backend.
///
/// Implementations must be usable from several threads: the CAS keeps one
/// solver behind an `Arc` and may call it from a worker pool. `&self` methods
/// also mean a single backend can serialise requests internally without the
/// trait having to say so.
pub trait Solver: Send + Sync {
    /// Put `e` into the backend's preferred form.
    fn simplify(&self, e: &Expr) -> Result<Expr, SolverError>;

    /// `d/d(var) e`.
    fn diff(&self, e: &Expr, var: SymbolId) -> Result<Expr, SolverError>;

    /// An antiderivative of `e` with respect to `var`.
    ///
    /// [`SolverError::NotImplemented`] is expected and common: not every
    /// elementary function has an elementary antiderivative, and no backend
    /// knows all of the ones that do. The caller renders an unevaluated
    /// integral rather than guessing.
    fn integrate(&self, e: &Expr, var: SymbolId) -> Result<Expr, SolverError>;

    /// All solutions of `eq == 0` in `var`, as an unordered list.
    ///
    /// An empty vector is a real answer ("no solutions"); a backend that
    /// cannot solve the equation returns [`SolverError::NotImplemented`].
    ///
    /// [`Solver::solve_all`] asks the same question with an answer type that
    /// can also carry "every number", a family and an interval; new callers
    /// should prefer it and fall back to this only when they specifically want
    /// a list.
    fn solve(&self, eq: &Expr, var: SymbolId) -> Result<Vec<Expr>, SolverError>;

    /// All solutions of `eq == 0` in `var`, as an answer that can say more
    /// than a list can.
    ///
    /// The default maps [`Solver::solve`] into [`Solutions::Values`], so a
    /// backend written against the older signature keeps working without a
    /// change and keeps exactly the answer it used to give. A backend that can
    /// tell "no solution" from "every number", or answer with a family or an
    /// interval, overrides this.
    ///
    /// An empty [`Solutions::Values`] is the old `solve` contract's "no
    /// solutions"; [`Solutions::is_empty`] treats it like [`Solutions::None`]
    /// when only the answer, not the wording, matters.
    fn solve_all(&self, eq: &Expr, var: SymbolId) -> Result<Solutions, SolverError> {
        self.solve(eq, var).map(Solutions::Values)
    }

    /// The solution set of `relation` in `var`, as a union of intervals.
    ///
    /// The default refuses. That is deliberate: an inequality folded into
    /// `left − right` is indistinguishable from the same fold of its opposite,
    /// so a backend that cannot see the comparator must not answer. A backend
    /// that can answer overrides this; the native one does, with a sign chart.
    fn solve_relation(
        &self,
        _relation: &Relation,
        _var: SymbolId,
    ) -> Result<Solutions, SolverError> {
        Err(SolverError::NotImplemented)
    }

    /// `lim(var -> at) e`.
    fn limit(&self, e: &Expr, var: SymbolId, at: &Expr) -> Result<Expr, SolverError>;

    /// The expansion of `e` around `at`, truncated at `order`.
    fn series(&self, e: &Expr, var: SymbolId, at: &Expr, order: usize)
    -> Result<Expr, SolverError>;

    /// Solve a *system*: all the equations together, for all the variables.
    ///
    /// Returns one assignment per variable it could pin down, in a stable order.
    /// A backend that only knows single equations leaves this at the default,
    /// which is an honest decline — solving `x = 1, y = x` one equation at a time
    /// would silently drop `y`.
    fn solve_system(
        &self,
        _equations: &[Expr],
        _vars: &[SymbolId],
    ) -> Result<Vec<(SymbolId, Expr)>, SolverError> {
        Err(SolverError::NotImplemented)
    }

    /// Where several equations hold at once, as a set of **points**.
    ///
    /// Separate from [`Solver::solve_system`] because the two answers are
    /// different shapes: a linear system pins one value per unknown, while a
    /// circle and a line meet in two places and each place pairs an `x` with its
    /// own `y`. The default declines, so a backend that does not implement this
    /// cannot silently return one point and look finished.
    fn solve_nonlinear(
        &self,
        _equations: &[Expr],
        _vars: &[SymbolId],
    ) -> Result<SystemAnswer, SolverError> {
        Err(SolverError::NotImplemented)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declined_is_not_fatal() {
        assert!(SolverError::NotImplemented.is_declined());
        assert!(!SolverError::NotImplemented.is_fatal());
        assert!(!SolverError::Timeout.is_declined());
        assert!(SolverError::Timeout.is_fatal());
        assert!(SolverError::Backend("boom".into()).is_fatal());
        assert!(!SolverError::BadOutput("huh".into()).is_fatal());
    }

    #[test]
    fn errors_display_without_debug_noise() {
        assert_eq!(
            SolverError::Timeout.to_string(),
            "the backend did not answer before the deadline"
        );
        assert!(
            SolverError::Backend("worker exited".into())
                .to_string()
                .contains("worker exited")
        );
    }

    #[test]
    fn solver_error_is_a_std_error() {
        fn takes_error(_: &dyn std::error::Error) {}
        takes_error(&SolverError::NotImplemented);
    }

    /// A backend that only knows the answer type this crate had before
    /// `solve_all` existed. It exists to pin the default implementation.
    struct OnlySolve;

    impl Solver for OnlySolve {
        fn simplify(&self, _e: &Expr) -> Result<Expr, SolverError> {
            Err(SolverError::NotImplemented)
        }

        fn diff(&self, _e: &Expr, _var: SymbolId) -> Result<Expr, SolverError> {
            Err(SolverError::NotImplemented)
        }

        fn integrate(&self, _e: &Expr, _var: SymbolId) -> Result<Expr, SolverError> {
            Err(SolverError::NotImplemented)
        }

        fn solve(&self, _eq: &Expr, _var: SymbolId) -> Result<Vec<Expr>, SolverError> {
            Ok(vec![canlab_core::int(2), canlab_core::int(-2)])
        }

        fn limit(&self, _e: &Expr, _var: SymbolId, _at: &Expr) -> Result<Expr, SolverError> {
            Err(SolverError::NotImplemented)
        }

        fn series(
            &self,
            _e: &Expr,
            _var: SymbolId,
            _at: &Expr,
            _order: usize,
        ) -> Result<Expr, SolverError> {
            Err(SolverError::NotImplemented)
        }
    }

    #[test]
    fn the_default_solve_all_keeps_the_old_answer() {
        let var = SymbolId::intern("x");
        assert_eq!(
            OnlySolve.solve_all(&canlab_core::sym("x"), var),
            Ok(Solutions::Values(vec![
                canlab_core::int(2),
                canlab_core::int(-2)
            ]))
        );
        // A decline stays a decline; the default must not turn "I cannot" into
        // an empty answer.
        assert_eq!(
            MockSolver.solve_all(&canlab_core::sym("x"), var),
            Err(SolverError::NotImplemented)
        );
    }

    #[test]
    fn the_default_solve_relation_declines() {
        // A backend that cannot see the comparator must not answer an
        // inequality: the difference it would receive is the same for `<` and
        // `>`. The default is the refusal, not a guess.
        let relation = Relation::new(
            canlab_core::sym("x"),
            canlab_core::int(2),
            canlab_core::Comparator::Less,
        );
        assert_eq!(
            OnlySolve.solve_relation(&relation, SymbolId::intern("x")),
            Err(SolverError::NotImplemented)
        );
    }
}
