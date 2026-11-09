//! Compile an expression to a flat instruction stream, once, then evaluate it
//! in a tight loop.
//!
//! Tree-walking evaluation is 10–100x slower than this: every node visit is an
//! enum match plus a cache miss. Plotting evaluates thousands of points per
//! frame, so the flat form is not an optimisation — it is the only version fast
//! enough to ship.

use crate::expr::{Expr, ExprNode};
use crate::symbol::SymbolId;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    Const(f64),
    Var(u16),
    Neg,
    Add(u16),
    Mul(u16),
    Sub,
    Div,
    Pow,
    Sin,
    Cos,
    Tan,
    Ln,
    Exp,
    Sqrt,
    Abs,
    Asin,
    Acos,
    Atan,
    Sinh,
    Cosh,
    Tanh,
}

#[derive(Clone, Debug)]
pub struct Program {
    ops: Vec<Op>,
    n_vars: u16,
}

impl Program {
    /// Compile `expr` in terms of `vars`. Returns `None` for anything that
    /// cannot be evaluated numerically, such as an unevaluated integral.
    pub fn compile(expr: &Expr, vars: &[SymbolId]) -> Option<Program> {
        let mut ops = Vec::new();
        emit(expr, vars, &mut ops)?;
        Some(Program {
            ops,
            n_vars: vars.len() as u16,
        })
    }

    /// Evaluate. Out-of-domain results surface as `NaN`; poles as `±Inf`. The
    /// sampler in `canlab-plot` relies on exactly that convention.
    pub fn eval(&self, args: &[f64]) -> f64 {
        let mut stack: Vec<f64> = Vec::with_capacity(16);

        for op in &self.ops {
            match *op {
                Op::Const(c) => stack.push(c),
                Op::Var(i) => stack.push(args.get(i as usize).copied().unwrap_or(f64::NAN)),
                Op::Neg => {
                    let a = stack.pop().unwrap_or(f64::NAN);
                    stack.push(-a);
                }
                Op::Add(n) => {
                    let start = stack.len().saturating_sub(n as usize);
                    let sum: f64 = stack[start..].iter().sum();
                    stack.truncate(start);
                    stack.push(sum);
                }
                Op::Mul(n) => {
                    let start = stack.len().saturating_sub(n as usize);
                    let product: f64 = stack[start..].iter().product();
                    stack.truncate(start);
                    stack.push(product);
                }
                Op::Sub => {
                    let b = stack.pop().unwrap_or(f64::NAN);
                    let a = stack.pop().unwrap_or(f64::NAN);
                    stack.push(a - b);
                }
                Op::Div => {
                    let b = stack.pop().unwrap_or(f64::NAN);
                    let a = stack.pop().unwrap_or(f64::NAN);
                    stack.push(a / b);
                }
                Op::Pow => {
                    let b = stack.pop().unwrap_or(f64::NAN);
                    let a = stack.pop().unwrap_or(f64::NAN);
                    stack.push(a.powf(b));
                }
                unary => {
                    let a = stack.pop().unwrap_or(f64::NAN);
                    stack.push(apply(unary, a));
                }
            }
        }

        stack.pop().unwrap_or(f64::NAN)
    }

    pub fn n_vars(&self) -> u16 {
        self.n_vars
    }

    pub fn ops(&self) -> &[Op] {
        &self.ops
    }
}

fn apply(op: Op, a: f64) -> f64 {
    match op {
        Op::Sin => a.sin(),
        Op::Cos => a.cos(),
        Op::Tan => a.tan(),
        Op::Ln => a.ln(),
        Op::Exp => a.exp(),
        Op::Sqrt => a.sqrt(),
        Op::Abs => a.abs(),
        Op::Asin => a.asin(),
        Op::Acos => a.acos(),
        Op::Atan => a.atan(),
        Op::Sinh => a.sinh(),
        Op::Cosh => a.cosh(),
        Op::Tanh => a.tanh(),
        _ => f64::NAN,
    }
}

fn emit(expr: &Expr, vars: &[SymbolId], ops: &mut Vec<Op>) -> Option<()> {
    match &**expr {
        ExprNode::Num(n) => ops.push(Op::Const(n.to_f64())),
        ExprNode::Sym(s) => match s.as_str().as_str() {
            "pi" => ops.push(Op::Const(std::f64::consts::PI)),
            "e" => ops.push(Op::Const(std::f64::consts::E)),
            "tau" => ops.push(Op::Const(std::f64::consts::TAU)),
            _ => {
                let index = vars.iter().position(|v| *v == *s)?;
                ops.push(Op::Var(index as u16));
            }
        },
        ExprNode::Add(terms) => {
            for term in terms {
                emit(term, vars, ops)?;
            }
            ops.push(Op::Add(terms.len() as u16));
        }
        ExprNode::Mul(factors) => {
            for factor in factors {
                emit(factor, vars, ops)?;
            }
            ops.push(Op::Mul(factors.len() as u16));
        }
        ExprNode::Pow(base, exp) => {
            emit(base, vars, ops)?;
            emit(exp, vars, ops)?;
            ops.push(Op::Pow);
        }
        ExprNode::Fn(name, args) => {
            if args.len() != 1 {
                return None;
            }
            emit(&args[0], vars, ops)?;
            let fname = name.as_str();
            let op = match fname.as_str() {
                "sin" => Op::Sin,
                "cos" => Op::Cos,
                "tan" => Op::Tan,
                "ln" => Op::Ln,
                "log" => Op::Ln,
                "exp" => Op::Exp,
                "sqrt" => Op::Sqrt,
                "abs" => Op::Abs,
                "asin" | "arcsin" => Op::Asin,
                "acos" | "arccos" => Op::Acos,
                "atan" | "arctan" => Op::Atan,
                "sinh" => Op::Sinh,
                "cosh" => Op::Cosh,
                "tanh" => Op::Tanh,
                _ => return None,
            };
            ops.push(op);
        }
        ExprNode::Hold(_) => return None,
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{int, sym};
    use crate::normalize::{add, mul, pow};
    use crate::symbol::SymbolId;

    #[test]
    fn accepts_the_plan_target() {
        // The phase-1 acceptance test from the plan: (x+1)^2 at x = 2 is 9.
        let x = sym("x");
        let e = pow(add([x.clone(), int(1)]), int(2));
        let xi = SymbolId::intern("x");
        let program = Program::compile(&e, &[xi]).expect("compiles");
        assert_eq!(program.eval(&[2.0]), 9.0);
    }

    #[test]
    fn arithmetic() {
        let x = sym("x");
        let xi = SymbolId::intern("x");
        let e = add([mul([int(3), x.clone()]), int(1)]);
        let program = Program::compile(&e, &[xi]).unwrap();
        assert_eq!(program.eval(&[2.0]), 7.0);
    }

    #[test]
    fn constants_are_constants() {
        let x = sym("x");
        let xi = SymbolId::intern("x");
        let e = mul([sym("pi"), x]);
        let program = Program::compile(&e, &[xi]).unwrap();
        assert!((program.eval(&[1.0]) - std::f64::consts::PI).abs() < 1e-15);
    }

    #[test]
    fn unknown_symbol_is_none() {
        let e = sym("y");
        let xi = SymbolId::intern("x");
        assert!(Program::compile(&e, &[xi]).is_none());
    }

    #[test]
    fn unary_functions() {
        let x = sym("x");
        let xi = SymbolId::intern("x");
        let e = crate::expr::ExprNode::fn_raw(SymbolId::intern("sin"), vec![x]);
        let program = Program::compile(&e, &[xi]).unwrap();
        assert!((program.eval(&[0.0])).abs() < 1e-15);
    }

    #[test]
    fn domain_errors_surface_as_nan() {
        let x = sym("x");
        let xi = SymbolId::intern("x");
        // sqrt(x) at x = -1
        let e = pow(x, crate::expr::ratio(1, 2));
        let program = Program::compile(&e, &[xi]).unwrap();
        assert!(program.eval(&[-1.0]).is_nan());
    }
}
