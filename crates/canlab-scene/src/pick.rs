//! W8.5: what is under the pointer, by a CPU ray through the mesh.
//!
//! A viewport that cannot say what the reader is pointing at is a picture. This
//! is the smallest thing that makes it a viewport: a ray from the eye through a
//! screen position, and the nearest triangle it meets.
//!
//! # Why the CPU and not a GPU picking pass
//!
//! A picking pass would need a second render target and a readback, which is
//! more machinery than the question is worth for a mesh of a few thousand
//! triangles. The CPU version is exact — Möller–Trumbore, no tolerance — and it
//! works on the same `Mesh` the renderer draws, so the answer cannot disagree
//! with the picture.
//!
//! # The ray
//!
//! A screen position in normalised device coordinates `[-1, 1]²` becomes a
//! direction in the camera's own basis: `x·aspect/focal` along `right`,
//! `y/focal` along `up`, and one along `forward`. The origin is the camera's
//! position, so the ray starts where the eye is and not on the near plane —
//! which matters for a triangle between the two.

use crate::surface::Mesh;
use crate::{Camera, Vec3};

/// Where a ray met a triangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    /// The point in world space.
    pub point: Vec3,
    /// How far along the ray it is.
    pub distance: f64,
    /// Which triangle, as an index into [`Mesh::triangles`].
    pub triangle: usize,
}

/// The ray through a normalised device coordinate.
///
/// Returns `(origin, direction)`, the direction unit length. `None` when the
/// camera's basis is degenerate, which happens only at a pole.
pub fn ray(camera: &Camera, ndc: (f64, f64)) -> Option<(Vec3, Vec3)> {
    let focal = 1.0 / (camera.field_of_view * 0.5).tan();
    if !focal.is_finite() || focal <= 0.0 {
        return None;
    }
    let right = camera.right();
    let up = camera.up();
    let forward = camera.forward();
    let direction = right
        .scale(ndc.0 * camera.aspect / focal)
        .add(up.scale(ndc.1 / focal))
        .add(forward);
    let direction = direction.normalized()?;
    Some((camera.position(), direction))
}

/// The nearest triangle the ray meets, or `None`.
///
/// Triangles behind the ray's origin are ignored, so a hit is always in front of
/// the reader.
pub fn pick(mesh: &Mesh, camera: &Camera, ndc: (f64, f64)) -> Option<Hit> {
    let (origin, direction) = ray(camera, ndc)?;
    let mut best: Option<Hit> = None;
    for (index, triangle) in mesh.triangles.iter().enumerate() {
        let a = mesh.vertices[triangle[0]].position;
        let b = mesh.vertices[triangle[1]].position;
        let c = mesh.vertices[triangle[2]].position;
        let Some(distance) = intersect(origin, direction, a, b, c) else {
            continue;
        };
        if distance <= 0.0 {
            continue;
        }
        if best.is_none_or(|hit| distance < hit.distance) {
            best = Some(Hit {
                point: origin.add(direction.scale(distance)),
                distance,
                triangle: index,
            });
        }
    }
    best
}

/// Möller–Trumbore: how far along the ray the triangle is, or `None`.
///
/// The barycentric test is strict — a point exactly on an edge counts as inside
/// both triangles that share it, which is what a reader expects when the pointer
/// is on a seam.
fn intersect(origin: Vec3, direction: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f64> {
    let edge1 = b.sub(a);
    let edge2 = c.sub(a);
    let h = direction.cross(edge2);
    let determinant = edge1.dot(h);
    // A determinant near zero is a ray parallel to the triangle's plane, or a
    // degenerate triangle. `1e-12` is far below any real triangle's area and far
    // above the rounding of one.
    if determinant.abs() < 1e-12 {
        return None;
    }
    let inverse = 1.0 / determinant;
    let s = origin.sub(a);
    let u = inverse * s.dot(h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(edge1);
    let v = inverse * direction.dot(q);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let distance = inverse * edge2.dot(q);
    distance.is_finite().then_some(distance)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Camera;

    /// A single triangle in the plane `z = 0`, facing the camera.
    fn triangle_mesh() -> Mesh {
        use crate::surface::Vertex;
        let vertex = |x: f64, y: f64| Vertex {
            position: Vec3::new(x, y, 0.0),
            normal: Vec3::new(0.0, 0.0, 1.0),
            shade: 0.5,
        };
        Mesh {
            vertices: vec![
                vertex(-1.0, -1.0),
                vertex(1.0, -1.0),
                vertex(0.0, 1.0),
            ],
            triangles: vec![[0, 1, 2]],
        }
    }

    fn looking_at_origin() -> Camera {
        Camera {
            target: Vec3::ZERO,
            distance: 5.0,
            azimuth: 0.0,
            elevation: 0.0,
            aspect: 1.0,
            ..Camera::default()
        }
    }

    #[test]
    fn a_ray_through_the_centre_meets_the_triangle_at_the_origin() {
        let mesh = triangle_mesh();
        let camera = looking_at_origin();
        let hit = pick(&mesh, &camera, (0.0, 0.0)).expect("a hit");
        assert!((hit.point.x).abs() < 1e-9, "{:?}", hit.point);
        assert!((hit.point.y).abs() < 1e-9, "{:?}", hit.point);
        assert!((hit.point.z).abs() < 1e-9, "{:?}", hit.point);
        assert!((hit.distance - 5.0).abs() < 1e-9, "{}", hit.distance);
        assert_eq!(hit.triangle, 0);
    }

    #[test]
    fn a_ray_outside_the_triangle_misses_it() {
        let mesh = triangle_mesh();
        let camera = looking_at_origin();
        // A normalised coordinate is not a world coordinate: at distance 5 with
        // a 45° field of view, `focal` is 2.414, so NDC 0.5 is `0.5/2.414·5 =
        // 1.04` world units from the axis. The first version of this test
        // assumed NDC 0.5 was world 0.5 and failed on a correct ray.
        assert!(pick(&mesh, &camera, (0.9, 0.9)).is_none());
        // NDC −0.2 is world −0.41, which is inside the triangle.
        assert!(pick(&mesh, &camera, (-0.2, -0.2)).is_some());
    }

    #[test]
    fn a_ray_pointing_away_from_the_mesh_misses_it() {
        let mesh = triangle_mesh();
        // The camera behind the triangle, looking the other way: the triangle is
        // behind the ray's origin, so a hit would be at a negative distance.
        let camera = Camera {
            target: Vec3::new(0.0, 0.0, -10.0),
            distance: 0.1,
            azimuth: 0.0,
            elevation: 0.0,
            aspect: 1.0,
            ..Camera::default()
        };
        assert!(pick(&mesh, &camera, (0.0, 0.0)).is_none());
    }

    #[test]
    fn the_nearest_triangle_wins() {
        // Two parallel triangles, one at `z = 0` and one at `z = 2`, both facing
        // the camera at `z = 5`. The nearer one is the `z = 2` triangle.
        use crate::surface::Vertex;
        let vertex = |x: f64, y: f64, z: f64| Vertex {
            position: Vec3::new(x, y, z),
            normal: Vec3::new(0.0, 0.0, 1.0),
            shade: 0.5,
        };
        let mesh = Mesh {
            vertices: vec![
                vertex(-1.0, -1.0, 0.0),
                vertex(1.0, -1.0, 0.0),
                vertex(0.0, 1.0, 0.0),
                vertex(-1.0, -1.0, 2.0),
                vertex(1.0, -1.0, 2.0),
                vertex(0.0, 1.0, 2.0),
            ],
            triangles: vec![[0, 1, 2], [3, 4, 5]],
        };
        let camera = looking_at_origin();
        let hit = pick(&mesh, &camera, (0.0, 0.0)).expect("a hit");
        assert_eq!(hit.triangle, 1, "the farther triangle was reported");
        assert!((hit.point.z - 2.0).abs() < 1e-9, "{:?}", hit.point);
        assert!((hit.distance - 3.0).abs() < 1e-9, "{}", hit.distance);
    }

    #[test]
    fn picking_a_sphere_returns_its_surface_within_a_pixel() {
        // The roadmap's check. A unit sphere meshed by surface nets, picked at
        // the centre of the frame: the hit must be on the sphere, and the
        // *pixel* it projects back to must be the pixel that was picked.
        use crate::implicit::{ImplicitOptions, mesh_implicit};
        use canlab_core::{add, int, pow, sym};
        let sphere = add([
            pow(sym("x"), int(2)),
            pow(sym("y"), int(2)),
            pow(sym("z"), int(2)),
            int(-1),
        ]);
        let options = ImplicitOptions {
            x_range: (-1.5, 1.5),
            y_range: (-1.5, 1.5),
            z_range: (-1.5, 1.5),
            x_cells: 16,
            y_cells: 16,
            z_cells: 16,
        };
        let mesh = mesh_implicit(&sphere, &options).expect("a sphere");
        let camera = crate::svg::framing_camera(&mesh, 0.5, 0.3, 1.0);
        // Points well inside the sphere's silhouette: `framing_camera` widens
        // `fit` by 1.8, so the sphere covers about ±0.55 of the frame.
        for ndc in [(0.0, 0.0), (0.15, -0.1), (-0.2, 0.2)] {
            let hit = pick(&mesh, &camera, ndc).unwrap_or_else(|| panic!("no hit at {ndc:?}"));
            // On the surface: the radius is one, to the mesh's own resolution.
            let radius = hit.point.length();
            assert!(
                (radius - 1.0).abs() < 0.08,
                "the hit is at radius {radius}, not on the sphere"
            );
            // And it projects back to the pixel that was picked, within one
            // pixel of a 800-wide frame.
            let projected = camera.project(hit.point).expect("in front");
            let (x, y) = Camera::to_pixels(projected, 800.0, 600.0);
            let expected = ((ndc.0 + 1.0) * 0.5 * 800.0, (1.0 - ndc.1) * 0.5 * 600.0);
            assert!(
                (x - expected.0).abs() < 1.0 && (y - expected.1).abs() < 1.0,
                "the hit projects to ({x:.2}, {y:.2}) and the pick was ({:.2}, {:.2})",
                expected.0,
                expected.1
            );
        }
    }
}
