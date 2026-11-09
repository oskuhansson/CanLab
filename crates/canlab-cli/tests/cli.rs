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
    std::env::temp_dir().join(name)
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
