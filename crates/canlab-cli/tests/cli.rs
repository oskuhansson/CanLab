//! End-to-end tests: they drive the real binary, so they fail if any crate in
//! the pipeline disagrees with its neighbours about the API or the output
//! format. These are the tests that prove the plan's week-8 milestone works:
//! LaTeX in, exact math and a plot out.

use std::path::PathBuf;
use std::process::Command;

fn canlab(args: &[&str]) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_canlab"))
        .args(args)
        .output()
        .expect("the canlab binary should be runnable");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

fn temp_svg(name: &str) -> PathBuf {
    // `CARGO_TARGET_TMPDIR` is inside the workspace. `std::env::temp_dir()` is
    // not: the suite passed for months and then failed with "Access is denied"
    // on a machine that does not allow writes to `%TEMP%`, which is a property
    // of the machine and nothing to do with plotting.
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name)
}

#[test]
fn eval_marks_an_approximate_integral_with_its_bound() {
    // `\int_0^1 e^{-x^2} dx` has no elementary closed form, so the answer is
    // numeric and carries a one-sided bound. Printing the value alone — which
    // is what `parse_latex` forced, because an `Expr` has nowhere to put a
    // bound — is the "approximate and unmarked" outcome the integral module's
    // own header says the CAS is built against.
    let (ok, stdout, stderr) = canlab(&["eval", r"\int_0^1 e^{-x^2} dx"]);
    assert!(ok, "stderr: {stderr}");
    assert!(
        stdout.contains("approximate: within"),
        "the bound is not printed: {stdout}"
    );
    // The value is the one the integral gives, and the bound is small.
    assert!(stdout.contains("0.74682"), "{stdout}");
    // An *exact* integral prints no marker: a bound of zero is not a bound.
    let (ok, stdout, stderr) = canlab(&["eval", r"\int_1^2 \frac{1}{x^2} dx"]);
    assert!(ok, "stderr: {stderr}");
    assert!(!stdout.contains("approximate"), "{stdout}");
    assert!(stdout.contains("1/2"), "{stdout}");
    // And a plain expression has no bound either.
    let (ok, stdout, _) = canlab(&["eval", "2x + 1"]);
    assert!(ok);
    assert!(!stdout.contains("approximate"), "{stdout}");
}

#[test]
fn eval_normalizes_and_prints_latex() {
    let (ok, stdout, stderr) = canlab(&["eval", "2x + 3x"]);
    assert!(ok, "failed: {stderr}");
    assert!(stdout.contains("5*x"), "plain form missing: {stdout}");
    assert!(stdout.contains("5x"), "latex form missing: {stdout}");
}

#[test]
fn tex_handles_fractions() {
    let (ok, stdout, stderr) = canlab(&["tex", r"\frac{1}{2} + \frac{1}{3}"]);
    assert!(ok, "failed: {stderr}");
    assert!(stdout.contains(r"\frac{5}{6}"), "expected 5/6: {stdout}");
}

#[test]
fn tex_handles_powers_and_roots() {
    let (ok, stdout, stderr) = canlab(&["tex", r"\sqrt{x} + x^2"]);
    assert!(ok, "failed: {stderr}");
    assert!(stdout.contains(r"\sqrt{x}"), "expected a radical: {stdout}");
    assert!(stdout.contains("x^{2}"), "expected a power: {stdout}");
}

#[test]
fn parse_error_is_reported_not_panicked() {
    let (ok, _stdout, stderr) = canlab(&["eval", r"\frac{1}"]);
    assert!(!ok, "a malformed expression should be an error");
    assert!(
        stderr.to_lowercase().contains("error"),
        "expected a diagnostic, got: {stderr}"
    );
}

#[test]
fn eval_refuses_a_subscript_on_a_variable() {
    // `x_1` used to print `x` and exit 0: the subscript was parsed and thrown
    // away, so `v_1` and `v_2` in a document both meant `v`. The field has
    // always refused it; `eval` must give the field's reason, not an answer.
    let (ok, stdout, stderr) = canlab(&["eval", "x_1"]);
    assert!(!ok, "eval accepted `x_1` and printed {stdout:?}");
    assert!(
        stderr.contains("subscripts are not expressions"),
        "the field's reason is missing: {stderr}"
    );
    assert!(
        stdout.is_empty(),
        "a refused expression must print nothing, got {stdout:?}"
    );
}

#[test]
fn tex_and_plot_refuse_the_same_subscript() {
    // `tex` and `plot` read LaTeX through the same parser, so a refusal that
    // held in `eval` but not here would be a second silent reader.
    for args in [["tex", "x_1"], ["plot", "x_1"]] {
        let (ok, stdout, stderr) = canlab(&args);
        assert!(!ok, "`{}` accepted `x_1`", args[0]);
        assert!(
            stderr.contains("subscripts are not expressions"),
            "`{}` gave the wrong reason: {stderr}",
            args[0]
        );
        assert!(
            stdout.is_empty(),
            "`{}` printed something for a refused expression: {stdout:?}",
            args[0]
        );
    }
}

#[test]
fn plot_writes_an_svg() {
    let out = temp_svg("cas-e2e-sin.svg");
    let _ = std::fs::remove_file(&out);
    let (ok, stdout, stderr) = canlab(&[
        "plot",
        r"\sin x",
        "-o",
        out.to_str().unwrap(),
        "--xmin",
        "0",
        "--xmax",
        "6.28",
    ]);
    assert!(ok, "plot failed: {stderr}");
    assert!(stdout.contains("wrote"), "no confirmation: {stdout}");

    let svg = std::fs::read_to_string(&out).expect("svg file should exist");
    assert!(svg.starts_with("<svg"), "not an svg: {}", &svg[..40.min(svg.len())]);
    assert!(svg.contains("polyline"), "no geometry in the svg");
    assert!(!svg.contains("NaN"), "svg contains NaN coordinates");
}

#[test]
fn plot_of_reciprocal_does_not_join_across_the_pole() {
    let out = temp_svg("cas-e2e-recip.svg");
    let _ = std::fs::remove_file(&out);
    let (ok, stdout, stderr) = canlab(&[
        "plot",
        r"\frac{1}{x}",
        "-o",
        out.to_str().unwrap(),
        "--xmin",
        "-1",
        "--xmax",
        "1",
    ]);
    assert!(ok, "plot failed: {stderr}");
    // The sampler reports how many segments survived; 1/x must not be one line.
    assert!(
        stdout.contains("2 segments") || stdout.contains("3 segments"),
        "expected the curve to be split at the pole: {stdout}"
    );
}

#[test]
fn integrate_uses_the_mock_solver_end_to_end() {
    let (ok, stdout, stderr) = canlab(&["int", "x^2", "-v", "x"]);
    assert!(ok, "int failed: {stderr}");
    assert!(
        stdout.contains("x^{3}") && stdout.contains("3"),
        "expected x^3/3, got: {stdout}"
    );
}

#[test]
fn help_is_available() {
    let (ok, stdout, _stderr) = canlab(&["help"]);
    assert!(ok);
    assert!(stdout.contains("USAGE"));
}

#[test]
fn solve_all_prints_the_library_solution_set() {
    // The binary must print exactly what `Solutions::to_latex` produced, not a
    // second formatting of the same answer. The expected value is computed
    // through the same backend choice `main.rs` makes, so the test also holds on
    // a machine where SymPy is installed.
    use canlab_core::{SymbolId, add, int, pow, sym};
    use canlab_solver::{NativeSolver, Solver};

    let var = SymbolId::intern("x");
    let equation = add([pow(sym("x"), int(2)), int(-4)]);
    let expected = match canlab_solver::sympy::open() {
        Ok(worker) => worker.solve_all(&equation, var),
        Err(_) => NativeSolver.solve_all(&equation, var),
    }
    .expect("x^2 - 4 has two real roots")
    .to_latex();

    let (ok, stdout, stderr) = canlab(&["solve-all", "x^2 - 4", "-v", "x"]);
    assert!(ok, "solve-all failed: {stderr}");
    assert_eq!(stdout.trim(), expected, "the CLI and the library disagree");
}

#[test]
fn solve_all_says_where_a_numeric_answer_came_from() {
    // `x^4 − x − 1` has two real roots and no exact form the native backend can
    // name. The first line is the answer; the `≈` lines are the evidence. Before
    // this, the command printed a bare `-0.7244… \cup 1.2207…`, which reads
    // exactly like an exact answer.
    let (ok, stdout, stderr) = canlab(&["solve-all", "x^4 - x - 1", "-v", "x"]);
    assert!(ok, "solve-all failed: {stderr}");
    let lines: Vec<&str> = stdout.lines().map(str::trim).collect();
    assert_eq!(lines.len(), 3, "one answer line and two residuals: {stdout}");
    assert!(lines[0].contains("1.2207"), "{stdout}");
    for line in &lines[1..] {
        assert!(line.contains('\u{2248}'), "no approximation mark: {line}");
        assert!(line.contains("f(x) = "), "no residual: {line}");
    }
}

#[test]
fn solve_all_reports_a_decline_with_its_reason() {
    // `sin x − x² − 5` is below −4 everywhere: no closed form, and no root for
    // the native numeric fallback to find either. A decline is a normal answer:
    // the command must refuse with the backend's reason and the equation it
    // could not solve, not exit quietly or panic. (`x + sin x = 0` used to be
    // the example here; its root at zero is now found numerically.)
    let (ok, stdout, stderr) = canlab(&["solve-all", r"\sin x - x^2 - 5", "-v", "x"]);
    assert!(!ok, "no backend solves this, yet stdout was {stdout:?}");
    assert!(
        stderr.contains("the solver could not give a solution set"),
        "the refusal is missing: {stderr}"
    );
    assert!(
        stderr.contains("declined"),
        "the reason is missing: {stderr}"
    );
    assert!(
        stderr.contains("\\sin"),
        "the equation is missing: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// canlab compile and canlab compile-sweep (W7.3)
//
// The command is a viewer for two modules that each have their own tests: what
// is asserted here is the wiring — that the LaTeX reaches the printer, that the
// printer's Lua reaches the compiler, and that every refusal is a message on
// stderr and a non-zero exit rather than a plausible-looking kernel.
// ---------------------------------------------------------------------------

#[test]
fn compile_prints_the_lua_and_the_wgsl() {
    let (ok, stdout, stderr) = canlab(&["compile", r"2x^2 + \sin(x)"]);
    assert!(ok, "failed: {stderr}");
    assert!(stdout.contains("-- Lua"), "{stdout}");
    assert!(stdout.contains("local kernel_expression = "), "{stdout}");
    assert!(stdout.contains("math.sin(x)"), "{stdout}");
    assert!(stdout.contains("-- WGSL"), "{stdout}");
    // The signature names the kernel's parameters. `x` is read and never
    // assigned, so it is a parameter — the compiler used to discover parameters
    // only from assignments, which left `fn kernel()` calling an undeclared `x`.
    assert!(stdout.contains("fn kernel(x: f32) -> f32 {"), "{stdout}");
    assert!(stdout.contains("pow(x, 2.0)"), "{stdout}");
    assert!(stdout.contains("sin(x)"), "{stdout}");
}

#[test]
fn compile_prints_only_the_lua_or_only_the_wgsl_when_asked() {
    let (ok, stdout, stderr) = canlab(&["compile", "x^2", "--lua-only"]);
    assert!(ok, "failed: {stderr}");
    assert!(stdout.contains("local kernel_expression"), "{stdout}");
    assert!(!stdout.contains("-- WGSL"), "{stdout}");
    assert!(!stdout.contains("fn kernel"), "{stdout}");

    let (ok, stdout, stderr) = canlab(&["compile", "x^2", "--wgsl-only"]);
    assert!(ok, "failed: {stderr}");
    assert!(!stdout.contains("-- Lua"), "{stdout}");
    assert!(stdout.contains("fn kernel(x: f32) -> f32 {"), "{stdout}");
}

#[test]
fn compile_refuses_both_only_flags_at_once() {
    // Two viewers, neither wanted. Printing nothing and exiting zero would look
    // like a successful compile of an expression that produced no kernel.
    let (ok, _stdout, stderr) = canlab(&["compile", "x^2", "--lua-only", "--wgsl-only"]);
    assert!(!ok, "asking for no output should fail");
    assert!(
        stderr.contains("--lua-only and --wgsl-only"),
        "the reason should name both flags: {stderr}"
    );
}

#[test]
fn compile_names_an_unsupported_function_and_exits_non_zero() {
    // `arcsin` is outside the Lua subset. The printer's own message is what the
    // user sees, with the function named, and the exit code is a failure.
    let (ok, stdout, stderr) = canlab(&["compile", r"\arcsin x"]);
    assert!(!ok, "an unsupported function must not compile");
    assert!(stderr.contains("arcsin"), "the name should be named: {stderr}");
    assert!(
        stderr.contains("not in the Lua subset"),
        "the printer's reason should be shown: {stderr}"
    );
    assert!(!stdout.contains("fn kernel"), "nothing should be compiled: {stdout}");
}

#[test]
fn compile_names_a_free_symbol_and_exits_non_zero() {
    // A name that is not `--var` and not a constant is a free variable, and a
    // kernel with a free variable has no meaning.
    let (ok, _stdout, stderr) = canlab(&["compile", "sin(z) + 1", "--var", "x"]);
    assert!(!ok, "a free symbol must not compile");
    assert!(stderr.contains("`z`"), "the symbol should be named: {stderr}");
    assert!(stderr.contains("no value"), "the reason: {stderr}");
}

#[test]
fn compile_names_the_variables_from_var() {
    // With `--var x` the kernel mentions `x`; without it the expression's own
    // symbols are the guess.
    let (ok, stdout, stderr) = canlab(&["compile", "x * y", "--var", "x", "--var", "y"]);
    assert!(ok, "failed: {stderr}");
    assert!(stdout.contains("(x * y)"), "{stdout}");
    let (ok, stdout, stderr) = canlab(&["compile", "x * y"]);
    assert!(ok, "failed: {stderr}");
    assert!(stdout.contains("(x * y)"), "{stdout}");
}

#[test]
fn compile_refuses_an_unknown_option_by_name() {
    let (ok, _stdout, stderr) = canlab(&["compile", "x^2", "--bogus"]);
    assert!(!ok);
    assert!(stderr.contains("--bogus"), "{stderr}");
}

#[test]
fn compile_refuses_a_second_expression() {
    let (ok, _stdout, stderr) = canlab(&["compile", "x^2", "y"]);
    assert!(!ok, "two expressions should be refused, not one silently printed");
    assert!(stderr.contains("one LaTeX expression"), "{stderr}");
}

#[test]
fn compile_compiles_an_integral_the_parser_already_evaluated() {
    // Not a refusal test, and it says why: `\int_0^1 x^2 dx` is evaluated by
    // the parser into `1/3` before `compile` ever sees it, so there is no
    // `Hold` to refuse at this layer. The printer's `Hold` refusal has its own
    // test in `canlab_core::lua`, built from an `Expr` with no parser in
    // between; this pins the honest behaviour of the command on that input
    // rather than pretending to test a refusal that cannot happen here.
    let (ok, stdout, stderr) = canlab(&["compile", r"\int_0^1 x^2 dx"]);
    assert!(ok, "failed: {stderr}");
    assert!(stdout.contains("(1 / 3)"), "{stdout}");
}

#[test]
fn compile_sweep_reports_a_clean_sweep() {
    let (ok, stdout, stderr) = canlab(&["compile-sweep", "--count", "40"]);
    assert!(ok, "sweep failed: {stderr}\n{stdout}");
    assert!(stdout.contains("40 generated expression(s) at seed 1"), "{stdout}");
    assert!(stdout.contains("worst relative difference"), "{stdout}");
    // The command must say what it did not run, on its own output.
    assert!(stdout.contains("does not run WGSL"), "{stdout}");
    assert!(!stdout.contains("FAIL"), "{stdout}");
}

#[test]
fn compile_sweep_is_the_same_sweep_for_the_same_seed() {
    let (_, first, _) = canlab(&["compile-sweep", "--seed", "9", "--count", "12"]);
    let (_, second, _) = canlab(&["compile-sweep", "--seed", "9", "--count", "12"]);
    assert_eq!(first, second, "the sweep must be reproducible");
}

#[test]
fn compile_sweep_refuses_a_bad_count() {
    let (ok, _stdout, stderr) = canlab(&["compile-sweep", "--count", "0"]);
    assert!(!ok);
    assert!(stderr.contains("positive whole number"), "{stderr}");
    let (ok, _stdout, stderr) = canlab(&["compile-sweep", "--seed", "x"]);
    assert!(!ok);
    assert!(stderr.contains("whole number"), "{stderr}");
}
