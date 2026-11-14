//! W4.2: linear algebra over exact expressions.
//!
//! A matrix is a `Vec<Vec<Expr>>` — exact entries, so a determinant is an exact
//! number and an inverse is exact whenever the entries are. Nothing here
//! evaluates to an `f64` unless the question has no exact answer, and then it
//! says so rather than rounding.
//!
//! # What is exact and what is not
//!
//! * **Determinant** — cofactor expansion, exact for any size. The expansion is
//!   `O(n!)`, which is right for the sizes an exam asks (2, 3, maybe 4) and
//!   wrong for a 20×20 matrix; the doc says so instead of pretending the
//!   algorithm scales.
//! * **Rank** — Gaussian elimination with exact arithmetic, so a rank is a
//!   count and not a threshold on a rounding error. This is the one place a
//!   floating-point implementation is *wrong* rather than imprecise: `1e-17` is
//!   not "zero" and a rank that depends on a tolerance is not a rank.
//! * **Inverse** — Gauss–Jordan, exact, refused when the determinant is zero.
//! * **Eigenvalues** — 2×2 by the quadratic formula (exact surds), 3×3 by the
//!   characteristic polynomial's rational roots and then a quadratic factor.
//!   A 3×3 with three irrational eigenvalues is refused with a message, because
//!   the cubic formula is not in this crate and a numeric answer would not be
//!   exact.
//! * **`Ax = b`** — the existing elimination, given the matrix and the vector
//!   as equations.
//!
//! # Why the determinant is not computed by elimination
//!
//! It could be, and it would be `O(n³)`. It is computed by expansion because the
//! expansion is the *definition* and a test can check the implementation against
//! it on a symbolic matrix; elimination with row swaps has a sign rule that is
//! easy to get wrong and hard to see. The sizes that matter here are small, and
//! the doc says which sizes the choice is for.

use canlab_core::expr::{Expr, ExprNode};
use canlab_core::number::Number;
use canlab_core::symbol::SymbolId;

use crate::SolverError;

/// Why a linear-algebra question has no answer.
#[derive(Debug, Clone, PartialEq)]
pub enum MatrixError {
    /// The matrix has rows of different lengths.
    Ragged {
        /// The first row's length.
        expected: usize,
        /// The length of the row that did not match.
        found: usize,
    },
    /// The matrix has no rows, or a row has no columns.
    Empty,
    /// The matrix is not square, and the question needs one.
    NotSquare {
        /// The number of rows.
        rows: usize,
        /// The number of columns.
        columns: usize,
    },
    /// The determinant is zero, so there is no inverse.
    Singular,
    /// The eigenvalues are not exact with the methods this crate has.
    EigenvaluesNotExact {
        /// The size of the matrix whose eigenvalues were asked for.
        size: usize,
    },
}

impl MatrixError {
    /// The message a caller shows.
    pub fn message(&self) -> String {
        match self {
            MatrixError::Ragged { expected, found } => format!(
                "the matrix has rows of different lengths: {expected} and {found}"
            ),
            MatrixError::Empty => "the matrix has no entries".to_string(),
            MatrixError::NotSquare { rows, columns } => {
                format!("the matrix is {rows}×{columns}; this needs a square one")
            }
            MatrixError::Singular => {
                "the determinant is zero, so the matrix has no inverse".to_string()
            }
            MatrixError::EigenvaluesNotExact { size: 3 } => {
                "the eigenvalues are not exact with the methods here: a 3×3 needs a rational root \
                 or a quadratic factor, and a numeric answer would not be exact"
                    .to_string()
            }
            MatrixError::EigenvaluesNotExact { size } => format!(
                "the eigenvalues are not exact with the methods here: a {size}×{size} has no \
                 closed form in this crate, and a numeric answer would not be exact"
            ),
        }
    }
}

/// Check that a matrix is rectangular and non-empty.
fn check(matrix: &[Vec<Expr>]) -> Result<(), MatrixError> {
    let Some(first) = matrix.first() else {
        return Err(MatrixError::Empty);
    };
    if first.is_empty() {
        return Err(MatrixError::Empty);
    }
    for row in matrix {
        if row.len() != first.len() {
            return Err(MatrixError::Ragged {
                expected: first.len(),
                found: row.len(),
            });
        }
    }
    Ok(())
}

fn check_square(matrix: &[Vec<Expr>]) -> Result<usize, MatrixError> {
    check(matrix)?;
    let size = matrix.len();
    if matrix[0].len() != size {
        return Err(MatrixError::NotSquare {
            rows: size,
            columns: matrix[0].len(),
        });
    }
    Ok(size)
}

/// The determinant, by cofactor expansion along the first row.
pub fn determinant(matrix: &[Vec<Expr>]) -> Result<Expr, MatrixError> {
    let size = check_square(matrix)?;
    Ok(determinant_of(matrix, size))
}

fn determinant_of(matrix: &[Vec<Expr>], size: usize) -> Expr {
    match size {
        0 => ExprNode::int(1),
        1 => matrix[0][0].clone(),
        2 => canlab_core::add([
            canlab_core::mul([matrix[0][0].clone(), matrix[1][1].clone()]),
            canlab_core::mul([
                ExprNode::int(-1),
                canlab_core::mul([matrix[0][1].clone(), matrix[1][0].clone()]),
            ]),
        ]),
        _ => {
            let mut terms = Vec::with_capacity(size);
            for column in 0..size {
                let minor: Vec<Vec<Expr>> = matrix[1..]
                    .iter()
                    .map(|row| {
                        row.iter()
                            .enumerate()
                            .filter(|(index, _)| *index != column)
                            .map(|(_, value)| value.clone())
                            .collect()
                    })
                    .collect();
                let cofactor = determinant_of(&minor, size - 1);
                let mut factor = canlab_core::mul([matrix[0][column].clone(), cofactor]);
                if column % 2 == 1 {
                    factor = canlab_core::mul([ExprNode::int(-1), factor]);
                }
                terms.push(factor);
            }
            canlab_core::rewrite::simplify(&canlab_core::add(terms))
        }
    }
}

/// The rank, by exact Gaussian elimination.
///
/// A row is reduced against the rows above it; a row that becomes all zeros is
/// dependent. Exact arithmetic is what makes the count meaningful: a floating
/// rank depends on a tolerance, and a tolerance is a choice the caller did not
/// make.
pub fn rank(matrix: &[Vec<Expr>]) -> Result<usize, MatrixError> {
    check(matrix)?;
    let mut rows: Vec<Vec<Expr>> = matrix.to_vec();
    let columns = rows[0].len();
    let mut rank = 0usize;
    for column in 0..columns {
        // Find a row at or below `rank` with a non-zero entry in this column.
        let pivot = (rank..rows.len()).find(|&row| !is_zero(&rows[row][column]));
        let Some(pivot) = pivot else {
            continue;
        };
        rows.swap(rank, pivot);
        // Eliminate the column from every row below.
        let pivot_value = rows[rank][column].clone();
        for row in (rank + 1)..rows.len() {
            if is_zero(&rows[row][column]) {
                continue;
            }
            let factor = canlab_core::mul([
                rows[row][column].clone(),
                canlab_core::pow(pivot_value.clone(), ExprNode::int(-1)),
            ]);
            // A row operation touches two rows at once, so the index is the
            // only thing that addresses both; clippy's iterator suggestion does
            // not apply to a loop that reads `rows[rank]` and writes
            // `rows[row]`.
            #[allow(clippy::needless_range_loop)]
            for index in column..columns {
                let reduction = canlab_core::mul([factor.clone(), rows[rank][index].clone()]);
                rows[row][index] = canlab_core::rewrite::simplify(&canlab_core::add([
                    rows[row][index].clone(),
                    canlab_core::mul([ExprNode::int(-1), reduction]),
                ]));
            }
        }
        rank += 1;
        if rank == rows.len() {
            break;
        }
    }
    Ok(rank)
}

/// The inverse, by Gauss–Jordan over exact arithmetic.
pub fn inverse(matrix: &[Vec<Expr>]) -> Result<Vec<Vec<Expr>>, MatrixError> {
    let size = check_square(matrix)?;
    // The augmented matrix `[A | I]`.
    let mut rows: Vec<Vec<Expr>> = matrix
        .iter()
        .enumerate()
        .map(|(row, values)| {
            let mut out = values.clone();
            for column in 0..size {
                out.push(if row == column {
                    ExprNode::int(1)
                } else {
                    ExprNode::int(0)
                });
            }
            out
        })
        .collect();
    for column in 0..size {
        let pivot = (column..size).find(|&row| !is_zero(&rows[row][column]));
        let Some(pivot) = pivot else {
            return Err(MatrixError::Singular);
        };
        rows.swap(column, pivot);
        // Normalise the pivot row so the pivot is 1.
        let pivot_value = rows[column][column].clone();
        if !pivot_value.as_number().is_some_and(Number::is_one) {
            let inverse_pivot = canlab_core::pow(pivot_value, ExprNode::int(-1));
            #[allow(clippy::needless_range_loop)]
            for index in 0..(2 * size) {
                rows[column][index] =
                    canlab_core::rewrite::simplify(&canlab_core::mul([
                        rows[column][index].clone(),
                        inverse_pivot.clone(),
                    ]));
            }
        }
        // Eliminate the column from every other row.
        for row in 0..size {
            if row == column || is_zero(&rows[row][column]) {
                continue;
            }
            let factor = rows[row][column].clone();
            #[allow(clippy::needless_range_loop)]
            for index in 0..(2 * size) {
                let reduction = canlab_core::mul([factor.clone(), rows[column][index].clone()]);
                rows[row][index] = canlab_core::rewrite::simplify(&canlab_core::add([
                    rows[row][index].clone(),
                    canlab_core::mul([ExprNode::int(-1), reduction]),
                ]));
            }
        }
    }
    Ok(rows
        .into_iter()
        .map(|row| row[size..].to_vec())
        .collect())
}

/// The eigenvalues of a 2×2 or 3×3 matrix, exactly where they are exact.
///
/// A 2×2 is `λ² − tr·λ + det = 0`, solved by the quadratic formula — exact
/// surds, and complex roots when the discriminant is negative (the complex
/// answer is exact too, and it is what `x² + 1 = 0` gets).
///
/// A 3×3 is the characteristic cubic `−λ³ + tr·λ² − c₂·λ + det = 0`. Its
/// rational roots are found exactly; what is left is a quadratic, which the
/// quadratic formula finishes. Three irrational eigenvalues are refused: the
/// cubic formula is not in this crate, and a numeric answer would not be exact.
pub fn eigenvalues(matrix: &[Vec<Expr>]) -> Result<Vec<Expr>, MatrixError> {
    let size = check_square(matrix)?;
    match size {
        1 => Ok(vec![matrix[0][0].clone()]),
        2 => {
            let trace = canlab_core::add([matrix[0][0].clone(), matrix[1][1].clone()]);
            let determinant = determinant(matrix)?;
            // `λ² − tr·λ + det = 0`.
            let a = Number::int(1);
            let b = canlab_core::mul([ExprNode::int(-1), trace]);
            let c = determinant;
            Ok(quadratic_roots(&a, &b, &c))
        }
        3 => {
            // The characteristic polynomial's coefficients, exactly.
            let trace = canlab_core::add([
                matrix[0][0].clone(),
                matrix[1][1].clone(),
                matrix[2][2].clone(),
            ]);
            let second = canlab_core::add([
                minor_determinant(matrix, 0, 0),
                minor_determinant(matrix, 1, 1),
                minor_determinant(matrix, 2, 2),
            ]);
            let determinant = determinant(matrix)?;
            // `−λ³ + tr·λ² − c₂·λ + det = 0`, multiplied by −1:
            // `λ³ − tr·λ² + c₂·λ − det = 0`.
            let coefficients = [
                canlab_core::mul([ExprNode::int(-1), determinant]),
                second,
                canlab_core::mul([ExprNode::int(-1), trace]),
                ExprNode::int(1),
            ];
            cubic_roots(&coefficients)
        }
        _ => Err(MatrixError::EigenvaluesNotExact { size }),
    }
}

/// The determinant of the submatrix with row `r` and column `c` removed.
fn minor_determinant(matrix: &[Vec<Expr>], r: usize, c: usize) -> Expr {
    let minor: Vec<Vec<Expr>> = matrix
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != r)
        .map(|(_, row)| {
            row.iter()
                .enumerate()
                .filter(|(index, _)| *index != c)
                .map(|(_, value)| value.clone())
                .collect()
        })
        .collect();
    determinant_of(&minor, minor.len())
}

/// The roots of `a·λ² + b·λ + c = 0`, exactly.
fn quadratic_roots(a: &Number, b: &Expr, c: &Expr) -> Vec<Expr> {
    let discriminant = canlab_core::add([
        canlab_core::pow(b.clone(), ExprNode::int(2)),
        canlab_core::mul([
            ExprNode::int(-4),
            ExprNode::num(a.clone()),
            c.clone(),
        ]),
    ]);
    let discriminant = canlab_core::rewrite::simplify(&discriminant);
    let denominator = ExprNode::num(Number::int(2).mul(a));
    let inverse = canlab_core::pow(denominator, ExprNode::int(-1));
    let negative_b = canlab_core::mul([ExprNode::int(-1), b.clone()]);
    // A negative discriminant: the roots are complex, and `√|Δ|·i` keeps them
    // exact — the same construction the quadratic solver uses.
    if let Some(number) = discriminant.as_number() {
        if number.is_negative() {
            let magnitude = number.neg();
            let root = if let Some(exact) = magnitude.exact_nth_root(2) {
                ExprNode::num(exact)
            } else {
                canlab_core::pow(ExprNode::num(magnitude), ExprNode::num(Number::ratio(1, 2)))
            };
            let imaginary = canlab_core::mul([root, ExprNode::sym(SymbolId::intern("i"))]);
            return vec![
                canlab_core::add([
                    canlab_core::mul([negative_b.clone(), inverse.clone()]),
                    canlab_core::mul([imaginary.clone(), inverse.clone()]),
                ]),
                canlab_core::add([
                    canlab_core::mul([negative_b, inverse.clone()]),
                    canlab_core::mul([
                        ExprNode::int(-1),
                        canlab_core::mul([imaginary, inverse]),
                    ]),
                ]),
            ];
        }
        if let Some(root) = number.exact_nth_root(2) {
            let root = ExprNode::num(root);
            return vec![
                canlab_core::rewrite::simplify(&canlab_core::mul([
                    canlab_core::add([negative_b.clone(), root.clone()]),
                    inverse.clone(),
                ])),
                canlab_core::rewrite::simplify(&canlab_core::mul([
                    canlab_core::add([
                        negative_b,
                        canlab_core::mul([ExprNode::int(-1), root]),
                    ]),
                    inverse,
                ])),
            ];
        }
    }
    // A surd: `(−b ± √Δ)/(2a)`, left as a surd.
    let root = canlab_core::pow(discriminant, ExprNode::num(Number::ratio(1, 2)));
    vec![
        canlab_core::mul([
            canlab_core::add([negative_b.clone(), root.clone()]),
            inverse.clone(),
        ]),
        canlab_core::mul([
            canlab_core::add([negative_b, canlab_core::mul([ExprNode::int(-1), root])]),
            inverse,
        ]),
    ]
}

/// The roots of `c₀ + c₁λ + c₂λ² + c₃λ³ = 0`, exactly where they are.
fn cubic_roots(coefficients: &[Expr; 4]) -> Result<Vec<Expr>, MatrixError> {
    // Rational roots first: a root `p/q` has `p | c₀` and `q | c₃`. The
    // coefficients must be numbers for that search, which for a matrix's
    // characteristic polynomial they are.
    let numeric: Option<Vec<Number>> = coefficients
        .iter()
        .map(|c| c.as_number().cloned())
        .collect();
    let Some(numeric) = numeric else {
        return Err(MatrixError::EigenvaluesNotExact { size: 3 });
    };
    let poly = canlab_poly::univariate::from_coefficients(
        vec![SymbolId::intern("lambda")],
        &numeric,
    );
    let Some(rational) = poly.rational_roots() else {
        return Err(MatrixError::EigenvaluesNotExact { size: 3 });
    };
    let mut roots: Vec<Expr> = rational.iter().map(|n| ExprNode::num(n.clone())).collect();
    // Divide out the rational roots and finish the quadratic.
    let mut working = poly.clone();
    for root in &rational {
        let Some(quotient) = working.deflate(root) else {
            return Err(MatrixError::EigenvaluesNotExact { size: 3 });
        };
        working = quotient;
    }
    match working.degree() {
        0 => {}
        1 => {
            // `a + bλ = 0` → `λ = −a/b`.
            let coefficients = working.univariate_coefficients();
            let Some(map) = coefficients else {
                return Err(MatrixError::EigenvaluesNotExact { size: 3 });
            };
            let a = map.get(&0).cloned().unwrap_or_else(Number::zero);
            let b = map.get(&1).cloned().unwrap_or_else(Number::zero);
            if b.is_zero() {
                return Err(MatrixError::EigenvaluesNotExact { size: 3 });
            }
            roots.push(ExprNode::num(a.neg().div(&b)));
        }
        2 => {
            let Some(map) = working.univariate_coefficients() else {
                return Err(MatrixError::EigenvaluesNotExact { size: 3 });
            };
            let a = map.get(&2).cloned().unwrap_or_else(Number::zero);
            let b = map.get(&1).cloned().unwrap_or_else(Number::zero);
            let c = map.get(&0).cloned().unwrap_or_else(Number::zero);
            let b_expr = ExprNode::num(b);
            let c_expr = ExprNode::num(c);
            roots.extend(quadratic_roots(&a, &b_expr, &c_expr));
        }
        _ => return Err(MatrixError::EigenvaluesNotExact { size: 3 }),
    }
    Ok(roots)
}

/// `A·x = b` by the existing elimination.
///
/// The matrix and the vector are turned into equations — one per row — and
/// `solve_system` answers them. Reusing the elimination is the point: a second
/// Gaussian elimination would be a second place for a sign error.
pub fn solve_linear(
    matrix: &[Vec<Expr>],
    rhs: &[Expr],
) -> Result<Vec<(SymbolId, Expr)>, SolverError> {
    let size = check_square(matrix).map_err(|error| SolverError::BadOutput(error.message()))?;
    if rhs.len() != size {
        return Err(SolverError::BadOutput(format!(
            "the right-hand side has {} entries for a {size}×{size} matrix",
            rhs.len()
        )));
    }
    let vars: Vec<SymbolId> = (0..size)
        .map(|index| SymbolId::intern(&format!("x{}", index + 1)))
        .collect();
    let equations: Vec<Expr> = (0..size)
        .map(|row| {
            let terms: Vec<Expr> = (0..size)
                .map(|column| {
                    canlab_core::mul([matrix[row][column].clone(), ExprNode::sym(vars[column])])
                })
                .collect();
            canlab_core::add([
                canlab_core::add(terms),
                canlab_core::mul([ExprNode::int(-1), rhs[row].clone()]),
            ])
        })
        .collect();
    crate::system::solve_system(&crate::NativeSolver, &equations, &vars)
}

fn is_zero(expression: &Expr) -> bool {
    expression.as_number().is_some_and(Number::is_zero)
}

#[cfg(test)]
mod tests {
    use super::*;
    use canlab_core::int;

    fn m(rows: &[&[i64]]) -> Vec<Vec<Expr>> {
        rows.iter()
            .map(|row| row.iter().map(|value| int(*value)).collect())
            .collect()
    }

    #[test]
    fn the_determinants_are_the_ones_a_textbook_gives() {
        // `|2 1; 1 3| = 5`, `|1 2 3; 4 5 6; 7 8 10| = −3`.
        assert_eq!(determinant(&m(&[&[2, 1], &[1, 3]])).expect("a determinant"), int(5));
        assert_eq!(
            determinant(&m(&[&[1, 2, 3], &[4, 5, 6], &[7, 8, 10]])).expect("a determinant"),
            int(-3)
        );
        // A singular matrix has determinant zero.
        assert_eq!(
            determinant(&m(&[&[1, 2], &[2, 4]])).expect("a determinant"),
            int(0)
        );
    }

    #[test]
    fn two_hundred_integer_matrices_have_an_inverse_that_multiplies_to_the_identity() {
        // The roadmap's check. Deterministic pseudo-random integer matrices with
        // a non-zero determinant; `A·A⁻¹ = I` exactly, entry by entry.
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            ((state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) % 9) as i64 - 4
        };
        let mut checked = 0;
        while checked < 200 {
            let matrix: Vec<Vec<Expr>> = (0..3)
                .map(|_| (0..3).map(|_| int(next())).collect())
                .collect();
            let Ok(det) = determinant(&matrix) else {
                panic!("a square matrix has a determinant");
            };
            if det.as_number().is_some_and(Number::is_zero) {
                continue;
            }
            let inverse = inverse(&matrix).expect("a non-singular matrix has an inverse");
            // `A·A⁻¹` must be the identity, exactly.
            // The test indexes two matrices at once, which is the same reason
            // the elimination loops do.
            #[allow(clippy::needless_range_loop)]
            for row in 0..3 {
                for column in 0..3 {
                    let entry: Expr = canlab_core::add(
                        (0..3)
                            .map(|k| {
                                canlab_core::mul([
                                    matrix[row][k].clone(),
                                    inverse[k][column].clone(),
                                ])
                            })
                            .collect::<Vec<_>>(),
                    );
                    let entry = canlab_core::rewrite::simplify(&entry);
                    let expected = if row == column { 1 } else { 0 };
                    assert_eq!(
                        entry.as_number(),
                        Some(&Number::int(expected)),
                        "entry ({row},{column}) of A·A⁻¹ is {entry:?}, matrix {matrix:?}"
                    );
                }
            }
            checked += 1;
        }
    }

    #[test]
    fn a_singular_matrix_has_no_inverse_and_says_so() {
        let error = inverse(&m(&[&[1, 2], &[2, 4]])).expect_err("singular");
        assert_eq!(error, MatrixError::Singular);
        assert!(error.message().contains("determinant is zero"));
    }

    #[test]
    fn the_rank_is_a_count_and_not_a_tolerance() {
        // The identity has full rank; the zero matrix has none; a matrix whose
        // third row is the sum of the first two has rank 2.
        assert_eq!(rank(&m(&[&[1, 0], &[0, 1]])).expect("a rank"), 2);
        assert_eq!(rank(&m(&[&[0, 0], &[0, 0]])).expect("a rank"), 0);
        assert_eq!(
            rank(&m(&[&[1, 0, 0], &[0, 1, 0], &[1, 1, 0]])).expect("a rank"),
            2
        );
        // A matrix that is singular only because of exact cancellation still has
        // rank 1: `[1, 1]` and `[1, 1]`.
        assert_eq!(rank(&m(&[&[1, 1], &[1, 1]])).expect("a rank"), 1);
    }

    #[test]
    fn a_symmetric_two_by_two_has_the_eigenvalues_its_characteristic_polynomial_says() {
        // The roadmap's check. `[2 1; 1 2]` has `λ² − 4λ + 3 = 0`, so `λ = 1, 3`.
        let matrix = m(&[&[2, 1], &[1, 2]]);
        let mut values = eigenvalues(&matrix).expect("eigenvalues");
        values.sort_by(|a, b| {
            a.as_number()
                .map(Number::to_f64)
                .unwrap_or(f64::NAN)
                .total_cmp(&b.as_number().map(Number::to_f64).unwrap_or(f64::NAN))
        });
        assert_eq!(values, vec![int(1), int(3)]);
        // And the characteristic polynomial is what it says: `λ² − tr·λ + det`.
        let trace = canlab_core::add([matrix[0][0].clone(), matrix[1][1].clone()]);
        assert_eq!(trace.as_number(), Some(&Number::int(4)));
        assert_eq!(determinant(&matrix).expect("a determinant"), int(3));
        // A rotation by 90° has no real eigenvalues: `[0 −1; 1 0]` gives `±i`.
        let rotation = m(&[&[0, -1], &[1, 0]]);
        let values = eigenvalues(&rotation).expect("eigenvalues");
        assert!(values.iter().all(|v| v.to_string().contains('i')), "{values:?}");
    }

    #[test]
    fn a_three_by_three_with_rational_eigenvalues_answers_exactly() {
        // `[2 0 0; 0 3 0; 0 0 4]` has `λ = 2, 3, 4`.
        let matrix = m(&[&[2, 0, 0], &[0, 3, 0], &[0, 0, 4]]);
        let mut values = eigenvalues(&matrix).expect("eigenvalues");
        values.sort_by(|a, b| {
            a.as_number()
                .map(Number::to_f64)
                .unwrap_or(f64::NAN)
                .total_cmp(&b.as_number().map(Number::to_f64).unwrap_or(f64::NAN))
        });
        assert_eq!(values, vec![int(2), int(3), int(4)]);
    }

    #[test]
    fn an_eigenvalue_refusal_names_the_size_the_caller_asked_for() {
        // `diag(1,2,3,4)` is not a 3×3, and the refusal used to tell the caller
        // about a size they had not asked about. The variant carries the size
        // now, and the message repeats it.
        let four = m(&[
            &[1, 0, 0, 0],
            &[0, 2, 0, 0],
            &[0, 0, 3, 0],
            &[0, 0, 0, 4],
        ]);
        let error = eigenvalues(&four).expect_err("a 4×4 is not solved exactly");
        assert_eq!(error, MatrixError::EigenvaluesNotExact { size: 4 });
        assert!(error.message().contains("4×4"), "{}", error.message());
        assert!(!error.message().contains("3×3"), "{}", error.message());
        // And the same for the next size up, so the message is not a special
        // case for four.
        let five = m(&[
            &[1, 0, 0, 0, 0],
            &[0, 2, 0, 0, 0],
            &[0, 0, 3, 0, 0],
            &[0, 0, 0, 4, 0],
            &[0, 0, 0, 0, 5],
        ]);
        let error = eigenvalues(&five).expect_err("a 5×5 is not solved exactly");
        assert_eq!(error, MatrixError::EigenvaluesNotExact { size: 5 });
        assert!(error.message().contains("5×5"), "{}", error.message());
    }

    #[test]
    fn a_matrix_that_is_not_square_is_refused_with_its_shape() {
        let error = determinant(&m(&[&[1, 2, 3], &[4, 5, 6]])).expect_err("not square");
        assert_eq!(
            error,
            MatrixError::NotSquare {
                rows: 2,
                columns: 3
            }
        );
        assert!(error.message().contains("2×3"));
        // A ragged matrix is a different error, with both lengths.
        let ragged = vec![vec![int(1), int(2)], vec![int(3)]];
        assert_eq!(
            determinant(&ragged),
            Err(MatrixError::Ragged {
                expected: 2,
                found: 1
            })
        );
    }

    #[test]
    fn a_linear_system_solves_through_the_existing_elimination() {
        // `2x + y = 5`, `x + 3y = 10` → `x = 1, y = 3`.
        let matrix = m(&[&[2, 1], &[1, 3]]);
        let rhs = vec![int(5), int(10)];
        let solution = solve_linear(&matrix, &rhs).expect("a solution");
        let value = |name: &str| {
            solution
                .iter()
                .find(|(variable, _)| variable.as_str().as_str() == name)
                .map(|(_, value)| value.clone())
        };
        assert_eq!(value("x1").and_then(|v| v.as_number().cloned()), Some(Number::int(1)));
        assert_eq!(value("x2").and_then(|v| v.as_number().cloned()), Some(Number::int(3)));

    }
}
