//! A mesh as an SVG, so that a surface can be looked at without a window.
//!
//! W8.4's viewport is planned on Bevy, which does not fit on this machine. This
//! is not that viewport: it is the smallest thing that makes a mesh **visible**,
//! and it uses the same camera, projection and depth sort the viewport will.
//! What it does not have is interaction — no orbit, no zoom, no lighting.
//!
//! # Back faces are culled, and that is what makes the picture readable
//!
//! The first version drew every triangle. On a paraboloid that produced a
//! pinched sail instead of a bowl, because near the rim the surface tilts away
//! from the camera — measured: 6 of 32 triangles on a 4×4 grid are back-facing —
//! and drawing them shows the *inside* of the far wall through the surface. A
//! triangle is culled when its projected winding is clockwise, which is the
//! winding a front-facing triangle does not have under this projection.
//!
//! # The painter's algorithm, and what it gets wrong
//!
//! Triangles are drawn back to front by their average depth. That is correct for
//! a surface that does not intersect itself, and wrong for one that does: two
//! triangles that pass through each other have no back-to-front order. A depth
//! buffer is the fix and this is not one.

use crate::surface::Mesh;
use crate::{Camera, Vec3, project_triangle};

/// How to draw a mesh.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SvgOptions {
    /// The picture's width in pixels.
    pub width: f64,
    /// Its height.
    pub height: f64,
    /// The colour at shade 0.
    pub low: [u8; 3],
    /// The colour at shade 1.
    pub high: [u8; 3],
    /// Whether to draw each triangle's outline.
    pub edges: bool,
}

impl Default for SvgOptions {
    fn default() -> Self {
        SvgOptions {
            width: 800.0,
            height: 600.0,
            low: [40, 70, 130],
            high: [240, 200, 120],
            edges: true,
        }
    }
}

/// A mesh as an SVG document.
pub fn mesh_svg(mesh: &Mesh, camera: &Camera, options: &SvgOptions) -> String {
    let mut triangles = Vec::new();
    for triangle in &mesh.triangles {
        let corners = [
            mesh.vertices[triangle[0]].position,
            mesh.vertices[triangle[1]].position,
            mesh.vertices[triangle[2]].position,
        ];
        let Some(projected) = project_triangle(camera, corners) else {
            continue;
        };
        // Back-face culling. A front-facing triangle keeps the mesh's winding
        // under this projection and comes out with a **positive** cross product;
        // a triangle on the far side of the surface comes out negative. The first
        // version had the sign the other way round, which culled a horizontal
        // plane seen from above — every triangle of it — and kept the bowl's far
        // wall, which is the opposite of what the picture needed.
        let (a, b, c) = (
            projected.corners[0],
            projected.corners[1],
            projected.corners[2],
        );
        let cross = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        if cross <= 0.0 {
            continue;
        }
        let shade = (mesh.vertices[triangle[0]].shade
            + mesh.vertices[triangle[1]].shade
            + mesh.vertices[triangle[2]].shade)
            / 3.0;
        triangles.push((projected, shade));
    }
    let mut order: Vec<usize> = (0..triangles.len()).collect();
    order.sort_by(|a, b| triangles[*b].0.depth.total_cmp(&triangles[*a].0.depth));

    let mut svg = String::new();
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">\n",
        options.width, options.height, options.width, options.height
    ));
    svg.push_str(&format!(
        "<rect width=\"{}\" height=\"{}\" fill=\"#ffffff\"/>\n",
        options.width, options.height
    ));
    for index in order {
        let (projected, shade) = &triangles[index];
        let points: Vec<String> = projected
            .corners
            .iter()
            .map(|corner| {
                let (x, y) = Camera::to_pixels(*corner, options.width, options.height);
                format!("{x:.2},{y:.2}")
            })
            .collect();
        let stroke = if options.edges {
            String::from(" stroke=\"#1b1b1b\" stroke-width=\"0.4\" stroke-opacity=\"0.5\"")
        } else {
            String::new()
        };
        svg.push_str(&format!(
            "<polygon points=\"{}\" fill=\"{}\"{stroke}/>\n",
            points.join(" "),
            ramp(*shade, options.low, options.high)
        ));
    }
    svg.push_str("</svg>\n");
    svg
}

/// A colour between two stops, clamped.
fn ramp(shade: f64, low: [u8; 3], high: [u8; 3]) -> String {
    let t = shade.clamp(0.0, 1.0);
    let mix = |a: u8, b: u8| -> u8 {
        let value = f64::from(a) + (f64::from(b) - f64::from(a)) * t;
        value.round().clamp(0.0, 255.0) as u8
    };
    format!(
        "#{:02x}{:02x}{:02x}",
        mix(low[0], high[0]),
        mix(low[1], high[1]),
        mix(low[2], high[2])
    )
}

/// The camera that frames a mesh, from the angles the caller gives.
///
/// `fit` puts the bounding sphere exactly inside the frustum, which for a surface
/// with a large depth extent leaves the near rim very close and the perspective
/// extreme; the distance is widened so the whole surface is in frame with a
/// gentler projection.
pub fn framing_camera(mesh: &Mesh, azimuth: f64, elevation: f64, aspect: f64) -> Camera {
    let points: Vec<Vec3> = mesh.vertices.iter().map(|vertex| vertex.position).collect();
    let camera = Camera {
        azimuth,
        elevation,
        aspect,
        ..Camera::default()
    };
    let mut fitted = camera.fit(&points);
    fitted.distance *= 1.8;
    fitted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::{SurfaceOptions, mesh_surface};
    use canlab_core::{add, int, pow, sym};

    fn paraboloid() -> Mesh {
        let expression = add([pow(sym("x"), int(2)), pow(sym("y"), int(2))]);
        let options = SurfaceOptions {
            x_range: (-2.0, 2.0),
            y_range: (-2.0, 2.0),
            x_cells: 4,
            y_cells: 4,
            pixel_error: 0.0,
            max_split: 0,
        };
        mesh_surface(&expression, &options, &Camera::default()).expect("a mesh")
    }

    #[test]
    fn back_faces_are_culled_and_the_rest_are_drawn() {
        // A bowl seen from above has back-facing triangles near its rim — 6 of
        // 32 on this grid — and drawing them shows the far wall's inside through
        // the surface, which is what made the first picture a pinched sail.
        let mesh = paraboloid();
        let camera = framing_camera(&mesh, 0.6, 0.5, 4.0 / 3.0);
        let svg = mesh_svg(&mesh, &camera, &SvgOptions::default());
        let drawn = svg.matches("<polygon").count();
        assert!(drawn > 0, "nothing was drawn");
        assert!(
            drawn < mesh.triangle_count(),
            "every triangle was drawn, so nothing was culled: {drawn} of {}",
            mesh.triangle_count()
        );
        // The culled ones are exactly the back-facing ones.
        let mut back = 0;
        for triangle in &mesh.triangles {
            let corners = [
                mesh.vertices[triangle[0]].position,
                mesh.vertices[triangle[1]].position,
                mesh.vertices[triangle[2]].position,
            ];
            if let Some(projected) = project_triangle(&camera, corners) {
                let (a, b, c) = (
                    projected.corners[0],
                    projected.corners[1],
                    projected.corners[2],
                );
                if (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x) <= 0.0 {
                    back += 1;
                }
            }
        }
        assert_eq!(drawn + back, mesh.triangle_count(), "{drawn} + {back}");
    }

    #[test]
    fn a_horizontal_plane_fills_the_expected_part_of_the_frame() {
        // The unambiguous case: `z = 0` over `[-2, 2]²` seen from 34° above is a
        // rectangle wider than it is tall, and its width is the frame's.
        // `z = 0`, a horizontal plane: the first version used `z = x`, which is
        // nearly edge-on at this angle and is culled entirely.
        let expression = canlab_core::int(0);
        let options = SurfaceOptions {
            x_range: (-2.0, 2.0),
            y_range: (-2.0, 2.0),
            x_cells: 4,
            y_cells: 4,
            pixel_error: 0.0,
            max_split: 0,
        };
        let mesh = mesh_surface(&expression, &options, &Camera::default()).expect("a mesh");
        let camera = framing_camera(&mesh, 0.0, 0.6, 4.0 / 3.0);
        let svg = mesh_svg(&mesh, &camera, &SvgOptions::default());
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        for line in svg.lines().filter(|line| line.starts_with("<polygon")) {
            let points = line
                .split("points=\"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .expect("points");
            for point in points.split(' ') {
                let (x, y) = point.split_once(',').expect("a pair");
                xs.push(x.parse::<f64>().expect("a number"));
                ys.push(y.parse::<f64>().expect("a number"));
            }
        }
        let width = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
            - xs.iter().cloned().fold(f64::INFINITY, f64::min);
        let height = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
            - ys.iter().cloned().fold(f64::INFINITY, f64::min);
        // `framing_camera` widens `fit` by 1.8, so the plane covers about a fifth
        // of the frame's width rather than all of it. The number is asserted so
        // that a change to the framing shows up here.
        assert!(width > 150.0, "the plane is only {width} px wide");
        assert!(
            width > height,
            "a plane seen from 34° above is wider than tall: {width} x {height}"
        );
    }

    #[test]
    fn no_coordinate_in_the_svg_is_not_a_number() {
        let mesh = paraboloid();
        for azimuth in [0.0, 1.0, 2.5] {
            for elevation in [-1.0, 0.0, 1.0] {
                let camera = framing_camera(&mesh, azimuth, elevation, 1.5);
                let svg = mesh_svg(&mesh, &camera, &SvgOptions::default());
                assert!(
                    !svg.contains("NaN") && !svg.contains("inf"),
                    "azimuth {azimuth} elevation {elevation}"
                );
            }
        }
    }

    #[test]
    fn a_mesh_entirely_behind_the_camera_draws_nothing() {
        let mesh = paraboloid();
        // The camera sits at `target + direction·distance` and looks back, so with
        // azimuth 0 it is at `z = target.z + distance` looking towards `−z`. The
        // mesh is behind it when the camera is at a lower `z` than every vertex.
        let camera = Camera {
            target: Vec3::new(0.0, 0.0, -10.0),
            distance: 0.1,
            azimuth: 0.0,
            elevation: 0.0,
            ..Camera::default()
        };
        let svg = mesh_svg(&mesh, &camera, &SvgOptions::default());
        assert_eq!(svg.matches("<polygon").count(), 0);
        assert!(svg.trim_end().ends_with("</svg>"));
    }

    #[test]
    fn the_shade_ramp_runs_between_its_two_stops() {
        let options = SvgOptions::default();
        assert_eq!(ramp(0.0, options.low, options.high), "#284682");
        assert_eq!(ramp(1.0, options.low, options.high), "#f0c878");
        assert_eq!(ramp(-1.0, options.low, options.high), "#284682");
        assert_eq!(ramp(2.0, options.low, options.high), "#f0c878");
    }
}
