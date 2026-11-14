//! A 3×3 solve and inverse, with the singularity test the callers actually need.
//!
//! Both callers — the direction fit and the Cramér–Rao bound — ask the same
//! question of a 3×3 matrix: is it invertible, and if so what is the inverse.
//! The question is not "is the determinant zero", because a determinant is not
//! dimensionless: a matrix of millimetres squared has a determinant in
//! millimetres to the sixth, and comparing that to a fixed epsilon compares two
//! different units. The test here is the determinant against the product of the
//! row norms, which is the volume of the box the rows span divided by the volume
//! of the box they would span if they were orthogonal — a number between 0 and 1
//! whatever the units.
//!
//! That number is the right one to test because it is also the right one to
//! *report*: a fit whose rows are nearly dependent has a large inverse, and the
//! bound built from it is large, which is the honest answer rather than an
//! error. Only an exactly singular matrix is refused.

/// The determinant of a 3×3 matrix given by rows.
pub(crate) fn determinant(m: [[f64; 3]; 3]) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

/// The determinant divided by the product of the row norms: 0 when the rows are
/// dependent, 1 when they are orthogonal. `NaN` for a zero row.
pub(crate) fn conditioning(m: [[f64; 3]; 3]) -> f64 {
    let norm = |row: [f64; 3]| (row[0] * row[0] + row[1] * row[1] + row[2] * row[2]).sqrt();
    let scale = norm(m[0]) * norm(m[1]) * norm(m[2]);
    if scale == 0.0 {
        return f64::NAN;
    }
    determinant(m) / scale
}

/// How dependent three rows may be before the solve is refused.
///
/// A millionth of the orthogonal volume: three rows that are within a
/// microradian of a plane are refused, and three that are a thousandth of a
/// degree off it are solved and give a large answer. The threshold is a
/// *refusal* threshold, not an accuracy claim — the arithmetic of the solve is
/// exact to rounding either way.
pub(crate) const SINGULAR: f64 = 1e-12;

/// Solve `m·x = b`, or `None` when the rows are dependent.
pub(crate) fn solve(m: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let det = determinant(m);
    if !det.is_finite() || conditioning(m).abs() <= SINGULAR {
        return None;
    }
    // Cramer's rule, one determinant per unknown. Three 3×3 determinants is
    // cheaper than an elimination with pivoting for a matrix this size, and it
    // has no branch that can pick the wrong pivot.
    let column = |index: usize| {
        let mut replaced = m;
        for row in 0..3 {
            replaced[row][index] = b[row];
        }
        determinant(replaced)
    };
    Some([column(0) / det, column(1) / det, column(2) / det])
}

/// The inverse of a 3×3 matrix, or `None` when the rows are dependent.
pub(crate) fn inverse(m: [[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let det = determinant(m);
    if !det.is_finite() || conditioning(m).abs() <= SINGULAR {
        return None;
    }
    let cofactor = |row: usize, column: usize| {
        let rows: Vec<usize> = (0..3).filter(|r| *r != row).collect();
        let columns: Vec<usize> = (0..3).filter(|c| *c != column).collect();
        let minor = m[rows[0]][columns[0]] * m[rows[1]][columns[1]]
            - m[rows[0]][columns[1]] * m[rows[1]][columns[0]];
        // The adjugate is the cofactor matrix transposed, and the sign is
        // `(−1)^(row+column)`.
        if (row + column).is_multiple_of(2) { minor } else { -minor }
    };
    let mut result = [[0.0f64; 3]; 3];
    // The indices are the algorithm: entry (row, column) is the cofactor of
    // (column, row), and an iterator would have to carry both to say that.
    #[allow(clippy::needless_range_loop)]
    for row in 0..3 {
        for column in 0..3 {
            // Transposed: entry (row, column) of the inverse is the cofactor of
            // (column, row) divided by the determinant.
            result[row][column] = cofactor(column, row) / det;
        }
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_solve_is_the_answer_it_claims() {
        // A matrix with a known answer: the identity, and a rotation-free
        // diagonal, and one with off-diagonal terms checked by substitution.
        let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        assert_eq!(solve(identity, [1.0, 2.0, 3.0]), Some([1.0, 2.0, 3.0]));
        let m = [[2.0, 1.0, 0.0], [1.0, 3.0, 1.0], [0.0, 1.0, 2.0]];
        let b = [1.0, 2.0, 3.0];
        let x = solve(m, b).expect("invertible");
        for row in 0..3 {
            let sum = m[row][0] * x[0] + m[row][1] * x[1] + m[row][2] * x[2];
            assert!((sum - b[row]).abs() < 1e-12, "row {row}: {sum} vs {}", b[row]);
        }
    }

    #[test]
    #[allow(clippy::needless_range_loop)]
    fn the_inverse_is_the_inverse() {
        let m = [[2.0, 1.0, 0.5], [1.0, 3.0, 1.0], [0.5, 1.0, 2.0]];
        let inverse = inverse(m).expect("invertible");
        for row in 0..3 {
            for column in 0..3 {
                let mut sum = 0.0;
                for k in 0..3 {
                    sum += m[row][k] * inverse[k][column];
                }
                let expected = if row == column { 1.0 } else { 0.0 };
                assert!((sum - expected).abs() < 1e-12, "({row},{column}): {sum}");
            }
        }
    }

    #[test]
    fn dependent_rows_are_refused_and_near_dependent_ones_are_not() {
        // Exactly singular: row 2 is row 0 plus row 1.
        let singular = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [5.0, 7.0, 9.0]];
        assert_eq!(solve(singular, [1.0, 2.0, 3.0]), None);
        assert_eq!(inverse(singular), None);
        // The same shape, perturbed by a thousandth: solvable, and the answer is
        // large rather than refused. A threshold that refused this would refuse
        // every ill-conditioned geometry, which is a *result* about a detector
        // and not an error.
        let nearly = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [5.0, 7.0, 9.001]];
        assert!(solve(nearly, [1.0, 2.0, 3.0]).is_some());
        assert!(inverse(nearly).is_some());
    }

    #[test]
    fn the_conditioning_number_does_not_depend_on_the_units() {
        // The same three directions in millimetres and in metres: the
        // determinant changes by 10⁹ and the conditioning number does not. That
        // is the whole reason the test is not `det == 0`.
        let millimetres = [[1000.0, 0.0, 0.0], [0.0, 2000.0, 0.0], [0.0, 0.0, 3000.0]];
        let metres = [[1.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 3.0]];
        assert!((conditioning(millimetres) - 1.0).abs() < 1e-12);
        assert!((conditioning(metres) - 1.0).abs() < 1e-12);
        assert!((determinant(millimetres) / determinant(metres) - 1e9).abs() < 1.0);
    }
}
