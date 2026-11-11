//! `canlab-corpus` — exam problems as rows, scored by `canlab corpus`.
//!
//! The format is specified in `docs/corpus.md`; this crate is the implementation
//! of that specification and nothing else. Three jobs live here, and they are
//! kept apart on purpose:
//!
//! * [`row`] reads one TOML file into a [`row::Row`], refusing anything outside
//!   the restricted subset instead of guessing at it.
//! * [`document`] turns a row's `document` into a question with the editor's own
//!   reader (`canlab-input`), so the corpus and the field cannot disagree about
//!   what a document means.
//! * [`batch`] runs rows through child processes, so a crash or a leak costs one
//!   batch and never the whole run.
//!
//! [`report`] renders the coverage table and [`fetch`] downloads the cited
//! datasets. Nothing here opens a window, and no code path needs a network
//! unless `fetch` or `--update` is asked for.
//!
//! # What a refusal means here
//!
//! [`canlab_solver::SolverError::NotImplemented`] and a document the editor
//! cannot read are *answers*: the row may expect exactly that refusal, and a
//! refusal that names the right construct passes. Only a backend failure — a
//! dead worker, a malformed protocol line — is an error.

#![warn(missing_docs)]

pub mod batch;
pub mod compare;
pub mod document;
pub mod fetch;
pub mod json;
pub mod report;
pub mod row;
pub mod run;
pub mod sha256;

pub use row::{Expect, LoadError, Row, Subject, load_row, load_rows};
pub use run::{Outcome, RunReport};
