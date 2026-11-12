//! Exact-as-possible ray geometry with no trigonometry in the inner loop.
//!
//! A photon bouncing around a block makes three operations: travel to the next
//! plane, reflect, and (once) refract out into a sensor and onto the wall of the
//! can. Each is done here without calling `acos`, `sin` or `cos`, because those
//! cost more than the arithmetic around them and every one of them throws away
//! accuracy at the end of its domain — near grazing incidence, which is exactly
//! where a block's light actually escapes.

use std::ops::{Add, Mul, Neg, Sub};

/// A point or direction in millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3 {
    /// x, across the block.
    pub x: f64,
    /// y, along the block.
    pub y: f64,
    /// z, up the can's axis.
    pub z: f64,
}

impl Vec3 {
    /// The origin.
    pub const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };
    /// `+z`: up the can.
    pub const Z: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 1.0 };

    /// A vector.
    pub const fn new(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3 { x, y, z }
    }

    /// `a + b`.
    pub fn add(self, other: Vec3) -> Vec3 {
        Vec3::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }

    /// `a − b`.
    pub fn sub(self, other: Vec3) -> Vec3 {
        Vec3::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }

    /// `s·a`.
    pub fn scale(self, s: f64) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }

    /// The dot product.
    pub fn dot(self, other: Vec3) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// The cross product.
    pub fn cross(self, other: Vec3) -> Vec3 {
        Vec3::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    /// The Euclidean length.
    ///
    /// `hypot` twice rather than `sqrt(x² + y² + z²)`: the naive form overflows
    /// for coordinates near `1e154` and underflows below `1e-154`, and while
    /// neither occurs in a millimetre-scale detector, the scaled form costs
    /// nothing here and is one fewer thing to reason about.
    pub fn length(self) -> f64 {
        self.x.hypot(self.y).hypot(self.z)
    }

    /// The unit vector in this direction, or `None` for the zero vector.
    ///
    /// `None` rather than `NaN`: a ray with no direction is a bug in the caller,
    /// and the type makes it handle it.
    pub fn normalized(self) -> Option<Vec3> {
        let length = self.length();
        if !(length.is_finite() && length > 0.0) {
            return None;
        }
        // Scaling by the reciprocal is *not* used: it costs one division instead
        // of three and loses a half-ulp per component for the privilege.
        Some(Vec3::new(self.x / length, self.y / length, self.z / length))
    }

    /// This vector reflected in a *unit* normal: `v − 2(v·n)n`.
    ///
    /// The incident direction must point *into* the surface (against the
    /// normal); the result points away. For a unit normal and a unit `v` the
    /// result is unit up to rounding, and re-normalising it each bounce is what
    /// keeps a thousand reflections from drifting.
    pub fn reflect(self, unit_normal: Vec3) -> Vec3 {
        self.sub(unit_normal.scale(2.0 * self.dot(unit_normal)))
    }

    /// The component of `self` along `axis`.
    pub fn along(self, axis: Vec3) -> f64 {
        self.dot(axis)
    }

    /// `self` with every component made of `f64`s that are not `NaN` or infinite.
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }

    /// The angle from `+z` in the `x`–`z` half-plane, without `acos` where the
    /// caller only needs the *cosine*. Kept here so the convention (z up, angle
    /// from the normal) is one place.
    pub fn cos_with(self, other: Vec3) -> f64 {
        let lengths = self.length() * other.length();
        if lengths == 0.0 { 0.0 } else { (self.dot(other) / lengths).clamp(-1.0, 1.0) }
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, other: Vec3) -> Vec3 {
        self.add(other)
    }
}

impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, other: Vec3) -> Vec3 {
        Vec3::sub(self, other)
    }
}

impl Mul<f64> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: f64) -> Vec3 {
        self.scale(s)
    }
}

impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

/// A photon's state: where it is and which way it is going.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray {
    /// The current point.
    pub origin: Vec3,
    /// The current direction; normalised on construction.
    pub direction: Vec3,
}

impl Ray {
    /// A ray, or `None` if the direction has no length or is not finite.
    pub fn new(origin: Vec3, direction: Vec3) -> Option<Ray> {
        if !origin.is_finite() {
            return None;
        }
        direction.normalized().map(|direction| Ray { origin, direction })
    }

    /// The point `distance` along the ray.
    pub fn at(self, distance: f64) -> Vec3 {
        self.origin + self.direction * distance
    }

    /// Where this ray meets the infinite plane through `point` with `normal`.
    ///
    /// `None` for a ray parallel to the plane, for an intersection behind the
    /// origin, and for a non-finite result. The caller that needs "including
    /// behind" (a photon leaving a block through a plane it is on the other side
    /// of) uses [`Ray::plane_distance_signed`] instead.
    pub fn plane_distance(self, point: Vec3, normal: Vec3) -> Option<f64> {
        let denominator = self.direction.dot(normal);
        if denominator.abs() < 1e-15 {
            return None;
        }
        let distance = (point - self.origin).dot(normal) / denominator;
        // A tiny negative distance is the ray's own starting plane, not an
        // intersection: a photon pushed 1e-9 mm off a wall must not hit that wall
        // again forever.
        if distance < -1e-9 {
            return None;
        }
        let distance = distance.max(0.0);
        distance.is_finite().then_some(distance)
    }

    /// The signed distance to a plane, negative when the plane is behind.
    pub fn plane_distance_signed(self, point: Vec3, normal: Vec3) -> Option<f64> {
        let denominator = self.direction.dot(normal);
        if denominator.abs() < 1e-15 {
            return None;
        }
        Some((point - self.origin).dot(normal) / denominator)
    }
}

/// A flat reflecting surface, infinite for the purpose of intersection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mirror {
    /// Any point on the plane.
    pub point: Vec3,
    /// The unit normal.
    pub normal: Vec3,
    /// The fraction of photons that survive a bounce (`0.0..=1.0`).
    ///
    /// Kept with the mirror rather than applied by the caller because a surface's
    /// reflectance is a property of *the surface*, and the simulation's
    /// random stream must spend exactly one draw per bounce to stay
    /// reproducible.
    pub reflectance: f64,
}

impl Mirror {
    /// A mirror; `None` if the normal has no direction.
    pub fn new(point: Vec3, normal: Vec3, reflectance: f64) -> Option<Mirror> {
        let normal = normal.normalized()?;
        if !point.is_finite() || !(0.0..=1.0).contains(&reflectance) {
            return None;
        }
        Some(Mirror { point, normal, reflectance })
    }

    /// Reflect a ray off this surface.
    ///
    /// The returned direction points away from the surface; the origin is left
    /// where it was, so the caller advances to the hit point and nudges it along
    /// the new direction by `epsilon` before the next intersection test.
    pub fn reflect(&self, ray: Ray) -> Option<Ray> {
        let direction = ray.direction.reflect(self.normal).normalized()?;
        Ray::new(ray.origin, direction)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-12
    }

    #[test]
    fn a_ray_straight_up_meets_a_horizontal_plane_at_its_height() {
        let ray = Ray::new(Vec3::new(1.0, 2.0, 0.0), Vec3::Z).unwrap();
        let distance = ray.plane_distance(Vec3::new(0.0, 0.0, 5.0), Vec3::Z).unwrap();
        assert!((distance - 5.0).abs() < 1e-12);
        assert!(close(ray.at(distance), Vec3::new(1.0, 2.0, 5.0)));
    }

    #[test]
    fn a_plane_behind_the_ray_is_not_an_intersection() {
        let ray = Ray::new(Vec3::new(0.0, 0.0, 0.0), Vec3::Z).unwrap();
        assert!(ray.plane_distance(Vec3::new(0.0, 0.0, -5.0), Vec3::Z).is_none());
    }

    #[test]
    fn a_ray_parallel_to_a_plane_misses_it() {
        let ray = Ray::new(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0)).unwrap();
        assert!(ray.plane_distance(Vec3::new(0.0, 0.0, 5.0), Vec3::Z).is_none());
    }

    #[test]
    fn a_starting_plane_is_not_hit_again() {
        // The point of the 1e-9 tolerance: a photon nudged off a wall must not
        // instantly hit the same wall with a distance of -1e-16.
        let ray = Ray::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, -1.0)).unwrap();
        let distance = ray.plane_distance(Vec3::new(0.0, 0.0, 1e-12), Vec3::Z).unwrap();
        assert!(distance.abs() < 1e-9, "{distance}");
    }

    #[test]
    fn reflection_bends_the_component_into_the_mirror_and_nothing_else() {
        // 45 degrees onto a horizontal mirror: up and sideways becomes down and
        // the same sideways.
        let normal = Vec3::Z;
        let incoming = Vec3::new(1.0, 0.0, -1.0).normalized().unwrap();
        let out = incoming.reflect(normal);
        assert!((out.x - 0.7071067811865476).abs() < 1e-15, "{out:?}");
        assert!(out.z.abs() > 0.707 && out.z > 0.0, "{out:?}");
    }

    #[test]
    fn a_mirror_reflects_and_keeps_the_direction_a_unit_vector() {
        let mirror = Mirror::new(Vec3::ZERO, Vec3::Z, 0.98).unwrap();
        let ray = Ray::new(Vec3::new(0.0, 0.0, 1.0), Vec3::new(3.0, 0.0, -4.0)).unwrap();
        let bounced = mirror.reflect(ray).unwrap();
        assert!((bounced.direction.length() - 1.0).abs() < 1e-15);
        assert!(bounced.direction.z > 0.0);
    }

    #[test]
    fn an_oblique_mirror_normal_is_normalised_on_construction() {
        let mirror = Mirror::new(Vec3::ZERO, Vec3::new(0.0, 3.0, 4.0), 0.5).unwrap();
        assert!((mirror.normal.length() - 1.0).abs() < 1e-15);
        assert!((mirror.normal.y - 0.6).abs() < 1e-15);
    }

    #[test]
    fn a_mirror_reflects_correctly_when_its_normal_is_not_axis_aligned() {
        // A 45-degree mirror in the x-z plane turns +z into +x.
        let normal = Vec3::new(-1.0, 0.0, 1.0).normalized().unwrap();
        let mirror = Mirror::new(Vec3::ZERO, normal, 1.0).unwrap();
        let ray = Ray::new(Vec3::ZERO, Vec3::Z).unwrap();
        let out = mirror.reflect(ray).unwrap().direction;
        assert!((out.x - 1.0).abs() < 1e-15, "{out:?}");
        assert!(out.z.abs() < 1e-15, "{out:?}");
    }

    #[test]
    fn a_reflectance_outside_zero_to_one_is_refused() {
        assert!(Mirror::new(Vec3::ZERO, Vec3::Z, 1.5).is_none());
        assert!(Mirror::new(Vec3::ZERO, Vec3::Z, -0.1).is_none());
        assert!(Mirror::new(Vec3::ZERO, Vec3::ZERO, 0.5).is_none());
    }

    #[test]
    fn a_zero_direction_ray_does_not_exist() {
        assert!(Ray::new(Vec3::ZERO, Vec3::ZERO).is_none());
        assert!(Ray::new(Vec3::new(f64::NAN, 0.0, 0.0), Vec3::Z).is_none());
    }

    #[test]
    fn the_length_does_not_overflow_or_underflow() {
        assert!(Vec3::new(1e200, 1e200, 0.0).length().is_finite());
        assert!(Vec3::new(1e-200, 1e-200, 0.0).length() > 0.0);
    }

    #[test]
    fn the_cosine_is_clamped_so_an_asin_downstream_cannot_go_imaginary() {
        let a = Vec3::new(1.0, 0.0, 0.0);
        assert!((a.cos_with(a) - 1.0).abs() < 1e-15);
        let plus_a_tiny = Vec3::new(1.0 + 1e-15, 0.0, 0.0);
        assert!(plus_a_tiny.cos_with(a) <= 1.0);
    }
}
