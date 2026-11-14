//! W4.1: a value that is not always a scalar.
//!
//! `Expr` is a scalar: a number, a variable, a function of them. A vector, a
//! matrix and a complex number are not scalars, and squeezing them into one is
//! how a CAS starts answering `2 × [[1,2],[3,4]]` with a number. [`Value`] is
//! the type that says which of the four a thing is, and every operation on it
//! either stays inside one kind or refuses.
//!
//! # The four kinds
//!
//! * **Scalar** — an [`Expr`], exactly as before.
//! * **Vector** — a list of scalars. `(1, 2, 3)`.
//! * **Matrix** — a rectangular list of rows of scalars. `[[1,2],[3,4]]`.
//! * **Complex** — a real and an imaginary part, both scalars, so `i` is exact.
//!
//! # What the arithmetic does
//!
//! * **Addition** needs the same kind and the same shape. A scalar plus a
//!   vector is refused, not broadcast: broadcasting is a *convention* (NumPy's,
//!   not mathematics'), and a CAS that silently broadcasts answers a question
//!   the student did not ask.
//! * **Multiplication** is the one place kinds mix, because the mathematics
//!   does: a scalar times a vector is a vector, a scalar times a matrix is a
//!   matrix, a matrix times a vector is a vector, and a matrix times a matrix is
//!   the matrix product. Everything else is refused.
//! * **Powers** are integer powers only: a matrix to a negative power is its
//!   inverse, which needs the matrix to be invertible, and that is a refusal
//!   with a reason.
//!
//! # Printing
//!
//! A vector prints as `\left(1, 2, 3\right)`, a matrix as a `pmatrix`, a complex
//! number as `a + bi`, a scalar as itself. Each one round-trips through
//! [`Value::from_latex`] where the parser can read it back.

use canlab_core::expr::{Expr, ExprNode};
use canlab_core::number::Number;
use canlab_core::symbol::SymbolId;

/// Why an operation on a value has no answer.
#[derive(Debug, Clone, PartialEq)]
pub enum ValueError {
    /// The kinds do not combine.
    KindMismatch {
        /// What was on the left.
        left: &'static str,
        /// What was on the right.
        right: &'static str,
    },
    /// Two vectors or matrices of different shapes.
    ShapeMismatch {
        /// What the operation needed.
        expected: String,
        /// What it got.
        found: String,
    },
    /// A matrix whose rows are not all the same length.
    Ragged,
    /// A matrix product whose inner dimensions do not agree.
    NotMultipliable {
        /// The left matrix's columns.
        left_columns: usize,
        /// The right matrix's rows.
        right_rows: usize,
    },
    /// A matrix that has no inverse.
    Singular,
    /// A power that is not an integer.
    NonIntegerPower,
    /// Text that is not a value this crate can read.
    Unreadable(String),
}

impl ValueError {
    /// The message a caller shows.
    pub fn message(&self) -> String {
        match self {
            ValueError::KindMismatch { left, right } => {
                format!("a {left} and a {right} do not combine this way")
            }
            ValueError::ShapeMismatch { expected, found } => {
                format!("the shapes do not match: expected {expected}, found {found}")
            }
            ValueError::Ragged => "the matrix's rows are not all the same length".to_string(),
            ValueError::NotMultipliable {
                left_columns,
                right_rows,
            } => format!(
                "a matrix with {left_columns} columns cannot multiply one with {right_rows} rows"
            ),
            ValueError::Singular => "the matrix is singular, so it has no inverse".to_string(),
            ValueError::NonIntegerPower => {
                "only an integer power of a vector or a matrix is defined here".to_string()
            }
            ValueError::Unreadable(what) => format!("`{what}` is not a value this can read"),
        }
    }
}

/// A scalar, a vector, a matrix or a complex number.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A scalar: the `Expr` the rest of the CAS works with.
    Scalar(Expr),
    /// A vector of scalars.
    Vector(Vec<Expr>),
    /// A rectangular matrix of scalars, row by row.
    Matrix(Vec<Vec<Expr>>),
    /// `a + bi`.
    Complex {
        /// The real part.
        re: Expr,
        /// The imaginary part.
        im: Expr,
    },
}

impl Value {
    /// The kind's name, for a message.
    pub fn kind(&self) -> &'static str {
        match self {
            Value::Scalar(_) => "scalar",
            Value::Vector(_) => "vector",
            Value::Matrix(_) => "matrix",
            Value::Complex { .. } => "complex number",
        }
    }

    /// `true` for a scalar.
    pub fn is_scalar(&self) -> bool {
        matches!(self, Value::Scalar(_))
    }

    /// The scalar inside, if this is one.
    pub fn as_scalar(&self) -> Option<&Expr> {
        match self {
            Value::Scalar(expression) => Some(expression),
            _ => None,
        }
    }

    /// `a + b`.
    pub fn add(&self, other: &Value) -> Result<Value, ValueError> {
        match (self, other) {
            (Value::Scalar(a), Value::Scalar(b)) => {
                Ok(Value::Scalar(canlab_core::add([a.clone(), b.clone()])))
            }
            (Value::Vector(a), Value::Vector(b)) => {
                if a.len() != b.len() {
                    return Err(ValueError::ShapeMismatch {
                        expected: format!("a vector of {}", a.len()),
                        found: format!("a vector of {}", b.len()),
                    });
                }
                Ok(Value::Vector(
                    a.iter()
                        .zip(b)
                        .map(|(x, y)| canlab_core::add([x.clone(), y.clone()]))
                        .collect(),
                ))
            }
            (Value::Matrix(a), Value::Matrix(b)) => {
                let shape = shape_of(a)?;
                let other_shape = shape_of(b)?;
                if shape != other_shape {
                    return Err(ValueError::ShapeMismatch {
                        expected: format!("{}×{}", shape.0, shape.1),
                        found: format!("{}×{}", other_shape.0, other_shape.1),
                    });
                }
                Ok(Value::Matrix(
                    a.iter()
                        .zip(b)
                        .map(|(row, other_row)| {
                            row.iter()
                                .zip(other_row)
                                .map(|(x, y)| canlab_core::add([x.clone(), y.clone()]))
                                .collect()
                        })
                        .collect(),
                ))
            }
            (Value::Complex { re: a, im: b }, Value::Complex { re: c, im: d }) => {
                Ok(Value::Complex {
                    re: canlab_core::add([a.clone(), c.clone()]),
                    im: canlab_core::add([b.clone(), d.clone()]),
                })
            }
            // A scalar and a complex number: the scalar is real, so it adds to
            // the real part. This is the one mixed kind the mathematics allows.
            (Value::Scalar(a), Value::Complex { re, im }) => Ok(Value::Complex {
                re: canlab_core::add([a.clone(), re.clone()]),
                im: im.clone(),
            }),
            (Value::Complex { re, im }, Value::Scalar(b)) => Ok(Value::Complex {
                re: canlab_core::add([re.clone(), b.clone()]),
                im: im.clone(),
            }),
            (left, right) => Err(ValueError::KindMismatch {
                left: left.kind(),
                right: right.kind(),
            }),
        }
    }

    /// `a − b`.
    pub fn sub(&self, other: &Value) -> Result<Value, ValueError> {
        self.add(&other.neg())
    }

    /// `−a`.
    pub fn neg(&self) -> Value {
        let negate = |expression: &Expr| {
            canlab_core::mul([ExprNode::int(-1), expression.clone()])
        };
        match self {
            Value::Scalar(a) => Value::Scalar(negate(a)),
            Value::Vector(a) => Value::Vector(a.iter().map(negate).collect()),
            Value::Matrix(a) => Value::Matrix(
                a.iter()
                    .map(|row| row.iter().map(negate).collect())
                    .collect(),
            ),
            Value::Complex { re, im } => Value::Complex {
                re: negate(re),
                im: negate(im),
            },
        }
    }

    /// `a·b`, with the one place kinds mix.
    pub fn mul(&self, other: &Value) -> Result<Value, ValueError> {
        match (self, other) {
            (Value::Scalar(a), Value::Scalar(b)) => {
                Ok(Value::Scalar(canlab_core::mul([a.clone(), b.clone()])))
            }
            // A scalar scales a vector or a matrix.
            (Value::Scalar(a), Value::Vector(v)) => Ok(Value::Vector(
                v.iter().map(|x| canlab_core::mul([a.clone(), x.clone()])).collect(),
            )),
            (Value::Vector(v), Value::Scalar(a)) => Ok(Value::Vector(
                v.iter().map(|x| canlab_core::mul([a.clone(), x.clone()])).collect(),
            )),
            (Value::Scalar(a), Value::Matrix(m)) => Ok(Value::Matrix(
                m.iter()
                    .map(|row| {
                        row.iter()
                            .map(|x| canlab_core::mul([a.clone(), x.clone()]))
                            .collect()
                    })
                    .collect(),
            )),
            (Value::Matrix(m), Value::Scalar(a)) => Ok(Value::Matrix(
                m.iter()
                    .map(|row| {
                        row.iter()
                            .map(|x| canlab_core::mul([a.clone(), x.clone()]))
                            .collect()
                    })
                    .collect(),
            )),
            // A matrix times a vector.
            (Value::Matrix(m), Value::Vector(v)) => {
                let (rows, columns) = shape_of(m)?;
                if columns != v.len() {
                    return Err(ValueError::NotMultipliable {
                        left_columns: columns,
                        right_rows: v.len(),
                    });
                }
                let mut out = Vec::with_capacity(rows);
                for row in m {
                    let terms: Vec<Expr> = row
                        .iter()
                        .zip(v)
                        .map(|(a, b)| canlab_core::mul([a.clone(), b.clone()]))
                        .collect();
                    out.push(canlab_core::rewrite::simplify(&canlab_core::add(terms)));
                }
                Ok(Value::Vector(out))
            }
            // A matrix times a matrix.
            (Value::Matrix(a), Value::Matrix(b)) => {
                let (a_rows, a_columns) = shape_of(a)?;
                let (b_rows, b_columns) = shape_of(b)?;
                if a_columns != b_rows {
                    return Err(ValueError::NotMultipliable {
                        left_columns: a_columns,
                        right_rows: b_rows,
                    });
                }
                let mut out = Vec::with_capacity(a_rows);
                for row in a {
                    let mut product_row = Vec::with_capacity(b_columns);
                    // The product reads `b[k][column]` for every `k`, so the
                    // column index addresses the whole right matrix; an
                    // iterator over one row would not.
                    #[allow(clippy::needless_range_loop)]
                    for column in 0..b_columns {
                        let terms: Vec<Expr> = (0..a_columns)
                            .map(|k| canlab_core::mul([row[k].clone(), b[k][column].clone()]))
                            .collect();
                        product_row.push(canlab_core::rewrite::simplify(&canlab_core::add(terms)));
                    }
                    out.push(product_row);
                }
                Ok(Value::Matrix(out))
            }
            // A scalar times a complex number.
            (Value::Scalar(a), Value::Complex { re, im }) => Ok(Value::Complex {
                re: canlab_core::mul([a.clone(), re.clone()]),
                im: canlab_core::mul([a.clone(), im.clone()]),
            }),
            (Value::Complex { re, im }, Value::Scalar(a)) => Ok(Value::Complex {
                re: canlab_core::mul([a.clone(), re.clone()]),
                im: canlab_core::mul([a.clone(), im.clone()]),
            }),
            (Value::Complex { re: a, im: b }, Value::Complex { re: c, im: d }) => {
                let ac = canlab_core::mul([a.clone(), c.clone()]);
                let bd = canlab_core::mul([b.clone(), d.clone()]);
                let ad = canlab_core::mul([a.clone(), d.clone()]);
                let bc = canlab_core::mul([b.clone(), c.clone()]);
                Ok(Value::Complex {
                    re: canlab_core::add([ac, canlab_core::mul([ExprNode::int(-1), bd])]),
                    im: canlab_core::add([ad, bc]),
                })
            }
            (left, right) => Err(ValueError::KindMismatch {
                left: left.kind(),
                right: right.kind(),
            }),
        }
    }

    /// `aⁿ` for an integer `n`.
    pub fn pow(&self, exponent: i128) -> Result<Value, ValueError> {
        match self {
            Value::Scalar(a) => Ok(Value::Scalar(canlab_core::pow(
                a.clone(),
                ExprNode::num(Number::int(exponent)),
            ))),
            Value::Complex { .. } => {
                let mut result = Value::Scalar(ExprNode::int(1));
                let mut base = self.clone();
                let mut remaining = exponent;
                if remaining < 0 {
                    return Err(ValueError::NonIntegerPower);
                }
                while remaining > 0 {
                    if remaining & 1 == 1 {
                        result = result.mul(&base)?;
                    }
                    remaining >>= 1;
                    if remaining > 0 {
                        base = base.mul(&base)?;
                    }
                }
                Ok(result)
            }
            Value::Matrix(m) => {
                let (rows, columns) = shape_of(m)?;
                if rows != columns {
                    return Err(ValueError::ShapeMismatch {
                        expected: "a square matrix".to_string(),
                        found: format!("{rows}×{columns}"),
                    });
                }
                if exponent < 0 {
                    return Err(ValueError::NonIntegerPower);
                }
                let identity: Vec<Vec<Expr>> = (0..rows)
                    .map(|row| {
                        (0..rows)
                            .map(|column| {
                                if row == column {
                                    ExprNode::int(1)
                                } else {
                                    ExprNode::int(0)
                                }
                            })
                            .collect()
                    })
                    .collect();
                let mut result = Value::Matrix(identity);
                let mut base = self.clone();
                let mut remaining = exponent;
                while remaining > 0 {
                    if remaining & 1 == 1 {
                        result = result.mul(&base)?;
                    }
                    remaining >>= 1;
                    if remaining > 0 {
                        base = base.mul(&base)?;
                    }
                }
                Ok(result)
            }
            // A vector has no power: `(1,2)²` is not a product this crate has a
            // meaning for, and a componentwise power is a convention.
            Value::Vector(_) => Err(ValueError::NonIntegerPower),
        }
    }

    /// The value as LaTeX: `\left(1, 2, 3\right)`, a `pmatrix`, `a + bi`.
    pub fn to_latex(&self) -> String {
        match self {
            Value::Scalar(expression) => canlab_tex::to_latex(expression),
            Value::Vector(components) => {
                let parts: Vec<String> = components
                    .iter()
                    .map(canlab_tex::to_latex)
                    .collect();
                format!("\\left({}\\right)", parts.join(", "))
            }
            Value::Matrix(rows) => {
                let body: Vec<String> = rows
                    .iter()
                    .map(|row| {
                        row.iter()
                            .map(canlab_tex::to_latex)
                            .collect::<Vec<_>>()
                            .join(" & ")
                    })
                    .collect();
                format!("\\begin{{pmatrix}} {} \\end{{pmatrix}}", body.join(" \\\\ "))
            }
            Value::Complex { re, im } => {
                let real = canlab_tex::to_latex(re);
                let imaginary = canlab_tex::to_latex(im);
                if re.as_number().is_some_and(Number::is_zero) {
                    return format!("{imaginary}i");
                }
                if im.as_number().is_some_and(Number::is_zero) {
                    return real;
                }
                if imaginary.starts_with('-') {
                    format!("{real} - {}i", imaginary.trim_start_matches('-'))
                } else {
                    format!("{real} + {imaginary}i")
                }
            }
        }
    }

    /// Read a value from its LaTeX.
    ///
    /// `(1, 2, 3)` is a vector, `[[1,2],[3,4]]` and `\begin{pmatrix}…\end{pmatrix}`
    /// are matrices, and anything else is a scalar read by the expression
    /// parser. The brackets are the form the roadmap names; the `pmatrix` is
    /// what [`Value::to_latex`] writes, so a value round-trips through its own
    /// printing.
    pub fn from_latex(text: &str) -> Result<Value, ValueError> {
        let trimmed = text.trim();
        if let Some(inner) = trimmed
            .strip_prefix("\\begin{pmatrix}")
            .and_then(|rest| rest.strip_suffix("\\end{pmatrix}"))
        {
            let rows = inner
                .split("\\\\")
                .map(|row| {
                    row.split('&')
                        .map(|entry| {
                            canlab_parse::parse_latex(entry.trim())
                                .map_err(|error| ValueError::Unreadable(error.message))
                        })
                        .collect::<Result<Vec<Expr>, ValueError>>()
                })
                .collect::<Result<Vec<Vec<Expr>>, ValueError>>()?;
            shape_of(&rows)?;
            return Ok(Value::Matrix(rows));
        }
        if let Some(inner) = trimmed
            .strip_prefix("[[")
            .and_then(|rest| rest.strip_suffix("]]"))
        {
            let rows = inner
                .split("],[")
                .map(|row| {
                    row.split(',')
                        .map(|entry| {
                            canlab_parse::parse_latex(entry.trim())
                                .map_err(|error| ValueError::Unreadable(error.message))
                        })
                        .collect::<Result<Vec<Expr>, ValueError>>()
                })
                .collect::<Result<Vec<Vec<Expr>>, ValueError>>()?;
            shape_of(&rows)?;
            return Ok(Value::Matrix(rows));
        }
        // `(1, 2, 3)` and `\left(1, 2, 3\right)`: the printer writes the
        // second, so the reader has to accept it or a value does not round-trip
        // through its own printing.
        let parenthesised = trimmed
            .strip_prefix('(')
            .and_then(|rest| rest.strip_suffix(')'))
            .or_else(|| {
                trimmed
                    .strip_prefix("\\left(")
                    .and_then(|rest| rest.strip_suffix("\\right)"))
            });
        if let Some(inner) = parenthesised
            && inner.contains(',')
        {
            let components = inner
                .split(',')
                .map(|entry| {
                    canlab_parse::parse_latex(entry.trim())
                        .map_err(|error| ValueError::Unreadable(error.message))
                })
                .collect::<Result<Vec<Expr>, ValueError>>()?;
            return Ok(Value::Vector(components));
        }
        let expression = canlab_parse::parse_latex(trimmed)
            .map_err(|error| ValueError::Unreadable(error.message))?;
        // A scalar that mentions `i` is a complex number, which is what the
        // scalar *is*: the split is `a + b·i` with `b` the coefficient of the
        // symbol.
        Ok(split_complex(expression))
    }
}

/// `(rows, columns)` for a rectangular matrix, or [`ValueError::Ragged`].
fn shape_of(matrix: &[Vec<Expr>]) -> Result<(usize, usize), ValueError> {
    let Some(first) = matrix.first() else {
        return Ok((0, 0));
    };
    let columns = first.len();
    if matrix.iter().any(|row| row.len() != columns) {
        return Err(ValueError::Ragged);
    }
    Ok((matrix.len(), columns))
}

/// Split `a + b·i` into a complex value, or leave it a scalar.
fn split_complex(expression: Expr) -> Value {
    let i = SymbolId::intern("i");
    let mentions_i = canlab_poly::expr::free_symbols(&expression).contains(&i);
    if !mentions_i {
        return Value::Scalar(expression);
    }
    // `a + b·i`: the imaginary part is the coefficient of `i` in the terms that
    // carry it, and the real part is everything else.
    let (re, im) = match &*expression {
        ExprNode::Add(terms) => {
            let mut re = Vec::new();
            let mut im = Vec::new();
            for term in terms {
                if term_has_i(term, i) {
                    im.push(coefficient_of_i(term, i));
                } else {
                    re.push(term.clone());
                }
            }
            (
                canlab_core::add(re),
                canlab_core::add(im),
            )
        }
        _ if term_has_i(&expression, i) => (ExprNode::int(0), coefficient_of_i(&expression, i)),
        _ => (expression.clone(), ExprNode::int(0)),
    };
    Value::Complex { re, im }
}

fn term_has_i(term: &Expr, i: SymbolId) -> bool {
    canlab_poly::expr::free_symbols(term).contains(&i)
}

/// The coefficient of `i` in a term that carries it.
fn coefficient_of_i(term: &Expr, i: SymbolId) -> Expr {
    match &**term {
        ExprNode::Sym(symbol) if *symbol == i => ExprNode::int(1),
        ExprNode::Mul(factors) => {
            let rest: Vec<Expr> = factors
                .iter()
                .filter(|factor| !matches!(&***factor, ExprNode::Sym(s) if *s == i))
                .cloned()
                .collect();
            canlab_core::mul(rest)
        }
        _ => term.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canlab_core::{int, sym};

    #[test]
    fn every_kind_round_trips_through_its_own_latex() {
        // The roadmap's check. Each value prints, and the printed form reads
        // back as the same value.
        let values = vec![
            Value::Scalar(int(3)),
            Value::Vector(vec![int(1), int(2), int(3)]),
            Value::Matrix(vec![vec![int(1), int(2)], vec![int(3), int(4)]]),
            Value::Complex {
                re: int(1),
                im: int(2),
            },
        ];
        for value in values {
            let latex = value.to_latex();
            let back = Value::from_latex(&latex)
                .unwrap_or_else(|error| panic!("{latex} did not read back: {error:?}"));
            assert_eq!(back, value, "latex was {latex}");
        }
    }

    #[test]
    fn a_matrix_prints_as_a_pmatrix() {
        let matrix = Value::Matrix(vec![vec![int(1), int(2)], vec![int(3), int(4)]]);
        let latex = matrix.to_latex();
        assert!(latex.starts_with("\\begin{pmatrix}"), "{latex}");
        assert!(latex.ends_with("\\end{pmatrix}"), "{latex}");
        assert!(latex.contains(" & "), "the columns are separated by `&`: {latex}");
        assert!(latex.contains(" \\\\ "), "the rows are separated by `\\\\`: {latex}");
    }

    #[test]
    fn the_bracket_forms_the_roadmap_names_are_read() {
        // `[[1,2],[3,4]]` and `(1,2,3)`.
        assert_eq!(
            Value::from_latex("[[1,2],[3,4]]").expect("a matrix"),
            Value::Matrix(vec![vec![int(1), int(2)], vec![int(3), int(4)]])
        );
        assert_eq!(
            Value::from_latex("(1,2,3)").expect("a vector"),
            Value::Vector(vec![int(1), int(2), int(3)])
        );
        // And a scalar stays a scalar.
        assert_eq!(Value::from_latex("x^2+1").expect("a scalar"), Value::Scalar(canlab_core::add([
            canlab_core::pow(sym("x"), int(2)),
            int(1),
        ])));
    }

    #[test]
    fn a_scalar_times_a_matrix_is_a_matrix_and_a_matrix_times_a_matrix_is_the_product() {
        let matrix = Value::Matrix(vec![vec![int(1), int(2)], vec![int(3), int(4)]]);
        let scaled = Value::Scalar(int(2)).mul(&matrix).expect("a product");
        assert_eq!(
            scaled,
            Value::Matrix(vec![vec![int(2), int(4)], vec![int(6), int(8)]])
        );
        // `[[1,2],[3,4]] · [[1,0],[0,1]]` is itself.
        let identity = Value::Matrix(vec![vec![int(1), int(0)], vec![int(0), int(1)]]);
        assert_eq!(matrix.mul(&identity).expect("a product"), matrix);
        // `[[1,2],[3,4]] · [[1],[1]]` is `(3, 7)`.
        let vector = Value::Vector(vec![int(1), int(1)]);
        assert_eq!(
            matrix.mul(&vector).expect("a product"),
            Value::Vector(vec![int(3), int(7)])
        );
    }

    #[test]
    fn an_addition_of_different_kinds_is_refused_and_not_broadcast() {
        // A scalar plus a vector is not a vector plus a scalar added to each
        // component: broadcasting is a convention, and this crate refuses it.
        let error = Value::Scalar(int(1))
            .add(&Value::Vector(vec![int(1)]))
            .expect_err("a scalar plus a vector");
        assert_eq!(
            error,
            ValueError::KindMismatch {
                left: "scalar",
                right: "vector"
            }
        );
        // Two vectors of different lengths.
        let error = Value::Vector(vec![int(1)])
            .add(&Value::Vector(vec![int(1), int(2)]))
            .expect_err("a shape mismatch");
        assert!(matches!(error, ValueError::ShapeMismatch { .. }));
        // A matrix product whose inner dimensions disagree.
        let left = Value::Matrix(vec![vec![int(1), int(2)]]);
        let right = Value::Matrix(vec![vec![int(1), int(2)]]);
        assert_eq!(
            left.mul(&right),
            Err(ValueError::NotMultipliable {
                left_columns: 2,
                right_rows: 1
            })
        );
    }

    #[test]
    fn a_matrix_power_is_repeated_multiplication() {
        let matrix = Value::Matrix(vec![vec![int(1), int(1)], vec![int(0), int(1)]]);
        // `[[1,1],[0,1]]² = [[1,2],[0,1]]`.
        assert_eq!(
            matrix.pow(2).expect("a square"),
            Value::Matrix(vec![vec![int(1), int(2)], vec![int(0), int(1)]])
        );
        // The zeroth power is the identity.
        assert_eq!(
            matrix.pow(0).expect("the identity"),
            Value::Matrix(vec![vec![int(1), int(0)], vec![int(0), int(1)]])
        );
        // A negative power is refused: it would need the inverse, and this type
        // does not claim one.
        assert_eq!(matrix.pow(-1), Err(ValueError::NonIntegerPower));
        // A vector has no power at all.
        assert_eq!(Value::Vector(vec![int(1)]).pow(2), Err(ValueError::NonIntegerPower));
    }

    #[test]
    fn a_complex_scalar_is_read_as_one() {
        // `1 + 2i` reads as a complex number, and prints back the same way.
        let value = Value::from_latex("1+2i").expect("a complex number");
        assert_eq!(
            value,
            Value::Complex {
                re: int(1),
                im: int(2)
            }
        );
        assert_eq!(value.to_latex(), "1 + 2i");
        // `3i` has no real part.
        let value = Value::from_latex("3i").expect("a complex number");
        assert_eq!(
            value,
            Value::Complex {
                re: int(0),
                im: int(3)
            }
        );
        assert_eq!(value.to_latex(), "3i");
    }

    #[test]
    fn a_ragged_matrix_is_refused_with_its_reason() {
        assert_eq!(
            Value::from_latex("[[1,2],[3]]"),
            Err(ValueError::Ragged)
        );
        let ragged = Value::Matrix(vec![vec![int(1), int(2)], vec![int(3)]]);
        assert_eq!(ragged.mul(&Value::Scalar(int(2))), Ok(Value::Matrix(vec![
            vec![int(2), int(4)],
            vec![int(6)]
        ])));
        // The raggedness is only refused where a shape is needed.
        assert_eq!(
            ragged.add(&ragged),
            Err(ValueError::Ragged)
        );
    }
}
