//! Canonicalization.
//!
//! This is the only place that decides what "normal form" means. Everything
//! else — equality checks, the LaTeX printer, the solver bridge — relies on the
//! invariants established here.
//!
//! What normalization does *not* do is apply any simplification that would
//! change the domain: `x/x` stays put, `(x^2)^(1/2)` stays put. Those need
//! assumptions about the sign of `x`, and guessing is how CASs end up giving
//! wrong answers to schoolchildren.

use std::collections::BTreeMap;

use crate::expr::{Expr, ExprNode, Hold};
use crate::number::Number;
use crate::symbol::SymbolId;

/// Recursively put `expr` into canonical form.
pub fn normalize(expr: &Expr) -> Expr {
    match &**expr {
        ExprNode::Num(n) => ExprNode::num(n.clone()),
        ExprNode::Sym(s) => ExprNode::sym(*s),
        ExprNode::Add(terms) => add_norm(terms.iter().map(normalize)),
        ExprNode::Mul(factors) => mul_norm(factors.iter().map(normalize)),
        ExprNode::Pow(base, exp) => pow_norm(normalize(base), normalize(exp)),
        ExprNode::Fn(name, args) => fn_norm(*name, args.iter().map(normalize).collect()),
        ExprNode::Hold(hold) => match &**hold {
            Hold::Integral { body, var } => ExprNode::hold(Hold::Integral {
                body: normalize(body),
                var: *var,
            }),
            // The body is normalised and the accent is kept: `\vec{x + y}` and
            // `\vec{y + x}` are the same decoration of the same expression.
            Hold::Accent { kind, body } => ExprNode::hold(Hold::Accent {
                kind: *kind,
                body: normalize(body),
            }),
            Hold::Matrix { rows } => ExprNode::hold(Hold::Matrix {
                rows: rows.iter().map(|row| row.iter().map(normalize).collect()).collect(),
            }),
        },
    }
}

/// Build a normalized sum.
pub fn add<I: IntoIterator<Item = Expr>>(items: I) -> Expr {
    add_norm(items.into_iter().map(|e| normalize(&e)))
}

/// Build a normalized product.
pub fn mul<I: IntoIterator<Item = Expr>>(items: I) -> Expr {
    mul_norm(items.into_iter().map(|e| normalize(&e)))
}

/// Build a normalized power.
pub fn pow(base: Expr, exp: Expr) -> Expr {
    pow_norm(normalize(&base), normalize(&exp))
}

fn add_norm<I: IntoIterator<Item = Expr>>(items: I) -> Expr {
    let mut constant = Number::zero();
    let mut groups: BTreeMap<Expr, Number> = BTreeMap::new();
    let mut stack: Vec<Expr> = items.into_iter().collect();

    while let Some(term) = stack.pop() {
        match &*term {
            ExprNode::Num(n) => constant = constant.add(n),
            ExprNode::Add(inner) => stack.extend(inner.iter().cloned()),
            _ => {
                let (coeff, base) = term.split_coeff();
                let slot = groups.entry(base).or_insert_with(Number::zero);
                *slot = slot.add(&coeff);
            }
        }
    }

    let mut terms: Vec<Expr> = groups
        .into_iter()
        .filter_map(|(base, coeff)| {
            if coeff.is_zero() {
                None
            } else if coeff.is_one() {
                Some(base)
            } else {
                Some(attach_coeff(coeff, base))
            }
        })
        .collect();

    if !constant.is_zero() {
        terms.push(ExprNode::num(constant));
    }
    terms.sort();

    match terms.len() {
        0 => ExprNode::int(0),
        1 => terms.pop().expect("length checked"),
        _ => ExprNode::add_raw(terms),
    }
}

fn attach_coeff(coeff: Number, base: Expr) -> Expr {
    match &*base {
        ExprNode::Num(n) => ExprNode::num(coeff.mul(n)),
        ExprNode::Mul(factors) => {
            let mut out = Vec::with_capacity(factors.len() + 1);
            out.push(ExprNode::num(coeff));
            out.extend(factors.iter().cloned());
            ExprNode::mul_raw(out)
        }
        _ => ExprNode::mul_raw(vec![ExprNode::num(coeff), base]),
    }
}

fn mul_norm<I: IntoIterator<Item = Expr>>(items: I) -> Expr {
    let mut coeff = Number::one();
    let mut powers: BTreeMap<Expr, Number> = BTreeMap::new();
    let mut stack: Vec<Expr> = items.into_iter().collect();

    while let Some(factor) = stack.pop() {
        match &*factor {
            ExprNode::Num(n) => coeff = coeff.mul(n),
            ExprNode::Mul(inner) => stack.extend(inner.iter().cloned()),
            ExprNode::Pow(base, exp) => match &**exp {
                ExprNode::Num(n) => {
                    let slot = powers
                        .entry(base.clone())
                        .or_insert_with(Number::zero);
                    *slot = slot.add(n);
                }
                _ => bump(&mut powers, factor.clone()),
            },
            _ => bump(&mut powers, factor.clone()),
        }
    }

    if coeff.is_zero() {
        return ExprNode::int(0);
    }

    let mut factors: Vec<Expr> = Vec::new();
    for (base, exp) in powers {
        if exp.is_zero() {
            continue;
        }
        if exp.is_one() {
            factors.push(base);
        } else {
            factors.push(ExprNode::pow_raw(base, ExprNode::num(exp)));
        }
    }

    if !coeff.is_one() || factors.is_empty() {
        factors.insert(0, ExprNode::num(coeff));
    }

    match factors.len() {
        0 => ExprNode::int(1),
        1 => factors.pop().expect("length checked"),
        _ => ExprNode::mul_raw(factors),
    }
}

fn bump(powers: &mut BTreeMap<Expr, Number>, factor: Expr) {
    let slot = powers.entry(factor).or_insert_with(Number::zero);
    *slot = slot.add(&Number::one());
}

fn pow_norm(base: Expr, exp: Expr) -> Expr {
    if let ExprNode::Num(n) = &*exp {
        if n.is_zero() {
            return ExprNode::int(1);
        }
        if n.is_one() {
            return base;
        }
    }

    if let ExprNode::Num(b) = &*base {
        if b.is_one() {
            return ExprNode::int(1);
        }
        if let ExprNode::Num(e) = &*exp {
            if let Some(value) = b.pow(e) {
                return ExprNode::num(value);
            }
        }
    }

    // `(x^a)^b = x^(a*b)` is only valid without assumptions when `b` is a
    // non-negative integer. `(x^2)^(1/2)` is `|x|`, not `x`.
    if let ExprNode::Pow(inner_base, inner_exp) = &*base {
        if let (ExprNode::Num(a), ExprNode::Num(b)) = (&**inner_exp, &*exp) {
            if !b.is_negative() && b.is_integer() {
                return pow_norm(inner_base.clone(), ExprNode::num(a.mul(b)));
            }
        }
    }

    ExprNode::pow_raw(base, exp)
}

fn fn_norm(name: SymbolId, args: Vec<Expr>) -> Expr {
    let fname = name.as_str();

    if let Some(first) = args.first() {
        if let ExprNode::Num(x) = &**first {
            if x.is_zero() {
                match fname.as_str() {
                    "sin" | "tan" | "sinh" | "tanh" | "ln" | "log" => {
                        return ExprNode::int(0);
                    }
                    "cos" | "cosh" | "exp" => return ExprNode::int(1),
                    _ => {}
                }
            }
            if x.is_one() && matches!(fname.as_str(), "ln" | "log") {
                return ExprNode::int(0);
            }
        }
    }

    ExprNode::fn_raw(name, args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{int, sym};

    fn x() -> Expr {
        sym("x")
    }

    #[test]
    fn like_terms_collect() {
        let e = add([mul([int(2), x()]), mul([int(3), x()])]);
        assert_eq!(e.to_string(), "5*x");
    }

    #[test]
    fn like_factors_combine() {
        let e = mul([x(), x()]);
        assert_eq!(e.to_string(), "x^2");
    }

    #[test]
    fn powers_fold() {
        assert_eq!(mul([pow(x(), int(2)), pow(x(), int(3))]).to_string(), "x^5");
        assert_eq!(pow(x(), int(0)).to_string(), "1");
        assert_eq!(pow(x(), int(1)).to_string(), "x");
    }

    #[test]
    fn sqrt_of_square_stays_put() {
        // (x^2)^(1/2) is |x|; do not silently drop the absolute value.
        let e = pow(pow(x(), int(2)), crate::expr::ratio(1, 2));
        assert_eq!(e.to_string(), "(x^2)^(1/2)");
    }

    #[test]
    fn constants_fold() {
        let e = add([crate::expr::ratio(1, 3), crate::expr::ratio(1, 6)]);
        assert_eq!(e.to_string(), "1/2");
    }

    #[test]
    fn zero_and_one_identities() {
        assert_eq!(add([x(), int(0)]).to_string(), "x");
        assert_eq!(mul([x(), int(1)]).to_string(), "x");
        assert_eq!(mul([x(), int(0)]).to_string(), "0");
        assert_eq!(add([x(), mul([int(-1), x()])]).to_string(), "0");
    }

    #[test]
    fn exact_roots_evaluate() {
        let e = pow(int(4), crate::expr::ratio(1, 2));
        assert_eq!(e.to_string(), "2");
    }

    #[test]
    fn normalization_is_idempotent() {
        let e = add([
            mul([int(2), pow(x(), int(2))]),
            mul([int(3), x()]),
            int(1),
            mul([int(-2), pow(x(), int(2))]),
        ]);
        let once = normalize(&e);
        let twice = normalize(&once);
        assert_eq!(once.to_string(), twice.to_string());
        assert_eq!(once.to_string(), "1 + 3*x");
    }

    #[test]
    fn nary_add_nests_flat() {
        let e = add([add([add([x(), int(1)])]), mul([int(2), x()])]);
        assert_eq!(e.to_string(), "1 + 3*x");
    }
}
