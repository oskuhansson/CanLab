//! End-to-end tests for `canlab corpus` and `canlab bench`.
//!
//! Each test owns a scratch directory under `CARGO_TARGET_TMPDIR` and runs the
//! real binary there, so the repository's own `tests/cases` and
//! `docs/abimix-coverage.md` are never touched by a test.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn scratch(name: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("corpus-e2e")
        .join(name);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("tests/cases/math")).expect("math directory");
    std::fs::create_dir_all(root.join("tests/cases/physics")).expect("physics directory");
    root
}

fn write_row(root: &Path, subject: &str, id: &str, body: &str) {
    let path = root
        .join("tests/cases")
        .join(subject)
        .join(format!("{id}.toml"));
    std::fs::write(path, body).expect("row file");
}

fn row(id: &str, capability: &str, document: &str, expect: &str) -> String {
    format!(
        "id = {id:?}\n\
         subject = \"math\"\n\
         capability = {capability:?}\n\
         source = \"https://example.test/paper — 2024, uppgift 1\"\n\
         document = '{document}'\n\
         {expect}\n"
    )
}

fn physics_row(id: &str, document: &str, expect: &str) -> String {
    format!(
        "id = {id:?}\n\
         subject = \"physics\"\n\
         capability = \"units.sigfigs\"\n\
         source = \"hf:xw27/scibench class#Problem 2.26\"\n\
         document = '{document}'\n\
         {expect}\n"
    )
}

fn canlab(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_canlab"))
        .current_dir(root)
        .args(args)
        .output()
        .expect("the canlab binary runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn a_passing_row_exits_zero_and_writes_the_report() {
    let root = scratch("pass");
    write_row(
        &root,
        "math",
        "a",
        &row(
            "a",
            "solve.linear",
            "x + 1 = 3",
            r"expect.exact = '\left\{2\right\}'",
        ),
    );
    let output = canlab(&root, &["corpus", "run"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("PASS  tests/cases/math/a.toml"), "{text}");
    assert!(text.contains("expected exact \\left\\{2\\right\\}"), "{text}");
    assert!(text.contains("got \\left\\{2\\right\\}"), "{text}");

    let report = std::fs::read_to_string(root.join("docs/abimix-coverage.md"))
        .expect("the report was written");
    assert!(report.contains("| `solve.linear` | 1 | 0 | 0 | 1 | 0 | 0 |"), "{report}");
    assert!(report.contains("Provenance: unchecked"), "{report}");
}

#[test]
fn a_wrong_expected_value_exits_one_and_names_both_answers() {
    let root = scratch("fail");
    write_row(
        &root,
        "math",
        "a",
        &row("a", "solve.linear", "x + 1 = 3", "expect.exact = '3'"),
    );
    let output = canlab(&root, &["corpus", "run"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("FAIL  tests/cases/math/a.toml"), "{text}");
    assert!(text.contains("expected exact 3"), "{text}");
    assert!(text.contains("got \\left\\{2\\right\\}"), "{text}");

    let report = std::fs::read_to_string(root.join("docs/abimix-coverage.md")).expect("report");
    assert!(report.contains("## Failures"), "{report}");
    assert!(report.contains("tests/cases/math/a.toml"), "{report}");
    assert!(report.contains("\\left\\{2\\right\\}"), "{report}");
}

#[test]
fn a_load_error_exits_two_and_the_row_is_not_run() {
    let root = scratch("load-error");
    write_row(
        &root,
        "math",
        "bad",
        "id = \"bad\"\nsubject = \"math\"\ncapability = \"solve.linear\"\n\
         source = \"https://example.test\"\ndocument = 'x + 1 = 3'\nexpect.exact = '2'\n\
         surprise = 'not a field'\n",
    );
    let output = canlab(&root, &["corpus", "run"]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(stderr(&output).contains("load error"), "{}", stderr(&output));
    assert!(!stdout(&output).contains("PASS"), "{}", stdout(&output));
    let report = std::fs::read_to_string(root.join("docs/abimix-coverage.md")).expect("report");
    assert!(report.contains("## Load errors"), "{report}");
}

#[test]
fn the_tag_matches_itself_and_its_children_and_nothing_else_exits_two() {
    let root = scratch("tag");
    write_row(
        &root,
        "math",
        "a",
        &row(
            "a",
            "solve.linear",
            "x + 1 = 3",
            r"expect.exact = '\left\{2\right\}'",
        ),
    );
    write_row(
        &root,
        "math",
        "b",
        &row("b", "input.malformed", "\\frac{1}{0}", "expect.refused = '1/0'"),
    );

    for tag in ["solve", "solve.linear"] {
        let output = canlab(&root, &["corpus", "run", "--tag", tag]);
        assert_eq!(output.status.code(), Some(0), "{tag}: {}", stderr(&output));
        assert!(stdout(&output).contains("tests/cases/math/a.toml"));
        assert!(!stdout(&output).contains("tests/cases/math/b.toml"));
    }

    let no_match = canlab(&root, &["corpus", "run", "--tag", "does.not-exist"]);
    assert_eq!(no_match.status.code(), Some(2));
    assert!(stderr(&no_match).contains("no row matched"), "{}", stderr(&no_match));
}

#[test]
fn a_refusal_row_passes_on_its_reason_and_fails_on_another() {
    let root = scratch("refusal");
    write_row(
        &root,
        "math",
        "matches",
        &row(
            "matches",
            "input.malformed",
            "\\frac{1}{0}",
            "expect.refused = '1/0'",
        ),
    );
    let output = canlab(&root, &["corpus", "run"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(stdout(&output).contains("PASS"), "{}", stdout(&output));
    assert!(stdout(&output).contains("refused: cannot read"), "{}", stdout(&output));

    write_row(
        &root,
        "math",
        "mismatch",
        &row(
            "mismatch",
            "input.malformed",
            "\\frac{1}{0}",
            "expect.refused = 'division by zero'",
        ),
    );
    let output = canlab(&root, &["corpus", "run", "--tag", "input"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("FAIL  tests/cases/math/mismatch.toml"), "{text}");
    assert!(text.contains("refused for a different reason"), "{text}");
}

#[test]
fn batching_through_children_produces_the_same_rows() {
    let root = scratch("batch");
    write_row(
        &root,
        "math",
        "a",
        &row(
            "a",
            "solve.linear",
            "x + 1 = 3",
            r"expect.exact = '\left\{2\right\}'",
        ),
    );
    write_row(
        &root,
        "math",
        "b",
        &row(
            "b",
            "solve.linear",
            "2x = 6",
            r"expect.exact = '\left\{3\right\}'",
        ),
    );
    write_row(
        &root,
        "physics",
        "c",
        &physics_row(
            "c",
            "\\sqrt{\\frac{2 \\cdot 4^2}{6}}",
            "expect.numeric = 2.3\nexpect.tolerance = 0.05\nexpect.unit = 'm'\nexpect.digits = 2",
        ),
    );
    let output = canlab(&root, &["corpus", "run", "--batch", "1"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("PASS  tests/cases/math/a.toml"), "{text}");
    assert!(text.contains("PASS  tests/cases/math/b.toml"), "{text}");
    assert!(text.contains("PASS (unit unchecked)  tests/cases/physics/c.toml"), "{text}");
}

#[test]
fn the_exec_child_answers_one_json_line_per_request() {
    let root = scratch("exec");
    write_row(
        &root,
        "math",
        "a",
        &row(
            "a",
            "solve.linear",
            "x + 1 = 3",
            r"expect.exact = '\left\{2\right\}'",
        ),
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_canlab"))
        .current_dir(&root)
        .args(["corpus", "exec"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("exec runs");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"{\"path\":\"tests/cases/math/a.toml\"}\n")
        .expect("request written");
    let output = child.wait_with_output().expect("exec finishes");
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("\"status\":\"pass\""), "{text}");
    assert!(text.contains("\"path\":\"tests/cases/math/a.toml\""), "{text}");
    assert!(text.contains("\\\\left\\\\{2\\\\right\\\\}"), "{text}");
}

#[test]
fn fetch_refuses_an_unknown_dataset_and_reads_a_local_mirror() {
    let root = scratch("fetch");
    let unknown = canlab(&root, &["corpus", "fetch", "nope"]);
    assert_eq!(unknown.status.code(), Some(2));
    assert!(stderr(&unknown).contains("unknown dataset"), "{}", stderr(&unknown));

    let mirror = root.join("mirror");
    std::fs::create_dir_all(mirror.join("scibench/data")).expect("mirror");
    std::fs::write(mirror.join("scibench/data/class.json"), "[]").expect("mirror file");
    let cache = root.join("cache");
    let output = Command::new(env!("CARGO_BIN_EXE_canlab"))
        .current_dir(&root)
        .args(["corpus", "fetch", "scibench"])
        .env("CANLAB_CORPUS_MIRROR", &mirror)
        .env("CANLAB_CORPUS_HOME", &cache)
        .output()
        .expect("fetch runs");
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(stdout(&output).contains("fetched scibench main: 1 file(s)"), "{}", stdout(&output));
    assert!(cache.join("scibench/main/manifest.toml").exists());
}

#[test]
fn update_checks_an_hf_row_against_the_mirror() {
    let root = scratch("update");
    write_row(
        &root,
        "physics",
        "a",
        &physics_row(
            "a",
            "2.3",
            "expect.numeric = 2.3\nexpect.tolerance = 0.05",
        ),
    );
    let mirror = root.join("mirror");
    std::fs::create_dir_all(mirror.join("scibench/data")).expect("mirror");
    std::fs::write(
        mirror.join("scibench/data/class.json"),
        "[{\"problemid\":\"Problem 2.26\",\"answer_number\":2.3}]",
    )
    .expect("mirror file");

    let output = Command::new(env!("CARGO_BIN_EXE_canlab"))
        .current_dir(&root)
        .args(["corpus", "run", "--update"])
        .env("CANLAB_CORPUS_MIRROR", &mirror)
        .env("CANLAB_CORPUS_HOME", root.join("cache"))
        .output()
        .expect("run finishes");
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let report = std::fs::read_to_string(root.join("docs/abimix-coverage.md")).expect("report");
    assert!(report.contains("Provenance: checked"), "{report}");
}

#[test]
fn bench_prints_and_writes_a_table() {
    let root = scratch("bench");
    let output = canlab(&root, &["bench"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("# Benchmarks"), "{text}");
    assert!(text.contains("implicit tracer, 240×240"), "{text}");
    assert!(text.contains("`Program::eval`, 20-node expression"), "{text}");
    assert!(text.contains("`Program::eval_batch`"), "{text}");
    assert!(text.contains("`canlab_plot::sample`, 2000-point range"), "{text}");
    assert!(text.contains("Generated "), "{text}");

    let written = canlab(&root, &["bench", "--write"]);
    assert_eq!(written.status.code(), Some(0), "{}", stderr(&written));
    let file = std::fs::read_to_string(root.join("docs/bench.md")).expect("bench.md");
    assert!(file.contains("# Benchmarks"), "{file}");
}
