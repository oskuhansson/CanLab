//! W8.1: a camera, a projection and a frustum, with no GPU and no window.
//!
//! The 3D view (W8) needs exactly three pieces of mathematics before any
//! rendering: where the camera is, how a world point lands on the screen, and
//! whether a point is in front of the camera at all. This crate is those three,
//! and it is *pure*: no `wgpu`, no `egui`, no window. The reason is the same one
//! that put the plot crate's sampler in a library of its own — a projection that
//! can only be checked by looking at a picture is a projection whose bugs are
//! pictures.
//!
//! # The camera, and why it is orbit-shaped
//!
//! A camera is an *orbit* camera: an azimuth, an elevation and a distance about
//! a target. That is what a 3D view in a CAS wants — the reader turns the
//! surface, and the target stays where it is — and it is also the form whose
//! inverse is a `fit`: given a set of points, choose the distance that puts them
//! all in view. The round trip `fit` then `project` is the test that the two
//! agree.
//!
//! # Coordinates
//!
//! Right-handed, `y` up, `z` towards the viewer. The camera looks from
//! `target + distance·direction` at `target`, where `direction` is
//! `(cos e·sin a, sin e, cos e·cos a)` — an azimuth of 0 looks from `+z`, and an
//! elevation of 0 is level with the target.
//!
//! # The projection
//!
//! A perspective projection with a vertical field of view and the screen's
//! aspect ratio. The result is normalised device coordinates in `[-1, 1]` on
//! both axes, with `w` as the depth, and [`Camera::project`] returns a screen
//! point only when the point is in front of the camera: a point behind it
//! projects to a point that is a mirror image of where it would be if it were in
//! front, and drawing that point is the classic 3D-view bug.

pub mod implicit;
pub mod surface;
pub mod svg;

use std::f64::consts::PI;

/// A point or a direction in three dimensions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3 {
    /// x, right.
    pub x: f64,
    /// y, up.
    pub y: f64,
    /// z, towards the viewer.
    pub z: f64,
}

impl Vec3 {
    /// The origin.
    pub const ZERO: Vec3 = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    /// `+y`.
    pub const UP: Vec3 = Vec3 {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    };

    /// A vector.
    pub const fn new(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3 { x, y, z }
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
    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }

    /// The unit vector, or `None` for the zero vector.
    pub fn normalized(self) -> Option<Vec3> {
        let length = self.length();
        // `!(length > 0.0)` and not `length <= 0.0`: a `NaN` length compares
        // false to everything, and this form rejects it.
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !(length > 0.0) || !length.is_finite() {
            return None;
        }
        Some(self.scale(1.0 / length))
    }
}

impl Vec3 {
    /// `a + b`.
    ///
    /// An inherent method beside the `Add` trait: the trait makes `a + b` work
    /// and the method makes `a.add(b)` work, which is what the camera's
    /// arithmetic reads better as in a chain.
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, other: Vec3) -> Vec3 {
        Vec3::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }

    /// `a − b`.
    #[allow(clippy::should_implement_trait)]
    pub fn sub(self, other: Vec3) -> Vec3 {
        Vec3::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }
}

impl std::ops::Add for Vec3 {
    type Output = Vec3;
    fn add(self, other: Vec3) -> Vec3 {
        Vec3::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl std::ops::Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, other: Vec3) -> Vec3 {
        Vec3::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }
}

impl std::ops::Mul<f64> for Vec3 {
    type Output = Vec3;
    fn mul(self, scale: f64) -> Vec3 {
        Vec3::new(self.x * scale, self.y * scale, self.z * scale)
    }
}

/// An orbit camera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// What it looks at.
    pub target: Vec3,
    /// How far away it is.
    pub distance: f64,
    /// The angle about the vertical axis, radians. 0 looks from `+z`.
    pub azimuth: f64,
    /// The angle above the horizontal, radians, in `(-π/2, π/2)`.
    pub elevation: f64,
    /// The vertical field of view, radians.
    pub field_of_view: f64,
    /// The screen's width over its height.
    pub aspect: f64,
    /// The near plane's distance.
    pub near: f64,
    /// The far plane's distance.
    pub far: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Camera {
            target: Vec3::ZERO,
            distance: 5.0,
            azimuth: 0.0,
            elevation: 0.0,
            field_of_view: PI / 4.0,
            aspect: 16.0 / 9.0,
            near: 0.1,
            far: 100.0,
        }
    }
}

impl Camera {
    /// Where the camera is.
    pub fn position(&self) -> Vec3 {
        let cos_elevation = self.elevation.cos();
        let direction = Vec3::new(
            cos_elevation * self.azimuth.sin(),
            self.elevation.sin(),
            cos_elevation * self.azimuth.cos(),
        );
        self.target.add(direction.scale(self.distance))
    }

    /// The camera's forward axis: from the camera towards the target.
    pub fn forward(&self) -> Vec3 {
        self.target.sub(self.position()).normalized().unwrap_or(Vec3::new(0.0, 0.0, -1.0))
    }

    /// The camera's right axis.
    ///
    /// `forward × up`, which is the right-hand rule with `y` up; at the poles
    /// the cross product degenerates and the fallback is `+x`, which is what a
    /// camera looking straight down sees as right.
    pub fn right(&self) -> Vec3 {
        self.forward()
            .cross(Vec3::UP)
            .normalized()
            .unwrap_or(Vec3::new(1.0, 0.0, 0.0))
    }

    /// The camera's up axis.
    pub fn up(&self) -> Vec3 {
        self.right().cross(self.forward()).normalized().unwrap_or(Vec3::UP)
    }

    /// The view matrix's three rows: `(right, up, −forward)`.
    ///
    /// A world point is transformed by subtracting the camera's position and
    /// taking its components along the three axes. The third row is `−forward`
    /// because the camera looks down `−z` in view space.
    pub fn view_axes(&self) -> (Vec3, Vec3, Vec3) {
        (self.right(), self.up(), self.forward().scale(-1.0))
    }

    /// A world point in view space: `x` right, `y` up, `z` towards the viewer.
    pub fn to_view(&self, point: Vec3) -> Vec3 {
        let (right, up, back) = self.view_axes();
        let offset = point.sub(self.position());
        Vec3::new(offset.dot(right), offset.dot(up), offset.dot(back))
    }

    /// A world point in normalised device coordinates, or `None` when it is not
    /// in front of the camera.
    ///
    /// The perspective divide is the whole of the projection: `x/w` and `y/w`
    /// with `w = −z_view`. A point at `z_view = 0` is on the camera's own plane
    /// and has no projection; a point behind it has a `w` of the wrong sign, and
    /// the division would put it on the screen mirrored, which is why this
    /// returns `None` rather than the mirrored point.
    pub fn project(&self, point: Vec3) -> Option<ScreenPoint> {
        let view = self.to_view(point);
        // The camera looks down `−z`, so a point in front has `z < 0`.
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !(view.z < -self.near) {
            return None;
        }
        let w = -view.z;
        let focal = 1.0 / (self.field_of_view * 0.5).tan();
        let x = view.x * focal / (self.aspect * w);
        let y = view.y * focal / w;
        if !(x.is_finite() && y.is_finite()) {
            return None;
        }
        Some(ScreenPoint {
            x,
            y,
            depth: w,
        })
    }

    /// A normalised device coordinate as a pixel, given the screen's size.
    pub fn to_pixels(point: ScreenPoint, width: f64, height: f64) -> (f64, f64) {
        (
            (point.x + 1.0) * 0.5 * width,
            (1.0 - point.y) * 0.5 * height,
        )
    }

    /// Whether a point is inside the view frustum: in front, and inside the
    /// field of view on both axes.
    pub fn contains(&self, point: Vec3) -> bool {
        let Some(projected) = self.project(point) else {
            return false;
        };
        projected.x.abs() <= 1.0 && projected.y.abs() <= 1.0 && projected.depth <= self.far
    }

    /// The camera that fits `points` in view.
    ///
    /// The distance is chosen so that the farthest point's *angle* from the
    /// view axis is inside the field of view, with a margin. The margin is a
    /// tenth, so a surface touches the frame's edge and does not sit on it.
    ///
    /// The azimuth and the elevation are kept: `fit` changes how far away the
    /// camera is, not where it looks from, which is what a reader turning a
    /// surface expects.
    pub fn fit(&self, points: &[Vec3]) -> Camera {
        if points.is_empty() {
            return *self;
        }
        let mut low = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
        let mut high = Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for point in points {
            low = Vec3::new(low.x.min(point.x), low.y.min(point.y), low.z.min(point.z));
            high = Vec3::new(high.x.max(point.x), high.y.max(point.y), high.z.max(point.z));
        }
        let target = low.add(high).scale(0.5);
        let radius = high.sub(low).length() * 0.5;
        // The half-angle the sphere subtends, from the vertical field of view
        // and the horizontal one, whichever is tighter.
        let half_vertical = self.field_of_view * 0.5;
        let half_horizontal = (half_vertical.tan() * self.aspect).atan();
        let half = half_vertical.min(half_horizontal) * 0.9;
        // `sin(half) = radius / distance` for a sphere the camera sees at that
        // angle, so `distance = radius / sin(half)`.
        let distance = if radius > 0.0 {
            (radius / half.sin()).max(self.near * 2.0)
        } else {
            self.distance
        };
        Camera {
            target,
            distance,
            ..*self
        }
    }
}

/// A projected point, in normalised device coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenPoint {
    /// `[-1, 1]`, left to right.
    pub x: f64,
    /// `[-1, 1]`, bottom to top.
    pub y: f64,
    /// The distance in front of the camera.
    pub depth: f64,
}

/// A triangle after projection, with its corners in screen space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectedTriangle {
    /// The three corners, in normalised device coordinates.
    pub corners: [ScreenPoint; 3],
    /// The average depth, for the painter's algorithm.
    pub depth: f64,
}

/// Project a triangle, or `None` when any corner is behind the camera.
///
/// A triangle with one corner behind the camera would need clipping against the
/// near plane to be drawn correctly; this function refuses it rather than
/// drawing the mirrored corner, which is the same rule [`Camera::project`]
/// follows and the same reason.
pub fn project_triangle(camera: &Camera, corners: [Vec3; 3]) -> Option<ProjectedTriangle> {
    let a = camera.project(corners[0])?;
    let b = camera.project(corners[1])?;
    let c = camera.project(corners[2])?;
    Some(ProjectedTriangle {
        corners: [a, b, c],
        depth: (a.depth + b.depth + c.depth) / 3.0,
    })
}

/// Sort triangles back to front, for the painter's algorithm.
///
/// The order is by average depth, descending: the farthest is drawn first. A
/// stable sort keeps the input order for two triangles at the same depth, which
/// is what makes a mesh's own order the tie-breaker rather than an accident of
/// the sort.
pub fn depth_sort(mut triangles: Vec<ProjectedTriangle>) -> Vec<ProjectedTriangle> {
    triangles.sort_by(|a, b| b.depth.total_cmp(&a.depth));
    triangles
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube() -> Vec<Vec3> {
        let mut points = Vec::new();
        for x in [-1.0, 1.0] {
            for y in [-1.0, 1.0] {
                for z in [-1.0, 1.0] {
                    points.push(Vec3::new(x, y, z));
                }
            }
        }
        points
    }

    #[test]
    fn a_known_cube_projects_to_known_screen_coordinates() {
        // The roadmap's first check. A camera at `(0, 0, 5)` looking at the
        // origin with a 90° field of view: the frame's edge at that depth is
        // `5` world units from the axis, so the point `(5, 5, 0)` is exactly the
        // corner — `(1, 1)`. The first version of this test used `(1, 1, 0)`
        // and expected `(1, 1)`, which would be a 90° field of view from a
        // camera one unit away; the code was right and the expectation was not.
        let camera = Camera {
            target: Vec3::ZERO,
            distance: 5.0,
            azimuth: 0.0,
            elevation: 0.0,
            field_of_view: PI / 2.0,
            aspect: 1.0,
            near: 0.1,
            far: 100.0,
        };
        assert_eq!(camera.position(), Vec3::new(0.0, 0.0, 5.0));
        let corner = camera.project(Vec3::new(5.0, 5.0, 0.0)).expect("in front");
        assert!((corner.x - 1.0).abs() < 1e-9, "{corner:?}");
        assert!((corner.y - 1.0).abs() < 1e-9, "{corner:?}");
        assert!((corner.depth - 5.0).abs() < 1e-9, "{corner:?}");
        // The centre projects to the centre.
        let centre = camera.project(Vec3::ZERO).expect("in front");
        assert!(centre.x.abs() < 1e-9 && centre.y.abs() < 1e-9, "{centre:?}");
        // And a point a fifth as far across is a fifth as far out: `(1, 0, 0)`
        // is `0.2` of the half-width.
        let fifth = camera.project(Vec3::new(1.0, 0.0, 0.0)).expect("in front");
        assert!((fifth.x - 0.2).abs() < 1e-9, "{fifth:?}");
        // In pixels, with a 800×600 screen, the centre is the middle.
        let (px, py) = Camera::to_pixels(centre, 800.0, 600.0);
        assert!((px - 400.0).abs() < 1e-9 && (py - 300.0).abs() < 1e-9);
    }

    #[test]
    fn a_point_behind_the_camera_is_clipped() {
        // The roadmap's second check. `(0, 0, 10)` is behind a camera at
        // `(0, 0, 5)` looking at the origin, and it has no projection — not a
        // mirrored one.
        let camera = Camera::default();
        assert!(camera.project(Vec3::new(0.0, 0.0, 10.0)).is_none());
        // A point exactly on the camera's plane has no projection either.
        assert!(camera.project(camera.position()).is_none());
        // And a point just in front does.
        let in_front = camera.position().add(Vec3::new(0.0, 0.0, -1.0));
        assert!(camera.project(in_front).is_some());
        // A triangle with one corner behind is refused, not drawn mirrored.
        assert!(
            project_triangle(
                &camera,
                [
                    Vec3::new(-1.0, -1.0, 0.0),
                    Vec3::new(1.0, -1.0, 0.0),
                    Vec3::new(0.0, 1.0, 10.0),
                ]
            )
            .is_none()
        );
    }

    #[test]
    fn the_camera_round_trips_through_fit() {
        // The roadmap's third check. `fit` puts every point inside the frustum,
        // and looking from the fitted camera still sees them all.
        let camera = Camera::default();
        let points = cube();
        let fitted = camera.fit(&points);
        for point in &points {
            assert!(
                fitted.contains(*point),
                "{point:?} is outside the fitted frustum: {fitted:?}"
            );
            assert!(fitted.project(*point).is_some(), "{point:?}");
        }
        // The target is the centre of the points.
        assert!((fitted.target.x).abs() < 1e-9);
        assert!((fitted.target.y).abs() < 1e-9);
        assert!((fitted.target.z).abs() < 1e-9);
        // Fitting again changes nothing: `fit` is idempotent on its own result.
        let again = fitted.fit(&points);
        assert!((again.distance - fitted.distance).abs() < 1e-9, "{again:?}");
    }

    #[test]
    fn the_camera_axes_are_orthonormal() {
        // Every camera's three axes are unit vectors and perpendicular, which is
        // what makes the view transform a rotation and not a shear.
        for azimuth in [0.0, 0.5, 1.5, 3.0] {
            for elevation in [-1.0, -0.3, 0.0, 0.3, 1.0] {
                let camera = Camera {
                    azimuth,
                    elevation,
                    ..Camera::default()
                };
                let (right, up, back) = camera.view_axes();
                assert!((right.length() - 1.0).abs() < 1e-12, "{right:?}");
                assert!((up.length() - 1.0).abs() < 1e-12, "{up:?}");
                assert!((back.length() - 1.0).abs() < 1e-12, "{back:?}");
                assert!(right.dot(up).abs() < 1e-12);
                assert!(right.dot(back).abs() < 1e-12);
                assert!(up.dot(back).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn the_view_transform_puts_the_target_at_the_origin() {
        // The target is at view-space `(0, 0, −distance)`: straight ahead.
        let camera = Camera::default();
        let view = camera.to_view(camera.target);
        assert!(view.x.abs() < 1e-12 && view.y.abs() < 1e-12, "{view:?}");
        assert!((view.z + camera.distance).abs() < 1e-12, "{view:?}");
    }

    #[test]
    fn the_painter_s_algorithm_draws_the_far_one_first() {
        let camera = Camera::default();
        let near = project_triangle(
            &camera,
            [
                Vec3::new(-1.0, -1.0, 1.0),
                Vec3::new(1.0, -1.0, 1.0),
                Vec3::new(0.0, 1.0, 1.0),
            ],
        )
        .expect("in front");
        let far = project_triangle(
            &camera,
            [
                Vec3::new(-1.0, -1.0, -1.0),
                Vec3::new(1.0, -1.0, -1.0),
                Vec3::new(0.0, 1.0, -1.0),
            ],
        )
        .expect("in front");
        let sorted = depth_sort(vec![near, far]);
        assert!(sorted[0].depth > sorted[1].depth, "{sorted:?}");
    }

    #[test]
    fn a_point_outside_the_field_of_view_is_not_contained() {
        let camera = Camera {
            field_of_view: PI / 4.0,
            aspect: 1.0,
            ..Camera::default()
        };
        // A point far to the side is in front but outside the frame.
        let point = Vec3::new(100.0, 0.0, 0.0);
        assert!(camera.project(point).is_some(), "it is in front");
        assert!(!camera.contains(point), "but it is outside the frame");
    }

    #[test]
    fn the_elevation_and_the_azimuth_move_the_camera_where_they_say() {
        // Azimuth 0 looks from `+z`; azimuth π/2 from `+x`; elevation π/2 from
        // above.
        let camera = Camera {
            azimuth: 0.0,
            elevation: 0.0,
            distance: 5.0,
            ..Camera::default()
        };
        let position = camera.position();
        assert!((position.z - 5.0).abs() < 1e-12, "{position:?}");
        let camera = Camera {
            azimuth: PI / 2.0,
            ..camera
        };
        let position = camera.position();
        assert!((position.x - 5.0).abs() < 1e-12, "{position:?}");
        let camera = Camera {
            elevation: PI / 2.0,
            ..camera
        };
        let position = camera.position();
        assert!((position.y - 5.0).abs() < 1e-12, "{position:?}");
    }
}
