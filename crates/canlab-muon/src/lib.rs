//! The muon-direction detector simulation.
//!
//! A separate crate on purpose: this is a physics simulation with its own
//! parameters and its own accuracy budget, and the CAS is used only to draw its
//! results. See `DESIGN.md` for the overview and the first iteration.
//!
//! Nothing is implemented yet. The design document is the deliverable of the
//! first round, and the simulation follows once the questions in it are answered
//! — the first of which is whether the timing budget is the TDC's or the SiPM's,
//! because that decides what the simulation has to be accurate about.
