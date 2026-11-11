//! Compile an expression to a flat instruction stream, once, then evaluate it
//! in a tight loop.
//!
//! Tree-walking evaluation is 10–100x slower than this: every node visit is an
//! enum match plus a cache miss. Plotting evaluates thousands of points per
//! frame, so the flat form is not an optimisation — it is the only version fast
//! enough to ship.
//!
//! The same callers run the evaluator millions of times, so a compiled
//! [`Program`] records the deepest its instruction stream leaves the operand
//! stack. With that number known at compile time, [`Program::eval`] can use a
//! fixed array and [`Program::eval_into`] can borrow the caller's buffer; the
//! `Vec` the old implementation allocated on every call is gone from both paths.

use crate::expr::{Expr, ExprNode};
use crate::symbol::SymbolId;

/// Programs whose operand stack never exceeds this many slots evaluate from a
/// fixed array, so [`Program::eval`] allocates nothing.
///
/// Thirty-two covers ordinary expressions; a deeply right-nested one (a chain
/// of powers, say) can exceed it, and a caller in a hot loop should then use
/// [`Program::eval_into`] with its own buffer rather than pay for the heap.
pub const INLINE_STACK: usize = 32;

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
    max_stack: usize,
}

impl Program {
    /// Compile `expr` in terms of `vars`. Returns `None` for anything that
    /// cannot be evaluated numerically, such as an unevaluated integral.
    pub fn compile(expr: &Expr, vars: &[SymbolId]) -> Option<Program> {
        let mut ops = Vec::new();
        emit(expr, vars, &mut ops)?;
        Some(Program {
            max_stack: required_stack(&ops),
            ops,
            n_vars: vars.len() as u16,
        })
    }

    /// The peak depth of the operand stack, recorded while compiling. It is the
    /// size of the buffer [`Program::eval_into`] needs and the reason
    /// [`Program::eval`] can avoid the heap on the common path.
    pub fn max_stack(&self) -> usize {
        self.max_stack
    }

    /// Evaluate. Out-of-domain results surface as `NaN`; poles as `±Inf`. The
    /// sampler in `canlab-plot` relies on exactly that convention.
    ///
    /// Allocates only when the program is deeper than [`INLINE_STACK`]; for the
    /// expressions plotting walks it does not.
    #[inline]
    pub fn eval(&self, args: &[f64]) -> f64 {
        if self.max_stack <= INLINE_STACK {
            let mut stack = [0.0; INLINE_STACK];
            self.eval_into(args, &mut stack[..self.max_stack])
        } else {
            let mut stack = vec![0.0; self.max_stack];
            self.eval_into(args, &mut stack)
        }
    }

    /// Evaluate into a caller-provided stack of at least
    /// [`Program::max_stack`] slots. Nothing is allocated, whatever the
    /// program's depth — this is the version for an ODE integrator's inner loop.
    ///
    /// # Panics
    ///
    /// Panics if `stack` is shorter than [`Program::max_stack`]. A short buffer
    /// is a programming error, and returning `NaN` would be indistinguishable
    /// from a domain error and so would hide it.
    #[inline]
    pub fn eval_into(&self, args: &[f64], stack: &mut [f64]) -> f64 {
        assert!(
            stack.len() >= self.max_stack,
            "Program::eval_into needs a stack of {} slots, got {}",
            self.max_stack,
            stack.len()
        );

        // `sp` is the operand-stack height, and this bookkeeping is exactly what
        // `required_stack` replays over the same stream at compile time. The two
        // are kept side by side so one number genuinely bounds the other.
        let mut sp = 0usize;
        for op in &self.ops {
            match *op {
                Op::Const(c) => {
                    stack[sp] = c;
                    sp += 1;
                }
                Op::Var(i) => {
                    stack[sp] = args.get(i as usize).copied().unwrap_or(f64::NAN);
                    sp += 1;
                }
                Op::Neg => {
                    let a = pop(stack, &mut sp);
                    stack[sp] = -a;
                    sp += 1;
                }
                Op::Add(n) => {
                    // Saturating because the old `Vec` implementation used
                    // `saturating_sub` too: an `Add` naming more operands than
                    // the stack holds sums what is there instead of underflowing.
                    let start = sp.saturating_sub(n as usize);
                    // `-0.0` is the identity `Iterator::sum` uses for `f64`, and
                    // the only zero that leaves every operand's sign alone:
                    // starting from `+0.0` would turn `(-0.0) + (-0.0)` into
                    // `+0.0`, and `asin` of that is a different pole direction.
                    let mut sum = -0.0;
                    for value in &stack[start..sp] {
                        sum += *value;
                    }
                    sp = start + 1;
                    stack[start] = sum;
                }
                Op::Mul(n) => {
                    let start = sp.saturating_sub(n as usize);
                    let mut product = 1.0;
                    for value in &stack[start..sp] {
                        product *= *value;
                    }
                    sp = start + 1;
                    stack[start] = product;
                }
                Op::Sub => {
                    let b = pop(stack, &mut sp);
                    let a = pop(stack, &mut sp);
                    stack[sp] = a - b;
                    sp += 1;
                }
                Op::Div => {
                    let b = pop(stack, &mut sp);
                    let a = pop(stack, &mut sp);
                    stack[sp] = a / b;
                    sp += 1;
                }
                Op::Pow => {
                    let b = pop(stack, &mut sp);
                    let a = pop(stack, &mut sp);
                    stack[sp] = a.powf(b);
                    sp += 1;
                }
                unary => {
                    let a = pop(stack, &mut sp);
                    stack[sp] = apply(unary, a);
                    sp += 1;
                }
            }
        }

        if sp == 0 { f64::NAN } else { stack[sp - 1] }
    }

    /// Evaluate `out.len()` rows in one pass, row `r` taking its variables from
    /// `args[r * stride]` onwards.
    ///
    /// This is the reference the GPU path will be compared against, so every row
    /// goes through [`Program::eval_into`]: the same arithmetic in the same order
    /// as [`Program::eval`], and therefore bit-identical.
    ///
    /// A `stride` smaller than `n_vars` makes adjacent rows share variables; that
    /// is well defined, not an error. Slots between rows are never read, and a
    /// row whose variables lie past the end of `args` reads `NaN`, exactly as
    /// `eval` does when too few arguments are supplied.
    pub fn eval_batch(&self, args: &[f64], stride: usize, out: &mut [f64]) {
        let mut inline = [0.0; INLINE_STACK];
        let mut heap = Vec::new();
        let stack: &mut [f64] = if self.max_stack <= INLINE_STACK {
            &mut inline[..self.max_stack]
        } else {
            heap.resize(self.max_stack, 0.0);
            &mut heap
        };

        for (row, slot) in out.iter_mut().enumerate() {
            *slot = self.eval_into(self.row_args(args, stride, row), stack);
        }
    }

    /// Row `row`'s variables, clipped to what `args` actually holds so the
    /// missing ones keep `eval`'s `NaN` convention instead of panicking.
    fn row_args<'a>(&self, args: &'a [f64], stride: usize, row: usize) -> &'a [f64] {
        let start = row.saturating_mul(stride).min(args.len());
        let end = start.saturating_add(self.n_vars as usize).min(args.len());
        &args[start..end]
    }

    pub fn n_vars(&self) -> u16 {
        self.n_vars
    }

    pub fn ops(&self) -> &[Op] {
        &self.ops
    }
}

/// Pop one operand, reading `NaN` from an empty stack — the same answer the
/// previous implementation got from `Vec::pop().unwrap_or(NAN)`.
#[inline]
fn pop(stack: &[f64], sp: &mut usize) -> f64 {
    if *sp == 0 {
        f64::NAN
    } else {
        *sp -= 1;
        stack[*sp]
    }
}

/// The stack depth the instruction stream reaches, derived from the same
/// pop/push bookkeeping `eval_into` uses. Computed once at compile time so no
/// call pays for it, and kept beside the interpreter so the two cannot drift.
fn required_stack(ops: &[Op]) -> usize {
    let mut depth = 0usize;
    let mut peak = 0usize;
    for op in ops {
        let (pops, pushes) = match *op {
            Op::Const(_) | Op::Var(_) => (0, 1),
            Op::Neg => (1, 1),
            Op::Add(n) | Op::Mul(n) => (n as usize, 1),
            Op::Sub | Op::Div | Op::Pow => (2, 1),
            Op::Sin
            | Op::Cos
            | Op::Tan
            | Op::Ln
            | Op::Exp
            | Op::Sqrt
            | Op::Abs
            | Op::Asin
            | Op::Acos
            | Op::Atan
            | Op::Sinh
            | Op::Cosh
            | Op::Tanh => (1, 1),
        };
        depth = depth.saturating_sub(pops) + pushes;
        peak = peak.max(depth);
    }
    peak
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

    /// The compiler records `max_stack` from the same pop/push rules the
    /// interpreter runs, so a program this test cannot see inside still has to
    /// agree with a hand-derived peak.
    #[test]
    fn max_stack_is_the_peak_a_nested_program_reaches() {
        // Raw constructors are used on purpose: normalization sorts the factors
        // of a product, and the order would decide which subtree the peak falls
        // in. Raw, the stream is exactly this shape:
        //
        //   sin(x) * exp(x*y) / (1 + x^2 + y^2)
        //
        // The peak comes while the denominator is built: the stack holds the
        // numerator's result, the two terms of the sum already built, and the
        // `2` exponent of the second square — five slots.
        let x = sym("x");
        let y = sym("y");
        let xi = SymbolId::intern("x");
        let yi = SymbolId::intern("y");
        let numerator = crate::expr::ExprNode::mul_raw(vec![
            crate::expr::ExprNode::fn_raw(SymbolId::intern("sin"), vec![x.clone()]),
            crate::expr::ExprNode::fn_raw(
                SymbolId::intern("exp"),
                vec![crate::expr::ExprNode::mul_raw(vec![x.clone(), y.clone()])],
            ),
        ]);
        let denominator = crate::expr::ExprNode::add_raw(vec![
            int(1),
            crate::expr::ExprNode::pow_raw(x.clone(), int(2)),
            crate::expr::ExprNode::pow_raw(y.clone(), int(2)),
        ]);
        let expression = crate::expr::ExprNode::mul_raw(vec![
            numerator,
            crate::expr::ExprNode::pow_raw(denominator, int(-1)),
        ]);

        let program = Program::compile(&expression, &[xi, yi]).expect("compiles");
        assert_eq!(program.max_stack(), 5);

        // Exactly `max_stack` slots must be enough: a write past the end would
        // panic here, and the answer must match the allocating path.
        let mut stack = vec![f64::NAN; program.max_stack()];
        let direct = program.eval_into(&[0.5, 0.25], &mut stack);
        assert_eq!(direct.to_bits(), program.eval(&[0.5, 0.25]).to_bits());
    }

    fn program_from(ops: Vec<Op>, n_vars: u16) -> Program {
        Program {
            max_stack: required_stack(&ops),
            ops,
            n_vars,
        }
    }

    /// `emit` lowers subtraction to `Add(-1 * b)` and division to `Mul(b^-1)`,
    /// so `Sub`, `Div` and `Neg` can be produced only by a hand-built stream
    /// today. They are still part of the machine, and `Div` is what documents
    /// the pole convention: `1/0` is `+Inf`, not `NaN`.
    #[test]
    fn sub_div_and_negate_follow_the_documented_convention() {
        let difference = program_from(vec![Op::Const(3.0), Op::Const(5.0), Op::Sub], 0);
        assert_eq!(difference.eval(&[]), -2.0);

        let quotient = program_from(vec![Op::Const(1.0), Op::Const(0.0), Op::Div], 0);
        assert_eq!(quotient.eval(&[]), f64::INFINITY);

        let negative = program_from(vec![Op::Const(2.0), Op::Neg], 0);
        assert_eq!(negative.eval(&[]), -2.0);

        // Negating an empty stack is `NaN`, not a panic: the pre-W0.2 evaluator
        // read `Vec::pop().unwrap_or(NAN)`, and `eval_into` kept that behaviour
        // so a malformed hand-built program cannot become a crash.
        let empty = program_from(vec![Op::Neg], 0);
        assert!(empty.eval(&[]).is_nan());
    }
}
