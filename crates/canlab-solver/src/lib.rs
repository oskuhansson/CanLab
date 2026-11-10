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

use canlab_core::{Expr, SymbolId};

pub mod json;
pub mod mock;
pub mod native;
pub mod session;
pub mod sympy;

pub use json::{decode, decode_array, encode};
pub use mock::MockSolver;
pub use native::NativeSolver;
pub use session::Session;
pub use sympy::SympyWorker;

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
    fn solve(&self, eq: &Expr, var: SymbolId) -> Result<Vec<Expr>, SolverError>;

    /// `lim(var -> at) e`.
    fn limit(&self, e: &Expr, var: SymbolId, at: &Expr) -> Result<Expr, SolverError>;

    /// The expansion of `e` around `at`, truncated at `order`.
    fn series(&self, e: &Expr, var: SymbolId, at: &Expr, order: usize)
    -> Result<Expr, SolverError>;
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
}
