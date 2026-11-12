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
