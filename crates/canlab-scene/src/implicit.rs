//! W8.3: an implicit surface `f(x, y, z) = 0` as a mesh, by surface nets.
//!
//! A surface `z = f(x, y)` is a height over a grid, and W8.2 walks that grid.
//! `x² + y² + z² − 1 = 0` has no height — it is a shell, and a mesh for it has
//! to be found in three dimensions. Surface nets is the 3D counterpart of
//! marching squares, chosen over marching cubes because marching cubes needs a
//! 256-entry triangle table to decide where a cell's crossings go, and that
//! table is a thousand lines nobody reads and a second answer to "which
//! triangles".
//!
//! # How a net is made
//!
//! 1. Sample `f` at every corner of a 3D grid.
//! 2. An edge whose two corners are on opposite sides of zero *crosses*. One
//!    vertex goes in each cell that has at least one crossing, at the mean of
//!    the crossings on that cell's *own* twelve edges — each found by linear
//!    interpolation along its edge, the same approximation marching squares
//!    makes.
//! 3. A crossing edge is shared by four cells. Where all four have a vertex,
//!    one quad joins them, in the order they go round the edge. The lookup is
//!    not a table: a quad has four corners and each is one cell, so the four
//!    are named by a fixed cycle of the cells round the edge.
//!
//! An edge's four cells are named from its *upper* one — the cell above the
//! crossing, which is the cell the crossing is in when it is in the upper half
//! of the edge and the cell below it when it is not. Every crossing is in one
//! of those two, so the cell it names has the crossing among its own edges, and
//! the choice depends only on the two samples, so both ends of an edge agree.
//! That is a surface net: one vertex per cell, and the four cells of a crossing
//! joined in a ring.
//!
//! # Where the rule stops, and why
//!
//! An *interior* grid is a closed surface net. Where the surface reaches the
//! grid's own boundary, a cell may be cut on a face that has no cell on its
//! other side, so its single vertex averages crossings from several sides, and
//! a quad whose fourth cell has no crossing on the edge is not drawn. The
//! result on that boundary is a net with an edge left alone here and a face
//! missing there, not a closed one — a sphere in a box that it reaches is drawn
//! with a ragged seam round the box's faces. The fix is a vertex *per side* of a
//! cell at the boundary, which is a different surface nets; it is not done here,
//! and this file does not claim it.
//!
//! # The rules that keep a bad sample out of the picture
//!
//! - A corner where `f` is not finite is **dropped and its cells are not
//!   drawn**, the same break rule as 2D and W8.2. A dropped corner is neither
//!   inside nor outside, so it is never interpolated across and no vertex is
//!   ever a `NaN`.
//! - A corner that is exactly `0` is on the *outside* (`f < 0` is inside), so
//!   every crossing is between values of different sign and dividing by their
//!   difference is safe.
//! - A quad is drawn only when all four of its cells are inside the grid, have
//!   no non-finite corner, and have a vertex and a normal at it.
//!
//! # Normals
//!
//! `∇f / |∇f|`, from [`canlab_core::differentiate`]. It is the *analytic*
//! normal, not the triangle's own, for the same reason W8.2 uses the analytic
//! one. The sign follows `f`: `x² + y² + z² − 1` has an outward normal and
//! `1 − x² − y² − z²` has the inward one.
//!
//! A cell whose gradient is zero or not finite at its vertex has no normal, and
//! is not drawn: a guessed normal would light the surface wrongly and the guess
//! would not be in the function.

use canlab_core::expr::Expr;
use canlab_core::symbol::SymbolId;

use crate::surface::{Mesh, Vertex};
use crate::Vec3;

/// Why an implicit surface cannot be meshed.
#[derive(Debug, Clone, PartialEq)]
pub enum ImplicitError {
    /// The expression does not compile, so it has no value to find a surface
    /// between.
    NoEvaluation,
    /// The expression compiles and evaluates, but one of its three partial
    /// derivatives is missing, so its surface has no normals.
    NoDerivative,
    /// The grid has no cells, or a range that is not a real interval.
    EmptyGrid,
}

impl ImplicitError {
    /// The message a caller shows.
    pub fn message(&self) -> String {
        match self {
            ImplicitError::NoEvaluation => {
                "the expression cannot be evaluated, so it has no surface".to_string()
            }
            ImplicitError::NoDerivative => {
                "the expression cannot be differentiated, so its surface has no normals"
                    .to_string()
            }
            ImplicitError::EmptyGrid => "the grid has no cells".to_string(),
        }
    }
}

/// How to mesh an implicit surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImplicitOptions {
    /// The `x` range.
    pub x_range: (f64, f64),
    /// The `y` range.
    pub y_range: (f64, f64),
    /// The `z` range.
    pub z_range: (f64, f64),
    /// How many cells along `x`.
    pub x_cells: usize,
    /// How many cells along `y`.
    pub y_cells: usize,
    /// How many cells along `z`.
    pub z_cells: usize,
}

impl Default for ImplicitOptions {
    fn default() -> Self {
        ImplicitOptions {
            x_range: (-2.0, 2.0),
            y_range: (-2.0, 2.0),
            z_range: (-2.0, 2.0),
            x_cells: 24,
            y_cells: 24,
            z_cells: 24,
        }
    }
}

/// The eight corners of a cell, as offsets. Bit `2` of the corner number is set
/// on `x`, bit `1` on `y` and bit `0` on `z`, so the number and the offset say
/// the same thing.
const CORNERS: [(usize, usize, usize); 8] = [
    (0, 0, 0),
    (1, 0, 0),
    (0, 1, 0),
    (1, 1, 0),
    (0, 0, 1),
    (1, 0, 1),
    (0, 1, 1),
    (1, 1, 1),
];

/// The twelve edges of a cell, each as the corners it joins.
const EDGES: [(usize, usize); 12] = [
    (0, 1),
    (2, 3),
    (4, 5),
    (6, 7),
    (0, 2),
    (1, 3),
    (4, 6),
    (5, 7),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
];

/// The four cells round an edge, as offsets on the two axes the edge does not
/// run along, in the order they go round it.
///
/// An edge along `+x` separates the cell behind it in `y` and `z` from the cell
/// beyond it in both, so those two are opposite each other in the ring.
/// Adjacent entries share a wall, which is what makes the ring a quad and not a
/// bow tie.
const RING: [(usize, usize); 4] = [(0, 1), (1, 1), (1, 0), (0, 0)];
/// The eight corners of a cell, as offsets. Bit `2` of the corner number is set
/// Whether a sample is inside. `0` is outside, so a crossing is always between
/// a strictly negative and a strictly positive sample.
fn inside(value: f64) -> bool {
    value < 0.0
}

/// Whether the pair of samples crosses, which needs both of them finite: a
/// non-finite sample is a break and not a side.
fn crosses(a: f64, b: f64) -> bool {
    a.is_finite() && b.is_finite() && inside(a) != inside(b)
}

/// A grid position from a range, a cell count and a corner index.
fn coordinate(range: (f64, f64), cells: usize, at: usize) -> f64 {
    range.0 + (range.1 - range.0) * at as f64 / cells as f64
}

/// Where the segment from `a` to `b` crosses zero, as a fraction of the way.
fn at_crossing(a: f64, b: f64) -> f64 {
    a / (a - b)
}

/// Mesh the surface `f(x, y, z) = 0`.
///
/// `expression` is `f` in terms of `x`, `y` and `z`, written so that it
/// vanishes on the surface and is negative inside it: `x² + y² + z² − 1` is the
/// unit ball, and its normal points outward.
///
/// The mesh is one vertex per cell with a crossing, and one quad per crossing
/// edge whose four cells can draw it; two triangles per quad. A grid that the
/// surface misses produces an empty mesh, which is an answer and not an error.
pub fn mesh_implicit(expression: &Expr, options: &ImplicitOptions) -> Result<Mesh, ImplicitError> {
    let x = SymbolId::intern("x");
    let y = SymbolId::intern("y");
    let z = SymbolId::intern("z");

    // A range that is not an interval has no cells, whatever its count says:
    // the step would be zero or negative and every corner would land on one
    // line.
    // `!(a > b)` and not `a <= b`: a `NaN` end compares false to everything, and
    // the negated form rejects it. The `allow` says why rather than hiding it.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    for range in [options.x_range, options.y_range, options.z_range] {
        if !(range.1 > range.0) || !(range.0.is_finite() && range.1.is_finite()) {
            return Err(ImplicitError::EmptyGrid);
        }
    }
    if options.x_cells == 0 || options.y_cells == 0 || options.z_cells == 0 {
        return Err(ImplicitError::EmptyGrid);
    }

    let program =
        canlab_core::Program::compile(expression, &[x, y, z]).ok_or(ImplicitError::NoEvaluation)?;
    let [dx, dy, dz] = gradient(expression, [x, y, z])?;
    let normal_at = |point: Vec3| -> Option<Vec3> {
        Vec3::new(
            dx.eval(&[point.x, point.y, point.z]),
            dy.eval(&[point.x, point.y, point.z]),
            dz.eval(&[point.x, point.y, point.z]),
        )
        .normalized()
    };

    let ranges = [options.x_range, options.y_range, options.z_range];
    let counts = [options.x_cells, options.y_cells, options.z_cells];
    // Corners per axis, and the array strides of the corner grid.
    let corners = [counts[0] + 1, counts[1] + 1, counts[2] + 1];
    let position = |corner: [usize; 3]| {
        Vec3::new(
            coordinate(ranges[0], counts[0], corner[0]),
            coordinate(ranges[1], counts[1], corner[1]),
            coordinate(ranges[2], counts[2], corner[2]),
        )
    };
    let corner_index =
        |corner: [usize; 3]| (corner[0] * corners[1] + corner[1]) * corners[2] + corner[2];
    let cell_index = |cell: [usize; 3]| (cell[0] * counts[1] + cell[1]) * counts[2] + cell[2];

    // The samples, corner by corner.
    let mut values = vec![f64::NAN; corners[0] * corners[1] * corners[2]];
    for i in 0..corners[0] {
        for j in 0..corners[1] {
            for k in 0..corners[2] {
                let point = position([i, j, k]);
                values[corner_index([i, j, k])] = program.eval(&[point.x, point.y, point.z]);
            }
        }
    }

    // A cell's eight samples, or `None` when any is not finite. A cell with a
    // non-finite corner is a hole: it is neither inside nor outside, so it
    // crosses nothing and is drawn by nothing.
    let cube = |cell: [usize; 3]| -> Option<[f64; 8]> {
        let mut samples = [0.0; 8];
        for (n, slot) in samples.iter_mut().enumerate() {
            let (di, dj, dk) = CORNERS[n];
            *slot = values[corner_index([cell[0] + di, cell[1] + dj, cell[2] + dk])];
            if !slot.is_finite() {
                return None;
            }
        }
        Some(samples)
    };

    // The vertex of a cell: the mean of the crossings on its own twelve edges,
    // or `None` when it has none, has a non-finite corner, or has no normal at
    // the mean. There is one per cell, which is what makes the net a net: two
    // quads round one vertex share it instead of drawing two coincident
    // vertices and an edge that is not there.
    let cell_count = counts[0] * counts[1] * counts[2];
    let mut placed: Vec<Option<usize>> = vec![None; cell_count];
    let mut mesh = Mesh::default();
    let vertex_of = |cell: [usize; 3], mesh: &mut Mesh, placed: &mut [Option<usize>]| {
        let slot = cell_index(cell);
        if let Some(index) = placed[slot] {
            return Some(index);
        }
        let samples = cube(cell)?;
        let mut sum = Vec3::ZERO;
        let mut number = 0.0;
        for (a, b) in EDGES {
            let (sa, sb) = (samples[a], samples[b]);
            if !crosses(sa, sb) {
                continue;
            }
            let (da, db) = (CORNERS[a], CORNERS[b]);
            let from = position([cell[0] + da.0, cell[1] + da.1, cell[2] + da.2]);
            let to = position([cell[0] + db.0, cell[1] + db.1, cell[2] + db.2]);
            sum = sum.add(from.add(to.sub(from).scale(at_crossing(sa, sb))));
            number += 1.0;
        }
        if number == 0.0 {
            return None;
        }
        let point = sum.scale(1.0 / number);
        // The one thing this module promises never to emit is a non-finite
        // vertex, so it is checked rather than assumed.
        if !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite() {
            return None;
        }
        let normal = normal_at(point)?;
        let index = mesh.vertices.len();
        mesh.vertices.push(Vertex {
            position: point,
            normal,
            shade: 0.5,
        });
        placed[slot] = Some(index);
        Some(index)
    };

    // One quad per crossing edge whose four cells can all draw it. The edge is
    // named by the corner it starts at — the lower of its two — and its axis.
    for i in 0..corners[0] {
        for j in 0..corners[1] {
            for k in 0..corners[2] {
                for axis in 0..3 {
                    let corner = [i, j, k];
                    if corner[axis] + 1 >= corners[axis] {
                        continue;
                    }
                    let mut far = corner;
                    far[axis] += 1;
                    let (sa, sb) = (values[corner_index(corner)], values[corner_index(far)]);
                    // A non-finite sample is a break and not a side, so neither
                    // of the two crossings the four cells share is a crossing.
                    if !crosses(sa, sb) {
                        continue;
                    }
                    // The cell the crossing is in: the upper cell when the
                    // crossing is in the upper half of the edge. It is the cell
                    // on the side of the crossing the quad is drawn from, so its
                    // vertex — the mean of its own crossings — has the crossing
                    // among the points it averages. The other three cells of the
                    // quad are named from it, and the choice depends only on the
                    // two samples, so both directions of the same edge agree.
                    let mut holder = corner;
                    // The upper cell only exists when the crossing is inside the
                    // grid: at the last corner there is nothing above it, so the
                    // cell it is in is the one below. That case is on the
                    // boundary and draws nothing, so the two rules do not
                    // disagree about any quad that is drawn.
                    if at_crossing(sa, sb) >= 0.5 && far[axis] < counts[axis] {
                        holder[axis] += 1;
                    }
                    // The edge's other two axes, always in increasing order so
                    // that `RING`'s cycle turns the same way for every axis.
                    let (low, high) = ((axis + 1) % 3, (axis + 2) % 3);
                    let (second, third) = (low.min(high), low.max(high));
                    // A quad needs four cells, so the edge's other two axes must
                    // have a cell on each side of them.
                    if corner[second] == 0
                        || corner[third] == 0
                        || corner[second] + 1 >= corners[second]
                        || corner[third] + 1 >= corners[third]
                    {
                        continue;
                    }
                    let mut quad = [0usize; 4];
                    let mut drawn = true;
                    for (slot, &(away, down)) in RING.iter().enumerate() {
                        // The four cells round the edge, in the ring's order.
                        let mut cell = holder;
                        cell[second] -= usize::from(away == 0);
                        cell[third] -= usize::from(down == 0);
                        // A cell outside the grid, or one with a non-finite
                        // corner, is a break: the quad stops.
                        if cell.iter().zip(counts).any(|(at, count)| *at >= count) {
                            drawn = false;
                            break;
                        }
                        if cube([cell[0], cell[1], cell[2]]).is_none() {
                            drawn = false;
                            break;
                        }
                        match vertex_of(cell, &mut mesh, &mut placed) {
                            Some(vertex) => quad[slot] = vertex,
                            None => {
                                drawn = false;
                                break;
                            }
                        }
                    }
                    if !drawn {
                        continue;
                    }
                    // The ring goes round the edge once, so `(0, 1, 2)` and
                    // `(0, 2, 3)` split the quad along its diagonal.
                    mesh.triangles.push([quad[0], quad[1], quad[2]]);
                    mesh.triangles.push([quad[0], quad[2], quad[3]]);
                }
            }
        }
    }

    // The shade runs with height, as W8.2's does, over the mesh's own range.
    let mut bottom = f64::INFINITY;
    let mut top = f64::NEG_INFINITY;
    for vertex in &mesh.vertices {
        bottom = bottom.min(vertex.position.y);
        top = top.max(vertex.position.y);
    }
    let span = top - bottom;
    for vertex in &mut mesh.vertices {
        vertex.shade = if span > 0.0 {
            ((vertex.position.y - bottom) / span).clamp(0.0, 1.0)
        } else {
            0.5
        };
    }
    Ok(mesh)
}

/// Compile the three partial derivatives, refusing when one is missing or does
/// not compile.
fn gradient(
    expression: &Expr,
    variables: [SymbolId; 3],
) -> Result<[canlab_core::Program; 3], ImplicitError> {
    let mut programs = [None, None, None];
    for (slot, variable) in programs.iter_mut().zip(variables) {
        let derivative =
            canlab_core::differentiate(expression, variable).ok_or(ImplicitError::NoDerivative)?;
        *slot = Some(
            canlab_core::Program::compile(&derivative, &variables)
                .ok_or(ImplicitError::NoDerivative)?,
        );
    }
    let [Some(a), Some(b), Some(c)] = programs else {
        return Err(ImplicitError::NoDerivative);
    };
    Ok([a, b, c])
}

#[cfg(test)]
mod tests {
    use super::*;
    use canlab_core::{add, int, mul, pow, sym};

    /// `x² + y² + z² − 1`, negative inside: the unit ball, outward-positive.
    fn unit_sphere() -> Expr {
        let square = |name: &str| pow(sym(name), int(2));
        add([square("x"), square("y"), square("z"), int(-1)])
    }

    fn options() -> ImplicitOptions {
        ImplicitOptions {
            x_range: (-1.5, 1.5),
            y_range: (-1.5, 1.5),
            z_range: (-1.5, 1.5),
            x_cells: 12,
            y_cells: 12,
            z_cells: 12,
        }
    }

    /// The one-cell step of [`options`].
    fn step() -> f64 {
        3.0 / 12.0
    }

    #[test]
    fn a_sphere_lies_within_one_cell_of_the_true_surface() {
        // The roadmap's first case. A vertex is a mean of crossings on one
        // cell's edges, so the surface through it has a point within a cell of
        // it: no vertex is a whole cell away from the sphere.
        let mesh = mesh_implicit(&unit_sphere(), &options()).expect("a mesh");
        assert!(mesh.vertex_count() > 0, "the sphere produced no vertices");
        assert!(mesh.triangle_count() > 0, "the sphere produced no triangles");
        for vertex in &mesh.vertices {
            let radius = vertex.position.length();
            assert!(
                (radius - 1.0).abs() <= step(),
                "a vertex at {radius} is more than one cell (step {}) off the surface",
                step()
            );
        }
    }

    #[test]
    fn a_box_around_the_sphere_is_drawn_in_the_volume() {
        // The same sphere in a box that it does not reach: every vertex is
        // still within a cell of it, and the mesh covers more of the surface —
        // the wall does not add vertices that are off the sphere.
        let wide = ImplicitOptions {
            x_range: (-3.0, 3.0),
            y_range: (-3.0, 3.0),
            z_range: (-3.0, 3.0),
            x_cells: 24,
            y_cells: 24,
            z_cells: 24,
        };
        let mesh = mesh_implicit(&unit_sphere(), &wide).expect("a mesh");
        assert!(mesh.is_finite());
        let ceiling = 6.0 / 24.0;
        for vertex in &mesh.vertices {
            assert!(
                (vertex.position.length() - 1.0).abs() <= ceiling,
                "{:?}",
                vertex.position
            );
        }
    }

    #[test]
    fn the_spheres_normals_follow_the_analytic_gradient_and_its_sign() {
        // `f = x² + y² + z² − 1` has the gradient `(2x, 2y, 2z)`, so the unit
        // normal at a vertex is its own unit position.
        let mesh = mesh_implicit(&unit_sphere(), &options()).expect("a mesh");
        assert!(mesh.is_finite());
        for vertex in &mesh.vertices {
            let outward = vertex.position.normalized().expect("a radius");
            assert!(
                vertex.normal.dot(outward) > 1.0 - 1e-9,
                "the normal does not point outward at {:?}",
                vertex.position
            );
        }
        // The same surface with the other sign has the opposite normals,
        // because the sign of `f` says which side is inside.
        let inward = mesh_implicit(&mul([int(-1), unit_sphere()]), &options()).expect("a mesh");
        assert!(inward.is_finite());
        for vertex in &inward.vertices {
            let outward = vertex.position.normalized().expect("a radius");
            assert!(
                vertex.normal.dot(outward) < -1.0 + 1e-9,
                "the sign of f did not reach the normal at {:?}",
                vertex.position
            );
        }
    }

    #[test]
    fn an_empty_region_produces_no_triangles() {
        // The roadmap's second case: `x² + y² + z² − 100` never vanishes on a
        // grid that fits inside the sphere it describes. Nothing is drawn, and
        // nothing is refused: a grid that misses the surface is an answer.
        let far = add([
            pow(sym("x"), int(2)),
            pow(sym("y"), int(2)),
            pow(sym("z"), int(2)),
            int(-100),
        ]);
        let mesh = mesh_implicit(&far, &options()).expect("an empty mesh");
        assert_eq!(mesh.triangle_count(), 0);
        assert_eq!(mesh.vertex_count(), 0);
        // A grid wholly outside the sphere is empty for the same reason, and
        // so is one wholly inside it.
        for range in [(2.0, 3.0), (-0.5, 0.5)] {
            let away = ImplicitOptions {
                x_range: range,
                y_range: range,
                z_range: range,
                ..options()
            };
            let mesh = mesh_implicit(&unit_sphere(), &away).expect("an empty mesh");
            assert_eq!(mesh.triangle_count(), 0, "{range:?}");
        }
    }

    #[test]
    fn a_non_finite_region_is_a_hole_and_never_a_nan_vertex() {
        // `1/(xyz)` is not finite on any of the three axis planes. The cells
        // that touch them are dropped, so the axes are holes rather than walls,
        // and every vertex the mesh keeps is finite.
        let pole = pow(mul([sym("x"), sym("y"), sym("z")]), int(-1));
        // The box is off the grid planes so that no corner sits on an axis.
        let options = ImplicitOptions {
            x_range: (-1.5 + 0.125, 1.5 + 0.125),
            ..options()
        };
        let mesh = mesh_implicit(&pole, &options).expect("a mesh");
        assert!(mesh.is_finite(), "a non-finite vertex survived");
        for vertex in &mesh.vertices {
            let nearest = vertex
                .position
                .x
                .abs()
                .min(vertex.position.y.abs())
                .min(vertex.position.z.abs());
            assert!(
                nearest > 0.0,
                "a vertex sits on an axis plane: {:?}",
                vertex.position
            );
        }
    }

    #[test]
    fn a_zero_corner_is_outside_and_never_divides_by_zero() {
        // A corner exactly on the surface is outside, so a crossing is always
        // between values of different sign. `x` on a grid whose corners include
        // `x = 0` is that case: the corners on the plane are outside, and the
        // crossing either side of them is between `0` and `−step`.
        let through = ImplicitOptions {
            x_range: (-1.0, 1.0),
            y_range: (-1.0, 1.0),
            z_range: (-1.0, 1.0),
            x_cells: 4,
            y_cells: 4,
            z_cells: 4,
        };
        let mesh = mesh_implicit(&sym("x"), &through).expect("a mesh");
        assert!(mesh.is_finite(), "a zero corner produced a non-finite vertex");
        for vertex in &mesh.vertices {
            // The crossing between `0` and `−0.5` is at `−0.25`, and no cell on
            // the plane has a crossing on its far edge, so a vertex within a
            // cell of the plane is at the crossing or closer.
            assert!(
                (-0.5..=0.0).contains(&vertex.position.x),
                "a vertex is at {:?}, not within one cell of the plane",
                vertex.position
            );
            // `∇x = (1, 0, 0)`, whichever side of the plane the vertex is on.
            assert!(
                (vertex.normal.x - 1.0).abs() < 1e-12,
                "the plane's normal is {:?}",
                vertex.normal
            );
        }
    }

    #[test]
    fn an_expression_without_a_value_or_a_derivative_is_refused() {
        // A `Hold` neither evaluates nor differentiates, and the first refusal
        // is the one that says why.
        let hold = canlab_core::ExprNode::hold(canlab_core::expr::Hold::Integral {
            body: sym("x"),
            var: SymbolId::intern("x"),
        });
        let error = mesh_implicit(&hold, &options()).expect_err("a refusal");
        assert_eq!(error, ImplicitError::NoEvaluation);
        assert!(
            error.message().contains("cannot be evaluated"),
            "{}",
            error.message()
        );

        // `asin` evaluates but has no differentiation rule here, so it has a
        // value and no normal — a different refusal with a different reason.
        let arcsin = canlab_core::ExprNode::fn_raw(SymbolId::intern("asin"), vec![sym("x")]);
        assert!(
            canlab_core::Program::compile(&arcsin, &[SymbolId::intern("x")]).is_some(),
            "asin must evaluate for this test to be about the derivative"
        );
        let error = mesh_implicit(&arcsin, &options()).expect_err("a refusal");
        assert_eq!(error, ImplicitError::NoDerivative);
        assert!(
            error.message().contains("no normals"),
            "{}",
            error.message()
        );
    }

    #[test]
    fn a_grid_with_no_cells_or_no_interval_is_refused() {
        for empty in [
            ImplicitOptions {
                x_cells: 0,
                ..options()
            },
            ImplicitOptions {
                z_cells: 0,
                ..options()
            },
        ] {
            let error = mesh_implicit(&unit_sphere(), &empty).expect_err("a refusal");
            assert_eq!(error, ImplicitError::EmptyGrid);
            assert!(error.message().contains("no cells"), "{}", error.message());
        }
        // A range that is not an interval has no cells either, whatever its
        // count says.
        for bad in [(-1.0, -1.0), (1.0, -1.0), (f64::NAN, 1.0)] {
            let options = ImplicitOptions {
                y_range: bad,
                ..options()
            };
            let error = mesh_implicit(&unit_sphere(), &options).expect_err("a refusal");
            assert_eq!(error, ImplicitError::EmptyGrid, "{bad:?}");
            assert!(error.message().contains("no cells"), "{bad:?}");
        }
    }
}
