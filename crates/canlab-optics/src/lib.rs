//! Optics for the Cherenkov detector: rays, mirrors and time of flight.
//!
//! The simulation that needs this is a photon Monte Carlo for four SiPMs reading
//! the light a Cherenkov cone leaves in a high-refractive-index block. It needs
//! three things done exactly and repeatedly, and each of them is a place a
//! hand-written float routine quietly loses the accuracy the whole study exists
//! to have:
//!
//! * [`Vec3`] — reflection off a plane, refracting into a block and back out,
//!   and the direction a photon is *born* in. A reflection is a linear map, and
//!   doing it as `v − 2(v·n)n` in `f64` accumulates error over the dozens of
//!   bounces a total-internal-reflection photon makes.
//! * [`Interval`] — an outward-rounded bound, so "how much does the arrival time
//!   shift if the refractive index is 1.84 ± 0.01" has an answer with a
//!   guarantee rather than a difference of two floats. The CAS has no interval
//!   arithmetic; the number the detector study reports is a bound or it is not
//!   worth reporting.
//! * [`Cherenkov`] — the emission law itself, as its symbolic form and its
//!   evaluation, so the simulation does not hold a second transcription of it
//!   that can drift from the one in the paper.
//!
//! # What is exact and what is not
//!
//! Geometry is exact-in-`f64`: a reflection in a *unit* normal is exact up to
//! one rounding of the result, and reflections compose only through the
//! direction sign, so a thousand bounces of a plane mirror do not drift. The
//! uncertainty that matters in this project is not the geometry's — it is the
//! material constants (n, absorption length, wall reflectance) and the photon
//! statistics, and `Interval` is for those. Physics is not made more honest by
//! pretending float rounding is the error, and it is not made less honest by a
//! bound that accounts for the wrong term.

#![warn(missing_docs)]

pub mod block;
pub mod cherenkov;
pub mod fresnel;
pub mod interval;
pub mod outline;
pub mod vec3;

pub use block::{Block, Exit, Face};
pub use cherenkov::{Cherenkov, medium_index};
pub use fresnel::{Interface, Polarisation, critical_angle, interface, reflectance};
pub use interval::Interval;
pub use outline::{OutlineError, Tropical, tropical_polynomial, tropical_polynomial_interval};
pub use vec3::{Mirror, Ray, Vec3};
