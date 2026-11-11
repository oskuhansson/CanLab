//! W0.2: the evaluator's hot loop must not touch the heap.
//!
//! The counting allocator lives in this test binary on purpose. A
//! `#[global_allocator]` is process-wide, so putting one in the library's unit
//! tests would change the allocator under every other test in that binary; here
//! it owns the binary, and this file holds a single test so no other test
//! thread can allocate while the count is being read.

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};

use canlab_core::{Expr, ExprNode, INLINE_STACK, Program, SymbolId, add, int, mul, pow, sym};

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

struct CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn allocations() -> usize {
    ALLOCATIONS.load(Ordering::Relaxed)
}

/// An expression of the shape a plot evaluates, nowhere near `INLINE_STACK`.
fn hot_expression() -> Expr {
    let x = sym("x");
    let numerator = add([
        ExprNode::fn_raw(SymbolId::intern("sin"), vec![x.clone()]),
        pow(x.clone(), int(3)),
    ]);
    let denominator = add([int(1), pow(x.clone(), int(2))]);
    mul([numerator, pow(denominator, int(-1))])
}

/// Right-nested powers: every level holds its base while the next is pushed, so
/// forty levels need more slots than `INLINE_STACK` has.
fn deep_program() -> Program {
    let mut expression = sym("y");
    for _ in 0..40 {
        expression = pow(sym("x"), expression);
    }
    Program::compile(&expression, &[SymbolId::intern("x"), SymbolId::intern("y")])
        .expect("the nested expression compiles")
}

#[test]
fn the_evaluator_does_not_allocate() {
    let program = Program::compile(&hot_expression(), &[SymbolId::intern("x")])
        .expect("the hot expression compiles");
    assert!(
        program.max_stack() <= INLINE_STACK,
        "the hot expression must take the inline-buffer path, max_stack = {}",
        program.max_stack()
    );

    let mut args = [0.7];
    black_box(program.eval(&args));

    // First prove the counter is connected: one heap allocation must be seen.
    let before_sanity = allocations();
    black_box(vec![0u8; 1024]);
    assert!(
        allocations() > before_sanity,
        "the counting allocator is not counting; the rest of this test means nothing"
    );

    // The claim: a thousand evaluations, zero allocations.
    let before = allocations();
    let mut sum = 0.0;
    for index in 0..1_000 {
        args[0] = 0.5 + index as f64 * 1e-4;
        sum += black_box(program.eval(black_box(&args)));
    }
    let after = allocations();
    assert_eq!(
        after,
        before,
        "{} heap allocations in 1 000 eval calls",
        after - before
    );
    assert!(sum.is_finite(), "the benchmark expression must stay finite");

    // A batch is the caller's buffer too: no allocation per row, and none for
    // the internal operand stack while the program fits inline.
    let mut batch_args = vec![0.0; 256];
    for (row, value) in batch_args.iter_mut().enumerate() {
        *value = row as f64 * 1e-3;
    }
    let mut out = vec![0.0; 256];
    let before = allocations();
    program.eval_batch(&batch_args, 1, &mut out);
    let after = allocations();
    assert_eq!(
        after,
        before,
        "{} heap allocations in one 256-row eval_batch",
        after - before
    );
    assert!(out.iter().all(|value| value.is_finite()));

    // `eval_into` must not allocate even when the program is too deep for the
    // inline buffer: that is the whole point of the caller-provided stack.
    let deep = deep_program();
    assert!(
        deep.max_stack() > INLINE_STACK,
        "the deep program must exercise the heap fallback, max_stack = {}",
        deep.max_stack()
    );
    let deep_args = [0.5, 0.25];
    let mut stack = vec![0.0; deep.max_stack()];
    let expected = deep.eval_into(&deep_args, &mut stack);

    let before = allocations();
    for _ in 0..100 {
        black_box(deep.eval_into(black_box(&deep_args), &mut stack));
    }
    let after = allocations();
    assert_eq!(
        after,
        before,
        "{} heap allocations in 100 eval_into calls on a deep program",
        after - before
    );

    // The fallback `eval` path is allowed to allocate, but must agree with the
    // allocation-free path on the answer.
    assert_eq!(deep.eval(&deep_args).to_bits(), expected.to_bits());

    // `eval_batch` may allocate once for the whole batch when the program is
    // deep, never per row, and every row must still agree.
    let deep_rows = [0.5, 0.25, 0.5, 0.25];
    let mut batch = [0.0; 2];
    deep.eval_batch(&deep_rows, 2, &mut batch);
    for value in batch {
        assert_eq!(value.to_bits(), expected.to_bits());
    }
}
