//! W0.2: the evaluator's three entry points must agree, bit for bit.
//!
//! The oracle is the evaluator as it stood before this change — a `Vec` stack
//! allocated on every call — reproduced below on purpose rather than shared
//! with the crate. A test that called the crate's own helper could not detect a
//! change in it; this one can.
//!
//! Values are compared by bit. The one exception is `NaN` against `NaN`: its
//! payload and sign are decided by which register the compiler picked for an
//! addition of two NaNs, which is code generation, not arithmetic.
//! `assert_same_result` carries that argument, and the random test counts how
//! often it applies instead of hiding it.

use std::collections::BTreeSet;
use std::hint::black_box;
use std::time::Instant;

use canlab_core::{Expr, ExprNode, Op, Program, SymbolId, add, int, mul, pow, ratio, sym};

const VARIABLE_NAMES: [&str; 3] = ["x", "y", "z"];

const FUNCTIONS: [&str; 13] = [
    "sin", "cos", "tan", "ln", "exp", "sqrt", "abs", "asin", "acos", "atan", "sinh", "cosh", "tanh",
];

fn variable_ids() -> Vec<SymbolId> {
    VARIABLE_NAMES
        .iter()
        .map(|name| SymbolId::intern(name))
        .collect()
}

fn function(name: &str, argument: Expr) -> Expr {
    // There is no normalizing constructor for `Fn`; raw is the only way to put
    // every function in the corpus.
    ExprNode::fn_raw(SymbolId::intern(name), vec![argument])
}

// ---------------------------------------------------------------------------
// The pre-W0.2 evaluator, kept as the oracle
// ---------------------------------------------------------------------------

fn reference_eval(program: &Program, args: &[f64]) -> f64 {
    let mut stack: Vec<f64> = Vec::with_capacity(16);

    for op in program.ops() {
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
                stack.push(reference_apply(unary, a));
            }
        }
    }

    stack.pop().unwrap_or(f64::NAN)
}

fn reference_apply(op: Op, a: f64) -> f64 {
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

// ---------------------------------------------------------------------------
// A seeded generator, so the corpus is the same on every run
// ---------------------------------------------------------------------------

/// A 64-bit linear congruential generator. `rand` is not a dependency of this
/// crate and does not need to become one for a test.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Lcg(seed)
    }

    fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        // The high bits of an LCG are the good ones; the low bits cycle.
        (self.0 >> 32) as u32
    }

    fn below(&mut self, bound: usize) -> usize {
        self.next_u32() as usize % bound
    }
}

/// A tree of height at most `depth`, in terms of `variables`, using integers,
/// small rationals, and every operator the tree can express.
///
/// Subtraction and division are written the way the parser writes them — as
/// `a + (-1)*b` and `a * b^-1` — because the tree has no `Sub` or `Div` node.
fn random_expr(rng: &mut Lcg, depth: u32, variables: &[Expr]) -> Expr {
    if depth == 0 {
        return match rng.below(6) {
            0 | 1 => variables[rng.below(variables.len())].clone(),
            2 | 3 => int(rng.below(11) as i64 - 5),
            _ => {
                let denominator = rng.below(4) as i64 + 1;
                let numerator = rng.below(9) as i64 - 4;
                ratio(numerator, denominator)
            }
        };
    }

    match rng.below(16) {
        0..=3 => {
            let count = 2 + rng.below(2);
            let terms = (0..count)
                .map(|_| random_expr(rng, depth - 1, variables))
                .collect::<Vec<_>>();
            add(terms)
        }
        4..=6 => {
            let count = 2 + rng.below(2);
            let factors = (0..count)
                .map(|_| random_expr(rng, depth - 1, variables))
                .collect::<Vec<_>>();
            mul(factors)
        }
        7 | 8 => {
            let base = random_expr(rng, depth - 1, variables);
            let exponent = random_expr(rng, depth - 1, variables);
            pow(base, exponent)
        }
        9 => {
            let left = random_expr(rng, depth - 1, variables);
            let right = random_expr(rng, depth - 1, variables);
            add([left, mul([int(-1), right])])
        }
        10 => {
            let numerator = random_expr(rng, depth - 1, variables);
            let denominator = random_expr(rng, depth - 1, variables);
            mul([numerator, pow(denominator, int(-1))])
        }
        _ => {
            let argument = random_expr(rng, depth - 1, variables);
            function(FUNCTIONS[rng.below(FUNCTIONS.len())], argument)
        }
    }
}

/// Argument vectors deliberately include signed zero, poles, subnormals' worth
/// of small values and `NaN`/`±Inf`: the entry points must agree on those too,
/// and `NaN` payloads are compared bit for bit like everything else.
fn random_args(rng: &mut Lcg, count: usize) -> Vec<f64> {
    (0..count)
        .map(|_| match rng.below(9) {
            0 => 0.0,
            1 => -0.0,
            2 => 1.0,
            3 => -1.0,
            4 => f64::NAN,
            5 => f64::INFINITY,
            6 => f64::NEG_INFINITY,
            7 => -2.5,
            _ => (rng.next_u32() as f64 / u32::MAX as f64) * 8.0 - 4.0,
        })
        .collect()
}

fn op_name(op: Op) -> &'static str {
    match op {
        Op::Const(_) => "const",
        Op::Var(_) => "var",
        Op::Neg => "neg",
        Op::Add(_) => "add",
        Op::Mul(_) => "mul",
        Op::Sub => "sub",
        Op::Div => "div",
        Op::Pow => "pow",
        Op::Sin => "sin",
        Op::Cos => "cos",
        Op::Tan => "tan",
        Op::Ln => "ln",
        Op::Exp => "exp",
        Op::Sqrt => "sqrt",
        Op::Abs => "abs",
        Op::Asin => "asin",
        Op::Acos => "acos",
        Op::Atan => "atan",
        Op::Sinh => "sinh",
        Op::Cosh => "cosh",
        Op::Tanh => "tanh",
        Op::Binom => "binom",
    }
}

/// The peak the stream reaches, computed by simulation rather than from the
/// compiler's arithmetic. This is the independent check on `max_stack`.
fn stack_peak(program: &Program) -> usize {
    let mut stack: Vec<()> = Vec::new();
    let mut peak = 0usize;
    for op in program.ops() {
        match *op {
            Op::Const(_) | Op::Var(_) => stack.push(()),
            Op::Neg => {
                stack.pop();
                stack.push(());
            }
            Op::Add(n) | Op::Mul(n) => {
                for _ in 0..n as usize {
                    stack.pop();
                }
                stack.push(());
            }
            Op::Sub | Op::Div | Op::Pow => {
                stack.pop();
                stack.pop();
                stack.push(());
            }
            _ => {
                stack.pop();
                stack.push(());
            }
        }
        peak = peak.max(stack.len());
    }
    peak
}

/// Compare one result. Everything that has a value must agree to the bit,
/// including `-0.0` and `±Inf`. Two `NaN`s agree whatever their payload: when
/// both operands of an addition are `NaN`, which one is propagated depends on
/// the register the compiler chose for the destination, which is code
/// generation rather than arithmetic. `nan_payloads` counts the cases where only
/// that differed, so the test reports them instead of hiding them.
fn assert_same_result(label: &str, case: usize, got: f64, want: f64, nan_payloads: &mut usize) {
    if got.to_bits() == want.to_bits() {
        return;
    }
    if got.is_nan() && want.is_nan() {
        *nan_payloads += 1;
        return;
    }
    panic!(
        "{label} disagreed on case {case}: got {got:?} ({:#x}), expected {want:?} ({:#x})",
        got.to_bits(),
        want.to_bits()
    );
}

// ---------------------------------------------------------------------------
// The tests
// ---------------------------------------------------------------------------

#[test]
fn entry_points_agree_bit_for_bit_on_ten_thousand_random_expressions() {
    let variables = VARIABLE_NAMES
        .iter()
        .map(|name| sym(name))
        .collect::<Vec<_>>();
    let ids = variable_ids();
    let n_vars = ids.len();
    let mut rng = Lcg::new(0xC0FF_EE12_345678);
    let mut seen: BTreeSet<&'static str> = BTreeSet::new();
    let mut nan_payloads = 0usize;

    for case in 0..10_000 {
        let expression = random_expr(&mut rng, 5, &variables);
        let program = Program::compile(&expression, &ids)
            .unwrap_or_else(|| panic!("case {case} did not compile: {expression}"));

        for op in program.ops() {
            seen.insert(op_name(*op));
        }

        // The recorded depth must be exactly the peak this stream reaches. If
        // it were too small, the `eval_into` buffer below would be too small
        // and the run would panic on a write past its end.
        assert_eq!(
            program.max_stack(),
            stack_peak(&program),
            "case {case}: {expression}"
        );

        let args = random_args(&mut rng, n_vars);
        let want = reference_eval(&program, &args);

        assert_same_result("eval", case, program.eval(&args), want, &mut nan_payloads);

        let mut stack = vec![f64::NAN; program.max_stack()];
        assert_same_result(
            "eval_into",
            case,
            program.eval_into(&args, &mut stack),
            want,
            &mut nan_payloads,
        );

        // Rows are spaced wider than the variables on purpose: the gap slots
        // must never be read, and every row must match the reference exactly.
        let stride = n_vars + 2;
        let rows = 3;
        let mut batch_args = Vec::with_capacity(stride * rows);
        let mut expected = Vec::with_capacity(rows);
        for _ in 0..rows {
            let row = random_args(&mut rng, n_vars);
            batch_args.extend_from_slice(&row);
            batch_args.extend_from_slice(&[f64::NAN, f64::NEG_INFINITY]);
            expected.push(reference_eval(&program, &row));
        }
        let mut out = vec![f64::NAN; rows];
        program.eval_batch(&batch_args, stride, &mut out);
        for (row, want_row) in expected.iter().enumerate() {
            assert_same_result("eval_batch", case, out[row], *want_row, &mut nan_payloads);
        }
    }

    // Every operator the tree can express has to appear in the corpus, or the
    // 10 000 cases are not covering what they claim to.
    for expected_op in [
        "const", "var", "add", "mul", "pow", "sin", "cos", "tan", "ln", "exp", "sqrt", "abs",
        "asin", "acos", "atan", "sinh", "cosh", "tanh",
    ] {
        assert!(
            seen.contains(expected_op),
            "the corpus never exercised `{expected_op}`"
        );
    }

    // `emit` lowers `Neg`, `Sub` and `Div` before the stream exists, so no
    // expression can produce them. If emission ever learns to, this test should
    // grow cases for them instead of asserting they are absent.
    assert!(
        !seen.contains("neg") && !seen.contains("sub") && !seen.contains("div"),
        "emission now produces an op this corpus cannot generate: {seen:?}"
    );

    println!(
        "10 000 random expressions: every value agreed to the bit; \
         {nan_payloads} results were NaN on both sides with a different payload/sign"
    );
}

#[test]
fn missing_variables_read_as_nan_like_before() {
    let ids = variable_ids();
    let program = Program::compile(&add([sym("x"), sym("y")]), &ids).expect("compiles");
    assert_eq!(program.eval(&[1.0, 2.0]), 3.0);
    assert!(program.eval(&[1.0]).is_nan());

    // Row 0 is complete; rows 1 and 2 start past the end of `args`, so their
    // variables are missing and the batch reads `NaN`, exactly like `eval`.
    let args = [1.0, 2.0];
    let mut out = [0.0; 3];
    program.eval_batch(&args, 2, &mut out);
    assert_eq!(out[0], 3.0);
    assert!(out[1].is_nan());
    assert!(out[2].is_nan());
}

#[test]
fn a_sum_of_negative_zeros_stays_negative() {
    // `(-0.0) + (-0.0)` is `-0.0`. The pre-W0.2 evaluator got that from
    // `Iterator::sum`, whose identity for `f64` is `-0.0` — and it matters,
    // because `asin(-0.0)^-1` is `-inf` while `asin(+0.0)^-1` is `+inf`. An
    // accumulator that starts at `+0.0` flips the pole direction and still
    // looks like a correct sum, which is why this is pinned down here.
    let sum = Program::compile(&add([sym("x"), sym("y")]), &variable_ids()).expect("compiles");
    let value = sum.eval(&[-0.0, -0.0]);
    assert_eq!(value.to_bits(), (-0.0f64).to_bits());
    let mut nan_payloads = 0;
    assert_same_result(
        "reference",
        0,
        reference_eval(&sum, &[-0.0, -0.0]),
        value,
        &mut nan_payloads,
    );

    let reciprocal = pow(function("asin", add([sym("x"), sym("y")])), int(-1));
    let program = Program::compile(&reciprocal, &variable_ids()).expect("compiles");
    assert_eq!(program.eval(&[-0.0, -0.0]), f64::NEG_INFINITY);
}

#[test]
#[should_panic(expected = "needs a stack of 2 slots, got 1")]
fn eval_into_refuses_a_short_stack() {
    let program =
        Program::compile(&add([sym("x"), int(1)]), &[SymbolId::intern("x")]).expect("compiles");
    assert_eq!(program.max_stack(), 2);
    let mut stack = [0.0; 1];
    let _ = program.eval_into(&[2.0], &mut stack);
}

#[test]
fn domain_edge_cases_keep_the_documented_convention() {
    // Raw constructors on purpose: this is about what the machine does with an
    // out-of-domain value or a pole, not about what normalization folds away.
    let sqrt_negative = ExprNode::pow_raw(int(-1), ratio(1, 2));
    let ln_negative = function("ln", int(-1));
    let ln_zero = function("ln", int(0));
    let one_over_zero = ExprNode::pow_raw(int(0), int(-1));

    assert!(evaluate(&sqrt_negative).is_nan(), "sqrt(-1) must be NaN");
    assert!(evaluate(&ln_negative).is_nan(), "ln(-1) must be NaN");

    let zero_log = evaluate(&ln_zero);
    assert_eq!(zero_log, f64::NEG_INFINITY, "ln(0) must be -Inf");

    let pole = evaluate(&one_over_zero);
    assert_eq!(pole, f64::INFINITY, "1/0 must be +Inf");
}

/// Evaluate a constant expression through all three entry points and require
/// that they agree before returning the value.
fn evaluate(expression: &Expr) -> f64 {
    let program = Program::compile(expression, &[]).expect("constant expression compiles");
    let direct = program.eval(&[]);
    let mut nan_payloads = 0;

    let mut stack = vec![0.0; program.max_stack()];
    assert_same_result(
        "eval_into",
        0,
        program.eval_into(&[], &mut stack),
        direct,
        &mut nan_payloads,
    );

    let mut out = [0.0; 1];
    program.eval_batch(&[], 0, &mut out);
    assert_same_result("eval_batch", 0, out[0], direct, &mut nan_payloads);

    direct
}

// ---------------------------------------------------------------------------
// Throughput
// ---------------------------------------------------------------------------

/// Exactly twenty nodes: `sin(x)·cos(y)·exp(xy) / (1 + x² + y²)`.
///
/// Raw constructors on purpose. Normalization sorts factors by `Expr`, which
/// orders symbols by their process-wide interning ids; a canonical tree would
/// therefore execute in a different order depending on which test interned a
/// name first, and the timing would wobble between runs.
fn twenty_node_expression() -> Expr {
    let x = sym("x");
    let y = sym("y");
    let numerator = ExprNode::mul_raw(vec![
        function("sin", x.clone()),
        function("cos", y.clone()),
        function("exp", ExprNode::mul_raw(vec![x.clone(), y.clone()])),
    ]);
    let denominator = ExprNode::add_raw(vec![
        int(1),
        ExprNode::pow_raw(x.clone(), int(2)),
        ExprNode::pow_raw(y.clone(), int(2)),
    ]);
    ExprNode::mul_raw(vec![numerator, ExprNode::pow_raw(denominator, int(-1))])
}

fn twenty_node_program() -> Program {
    Program::compile(&twenty_node_expression(), &variable_ids())
        .expect("the 20-node expression compiles")
}

fn node_count(expression: &Expr) -> usize {
    match &**expression {
        ExprNode::Num(_) | ExprNode::Sym(_) => 1,
        ExprNode::Add(items) | ExprNode::Mul(items) => {
            1 + items.iter().map(node_count).sum::<usize>()
        }
        ExprNode::Pow(base, exponent) => 1 + node_count(base) + node_count(exponent),
        ExprNode::Fn(_, arguments) => 1 + arguments.iter().map(node_count).sum::<usize>(),
        ExprNode::Hold(_) => 1,
    }
}

fn time_per_call(calls: usize, rounds: usize, mut evaluation: impl FnMut(usize) -> f64) -> f64 {
    // Warm up, so the first timed round is not measuring cold code.
    let mut sink = 0.0;
    for index in 0..(calls / 10).max(1) {
        sink += black_box(evaluation(index));
    }
    black_box(sink);

    let mut best = f64::INFINITY;
    for _ in 0..rounds {
        let mut sink = 0.0;
        let start = Instant::now();
        for index in 0..calls {
            sink += black_box(evaluation(index));
        }
        let elapsed = start.elapsed().as_secs_f64();
        black_box(sink);
        best = best.min(elapsed);
    }
    best * 1e9 / calls as f64
}

/// Prints the numbers instead of asserting on them: a timing assertion is a
/// flaky test, and the comparison belongs in the report. The numbers only mean
/// something in a release build:
///
/// `cargo test -p canlab-core --release --test eval eval_throughput -- --nocapture`
#[test]
fn eval_throughput_is_reported() {
    let expression = twenty_node_expression();
    let program = twenty_node_program();
    assert_eq!(node_count(&expression), 20);
    assert_eq!(program.max_stack(), stack_peak(&program));

    let args = [0.7, 1.3];
    let want = reference_eval(&program, &args);
    let mut nan_payloads = 0;
    assert_same_result("eval", 0, program.eval(&args), want, &mut nan_payloads);
    let mut stack = vec![0.0; program.max_stack()];
    assert_same_result(
        "eval_into",
        0,
        program.eval_into(&args, &mut stack),
        want,
        &mut nan_payloads,
    );

    let calls = 100_000;
    let rounds = 7;

    let old = time_per_call(calls, rounds, |index| {
        let row = [0.7 + (index % 61) as f64 * 1e-6, 1.3];
        reference_eval(&program, &row)
    });

    let inline = time_per_call(calls, rounds, |index| {
        let row = [0.7 + (index % 61) as f64 * 1e-6, 1.3];
        program.eval(&row)
    });

    let mut stack = vec![0.0; program.max_stack()];
    let direct = time_per_call(calls, rounds, |index| {
        let row = [0.7 + (index % 61) as f64 * 1e-6, 1.3];
        program.eval_into(&row, &mut stack)
    });

    println!(
        "eval throughput: {} nodes, max_stack = {}, {} calls per round, best of {} rounds",
        node_count(&expression),
        program.max_stack(),
        calls,
        rounds
    );
    println!("  old implementation (Vec per call):  {old:8.2} ns/call");
    println!("  Program::eval (inline buffer):      {inline:8.2} ns/call");
    println!("  Program::eval_into (caller buffer): {direct:8.2} ns/call");
}
