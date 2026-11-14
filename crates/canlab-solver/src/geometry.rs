//! W4.3: vectors and the geometry an exam asks about.
//!
//! A vector is a list of [`Expr`]s — exact components, so `(1, 2, 2)` has norm
//! `3` and not `3.0000000000000004`. Every operation here is the CAS's own
//! arithmetic on those components; nothing is evaluated to an `f64` unless the
//! question genuinely has no exact answer (an angle that is not a rational
//! multiple of `π`), and even then the answer is a *name* (`\arccos(1/3)`) and
//! not a decimal.
//!
//! # What is here, and why each one
//!
//! * **dot, cross, norm** — the three products every later formula is built
//!   from. The cross product is the determinant form, so its sign convention is
//!   the one a textbook uses and a test can check against a right-handed frame.
//! * **angle** — `\arccos(u·v / (|u||v|))`. Exact when the cosine is a value the
//!   CAS can name (`0`, `1/2`, `√2/2`), and a name otherwise.
//! * **projection** — `(u·v / v·v)·v`, the vector component of `u` along `v`.
//! * **line and plane** — a point and a direction, a point and a normal. Both
//!   are the *equations* (`r = a + t·d`, `n·(r − a) = 0`) reduced to the data a
//!   caller needs, because a caller that wants the equation can write it from
//!   the data and a caller that wants the data should not have to parse it.
//! * **distances** — from a point to a line (`|(p − a) × d| / |d|`) and to a
//!   plane (`|n·(p − a)| / |n|`).
//! * **two planes** — their intersection is a line, or empty when they are
//!   parallel and distinct, or a plane when they coincide. All three are
//!   answers, and the type says which.
//!
//! # What is refused
//!
//! A vector of the wrong length (`dot` of a 2-vector and a 3-vector), a zero
//! direction (`cross` and `angle` need one), and a line and a plane that do not
//! meet — each with a message that names the mismatch. A refusal is an answer;
//! a wrong number is not.

use canlab_core::expr::{Expr, ExprNode};
use canlab_core::number::Number;
use canlab_core::symbol::SymbolId;

/// Why a geometry question has no answer.
#[derive(Debug, Clone, PartialEq)]
pub enum GeometryError {
    /// Two vectors of different lengths.
    DimensionMismatch {
        /// The first vector's length.
        left: usize,
        /// The second's.
        right: usize,
    },
    /// A direction or a normal that is the zero vector.
    ZeroVector,
    /// A vector with no components.
    Empty,
}

impl GeometryError {
    /// The message a caller shows.
    pub fn message(&self) -> String {
        match self {
            GeometryError::DimensionMismatch { left, right } => format!(
                "a {left}-component vector and a {right}-component one cannot be combined"
            ),
            GeometryError::ZeroVector => {
                "the direction is the zero vector, which has no direction".to_string()
            }
            GeometryError::Empty => "a vector needs at least one component".to_string(),
        }
    }
}

/// `u·v`.
pub fn dot(u: &[Expr], v: &[Expr]) -> Result<Expr, GeometryError> {
    same_length(u, v)?;
    let products: Vec<Expr> = u
        .iter()
        .zip(v)
        .map(|(a, b)| canlab_core::mul([a.clone(), b.clone()]))
        .collect();
    Ok(canlab_core::add(products))
}

/// `u × v`, for three-component vectors.
pub fn cross(u: &[Expr], v: &[Expr]) -> Result<Vec<Expr>, GeometryError> {
    same_length(u, v)?;
    if u.len() != 3 {
        return Err(GeometryError::DimensionMismatch {
            left: u.len(),
            right: 3,
        });
    }
    let component = |a: &Expr, b: &Expr, c: &Expr, d: &Expr| {
        canlab_core::add([
            canlab_core::mul([a.clone(), b.clone()]),
            canlab_core::mul([ExprNode::int(-1), canlab_core::mul([c.clone(), d.clone()])]),
        ])
    };
    Ok(vec![
        component(&u[1], &v[2], &u[2], &v[1]),
        component(&u[2], &v[0], &u[0], &v[2]),
        component(&u[0], &v[1], &u[1], &v[0]),
    ])
}

/// `|u|` as an exact expression: `√(u·u)`, simplified where it is a perfect
/// square.
pub fn norm(u: &[Expr]) -> Result<Expr, GeometryError> {
    if u.is_empty() {
        return Err(GeometryError::Empty);
    }
    let squared = dot(u, u)?;
    if let Some(root) = squared
        .as_number()
        .and_then(|number| number.exact_nth_root(2))
    {
        return Ok(ExprNode::num(root));
    }
    Ok(canlab_core::pow(squared, ExprNode::num(Number::ratio(1, 2))))
}

/// The angle between `u` and `v`, in radians.
///
/// `\arccos(u·v / (|u||v|))`. Exact when the cosine is a value the CAS can name
/// — `π/2`, `π/3`, `π/4`, `0`, `π` — and a name otherwise. A zero vector has no
/// angle to give, and is refused.
pub fn angle(u: &[Expr], v: &[Expr]) -> Result<Expr, GeometryError> {
    let product = dot(u, v)?;
    let lengths = canlab_core::mul([norm(u)?, norm(v)?]);
    if lengths.as_number().is_some_and(Number::is_zero) {
        return Err(GeometryError::ZeroVector);
    }
    let cosine = canlab_core::mul([
        product,
        canlab_core::pow(lengths, ExprNode::int(-1)),
    ]);
    let cosine = canlab_core::rewrite::simplify(&cosine);
    let pi = || ExprNode::sym(SymbolId::intern("pi"));
    if let Some(number) = cosine.as_number() {
        // The school angles, by their cosines.
        let quarter = || canlab_core::mul([ExprNode::num(Number::ratio(1, 2)), pi()]);
        let sixth = || canlab_core::mul([ExprNode::num(Number::ratio(1, 6)), pi()]);
        if number.is_zero() {
            return Ok(quarter());
        }
        if number.is_one() {
            return Ok(ExprNode::int(0));
        }
        if number.to_f64() == -1.0 {
            return Ok(pi());
        }
        if number.to_f64() == 0.5 {
            return Ok(sixth());
        }
        if number.to_f64() == -0.5 {
            return Ok(canlab_core::mul([ExprNode::num(Number::ratio(2, 3)), pi()]));
        }
        // `1/√2`, which only appears as `√2/2` — the CAS keeps it as a power.
    }
    if let Some(angle) = right_angle_from_surd(&cosine) {
        return Ok(angle);
    }
    Ok(ExprNode::fn_raw(
        SymbolId::intern("arccos"),
        vec![cosine],
    ))
}

/// `π/4` or `3π/4` when the cosine is `±√2/2`, which the CAS writes as
/// `(1/2)·2^(1/2)`.
fn right_angle_from_surd(cosine: &Expr) -> Option<Expr> {
    // The CAS writes `1/√2` in two shapes, and both mean the same angle:
    // `(1/2)·2^(1/2)` and `(2^(1/2))^(-1)`. The first version recognised only
    // the first and answered `arccos(1/√2)` for a 45° angle it could name.
    if let ExprNode::Pow(base, exponent) = &**cosine {
        // `(2^(1/2))^(-1)`, which is `1/√2`, and `2^(1/2)`, which is `√2` and
        // is not a cosine at all — only the first is the 45° angle.
        let exponent_value = exponent.as_number().map(Number::to_f64);
        let base_is_root_two = matches!(&**base, ExprNode::Pow(inner, inner_exponent)
            if inner.as_number().is_some_and(|n| n.to_f64() == 2.0)
                && inner_exponent.as_number().is_some_and(|n| n.to_f64() == 0.5));
        let base_is_two = base.as_number().is_some_and(|n| n.to_f64() == 2.0);
        let is_one_over_root_two = (base_is_root_two
            && exponent_value.is_some_and(|e| e == -1.0))
            || (base_is_two && exponent_value.is_some_and(|e| e == -0.5));
        if is_one_over_root_two {
            let pi = || ExprNode::sym(SymbolId::intern("pi"));
            return Some(canlab_core::mul([ExprNode::num(Number::ratio(1, 4)), pi()]));
        }
    }
    let ExprNode::Mul(factors) = &**cosine else {
        return None;
    };
    let mut coefficient = Number::int(1);
    let mut root = None;
    for factor in factors {
        if let Some(number) = factor.as_number() {
            coefficient = coefficient.mul(number);
        } else if let ExprNode::Pow(base, exponent) = &**factor {
            if base.as_number().is_some_and(|n| n.to_f64() == 2.0)
                && exponent.as_number().is_some_and(|n| n.to_f64() == 0.5)
            {
                root = Some(());
            } else {
                return None;
            }
        } else {
            return None;
        }
    }
    root?;
    let half = Number::ratio(1, 2);
    let pi = || ExprNode::sym(SymbolId::intern("pi"));
    let quarter = canlab_core::mul([ExprNode::num(Number::ratio(1, 4)), pi()]);
    if coefficient == half {
        Some(quarter)
    } else if coefficient == half.neg() {
        Some(canlab_core::mul([ExprNode::int(3), quarter]))
    } else {
        None
    }
}

/// The vector component of `u` along `v`: `(u·v / v·v)·v`.
pub fn projection(u: &[Expr], v: &[Expr]) -> Result<Vec<Expr>, GeometryError> {
    let product = dot(u, v)?;
    let squared = dot(v, v)?;
    if squared.as_number().is_some_and(Number::is_zero) {
        return Err(GeometryError::ZeroVector);
    }
    let scale = canlab_core::mul([product, canlab_core::pow(squared, ExprNode::int(-1))]);
    Ok(v
        .iter()
        .map(|component| canlab_core::mul([scale.clone(), component.clone()]))
        .collect())
}

/// A line: a point and a direction.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// A point on the line.
    pub point: Vec<Expr>,
    /// The direction.
    pub direction: Vec<Expr>,
}

/// A plane: a point and a normal.
#[derive(Debug, Clone, PartialEq)]
pub struct Plane {
    /// A point on the plane.
    pub point: Vec<Expr>,
    /// The normal.
    pub normal: Vec<Expr>,
}

/// The distance from `point` to `line`.
pub fn distance_point_line(point: &[Expr], line: &Line) -> Result<Expr, GeometryError> {
    same_length(point, &line.point)?;
    same_length(point, &line.direction)?;
    if line.direction.iter().all(|c| c.as_number().is_some_and(Number::is_zero)) {
        return Err(GeometryError::ZeroVector);
    }
    let offset: Vec<Expr> = point
        .iter()
        .zip(&line.point)
        .map(|(p, a)| canlab_core::add([
            p.clone(),
            canlab_core::mul([ExprNode::int(-1), a.clone()]),
        ]))
        .collect();
    let perpendicular = cross(&offset, &line.direction)?;
    let numerator = norm(&perpendicular)?;
    let denominator = norm(&line.direction)?;
    Ok(canlab_core::rewrite::simplify(&canlab_core::mul([
        numerator,
        canlab_core::pow(denominator, ExprNode::int(-1)),
    ])))
}

/// The distance from `point` to `plane`.
pub fn distance_point_plane(point: &[Expr], plane: &Plane) -> Result<Expr, GeometryError> {
    same_length(point, &plane.point)?;
    same_length(point, &plane.normal)?;
    if plane.normal.iter().all(|c| c.as_number().is_some_and(Number::is_zero)) {
        return Err(GeometryError::ZeroVector);
    }
    let offset: Vec<Expr> = point
        .iter()
        .zip(&plane.point)
        .map(|(p, a)| canlab_core::add([
            p.clone(),
            canlab_core::mul([ExprNode::int(-1), a.clone()]),
        ]))
        .collect();
    let numerator = dot(&plane.normal, &offset)?;
    let denominator = norm(&plane.normal)?;
    // The absolute value: a distance is not signed, and `|n·(p−a)|/|n|` is the
    // formula the textbook writes.
    let magnitude = absolute_of(numerator);
    Ok(canlab_core::rewrite::simplify(&canlab_core::mul([
        magnitude,
        canlab_core::pow(denominator, ExprNode::int(-1)),
    ])))
}

/// `|e|` as an expression: the number's magnitude when `e` is a number, and
/// `abs(e)` otherwise.
fn absolute_of(expression: Expr) -> Expr {
    if let Some(number) = expression.as_number() {
        return ExprNode::num(number.absolute());
    }
    ExprNode::fn_raw(SymbolId::intern("abs"), vec![expression])
}

/// Where two planes meet.
#[derive(Debug, Clone, PartialEq)]
pub enum PlaneIntersection {
    /// One line.
    Line(Line),
    /// No point is on both: the planes are parallel and distinct.
    None,
    /// Every point of one is on the other: the planes coincide.
    Coincident,
}

/// The intersection of two planes.
///
/// The direction is `n₁ × n₂`, which is perpendicular to both normals and
/// therefore lies in both planes. A point on the line is found by choosing the
/// coordinate with the largest cross-product component and solving the two
/// equations for the other two — the standard construction, and the one that
/// stays well-conditioned because the largest component is the one that is not
/// near zero.
pub fn plane_intersection(first: &Plane, second: &Plane) -> Result<PlaneIntersection, GeometryError> {
    same_length(&first.normal, &second.normal)?;
    if first.normal.len() != 3 {
        return Err(GeometryError::DimensionMismatch {
            left: first.normal.len(),
            right: 3,
        });
    }
    let direction = cross(&first.normal, &second.normal)?;
    let direction_is_zero = direction
        .iter()
        .all(|c| c.as_number().is_some_and(Number::is_zero));
    if direction_is_zero {
        // Parallel normals: the planes are the same or they never meet. They
        // are the same when the first plane's point is on the second.
        let offset: Vec<Expr> = first
            .point
            .iter()
            .zip(&second.point)
            .map(|(p, a)| canlab_core::add([
                p.clone(),
                canlab_core::mul([ExprNode::int(-1), a.clone()]),
            ]))
            .collect();
        let on_second = dot(&second.normal, &offset)?;
        return Ok(if on_second.as_number().is_some_and(Number::is_zero) {
            PlaneIntersection::Coincident
        } else {
            PlaneIntersection::None
        });
    }
    // The point: the two plane equations `n·r = n·a` solved with the largest
    // cross-product component set to zero.
    let magnitudes: Vec<f64> = direction
        .iter()
        .map(|c| c.as_number().map(Number::to_f64).map(f64::abs).unwrap_or(0.0))
        .collect();
    let largest = magnitudes
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(index, _)| index)
        .unwrap_or(0);
    let (i, j) = match largest {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let d1 = dot(&first.normal, &first.point)?;
    let d2 = dot(&second.normal, &second.point)?;
    // Solve the 2×2 system in the two free coordinates:
    //   n1[i]·x_i + n1[j]·x_j = d1
    //   n2[i]·x_i + n2[j]·x_j = d2
    let a = first.normal[i].clone();
    let b = first.normal[j].clone();
    let c = second.normal[i].clone();
    let d = second.normal[j].clone();
    let determinant = canlab_core::add([
        canlab_core::mul([a.clone(), d.clone()]),
        canlab_core::mul([ExprNode::int(-1), canlab_core::mul([b.clone(), c.clone()])]),
    ]);
    if determinant.as_number().is_some_and(Number::is_zero) {
        // The determinant is the largest cross-product component, so this can
        // only happen when that component is zero — which the check above
        // already excluded. Refusing rather than dividing by zero.
        return Err(GeometryError::ZeroVector);
    }
    let inverse = canlab_core::pow(determinant, ExprNode::int(-1));
    let x_i = canlab_core::mul([
        canlab_core::add([
            canlab_core::mul([d1.clone(), d]),
            canlab_core::mul([ExprNode::int(-1), canlab_core::mul([b, d2.clone()])]),
        ]),
        inverse.clone(),
    ]);
    let x_j = canlab_core::mul([
        canlab_core::add([
            canlab_core::mul([a, d2]),
            canlab_core::mul([ExprNode::int(-1), canlab_core::mul([c, d1])]),
        ]),
        inverse,
    ]);
    let mut point = vec![ExprNode::int(0); 3];
    point[i] = canlab_core::rewrite::simplify(&x_i);
    point[j] = canlab_core::rewrite::simplify(&x_j);
    Ok(PlaneIntersection::Line(Line {
        point,
        direction,
    }))
}

fn same_length(left: &[Expr], right: &[Expr]) -> Result<(), GeometryError> {
    if left.len() != right.len() {
        return Err(GeometryError::DimensionMismatch {
            left: left.len(),
            right: right.len(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use canlab_core::{int, sym};

    fn v3(a: i64, b: i64, c: i64) -> Vec<Expr> {
        vec![int(a), int(b), int(c)]
    }

    #[test]
    fn a_right_triangle_has_the_angles_it_should() {
        // The roadmap's check. `(1,0,0)` and `(1,1,0)` are at 45°; `(1,0,0)`
        // and `(0,1,0)` are at 90°; `(1,0,0)` and `(-1,0,0)` at 180°.
        let x = v3(1, 0, 0);
        let diagonal = v3(1, 1, 0);
        let y = v3(0, 1, 0);
        let back = v3(-1, 0, 0);
        let pi = || ExprNode::sym(SymbolId::intern("pi"));
        assert_eq!(
            angle(&x, &diagonal).expect("an angle").to_string(),
            format!("1/4*{}", pi())
        );
        assert_eq!(
            angle(&x, &y).expect("an angle").to_string(),
            format!("1/2*{}", pi())
        );
        assert_eq!(
            angle(&x, &back).expect("an angle").to_string(),
            format!("{}", pi())
        );
        // A 3-4-5 triangle: the angle between `(3,0)` and `(3,4)` has cosine
        // `3/5`, which is not a school angle and is named, not rounded.
        let a = vec![int(3), int(0)];
        let b = vec![int(3), int(4)];
        let named = angle(&a, &b).expect("an angle");
        assert!(named.to_string().contains("arccos"), "{named}");
    }

    #[test]
    fn the_products_are_the_ones_a_textbook_writes() {
        let x = v3(1, 0, 0);
        let y = v3(0, 1, 0);
        let z = v3(0, 0, 1);
        // `x × y = z`, which is the right-handed convention.
        assert_eq!(cross(&x, &y).expect("a cross product"), z);
        // `y × x = −z`: the cross product is antisymmetric.
        assert_eq!(
            cross(&y, &x).expect("a cross product"),
            vec![int(0), int(0), int(-1)]
        );
        // `u·v = |u||v|cos θ`, checked on the 3-4-5 pair.
        let a = vec![int(3), int(0)];
        let b = vec![int(3), int(4)];
        assert_eq!(dot(&a, &b).expect("a dot product"), int(9));
        assert_eq!(norm(&a).expect("a norm"), int(3));
        assert_eq!(norm(&b).expect("a norm"), int(5));
    }

    #[test]
    fn a_norm_is_exact_when_it_is_a_perfect_square_and_a_surd_otherwise() {
        assert_eq!(norm(&v3(1, 2, 2)).expect("a norm"), int(3));
        // `(1,1,1)` has norm `√3`, which is a surd and stays one.
        let surd = norm(&v3(1, 1, 1)).expect("a norm");
        assert!(surd.to_string().contains("3"), "{surd}");
        assert!(!surd.to_string().contains('.'), "a surd is not a decimal: {surd}");
    }

    #[test]
    fn a_projection_lands_on_the_line_it_projects_onto() {
        // `(1,2,0)` projected onto `(1,0,0)` is `(1,0,0)`.
        let u = v3(1, 2, 0);
        let v = v3(1, 0, 0);
        assert_eq!(projection(&u, &v).expect("a projection"), v3(1, 0, 0));
        // Onto `(1,1,0)`: the component along it is `(3/2, 3/2, 0)`.
        let diagonal = v3(1, 1, 0);
        let projected = projection(&u, &diagonal).expect("a projection");
        assert_eq!(projected[0].as_number(), Some(&Number::ratio(3, 2)));
        assert_eq!(projected[1].as_number(), Some(&Number::ratio(3, 2)));
        // The residual is perpendicular to the direction, which is what a
        // projection means.
        let residual: Vec<Expr> = u
            .iter()
            .zip(&projected)
            .map(|(a, b)| canlab_core::add([
                a.clone(),
                canlab_core::mul([ExprNode::int(-1), b.clone()]),
            ]))
            .collect();
        let product = dot(&residual, &diagonal).expect("a dot product");
        assert_eq!(product.as_number(), Some(&Number::int(0)), "{product}");
    }

    #[test]
    fn the_distances_are_the_ones_geometry_gives() {
        // The distance from `(0,0,0)` to the line `(1,0,0) + t·(0,1,0)` is 1.
        let line = Line {
            point: v3(1, 0, 0),
            direction: v3(0, 1, 0),
        };
        assert_eq!(
            distance_point_line(&v3(0, 0, 0), &line).expect("a distance"),
            int(1)
        );
        // The distance from `(0,0,3)` to the plane `z = 0` is 3.
        let plane = Plane {
            point: v3(0, 0, 0),
            normal: v3(0, 0, 1),
        };
        assert_eq!(
            distance_point_plane(&v3(0, 0, 3), &plane).expect("a distance"),
            int(3)
        );
        // And it is not signed: a point below the plane is the same distance.
        assert_eq!(
            distance_point_plane(&v3(0, 0, -3), &plane).expect("a distance"),
            int(3)
        );
    }

    #[test]
    fn two_planes_meet_in_a_line_miss_or_coincide() {
        // The roadmap's check: one pair that meets, one that misses.
        let horizontal = Plane {
            point: v3(0, 0, 0),
            normal: v3(0, 0, 1),
        };
        let vertical = Plane {
            point: v3(0, 0, 0),
            normal: v3(1, 0, 0),
        };
        match plane_intersection(&horizontal, &vertical).expect("an intersection") {
            PlaneIntersection::Line(line) => {
                // The line is the `y` axis: direction `(0,1,0)`, through the
                // origin.
                assert_eq!(line.direction, v3(0, 1, 0), "{:?}", line.direction);
                assert_eq!(line.point, v3(0, 0, 0), "{:?}", line.point);
            }
            other => panic!("expected a line, got {other:?}"),
        }
        // Parallel and distinct: `z = 0` and `z = 1`.
        let above = Plane {
            point: v3(0, 0, 1),
            normal: v3(0, 0, 1),
        };
        assert_eq!(
            plane_intersection(&horizontal, &above).expect("an answer"),
            PlaneIntersection::None
        );
        // The same plane twice.
        assert_eq!(
            plane_intersection(&horizontal, &horizontal).expect("an answer"),
            PlaneIntersection::Coincident
        );
    }

    #[test]
    fn a_mismatch_is_refused_with_both_lengths() {
        let two = vec![int(1), int(2)];
        let three = v3(1, 2, 3);
        let error = dot(&two, &three).expect_err("a mismatch");
        assert!(error.message().contains('2') && error.message().contains('3'));
        assert_eq!(
            cross(&two, &two),
            Err(GeometryError::DimensionMismatch { left: 2, right: 3 })
        );
        // A zero direction has no angle and no projection.
        let zero = v3(0, 0, 0);
        assert_eq!(angle(&three, &zero), Err(GeometryError::ZeroVector));
        assert_eq!(projection(&three, &zero), Err(GeometryError::ZeroVector));
    }

    #[test]
    fn a_symbolic_vector_keeps_its_symbols() {
        // The geometry is exact for symbolic components too: `(a, 0)` and
        // `(0, b)` are perpendicular whatever `a` and `b` are.
        let u = vec![sym("a"), int(0)];
        let v = vec![int(0), sym("b")];
        assert_eq!(dot(&u, &v).expect("a dot product").as_number(), Some(&Number::int(0)));
    }
}
