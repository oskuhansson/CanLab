//! The envelope: what actually fits inside a 115 mm × Ø66 mm can.
//!
//! The geometry sweep in `canlab_optics::bin::muon_study` varies the tent's
//! scale from 15 to 30 mm and reports that the largest scale gives the most
//! light. It never asks whether the block fits in the can, and the largest one
//! does not: the tent is `4.5·scale` tall, so a 30 mm scale is 135 mm of block in
//! a 115 mm can. The best row of that table is a detector that cannot be built.
//!
//! This module is the constraint that table is missing, and it is a *refusal*
//! rather than a filter: a caller that asks for a block that does not fit is told
//! which dimension is over and by how much, not silently given a smaller block.
//!
//! # The two ways a block can fail to fit
//!
//! The block is a prism: the tent's silhouette in one plane, extruded
//! `thickness_mm` deep. It is mounted with the tent's plane containing the can's
//! axis, so the can constrains it in two independent ways:
//!
//! * **Height.** The tent is `4.5·scale` tall, and the can is 115 mm. The block
//!   may be shifted along the axis (the tent is not symmetric about its own
//!   centre — the apex is `3·scale` above the waist and the base `1.5·scale`
//!   below), so only the total height is constrained, not the offset.
//! * **Width.** The block's cross-section perpendicular to the axis is a
//!   rectangle `2·scale` wide and `thickness_mm` deep. It must fit inside the
//!   can's circle, so its half-diagonal `√(scale² + (thickness/2)²)` must not
//!   exceed the inner radius.
//!
//! Both numbers come from [`canlab_optics::Tropical`]'s own corner list rather
//! than from constants written here: two transcriptions of one shape is one more
//! than a shape needs, and the second one is the one that goes stale.

use canlab_optics::Tropical;

/// A can's inner envelope, millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Can {
    /// The can's outer radius. The competition's can is Ø66 mm, so 33.
    pub radius_mm: f64,
    /// The can's outer height. The competition's can is 115 mm.
    pub height_mm: f64,
    /// Wall and tolerance allowance, subtracted from both dimensions.
    pub wall_mm: f64,
}

/// Why a block does not fit in a can.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CanError {
    /// The tent is taller than the can's inner height.
    TooTall {
        /// The block's height, millimetres.
        height_mm: f64,
        /// The can's inner height, millimetres.
        limit_mm: f64,
    },
    /// The block's cross-section does not fit in the can's circle.
    TooWide {
        /// The cross-section's half-diagonal, millimetres.
        half_diagonal_mm: f64,
        /// The can's inner radius, millimetres.
        limit_mm: f64,
    },
    /// A scale or thickness that is not positive and finite.
    BadSize,
}

impl std::fmt::Display for CanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CanError::TooTall { height_mm, limit_mm } => write!(
                formatter,
                "the block is {height_mm:.1} mm tall and the can has {limit_mm:.1} mm of inner height"
            ),
            CanError::TooWide { half_diagonal_mm, limit_mm } => write!(
                formatter,
                "the block's cross-section reaches {half_diagonal_mm:.1} mm from the axis and the can's inner radius is {limit_mm:.1} mm"
            ),
            CanError::BadSize => {
                write!(formatter, "the scale and the thickness must be positive and finite")
            }
        }
    }
}

impl std::error::Error for CanError {}

impl Can {
    /// The competition's can: 115 mm tall, Ø66 mm, with five millimetres of wall
    /// and tolerance taken off each dimension.
    ///
    /// Five millimetres is a guess at the tolerance and it is a *parameter*, not
    /// a constant hidden here: the real can has a rolled rim, a lid, and a
    /// mounting frame, and a team that measures its own can should pass its own
    /// number. The first version of this study used one millimetre, which is
    /// too small for a rolled rim and a lid: the detector's radius would be
    /// 32 mm against a 33 mm wall, and the rim alone is more than that.
    pub fn standard() -> Can {
        Can { radius_mm: 33.0, height_mm: 115.0, wall_mm: 5.0 }
    }

    /// The radius a block may actually use.
    pub fn inner_radius_mm(&self) -> f64 {
        self.radius_mm - self.wall_mm
    }

    /// The height a block may actually use.
    pub fn inner_height_mm(&self) -> f64 {
        self.height_mm - self.wall_mm
    }

    /// The area of the can's cross-section, mm² — the largest face a detector
    /// mounted across the axis could present.
    ///
    /// Ø66 mm is 3421 mm², or 34.2 cm²; with five millimetres of allowance it is
    /// 2463 mm², or 24.6 cm². The mission documents quote "~30 cm²" for a
    /// scintillator plate, which is this number with the mounting frame taken
    /// off.
    pub fn cross_section_mm2(&self) -> f64 {
        std::f64::consts::PI * self.inner_radius_mm() * self.inner_radius_mm()
    }

    /// Whether a tent of `scale_mm` and `thickness_mm` fits.
    pub fn admits(&self, scale_mm: f64, thickness_mm: f64) -> Result<(), CanError> {
        let Some((width_mm, height_mm)) = tent_extent_mm(scale_mm) else {
            return Err(CanError::BadSize);
        };
        if !(thickness_mm.is_finite() && thickness_mm > 0.0) {
            return Err(CanError::BadSize);
        }
        if height_mm > self.inner_height_mm() {
            return Err(CanError::TooTall { height_mm, limit_mm: self.inner_height_mm() });
        }
        let half_diagonal_mm = (width_mm * width_mm + thickness_mm * thickness_mm).sqrt() / 2.0;
        if half_diagonal_mm > self.inner_radius_mm() {
            return Err(CanError::TooWide {
                half_diagonal_mm,
                limit_mm: self.inner_radius_mm(),
            });
        }
        Ok(())
    }

    /// The largest tent scale that fits at a given thickness, millimetres.
    ///
    /// The height and the width bind at different scales, and the answer is the
    /// smaller of the two. `None` for a thickness that leaves no room at all.
    pub fn largest_scale_mm(&self, thickness_mm: f64) -> Option<f64> {
        let (width_per_unit, height_per_unit) = tent_extent_mm(1.0)?;
        if !(thickness_mm.is_finite() && thickness_mm > 0.0) {
            return None;
        }
        let by_height = self.inner_height_mm() / height_per_unit;
        // √(s²·w² + t²)/2 ≤ r  ⇒  s ≤ √(4r² − t²)/w
        let room = 4.0 * self.inner_radius_mm() * self.inner_radius_mm()
            - thickness_mm * thickness_mm;
        if room <= 0.0 {
            return None;
        }
        let by_width = room.sqrt() / width_per_unit;
        let scale = by_height.min(by_width);
        (scale > 0.0 && scale.is_finite()).then_some(scale)
    }
}

/// The tent's bounding box in its own plane, millimetres: `(width, height)`.
///
/// Taken from [`Tropical`]'s corner list, so the block's silhouette and this
/// envelope cannot disagree about how tall the tent is. `None` for a scale the
/// outline refuses.
pub fn tent_extent_mm(scale_mm: f64) -> Option<(f64, f64)> {
    let outline = Tropical::fits_in(scale_mm, 1.0).ok()?;
    let corners = outline.corners_mm();
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (x, y) in corners {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    Some((max_x - min_x, max_y - min_y))
}

/// The prism's volume, mm³, from the outline's own area.
pub fn tent_volume_mm3(scale_mm: f64, thickness_mm: f64) -> Option<f64> {
    let outline = Tropical::fits_in(scale_mm, thickness_mm).ok()?;
    Some(outline.volume_mm3())
}

/// The prism's mass in grams, from its volume and the material's density.
///
/// The can's whole budget is 300–350 g, so this is a number the design has to
/// carry. `density_g_per_cm3` is the caller's: 1.19 for PMMA, about 1.30 for a
/// high-index plastic.
pub fn tent_mass_g(scale_mm: f64, thickness_mm: f64, density_g_per_cm3: f64) -> Option<f64> {
    let volume_mm3 = tent_volume_mm3(scale_mm, thickness_mm)?;
    (density_g_per_cm3 > 0.0).then(|| volume_mm3 / 1000.0 * density_g_per_cm3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tent_is_two_by_four_and_a_half_units() {
        // The corner list is (-1, 1.5), (0, 3), (1, 1.5), (1, -1.5), (-1, -1.5):
        // two units wide and 4.5 tall. Pinned against the drawing, because every
        // number below is this one multiplied.
        let (width, height) = tent_extent_mm(1.0).expect("a unit tent");
        assert!((width - 2.0).abs() < 1e-12, "{width}");
        assert!((height - 4.5).abs() < 1e-12, "{height}");
        let (width, height) = tent_extent_mm(20.0).expect("a 20 mm tent");
        assert!((width - 40.0).abs() < 1e-9, "{width}");
        assert!((height - 90.0).abs() < 1e-9, "{height}");
    }

    #[test]
    fn the_design_block_fits_and_the_sweeps_best_one_does_not() {
        // The design point: a 20 mm tent, 10 mm deep. 40 × 90 mm of silhouette
        // in a 115 mm can, and a 40 × 10 cross-section in a Ø66 circle.
        let can = Can::standard();
        assert_eq!(can.admits(20.0, 10.0), Ok(()));
        // The sweep's best row is a 30 mm tent: 135 mm of block in a 115 mm can.
        // The study reported it as the best geometry and never asked.
        let refused = can.admits(30.0, 5.0);
        match refused {
            Err(CanError::TooTall { height_mm, limit_mm }) => {
                assert!((height_mm - 135.0).abs() < 1e-9, "{height_mm}");
                assert!((limit_mm - 110.0).abs() < 1e-9, "{limit_mm}");
            }
            other => panic!("a 30 mm tent was not refused for height: {other:?}"),
        }
        // 24 mm is 108 mm tall: it fits, with 2 mm to spare. 25 mm is 112.5 mm
        // and no longer does.
        assert_eq!(can.admits(24.0, 10.0), Ok(()));
        assert!(can.admits(25.0, 10.0).is_err(), "112.5 mm of block in a 110 mm can");
        assert!(can.admits(26.0, 10.0).is_err(), "117 mm of block in a 110 mm can");
    }

    #[test]
    fn a_thick_block_is_refused_for_width_not_height() {
        // The other side of the rule: a short tent with a thick extrusion fails
        // the circle test, and the reason names the circle. A refusal that
        // always said "too tall" would pass the test above and be wrong here.
        let can = Can::standard();
        match can.admits(10.0, 70.0) {
            Err(CanError::TooWide { half_diagonal_mm, limit_mm }) => {
                // √(10² + 35²) = 36.4 mm from the axis, against 28 mm of radius.
                assert!((half_diagonal_mm - 36.4).abs() < 0.05, "{half_diagonal_mm}");
                assert!((limit_mm - 28.0).abs() < 1e-9, "{limit_mm}");
            }
            other => panic!("a 70 mm thick block was not refused for width: {other:?}"),
        }
        // And the same tent at a thickness that fits is admitted, so the test is
        // about the thickness and not about the scale.
        assert_eq!(can.admits(10.0, 20.0), Ok(()));
    }

    #[test]
    fn impossible_sizes_are_refused_rather_than_answered() {
        let can = Can::standard();
        for (scale, thickness) in [(0.0, 10.0), (-5.0, 10.0), (20.0, 0.0), (20.0, -1.0), (f64::NAN, 10.0)] {
            assert_eq!(can.admits(scale, thickness), Err(CanError::BadSize), "{scale} {thickness}");
        }
    }

    #[test]
    fn the_largest_scale_is_the_smaller_of_the_two_bounds() {
        let can = Can::standard();
        // At 10 mm deep the height binds: 110/4.5 = 24.44 mm.
        let largest = can.largest_scale_mm(10.0).expect("some room");
        assert!((largest - 110.0 / 4.5).abs() < 1e-9, "{largest}");
        assert_eq!(can.admits(largest, 10.0), Ok(()));
        assert!(can.admits(largest + 0.1, 10.0).is_err());
        // At 40 mm deep the circle binds instead, and it is a different number.
        // (60 mm no longer fits the 56 mm circle at all.)
        let wide = can.largest_scale_mm(40.0).expect("some room");
        assert!(wide < largest, "{wide} vs {largest}");
        assert_eq!(can.admits(wide, 40.0), Ok(()));
        assert!(can.admits(wide + 0.1, 40.0).is_err());
        // A thickness that fills the circle leaves nothing.
        assert!(can.largest_scale_mm(56.0).is_none());
    }

    #[test]
    fn the_cross_section_is_the_can_and_not_a_rectangle() {
        // 34.2 cm² of circle, 24.6 cm² with the allowance — the number the
        // mission documents round to "~30 cm²". A 50 × 50 mm square would be
        // 25 cm² and its diagonal 70.7 mm, which does not fit the circle at all:
        // the area is not the constraint, the diagonal is.
        let can = Can::standard();
        let outer = std::f64::consts::PI * 33.0 * 33.0;
        assert!((outer / 100.0 - 34.21).abs() < 0.01, "{}", outer / 100.0);
        assert!((can.cross_section_mm2() / 100.0 - 24.63).abs() < 0.01);
        let square_diagonal = (50.0f64 * 50.0 + 50.0 * 50.0).sqrt();
        assert!(square_diagonal > 2.0 * can.inner_radius_mm(), "{square_diagonal}");
    }

    #[test]
    fn the_mass_is_the_volume_times_the_density() {
        // 7.5 · 20² · 10 = 30 000 mm³ = 30 cm³; PMMA at 1.19 g/cm³ is 35.7 g.
        let volume = tent_volume_mm3(20.0, 10.0).expect("a block");
        assert!((volume - 30_000.0).abs() < 1e-6, "{volume}");
        let mass = tent_mass_g(20.0, 10.0, 1.19).expect("a mass");
        assert!((mass - 35.7).abs() < 0.05, "{mass}");
        assert!(tent_mass_g(20.0, 10.0, 0.0).is_none());
        assert!(tent_mass_g(20.0, 10.0, -1.0).is_none());
    }
}
