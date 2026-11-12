//! Reflection and refraction at a dielectric interface, exactly as the laws
//! state them.
//!
//! Three formulas, and every one of them has a closed form a test can hold:
//!
//! * **Snell**: `n₁ sin θ₁ = n₂ sin θ₂`.
//! * **Fresnel**: the Fresnel coefficients for the two polarisations,
//!
//!   ```text
//!     r_s = (n₁ cos θ₁ − n₂ cos θ₂) / (n₁ cos θ₁ + n₂ cos θ₂)
//!     r_p = (n₂ cos θ₁ − n₁ cos θ₂) / (n₂ cos θ₁ + n₁ cos θ₂)
//!     R = (r_s² + r_p²) / 2,      T = 1 − R
//!   ```
//! * **Total internal reflection**: for `n₁ > n₂` and `sin θ₁ > n₂/n₁` there is
//!   no transmitted wave, and `R = 1`.
//!
//! The formulae are written in terms of `cos θ` and not `θ` because that is
//! what a ray carries — and because computing `cos θ` and then `√(1 − cos²)`
//! loses the cosine's sign near grazing incidence, which is exactly the
//! regime where `R → 1` and the answer needs to be right to parts in 10¹⁵.
//!
//! # What "unpolarised" means here
//!
//! Cherenkov light is unpolarised, so the reflectivity a photon sees is the
//! average of the two polarisations. A photon is *assigned* one polarisation at
//! emission (half `s`, half `p`) and keeps it across a bounce, because the
//! physical light is coherent only within the emission event — a photon does
//! not switch polarisation at a mirror. [`Fresnel::reflectance`] is the average
//! for a caller that only needs the probability; [`Fresnel::reflectance_of`]
//! is the one a transport uses so its random stream and its physics agree.
//!
//! # What it does not do
//!
//! It does not phase-shift, does not model evanescent waves beyond setting
//! `R = 1`, and does not handle absorbing media (a complex index). Outside the
//! visible band in a plastic, absorption is per path length in the transport,
//! not in a coefficient; a caller modelling a metal mirror needs more than is
//! here, and should say so.

use crate::vec3::Vec3;

/// The two linear polarisations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Polarisation {
    /// Perpendicular to the plane of incidence.
    S,
    /// In the plane of incidence.
    P,
}

impl Polarisation {
    /// The polarisation for a photon chosen uniformly from the two.
    pub fn of(unit: f64) -> Polarisation {
        if unit < 0.5 { Polarisation::S } else { Polarisation::P }
    }
}

/// The outcome of hitting an interface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Interface {
    /// The fraction of the incident intensity reflected (0..=1).
    pub reflectance: f64,
    /// The fraction transmitted.
    pub transmittance: f64,
    /// The refracted direction, or `None` under total internal reflection.
    pub refracted: Option<Vec3>,
    /// Whether the wave was totally internally reflected, which is also the
    /// `reflected = false` case for a transport that samples.
    pub total_internal_reflection: bool,
}

/// Fresnel and Snell at a planar interface with an *outward* normal.
///
/// `incident` points from the source *towards* the interface (that is, roughly
/// against the outward normal), `normal` is the outward unit normal, and
/// `cos_incidence` is `|incident · normal|`, kept as an argument so a caller
/// that has the vector can pass the exact quantity its ray already carries.
pub fn interface(
    incident: Vec3,
    normal: Vec3,
    n_outside: f64,
    n_inside: f64,
    polarisation: Polarisation,
) -> Option<Interface> {
    if !(n_outside > 0.0) || !(n_inside > 0.0) {
        return None;
    }
    let incident = incident.normalized()?;
    let normal = normal.normalized()?;
    // The interface is crossed from the medium on the side the photon comes
    // *from*. `normal` is the outward normal of the block, so a photon heading
    // out of the block has `incident · normal > 0` and is in the block; one
    // heading in has `incident · normal < 0` and is outside. The first version
    // of this function had the test inverted, so a photon leaving the block was
    // treated as if it were entering it.
    let leaving_block = incident.dot(normal) > 0.0;
    let (n_from, n_to) = if leaving_block {
        (n_inside, n_outside)
    } else {
        (n_outside, n_inside)
    };
    let cos_from = incident.dot(normal).abs().clamp(0.0, 1.0);
    let ratio = n_from / n_to;
    let sin_out_squared = ratio * ratio * (1.0 - cos_from * cos_from);
    if sin_out_squared > 1.0 {
        // Total internal reflection: no transmitted wave at any angle.
        return Some(Interface {
            reflectance: 1.0,
            transmittance: 0.0,
            refracted: None,
            total_internal_reflection: true,
        });
    }
    let cos_to = (1.0 - sin_out_squared).max(0.0).sqrt();
    // Fresnel, in the `cos` forms, for the ray travelling from `n_from` into
    // `n_to`. At normal incidence `cos_from = cos_to = 1` and both reduce to
    // `(n_from − n_to)²/(n_from + n_to)²`, which is the check the test makes.
    let (r_s, r_p) = (
        (n_from * cos_from - n_to * cos_to) / (n_from * cos_from + n_to * cos_to),
        (n_to * cos_from - n_from * cos_to) / (n_to * cos_from + n_from * cos_to),
    );
    let reflectance = match polarisation {
        Polarisation::S => r_s * r_s,
        Polarisation::P => r_p * r_p,
    }
    .clamp(0.0, 1.0);
    // Snell, in vector form, for an incident ray `i` pointing *against* the
    // interface normal `N`: `t = (n_from/n_to)·i + (ratio·cos_from − cos_to)·N`.
    // For a photon leaving the block `i` points along `+N`, so the formula is
    // applied to `−i` and the result flipped back. Using it on `i` directly
    // makes the transmitted ray bend the wrong way — which is what the Snell
    // test caught, twice.
    let (against, sign) = if leaving_block {
        (incident.scale(-1.0), -1.0)
    } else {
        (incident, 1.0)
    };
    let transmitted = against
        .scale(ratio)
        .add(normal.scale(cos_from * ratio - cos_to))
        .scale(sign);
    Some(Interface {
        reflectance,
        transmittance: 1.0 - reflectance,
        refracted: transmitted.normalized(),
        total_internal_reflection: false,
    })
}

/// The unpolarised reflectance of an interface: the mean of the two.
pub fn reflectance(incident: Vec3, normal: Vec3, n_outside: f64, n_inside: f64) -> Option<f64> {
    let s = interface(incident, normal, n_outside, n_inside, Polarisation::S)?;
    let p = interface(incident, normal, n_outside, n_inside, Polarisation::P)?;
    Some(0.5 * (s.reflectance + p.reflectance))
}

/// The critical angle for total internal reflection, radians.
///
/// `asin(n_outside / n_inside)` for `n_inside > n_outside`, and `None` when
/// there is no critical angle at all (traversing from the low to the high index,
/// every ray transmits).
pub fn critical_angle(n_inside: f64, n_outside: f64) -> Option<f64> {
    if n_outside <= 0.0 || n_inside <= n_outside {
        return None;
    }
    Some((n_outside / n_inside).asin())
}

/// Snell's law as an angle, for a test to compare against the closed form.
pub fn refracted_angle(theta_inside: f64, n_inside: f64, n_outside: f64) -> Option<f64> {
    if !(0.0..=std::f64::consts::FRAC_PI_2).contains(&theta_inside) {
        return None;
    }
    let sine = n_inside * theta_inside.sin() / n_outside;
    // `sin` of the transmitted angle is > 1 exactly under total internal
    // reflection, where there is no transmitted angle.
    (sine <= 1.0).then(|| sine.asin())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_PI_4;

    fn close(a: f64, b: f64, tolerance: f64) -> bool {
        (a - b).abs() <= tolerance
    }

    #[test]
    fn r_plus_t_is_one_everywhere_outside_total_internal_reflection() {
        for degrees in [0.0f64, 10.0, 30.0, 45.0, 60.0, 80.0] {
            let theta = degrees.to_radians();
            for (n1, n2) in [(1.0, 1.5), (1.5, 1.0), (1.0, 1.84)] {
                let incident = Vec3::new(theta.sin(), 0.0, -theta.cos());
                let out = interface(incident, Vec3::Z, n1, n2, Polarisation::S).unwrap();
                assert!(close(out.reflectance + out.transmittance, 1.0, 1e-15), "{degrees} {n1} {n2}");
            }
        }
    }

    #[test]
    fn the_refracted_angle_matches_snells_law() {
        for degrees in [0.0f64, 10.0, 30.0, 44.0] {
            let theta = degrees.to_radians();
            let incident = Vec3::new(theta.sin(), 0.0, -theta.cos());
            let out = interface(incident, Vec3::Z, 1.0, 1.5, Polarisation::S).unwrap();
            let transmitted = out.refracted.expect("transmits below critical");
            let sin_theta_2 = Vec3::new(transmitted.x, transmitted.y, 0.0).length();
            let expected = theta.sin() / 1.5;
            assert!(close(sin_theta_2, expected, 1e-12), "{degrees}: {sin_theta_2} vs {expected}");
            // And the transmitted ray bends *towards* the normal when entering
            // the denser medium. At normal incidence the two directions are the
            // same and the cosines differ only by rounding, so the strict
            // comparison belongs to the non-zero angles.
            if theta > 0.0 {
                assert!(transmitted.dot(Vec3::Z).abs() > theta.cos(), "{degrees}");
            }
        }
    }

    #[test]
    fn the_critical_angle_is_where_total_internal_reflection_starts() {
        let n = 1.5;
        let critical = critical_angle(n, 1.0).unwrap();
        assert!(close(critical.to_degrees(), 41.810, 0.001), "{}", critical.to_degrees());
        // Just inside: partly transmitted. Just outside: all reflected.
        let inside = critical - 1e-6;
        let outside = critical + 1e-6;
        let at = |theta: f64| {
            let incident = Vec3::new(theta.sin(), 0.0, -theta.cos());
            interface(incident, Vec3::Z, n, 1.0, Polarisation::S).unwrap()
        };
        assert!(!at(inside).total_internal_reflection);
        assert!(at(outside).total_internal_reflection);
        assert_eq!(at(outside).reflectance, 1.0);
        assert!(at(outside).refracted.is_none());
    }

    #[test]
    fn there_is_no_critical_angle_entering_a_denser_medium() {
        assert!(critical_angle(1.0, 1.5).is_none());
        assert!(critical_angle(1.5, 1.5).is_none());
    }

    #[test]
    fn the_brewster_angle_drives_the_p_reflectance_to_zero() {
        // Brewster: tan(theta_B) = n2/n1. At that angle the p-polarised wave is
        // reflected not at all, and s-polarised is not zero — that contrast is
        // the test that the two formulas were not swapped.
        let (n1, n2) = (1.0, 1.5f64);
        let brewster = (n2 / n1).atan();
        let incident = Vec3::new(brewster.sin(), 0.0, -brewster.cos());
        let p = interface(incident, Vec3::Z, n1, n2, Polarisation::P).unwrap();
        let s = interface(incident, Vec3::Z, n1, n2, Polarisation::S).unwrap();
        assert!(p.reflectance < 1e-14, "p at Brewster: {}", p.reflectance);
        assert!(s.reflectance > 0.05, "s at Brewster: {}", s.reflectance);
        assert!(close(brewster.to_degrees(), 56.3099, 1e-3));
    }

    #[test]
    fn normal_incidence_has_the_textbook_reflectance() {
        // R = ((n2 − n1)/(n2 + n1))², for both polarisations.
        let r = reflectance(Vec3::new(0.0, 0.0, -1.0), Vec3::Z, 1.5, 1.0).unwrap();
        let expected = ((1.5f64 - 1.0) / (1.5 + 1.0)).powi(2);
        assert!(close(r, expected, 1e-15), "{r} vs {expected}");
    }

    #[test]
    fn reflectance_rises_monotonically_to_one_at_grazing() {
        let mut previous = -1.0;
        for step in 0..=90 {
            let theta = (step as f64).to_radians();
            let incident = Vec3::new(theta.sin(), 0.0, -theta.cos());
            let r = reflectance(incident, Vec3::Z, 1.0, 1.5).unwrap();
            assert!(r >= previous - 1e-12, "fell at {step} degrees");
            previous = r;
            if step == 90 {
                assert!(close(r, 1.0, 1e-9), "grazing: {r}");
            }
        }
    }

    #[test]
    fn a_degenerate_direction_or_medium_is_refused() {
        assert!(interface(Vec3::ZERO, Vec3::Z, 1.0, 1.5, Polarisation::S).is_none());
        assert!(interface(Vec3::new(0.0, 0.0, -1.0), Vec3::Z, 0.0, 1.5, Polarisation::S).is_none());
        assert!(interface(Vec3::new(0.0, 0.0, -1.0), Vec3::ZERO, 1.0, 1.5, Polarisation::S).is_none());
    }

    #[test]
    fn the_angle_helpers_agree_with_the_vector_form() {
        let theta = FRAC_PI_4;
        let by_angle = refracted_angle(theta, 1.0, 1.5).unwrap();
        let incident = Vec3::new(theta.sin(), 0.0, -theta.cos());
        let by_vector = interface(incident, Vec3::Z, 1.0, 1.5, Polarisation::S)
            .unwrap()
            .refracted
            .unwrap();
        // The transmitted vector's angle from the normal is what the angle form
        // computes; its transverse part is that angle's sine.
        let from_vector = Vec3::new(by_vector.x, by_vector.y, 0.0).length().asin();
        assert!(close(by_angle, from_vector, 1e-12), "{by_angle} vs {from_vector}");
        assert!(close(by_vector.z.abs(), by_angle.cos(), 1e-12));
    }
}
