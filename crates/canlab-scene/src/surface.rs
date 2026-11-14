//! W8.2: a surface `z = f(x, y)` as a mesh.
//!
//! A grid over `(x, y)`, a `z` at every corner, triangles between the corners,
//! and a normal at every vertex from the *analytic* derivative — not from the
//! triangle's own plane, which is a second, worse answer to "which way does the
//! surface face".
//!
//! # The adaptive split, and why the error is measured on the screen
//!
//! A grid fine enough for a paraboloid's steep side is far too fine for its
//! flat middle. The split is therefore driven by the *screen-space* error: a
//! cell is split when the difference between its corners' `z` is larger than a
//! fraction of the pixel the surface would move — which is the error a reader
//! can see. The threshold is in world units per pixel, computed from the
//! camera's field of view and the grid's spacing, so a zoomed-in view refines
//! and a zoomed-out one coarsens, which is what a reader expects.
//!
//! # The break rule, which is the 2D rule
//!
//! A corner whose `z` is not finite is **dropped and its cells are not
//! drawn**. That is the same rule the 2D sampler follows — a break means "not
//! connected here" — and it is why a pole in `f` is a hole in the surface rather
//! than a spike to infinity. A mesh that joined across the pole would draw a
//! wall that is not in the function.
//!
//! # Normals
//!
//! For `z = f(x, y)` the normal is `(−∂f/∂x, −∂f/∂y, 1)`, normalised. The
//! derivatives come from [`canlab_core::differentiate`], so the normal is the
//! *analytic* one: it is right at a coarse cell where the triangle's plane is
//! not, and it is the one a lighting model wants.

use canlab_core::expr::Expr;
use canlab_core::symbol::SymbolId;

use crate::{Camera, Vec3};

/// A vertex of a surface mesh.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    /// Where it is.
    pub position: Vec3,
    /// The analytic normal, unit length.
    pub normal: Vec3,
    /// The palette position, in `[0, 1]`, from the height.
    pub shade: f64,
}

/// A surface mesh: vertices and the triangles that index them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mesh {
    /// The vertices.
    pub vertices: Vec<Vertex>,
    /// The triangles, as three indices into `vertices`.
    pub triangles: Vec<[usize; 3]>,
}

impl Mesh {
    /// How many triangles.
    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }

    /// How many vertices.
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// `true` when no vertex has a non-finite coordinate or normal.
    pub fn is_finite(&self) -> bool {
        self.vertices.iter().all(|vertex| {
            vertex.position.x.is_finite()
                && vertex.position.y.is_finite()
                && vertex.position.z.is_finite()
                && vertex.normal.x.is_finite()
                && vertex.normal.y.is_finite()
                && vertex.normal.z.is_finite()
        })
    }
}

/// Why a surface cannot be meshed.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceError {
    /// The expression does not differentiate, so it has no normal.
    NoDerivative,
    /// The grid has no cells.
    EmptyGrid,
}

impl SurfaceError {
    /// The message a caller shows.
    pub fn message(&self) -> String {
        match self {
            SurfaceError::NoDerivative => {
                "the surface cannot be differentiated, so it has no normals".to_string()
            }
            SurfaceError::EmptyGrid => "the grid has no cells".to_string(),
        }
    }
}

/// How to mesh a surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceOptions {
    /// The `x` range.
    pub x_range: (f64, f64),
    /// The `y` range.
    pub y_range: (f64, f64),
    /// How many cells along `x`.
    pub x_cells: usize,
    /// How many cells along `y`.
    pub y_cells: usize,
    /// The screen-space error that stops a split, in world units per pixel.
    ///
    /// A cell is split while its corners' `z` differ by more than this times
    /// the cell's screen size. `0.0` means "split every cell to the maximum
    /// depth", which is what a test wants and a reader does not.
    pub pixel_error: f64,
    /// The maximum number of splits along one axis.
    pub max_split: u32,
}

impl Default for SurfaceOptions {
    fn default() -> Self {
        SurfaceOptions {
            x_range: (-5.0, 5.0),
            y_range: (-5.0, 5.0),
            x_cells: 24,
            y_cells: 24,
            pixel_error: 0.5,
            max_split: 3,
        }
    }
}

/// Mesh `z = f(x, y)`.
///
/// `expression` is `f` in terms of `x` and `y`; `camera` supplies the field of
/// view and the aspect the screen-space error is measured against, so a
/// zoomed-in camera refines more.
pub fn mesh_surface(
    expression: &Expr,
    options: &SurfaceOptions,
    camera: &Camera,
) -> Result<Mesh, SurfaceError> {
    if options.x_cells == 0 || options.y_cells == 0 {
        return Err(SurfaceError::EmptyGrid);
    }
    let x = SymbolId::intern("x");
    let y = SymbolId::intern("y");
    let program = canlab_core::Program::compile(expression, &[x, y]).ok_or(SurfaceError::NoDerivative)?;
    let dx = canlab_core::differentiate(expression, x).ok_or(SurfaceError::NoDerivative)?;
    let dy = canlab_core::differentiate(expression, y).ok_or(SurfaceError::NoDerivative)?;
    let dx_program = canlab_core::Program::compile(&dx, &[x, y]).ok_or(SurfaceError::NoDerivative)?;
    let dy_program = canlab_core::Program::compile(&dy, &[x, y]).ok_or(SurfaceError::NoDerivative)?;

    let height = |x_value: f64, y_value: f64| program.eval(&[x_value, y_value]);
    let normal = |x_value: f64, y_value: f64| {
        let slope_x = dx_program.eval(&[x_value, y_value]);
        let slope_y = dy_program.eval(&[x_value, y_value]);
        Vec3::new(-slope_x, -slope_y, 1.0).normalized()
    };

    let mut mesh = Mesh::default();
    // The vertices are built per cell-corner and deduplicated by position, so
    // that two cells sharing an edge share its vertices and the mesh is
    // *indexed* rather than a soup of triangles.
    let mut index_of: std::collections::HashMap<(u64, u64), usize> = std::collections::HashMap::new();
    let mut vertex = |x_value: f64, y_value: f64, mesh: &mut Mesh| -> Option<usize> {
        let z = height(x_value, y_value);
        if !z.is_finite() {
            return None;
        }
        let key = (x_value.to_bits(), y_value.to_bits());
        if let Some(index) = index_of.get(&key) {
            return Some(*index);
        }
        let Some(normal) = normal(x_value, y_value) else {
            return None;
        };
        let index = mesh.vertices.len();
        mesh.vertices.push(Vertex {
            position: Vec3::new(x_value, y_value, z),
            normal,
            shade: 0.0,
        });
        index_of.insert(key, index);
        Some(index)
    };

    let x_step = (options.x_range.1 - options.x_range.0) / options.x_cells as f64;
    let y_step = (options.y_range.1 - options.y_range.0) / options.y_cells as f64;
    // The world size of one pixel at the target's distance, which is the scale
    // the screen-space error is measured in.
    let world_per_pixel =
        2.0 * camera.distance * (camera.field_of_view * 0.5).tan() / 1000.0;
    let threshold = options.pixel_error * world_per_pixel;

    for i in 0..options.x_cells {
        for j in 0..options.y_cells {
            let x0 = options.x_range.0 + x_step * i as f64;
            let x1 = x0 + x_step;
            let y0 = options.y_range.0 + y_step * j as f64;
            let y1 = y0 + y_step;
            split_cell(
                &mut mesh,
                &mut vertex,
                (x0, x1, y0, y1),
                &height,
                threshold,
                options.max_split,
            );
        }
    }

    // The shade: the palette position from the height, over the mesh's own
    // range. A flat surface is all one colour, which is the right answer.
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    for vertex in &mesh.vertices {
        low = low.min(vertex.position.z);
        high = high.max(vertex.position.z);
    }
    let span = high - low;
    for vertex in &mut mesh.vertices {
        vertex.shade = if span > 0.0 {
            ((vertex.position.z - low) / span).clamp(0.0, 1.0)
        } else {
            0.5
        };
    }
    Ok(mesh)
}

/// Split one cell until its screen-space error is small enough, and emit its
/// triangles.
///
/// The cell is a quad split into two triangles. A cell whose corners' heights
/// differ by more than the threshold is split into four, recursively, to
/// `max_split`; a cell with a non-finite corner is **not drawn** and is not
/// split, because splitting it would only produce more non-finite corners.
fn split_cell(
    mesh: &mut Mesh,
    vertex: &mut impl FnMut(f64, f64, &mut Mesh) -> Option<usize>,
    (x0, x1, y0, y1): (f64, f64, f64, f64),
    height: &impl Fn(f64, f64) -> f64,
    threshold: f64,
    depth: u32,
) {
    let corners = [
        (x0, y0),
        (x1, y0),
        (x1, y1),
        (x0, y1),
    ];
    let heights: Vec<f64> = corners.iter().map(|(x, y)| height(*x, *y)).collect();
    // A cell with a non-finite corner is a hole: not drawn, not split.
    if heights.iter().any(|z| !z.is_finite()) {
        return;
    }
    let lowest = heights.iter().cloned().fold(f64::INFINITY, f64::min);
    let highest = heights.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if depth > 0 && highest - lowest > threshold {
        // Split into four.
        let xm = 0.5 * (x0 + x1);
        let ym = 0.5 * (y0 + y1);
        for (a, b, c, d) in [
            (x0, xm, y0, ym),
            (xm, x1, y0, ym),
            (x0, xm, ym, y1),
            (xm, x1, ym, y1),
        ] {
            split_cell(mesh, vertex, (a, b, c, d), height, threshold, depth - 1);
        }
        return;
    }
    // Two triangles from the four corners.
    let mut indices = Vec::with_capacity(4);
    for (x, y) in corners {
        let Some(index) = vertex(x, y, mesh) else {
            return;
        };
        indices.push(index);
    }
    mesh.triangles.push([indices[0], indices[1], indices[2]]);
    mesh.triangles.push([indices[0], indices[2], indices[3]]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use canlab_core::{add, int, mul, pow, sym};

    fn flat() -> Expr {
        add([mul([int(2), sym("x")]), mul([int(3), sym("y")])])
    }

    fn paraboloid() -> Expr {
        add([
            pow(sym("x"), int(2)),
            pow(sym("y"), int(2)),
        ])
    }

    fn options() -> SurfaceOptions {
        SurfaceOptions {
            x_range: (-1.0, 1.0),
            y_range: (-1.0, 1.0),
            x_cells: 4,
            y_cells: 4,
            pixel_error: 0.0,
            max_split: 0,
        }
    }

    #[test]
    fn a_plane_is_a_regular_grid_of_triangles() {
        // The roadmap's first case: a plane. Four cells each way is 32
        // triangles and 25 vertices, and every normal points the same way.
        let mesh = mesh_surface(&flat(), &options(), &Camera::default()).expect("a mesh");
        assert_eq!(mesh.triangle_count(), 32, "{:?}", mesh.triangles.len());
        assert_eq!(mesh.vertex_count(), 25, "{:?}", mesh.vertices.len());
        assert!(mesh.is_finite());
        // The normal of `z = 2x + 3y` is `(−2, −3, 1)/√14`.
        let expected = Vec3::new(-2.0, -3.0, 1.0).normalized().expect("a normal");
        for vertex in &mesh.vertices {
            assert!(
                (vertex.normal.x - expected.x).abs() < 1e-12
                    && (vertex.normal.y - expected.y).abs() < 1e-12
                    && (vertex.normal.z - expected.z).abs() < 1e-12,
                "{:?} vs {expected:?}",
                vertex.normal
            );
        }
        // The shade runs with the height, and `z = 2x + 3y` over `[-1, 1]²`
        // ranges from `-5` to `5`, so the shade is not constant. The first
        // version of this test asserted `0.5` everywhere, which would be right
        // only for a horizontal plane.
        let shades: Vec<f64> = mesh.vertices.iter().map(|vertex| vertex.shade).collect();
        let low = shades.iter().cloned().fold(f64::INFINITY, f64::min);
        let high = shades.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        assert!((low - 0.0).abs() < 1e-12, "the lowest shade is {low}");
        assert!((high - 1.0).abs() < 1e-12, "the highest shade is {high}");
    }

    #[test]
    fn a_paraboloid_has_normals_that_point_outward_and_down() {
        // The roadmap's second case. `z = x² + y²` has the normal
        // `(−2x, −2y, 1)` normalised, which at the origin is `+z` and at the
        // rim tilts outward.
        let mesh = mesh_surface(&paraboloid(), &options(), &Camera::default()).expect("a mesh");
        assert!(mesh.is_finite());
        for vertex in &mesh.vertices {
            let expected = Vec3::new(-2.0 * vertex.position.x, -2.0 * vertex.position.y, 1.0)
                .normalized()
                .expect("a normal");
            assert!(
                (vertex.normal.x - expected.x).abs() < 1e-12
                    && (vertex.normal.y - expected.y).abs() < 1e-12
                    && (vertex.normal.z - expected.z).abs() < 1e-12,
                "at {:?}: {:?} vs {expected:?}",
                vertex.position,
                vertex.normal
            );
        }
        // The shade runs from 0 at the lowest point to 1 at the highest.
        let low = mesh
            .vertices
            .iter()
            .map(|v| v.position.z)
            .fold(f64::INFINITY, f64::min);
        let high = mesh
            .vertices
            .iter()
            .map(|v| v.position.z)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!((low - 0.0).abs() < 1e-12, "{low}");
        assert!((high - 2.0).abs() < 1e-12, "{high}");
        for vertex in &mesh.vertices {
            let expected = (vertex.position.z - low) / (high - low);
            assert!((vertex.shade - expected).abs() < 1e-12);
        }
    }

    #[test]
    fn no_vertex_is_ever_a_nan() {
        // The roadmap's third check, on a surface with a pole: `1/(xy)` is not
        // finite on the axes. The cells that touch a non-finite corner are
        // dropped, and every vertex that remains is finite.
        let pole = canlab_core::pow(mul([sym("x"), sym("y")]), int(-1));
        let mesh = mesh_surface(&pole, &options(), &Camera::default()).expect("a mesh");
        assert!(mesh.is_finite(), "a non-finite vertex survived");
        // The cells on the axes are gone, so the mesh is not the full grid.
        assert!(
            mesh.triangle_count() < 32,
            "the pole's cells were drawn: {} triangles",
            mesh.triangle_count()
        );
    }

    #[test]
    fn an_adaptive_split_refines_where_the_surface_bends() {
        // A paraboloid with a tight threshold: the cells near the rim, where the
        // height changes fastest, are split, so there are more triangles than
        // the plain grid.
        let options = SurfaceOptions {
            x_range: (-1.0, 1.0),
            y_range: (-1.0, 1.0),
            x_cells: 2,
            y_cells: 2,
            pixel_error: 0.0001,
            max_split: 2,
        };
        let mesh = mesh_surface(&paraboloid(), &options, &Camera::default()).expect("a mesh");
        assert!(
            mesh.triangle_count() > 8,
            "the split did nothing: {} triangles",
            mesh.triangle_count()
        );
        assert!(mesh.is_finite());
    }

    #[test]
    fn a_surface_that_cannot_be_differentiated_is_refused() {
        // A `Hold` has no derivative, so it has no normal.
        let hold = canlab_core::ExprNode::hold(canlab_core::expr::Hold::Integral {
            body: sym("x"),
            var: SymbolId::intern("x"),
        });
        let error = mesh_surface(&hold, &options(), &Camera::default()).expect_err("a refusal");
        assert_eq!(error, SurfaceError::NoDerivative);
        // An empty grid is refused too.
        let empty = SurfaceOptions {
            x_cells: 0,
            ..options()
        };
        assert_eq!(
            mesh_surface(&flat(), &empty, &Camera::default()),
            Err(SurfaceError::EmptyGrid)
        );
    }
}
