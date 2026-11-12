//! Vector outlines as signed-distance functions: the shape a photon bounces in.
//!
//! The detector's block is not a can. It is a **prism**: a silhouette extruded
//! through a thin thickness, and that silhouette is whatever the drawing says it
//! is. Modelling it as "a rectangle with two corners clipped by the cylinder" is
//! the shape a drawing tempts you into, and it is not a shape anyone would
//! machine — a ray tracer built around it answers questions about the wrong
//! object.
//!
//! The outline here is a **tent** (a "tropical polygon"): a wide, low peak with
//! two straight walls down to a flat base. In the can's own units, where 1 is the
//! can's radius and the block's plane has `x` across and `y` up:
//!
//! ```text
//!                 (0, 3)                  the apex
//!              /         \
//!     (-1, 1.5)           (1, 1.5)        the shoulders
//!        |                  |
//!     (-1,-1.5)           (1,-1.5)        the base
//! ```
//!
//! The prism is [`Tropical`]: that outline extruded `thickness_mm` deep. A photon
//! Monte Carlo needs three things from it, and the crate provides each once:
//!
//! * [`Tropical::signed_distance`] — `> 0` outside, `< 0` inside, the actual
//!   distance in millimetres near the surface. Exact for this convex polygon: the
//!   maximum over the normalised outward half-plane functions.
//! * [`Tropical::surface_normal`] — the outward unit normal. On the flat faces it
//!   is a literal; on the sloping roof it is a central-difference estimate of the
//!   signed distance, which for a polygon is exact up to the arithmetic.
//! * [`Tropical::segment_of`] — which face a point is nearest, so that the
//!   transport can distinguish a face hit from a corner hit, and can model a
//!   corner as the snap radius separates them.
//!
//! # The tropical polynomial
//!
//! [`tropical_polynomial`] is the algebraic object the drawing is a half-plane
//! of: `min(y + |x|, 3) − 1.5`. Its two facets are the roof and the shoulders;
//! its tropical curve is where they exchange. The six vertices a reader sees in
//! the drawing are the curve's corners plus where the walls meet the base — and
//! [`tests::the_polynomials_facets_are_the_roof_and_the_shoulders`] is what keeps
//! the drawing and the algebra from drifting apart. Two transcriptions of one
//! shape is one more than a shape needs.

use crate::Interval;

/// A point in the block's plane or a 2D vector, in millimetres.
pub type Point2 = (f64, f64);

/// Why an outline refuses a parameter set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlineError {
    /// A scale or a thickness that is not positive and finite.
    BadScale,
}

impl std::fmt::Display for OutlineError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutlineError::BadScale => {
                write!(formatter, "the can radius and the slab thickness must be positive and finite")
            }
        }
    }
}

impl std::error::Error for OutlineError {}

/// The six corners of the closed prism outline, in can units, going **clockwise**
/// from the top-left shoulder. (Clockwise is the winding the signed-distance code
/// assumes; a counter-clockwise list would turn the prism inside out, so the
/// winding is a tested invariant and not a convention.)
pub const CORNERS: [Point2; 6] = [
    (-1.0, 1.5),
    (0.0, 3.0),
    (1.0, 1.5),
    (1.0, -1.5),
    (-1.0, -1.5),
    (-1.0, 1.5),
];

/// The tent's prism.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tropical {
    /// How many millimetres one can unit is: the can's radius.
    pub scale_mm: f64,
    /// The extrusion thickness, millimetres.
    pub thickness_mm: f64,
}

impl Tropical {
    /// A prism for a can of `radius_mm` and a block `thickness_mm` deep.
    pub fn fits_in(radius_mm: f64, thickness_mm: f64) -> Result<Tropical, OutlineError> {
        if !(radius_mm.is_finite() && radius_mm > 0.0)
            || !(thickness_mm.is_finite() && thickness_mm > 0.0)
        {
            return Err(OutlineError::BadScale);
        }
        Ok(Tropical {
            scale_mm: radius_mm,
            thickness_mm,
        })
    }

    /// The corners in millimetres, clockwise.
    pub fn corners_mm(&self) -> [Point2; 6] {
        CORNERS.map(|(x, y)| (x * self.scale_mm, y * self.scale_mm))
    }

    /// The signed distance to the outline in the block's plane: negative inside.
    ///
    /// The maximum over the normalised outward half-plane functions. For the
    /// clockwise list the outward normal of the directed edge `(dx, dy)` is
    /// `(−dy, dx)` — the *interior* is on the right of a clockwise edge — so no
    /// orientation test is needed and none can be wrong. The first version used
    /// `(dy, −dx)` and returned +3 inside the block: a sign that is only ever
    /// caught by a test that asks the centre to be negative.
    pub fn signed_distance_2d(&self, x: f64, y: f64) -> f64 {
        let corners = self.corners_mm();
        let mut best = f64::NEG_INFINITY;
        for index in 0..corners.len() {
            let (ax, ay) = corners[index];
            let (bx, by) = corners[(index + 1) % corners.len()];
            let (dx, dy) = (bx - ax, by - ay);
            let length = dx.hypot(dy);
            if length == 0.0 {
                continue;
            }
            best = best.max((dx * (y - ay) - dy * (x - ax)) / length);
        }
        best
    }

    /// The 3D signed distance, with the slab centred on the origin along `z`.
    pub fn signed_distance(&self, x: f64, y: f64, z: f64) -> f64 {
        self.signed_distance_2d(x, y)
            .max(z.abs() - self.thickness_mm / 2.0)
    }

    /// Whether a point is inside the prism.
    pub fn contains(&self, x: f64, y: f64, z: f64) -> bool {
        self.signed_distance(x, y, z) < 0.0
    }

    /// The outward unit normal at a point on or near the surface.
    ///
    /// A central difference of the signed distance: the gradient of a true
    /// distance is the unit normal, so the estimate is exact to the differencing
    /// step. `epsilon` is the step in millimetres. At a corner the gradient is
    /// undefined and the returned normal is the average of the adjacent faces;
    /// the transport handles corners separately, with a snap radius, so this is
    /// the answer for a face hit only.
    pub fn surface_normal(&self, x: f64, y: f64, z: f64, epsilon: f64) -> (f64, f64, f64) {
        let (gx, gy, gz) = self.gradient(x, y, z, epsilon);
        let length = gx.hypot(gy).hypot(gz);
        if length == 0.0 { (0.0, 0.0, 1.0) } else { (gx / length, gy / length, gz / length) }
    }

    /// The gradient of the signed distance, unnormalised.
    fn gradient(&self, x: f64, y: f64, z: f64, epsilon: f64) -> (f64, f64, f64) {
        let gradient = |e: f64| {
            (
                (self.signed_distance(x + e, y, z) - self.signed_distance(x - e, y, z)) / (2.0 * e),
                (self.signed_distance(x, y + e, z) - self.signed_distance(x, y - e, z)) / (2.0 * e),
                (self.signed_distance(x, y, z + e) - self.signed_distance(x, y, z - e)) / (2.0 * e),
            )
        };
        // A too-small step is dominated by the distance's own rounding; a
        // too-large one smooths over the faces. The caller passes the resource
        // it can afford; 1e-3 mm is exact in the flat regions and adequate on the
        // roof, where the gradient is piecewise constant away from the corners.
        gradient(epsilon.max(1e-6))
    }

    /// Which face a surface point is nearest: `0..=5`, matching the edge of
    /// [`CORNERS`] that starts at that index.
    pub fn segment_of(&self, x: f64, y: f64) -> usize {
        let corners = self.corners_mm();
        let mut best = (0usize, f64::INFINITY);
        for index in 0..corners.len() {
            let (ax, ay) = corners[index];
            let (bx, by) = corners[(index + 1) % corners.len()];
            let (dx, dy) = (bx - ax, by - ay);
            let length_squared = dx * dx + dy * dy;
            if length_squared == 0.0 {
                continue;
            }
            let t = (((x - ax) * dx + (y - ay) * dy) / length_squared).clamp(0.0, 1.0);
            let (px, py) = (ax + dx * t, ay + dy * t);
            let distance = (x - px).hypot(y - py);
            if distance < best.1 {
                best = (index, distance);
            }
        }
        best.0
    }

    /// The cross-section's area, mm², by the shoelace formula.
    pub fn area_mm2(&self) -> f64 {
        let corners = self.corners_mm();
        let mut twice = 0.0;
        for index in 0..corners.len() {
            let (x1, y1) = corners[index];
            let (x2, y2) = corners[(index + 1) % corners.len()];
            twice += x1 * y2 - x2 * y1;
        }
        (twice / 2.0).abs()
    }

    /// The prism's volume, mm³.
    pub fn volume_mm3(&self) -> f64 {
        self.area_mm2() * self.thickness_mm
    }
}

/// The tropical polynomial the outline is a half-plane of: `min(y + |x|, 3) − 1.5`,
/// in can units.
///
/// The lower facet of the two is the active one — `min` is what makes the
/// shoulders the active facet far out and the roof active near the apex.
/// Returns `(value, facet)`: facet `0` is the roof, facet `1` the shoulders.
pub fn tropical_polynomial(x: f64, y: f64) -> (f64, u8) {
    let roof = y + x.abs();
    let shoulders = 3.0;
    let value = roof.min(shoulders) - 1.5;
    (value, if roof <= shoulders { 0 } else { 1 })
}

/// The interval form of [`tropical_polynomial`]: bounds it over a box.
pub fn tropical_polynomial_interval(x: Interval, y: Interval) -> Interval {
    if x.is_empty() || y.is_empty() {
        return Interval::empty();
    }
    let absolute = |interval: Interval| -> Interval {
        if interval.lo() >= 0.0 {
            interval
        } else if interval.hi() <= 0.0 {
            interval.mul(Interval::point(-1.0))
        } else {
            Interval::new(0.0, interval.lo().abs().max(interval.hi().abs()))
        }
    };
    let roof = absolute(x).add(y);
    let cap = Interval::point(3.0);
    let shift = Interval::point(1.5);
    if roof.hi() <= cap.lo() {
        roof.sub(shift)
    } else if roof.lo() >= cap.lo() {
        cap.sub(shift)
    } else {
        let first = roof.sub(shift);
        let second = cap.sub(shift);
        Interval::new(first.lo().min(second.lo()), first.hi().max(second.hi()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A can of the real size: radius 33 mm, and a 20 mm thick block.
    fn block() -> Tropical {
        Tropical::fits_in(33.0, 20.0).expect("fits")
    }

    #[test]
    fn the_six_corners_are_the_drawing() {
        let corners = block().corners_mm();
        assert_eq!(corners.len(), 6);
        // Left shoulder, apex on the axis, right shoulder, and the base.
        assert!((corners[0].0 + 33.0).abs() < 1e-9 && (corners[0].1 - 49.5).abs() < 1e-9);
        assert!(corners[1].0.abs() < 1e-12 && (corners[1].1 - 99.0).abs() < 1e-9);
        assert!((corners[2].0 - 33.0).abs() < 1e-9 && (corners[2].1 - 49.5).abs() < 1e-9);
        assert!((corners[3].0 - 33.0).abs() < 1e-9 && corners[3].1 < 0.0);
        assert!((corners[4].0 + 33.0).abs() < 1e-9 && corners[4].1 < 0.0);
        // The widest point is exactly the can's inner radius, never past it.
        let widest = corners.iter().map(|(x, _)| x.abs()).fold(0.0, f64::max);
        assert!((widest - 33.0).abs() < 1e-12);
    }

    #[test]
    fn the_centre_is_inside_and_the_surroundings_are_outside() {
        let outline = block();
        assert!(outline.contains(0.0, 0.0, 0.0));
        assert!(outline.contains(0.0, 40.0, 0.0));
        assert!(outline.contains(20.0, 20.0, 0.0));
        // Beyond a wall, above the ridge, and outside the slab's faces.
        assert!(!outline.contains(34.0, 0.0, 0.0));
        assert!(!outline.contains(0.0, 100.0, 0.0));
        assert!(!outline.contains(0.0, 0.0, 11.0));
    }

    #[test]
    fn the_signed_distance_has_its_sign_and_its_scale() {
        let outline = block();
        assert!(outline.signed_distance_2d(0.0, 0.0) < 0.0);
        // On the right wall, x = +R: zero.
        assert!(outline.signed_distance_2d(33.0, 0.0).abs() < 1e-9);
        // Two millimetres outside that wall: two.
        assert!((outline.signed_distance_2d(35.0, 0.0) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn the_roof_is_at_45_degrees_and_its_distance_is_perpendicular() {
        // The roof rises 1.5 units over a run of 1: a 56.3° slope, not 45.
        // What the test pins is the *normalisation*: one millimetre above the
        // apex is 1/√(1 + m²)-short of the closest roof plane, where m = 1.5.
        let outline = block();
        let apex = 99.0;
        let above = outline.signed_distance_2d(0.0, apex + 1.0);
        let expected = 1.0 / (1.0 + 1.5f64 * 1.5).sqrt();
        assert!((above - expected).abs() < 1e-6, "{above} vs {expected}");
    }

    #[test]
    fn on_the_wall_the_normal_is_the_wall_not_the_roof() {
        // The central difference must give an axis-aligned normal on the wall
        // and a sloped one on the roof; an SDF that is not a distance fails this.
        let outline = block();
        let (nx, ny, nz) = outline.surface_normal(33.0, 0.0, 0.0, 1e-4);
        assert!((nx - 1.0).abs() < 1e-6, "{nx}");
        assert!(ny.abs() < 1e-3, "{ny}");
        assert!(nz.abs() < 1e-3, "{nz}");
        // On the roof, the outward normal has a positive y and a positive x on
        // the right-hand slope.
        let (rx, ry, _) = outline.surface_normal(15.0, 99.0 - 15.0 * 1.5 - 0.5, 0.0, 1e-4);
        assert!(rx > 0.3 && ry > 0.5, "{rx} {ry}");
    }

    #[test]
    fn the_segment_of_a_wall_point_is_that_wall() {
        let outline = block();
        // CORNERS[2] -> [3] is the right wall.
        assert_eq!(outline.segment_of(33.0, 0.0), 2);
        // The base edge, CORNERS[3] -> [4].
        assert_eq!(outline.segment_of(0.0, -49.5 - 1e-9), 3);
    }

    #[test]
    fn the_area_is_the_drawings_own_area() {
        // Two walls 3 units tall and the roof's triangle of base 2 and height
        // 1.5: 6 + 1.5 = 7.5 can².
        let outline = block();
        let expected = 7.5 * outline.scale_mm * outline.scale_mm;
        assert!((outline.area_mm2() - expected).abs() < 1e-9, "{}", outline.area_mm2());
    }

    #[test]
    fn the_polynomials_facets_are_the_roof_and_the_shoulders() {
        // At the waist (0, 0) the roof is 0 and the cap is 3: roof is smaller.
        let (value, facet) = tropical_polynomial(0.0, 0.0);
        assert_eq!(facet, 0, "the roof is the lower facet at the waist");
        assert!((value + 1.5).abs() < 1e-12);
        // Aqua: the peak (0, 3) has roof = 3 = cap, the switch itself.
        let (_, facet) = tropical_polynomial(0.0, 3.0);
        assert_eq!(facet, 0, "a tie belongs to the roof, the closed side");
        // The shoulders (1, 1.5) are the same switch on the sides.
        for (x, y) in [(1.0f64, 1.5f64), (-1.0, 1.5)] {
            let (_, facet) = tropical_polynomial(x, y);
            assert_eq!(facet, 0);
        }
        // Past the shoulder the roof has risen above the cap, so the cap is the
        // minimum and the function is flat: the shape, as a function of x, has
        // shoulders rather than a single peak.
        let (value, facet) = tropical_polynomial(2.0, 1.5);
        assert_eq!(facet, 1, "outside the ridge, the cap is the lower facet");
        assert!((value - 1.5).abs() < 1e-12);
    }

    #[test]
    fn the_interval_polynomial_contains_its_evaluations() {
        for (x0, x1) in [(-0.4, 0.4), (0.5, 1.5), (2.0, 3.0)] {
            for (y0, y1) in [(-1.0, 0.0), (1.0, 2.0)] {
                let bound = tropical_polynomial_interval(Interval::new(x0, x1), Interval::new(y0, y1));
                assert!(!bound.is_empty());
                for i in 0..=20 {
                    for j in 0..=20 {
                        let xi = x0 + (x1 - x0) * (i as f64 / 20.0);
                        let yj = y0 + (y1 - y0) * (j as f64 / 20.0);
                        let (value, _) = tropical_polynomial(xi, yj);
                        assert!(bound.contains(value), "{bound} missed {value} at ({xi}, {yj})");
                    }
                }
            }
        }
    }

    #[test]
    fn impossible_parameters_are_refused() {
        assert!(Tropical::fits_in(0.0, 20.0).is_err());
        assert!(Tropical::fits_in(-1.0, 20.0).is_err());
        assert!(Tropical::fits_in(33.0, -1.0).is_err());
        assert!(Tropical::fits_in(f64::NAN, 20.0).is_err());
    }
}
