//! The muon-direction detector study, kept apart from the CAS.
//!
//! The physics is [`canlab_optics`]: the tent prism, the Cherenkov source, the
//! photon transport and one event's four timestamps. This crate asks the *design*
//! questions of it, and it is separate because those questions have their own
//! parameters and their own answers, none of which the CAS needs.
//!
//! # The three questions, in the order they have to be answered
//!
//! 1. **Does it fit?** [`can`] — the tent is `4.5·scale` tall and the can is
//!    115 mm, so the geometry sweep's best row is 135 mm of block in a 115 mm
//!    can. The envelope is a refusal, and it is the first question because a
//!    detector that does not fit has no resolution to compute.
//! 2. **Can four timestamps determine a direction?** [`timing`] and [`bound`] —
//!    the fit exists and is correct, and the bound says the *scale* is wrong:
//!    light crosses the whole block in 0.27 ps while the SiPM jitters by 100 ps.
//!    The answer is not a poor resolution, it is no resolution.
//! 3. **What observable would?** [`bound::position_bound`] — a sensor that
//!    reports *where* a photon hit, rather than when, gives 1.5° for a millimetre
//!    of pixel. That is the design the numbers point at, and it is a different
//!    detector, not a better simulation of this one.
//!
//! # What this crate deliberately does not do
//!
//! It does not train a network. A network is a way to approach a bound, and the
//! bound for the timing observable is tens of thousands of degrees: a network
//! trained on four timestamps would learn to output the mean direction, and its
//! loss curve would be the loss curve of that mean. Training it would be a
//! demonstration that the bound is right, at the cost of the training. The
//! network is worth building for the observable whose bound is small, and that
//! observable is the position, not the time.

#![warn(missing_docs)]

pub mod bound;
pub mod can;
pub mod components;
pub mod designs;
pub mod domain;
pub mod net;
pub mod mission;
pub mod muon;
pub mod scintillator;
pub mod tracker;
pub mod timing;
pub mod vmf;
pub mod timing_study;

mod solve;

pub use bound::{position_bound, timing_bound, Bound};
pub use components::{budget, dominant, total_jitter_ps, Contribution, COMPARATOR_WALK_PS, UNMODELLED};
pub use designs::{Approach, Design, Metrics};
pub use mission::{Candidate, Manufacturing, Mission, Violation};
pub use scintillator::{Scintillator, SiPMChoice};
pub use domain::Domain;
pub use net::{Network, Sample};
pub use tracker::{Event, LightModel, Rng, Tracker};
pub use timing_study::{TimingDesign, TimingOutcome, TimingRun};
pub use muon::MuonFlux;
pub use vmf::{VmfNetwork, VmfSample};
pub use can::{tent_extent_mm, tent_mass_g, tent_volume_mm3, Can, CanError};
pub use timing::{
    reconstruct, speed_mm_per_ps, tangent_basis, timing_signal_ps, FitError, Observation,
    Reconstruction, C_MM_PER_PS,
};
