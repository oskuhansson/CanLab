//! Batches, and the child that runs one.
//!
//! `canlab corpus run --batch N` sends N rows to a child process and reads one
//! JSON result per row. The process boundary is not ceremony: a row that
//! segfaults, leaks or hangs takes its batch with it and the parent starts the
//! next one, so one bad row costs N rows and never the run.
//!
//! The child is this same binary, run as `canlab corpus exec`. It reads
//! `{"path":"…"}` lines on stdin and writes `{"path":…,"status":…}` lines on
//! stdout. A result is matched to its row by **path**, not by position: the
//! child is allowed to answer in any order, and a missing answer must be
//! visible as a missing answer.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use canlab_solver::Solver;

use crate::json::{self, Value};
use crate::row::Row;
use crate::run::{self, Outcome};

/// How many rows one child receives when `--batch` is not given.
pub const DEFAULT_BATCH: usize = 50;

/// How long one batch may take when `--batch` does not say otherwise.
///
/// Both numbers are the plan's placeholders until a slow row is measured.
pub const DEFAULT_DEADLINE: Duration = Duration::from_secs(120);

/// The command that starts a batch child.
#[derive(Clone, Debug)]
pub struct ExecCommand {
    /// The program to run, normally this executable.
    pub program: PathBuf,
    /// Its arguments, normally `corpus exec`.
    pub args: Vec<String>,
}

impl ExecCommand {
    /// The command that is this binary, running the internal `exec` loop.
    pub fn current_exe() -> Result<ExecCommand, String> {
        let program =
            std::env::current_exe().map_err(|error| format!("could not find this binary: {error}"))?;
        Ok(ExecCommand {
            program,
            args: vec![String::from("corpus"), String::from("exec")],
        })
    }

    /// Pin the child to the backend the parent chose, so one run is not half
    /// SymPy and half native.
    pub fn with_backend(mut self, backend: &str) -> ExecCommand {
        self.args.push(String::from("--backend"));
        self.args.push(backend.to_string());
        self
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        command
    }
}

/// What a child said about one row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecStatus {
    /// The row passed.
    Pass,
    /// The row ran and failed.
    Fail,
    /// The child could not load the row. The parent treats this as a load
    /// error, which is exit 2, not a failed row.
    LoadError,
    /// The child could not run the row. A failure, not a refusal.
    Error,
}

/// One row's answer from a child.
#[derive(Clone, Debug, PartialEq)]
pub struct ExecResult {
    /// What happened.
    pub status: ExecStatus,
    /// The answer, or what stood in for it.
    pub got: String,
    /// A physics unit was recorded but not checked.
    pub unit_unchecked: bool,
    /// Why, when the status is not a pass.
    pub reason: Option<String>,
}

/// One row's slot in a batch: a result, or the reason the batch lost the row.
pub type BatchResult = Result<ExecResult, String>;

/// Run `rows` in one child, with a deadline for the whole batch.
///
/// The returned vector is in the same order as `rows`, whatever order the child
/// answered in.
pub fn run_batch(command: &ExecCommand, rows: &[Row], deadline: Duration) -> Vec<BatchResult> {
    if rows.is_empty() {
        return Vec::new();
    }

    let lost = |reason: String| -> Vec<BatchResult> {
        rows.iter().map(|_| Err(reason.clone())).collect()
    };

    let mut child = match command
        .command()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            return lost(format!(
                "could not start the batch child (`{}`): {error}",
                command.program.display()
            ));
        }
    };

    let stdin = child.stdin.take().expect("stdin was piped");
    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");

    let requests: Vec<String> = rows
        .iter()
        .map(|row| row.path.to_string_lossy().into_owned())
        .collect();
    let writer = std::thread::spawn(move || -> std::io::Result<()> {
        let mut out = std::io::BufWriter::new(stdin);
        for path in requests {
            let request = json::encode(&Value::Object(vec![(
                String::from("path"),
                Value::String(path),
            )]));
            writeln!(out, "{request}")?;
        }
        out.flush()
    });

    let (sender, receiver) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    if sender.send(line).is_err() {
                        break;
                    }
                }
                // A broken pipe after the child died is the child's business;
                // the parent sees the exit status and the missing results.
                Err(_) => break,
            }
        }
    });

    let stderr_reader = std::thread::spawn(move || {
        let mut reader = stderr;
        let mut buffer = Vec::new();
        let _ = reader.read_to_end(&mut buffer);
        buffer
    });

    let start = Instant::now();
    let until = start + deadline;
    let mut lines: Vec<String> = Vec::new();
    let mut timed_out = false;
    while lines.len() < rows.len() {
        let now = Instant::now();
        if now >= until {
            timed_out = true;
            break;
        }
        match receiver.recv_timeout(until - now) {
            Ok(line) => lines.push(line),
            Err(RecvTimeoutError::Timeout) => {
                timed_out = true;
                break;
            }
            // The child closed stdout: it has said everything it will say.
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    let status = wait_with_deadline(&mut child, until);
    let _ = writer.join();
    let _ = reader.join();
    let stderr = stderr_reader.join().unwrap_or_default();

    if timed_out || status.is_none() {
        return lost(format!(
            "the batch missed its deadline of {} s{}",
            deadline.as_secs_f64(),
            stderr_note(&stderr)
        ));
    }

    let mut results: Vec<Option<BatchResult>> = vec![None; rows.len()];

    let mut protocol_error: Option<String> = None;
    for line in &lines {
        match parse_result(line) {
            Ok((path, result)) => {
                if let Some(index) = rows.iter().position(|row| row.path == path) {
                    results[index] = Some(Ok(result));
                }
                // A result for a path this batch did not send is ignored: it is
                // not this batch's row, and guessing which row it meant would
                // be worse than dropping it.
            }
            Err(reason) => {
                protocol_error = Some(reason);
                break;
            }
        }
    }

    let exit = status.expect("checked above");
    for slot in results.iter_mut() {
        if slot.is_some() {
            continue;
        }
        *slot = Some(Err(if let Some(reason) = &protocol_error {
            format!("the batch child broke the protocol: {reason}")
        } else if !exit.success() {
            let note = stderr_note(&stderr);
            format!("the batch child {}{note}", exit_description(&exit))
        } else {
            String::from("the batch child returned no result for this row")
        }));
    }

    results
        .into_iter()
        .map(|slot| slot.unwrap_or_else(|| Err(String::from("the row was never sent"))))
        .collect()
}

fn wait_with_deadline(child: &mut Child, until: Instant) -> Option<ExitStatus> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => {
                if Instant::now() >= until {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

fn exit_description(status: &ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exited with code {code}"),
        None => String::from("was terminated by a signal"),
    }
}

/// The first useful stderr line, prefixed, for a lost batch's reason.
fn stderr_note(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    if line.is_empty() {
        return String::new();
    }
    let line: String = line.chars().take(200).collect();
    format!(": {line}")
}

/// Parse one child result line.
fn parse_result(line: &str) -> Result<(PathBuf, ExecResult), String> {
    let value = json::parse(line)?;
    let path = value
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("a result line has no string `path`: {line:?}"))?;
    let status = match value.get("status").and_then(Value::as_str) {
        Some("pass") => ExecStatus::Pass,
        Some("fail") => ExecStatus::Fail,
        Some("load-error") => ExecStatus::LoadError,
        Some("error") => ExecStatus::Error,
        Some(other) => return Err(format!("unknown result status {other:?}")),
        None => return Err(format!("a result line has no `status`: {line:?}")),
    };
    let got = value
        .get("got")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let unit_unchecked = value
        .get("unit_unchecked")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let reason = value
        .get("reason")
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok((
        PathBuf::from(path),
        ExecResult {
            status,
            got,
            unit_unchecked,
            reason,
        },
    ))
}

fn result_object(path: &str, result: &ExecResult) -> Value {
    let status = match result.status {
        ExecStatus::Pass => "pass",
        ExecStatus::Fail => "fail",
        ExecStatus::LoadError => "load-error",
        ExecStatus::Error => "error",
    };
    let mut members = vec![
        (String::from("path"), Value::String(path.to_string())),
        (String::from("status"), Value::String(status.to_string())),
        (String::from("got"), Value::String(result.got.clone())),
        (
            String::from("unit_unchecked"),
            Value::Bool(result.unit_unchecked),
        ),
    ];
    if let Some(reason) = &result.reason {
        members.push((String::from("reason"), Value::String(reason.clone())));
    }
    Value::Object(members)
}

/// The child side of the protocol: read paths, run them, write results.
///
/// `canlab corpus exec` calls this. It returns an error only when the protocol
/// itself is broken (stdin cannot be read); a row that cannot be loaded is a
/// `load-error` result, because the parent must count it as exit 2 rather than
/// as a failed answer.
pub fn exec_loop(solver: &dyn Solver) -> Result<(), String> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.map_err(|error| format!("could not read a request: {error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let request = json::parse(&line)?;
        let path = value_path(&request)?;
        if path.is_empty() {
            return Err(format!("a request has an empty `path`: {line:?}"));
        }
        let path = PathBuf::from(path);
        let result = match crate::row::load_row(&path) {
            Ok(row) => {
                let outcome = run::run_row(&row, solver);
                match outcome {
                    Outcome::Pass {
                        got,
                        unit_unchecked,
                    } => ExecResult {
                        status: ExecStatus::Pass,
                        got,
                        unit_unchecked,
                        reason: None,
                    },
                    Outcome::Fail { got, reason } => ExecResult {
                        status: ExecStatus::Fail,
                        got,
                        unit_unchecked: false,
                        reason,
                    },
                }
            }
            Err(error) => ExecResult {
                status: ExecStatus::LoadError,
                got: String::new(),
                unit_unchecked: false,
                reason: Some(error.reason),
            },
        };
        let line = json::encode(&result_object(&path.to_string_lossy(), &result));
        writeln!(stdout, "{line}").map_err(|error| format!("could not write a result: {error}"))?;
        stdout
            .flush()
            .map_err(|error| format!("could not flush a result: {error}"))?;
    }
    Ok(())
}

fn value_path(request: &Value) -> Result<String, String> {
    request
        .get("path")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| String::from("a request has no string `path`"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::row::{Expect, Subject};
    use std::path::Path;

    fn row(name: &str) -> Row {
        Row {
            path: PathBuf::from(format!("tests/cases/math/{name}.toml")),
            id: name.to_string(),
            subject: Subject::Math,
            capability: String::from("solve.linear"),
            source: String::from("https://example.test/paper — 2024, uppgift 1"),
            document: String::from("x + 1 = 3"),
            expect: Expect::Exact(String::from("2")),
            note: None,
        }
    }

    #[cfg(windows)]
    fn crashing() -> ExecCommand {
        ExecCommand {
            program: PathBuf::from("cmd"),
            args: vec![
                String::from("/c"),
                String::from("exit"),
                String::from("7"),
            ],
        }
    }

    #[cfg(not(windows))]
    fn crashing() -> ExecCommand {
        ExecCommand {
            program: PathBuf::from("sh"),
            args: vec![String::from("-c"), String::from("exit 7")],
        }
    }

    #[cfg(windows)]
    fn hanging() -> ExecCommand {
        // A direct child, not `cmd /c ping`: killing the process must close the
        // pipe, and a grandchild would keep it open for the rest of its sleep.
        ExecCommand {
            program: PathBuf::from("ping"),
            args: vec![
                String::from("-n"),
                String::from("60"),
                String::from("127.0.0.1"),
            ],
        }
    }

    #[cfg(not(windows))]
    fn hanging() -> ExecCommand {
        ExecCommand {
            program: PathBuf::from("sh"),
            args: vec![String::from("-c"), String::from("sleep 60")],
        }
    }

    #[test]
    fn a_child_that_exits_non_zero_loses_exactly_its_batch() {
        let rows = vec![row("a"), row("b")];
        let results = run_batch(&crashing(), &rows, Duration::from_secs(5));
        assert_eq!(results.len(), 2);
        for result in results {
            let error = result.expect_err("a crashed batch has no result");
            assert!(
                error.contains("exited with code 7") || error.contains("exit code 7"),
                "unexpected reason: {error}"
            );
        }
    }

    #[test]
    fn a_child_that_hangs_loses_its_batch_at_the_deadline() {
        let rows = vec![row("a")];
        let results = run_batch(&hanging(), &rows, Duration::from_millis(300));
        let error = results[0]
            .clone()
            .expect_err("a hung batch has no result");
        assert!(error.contains("deadline"), "unexpected reason: {error}");
    }

    #[test]
    fn the_protocol_round_trips() {
        let result = ExecResult {
            status: ExecStatus::Pass,
            got: String::from("\\left\\{2\\right\\}"),
            unit_unchecked: true,
            reason: None,
        };
        let line = json::encode(&result_object("tests/cases/math/a.toml", &result));
        let (path, parsed) = parse_result(&line).expect("a result line");
        assert_eq!(path.as_path(), Path::new("tests/cases/math/a.toml"));
        assert_eq!(parsed, result);

        let failed = ExecResult {
            status: ExecStatus::Fail,
            got: String::from("3"),
            unit_unchecked: false,
            reason: Some(String::from("expected 2")),
        };
        let line = json::encode(&result_object("b.toml", &failed));
        let (_, parsed) = parse_result(&line).expect("a result line");
        assert_eq!(parsed, failed);
    }

    #[test]
    fn a_broken_result_line_is_refused() {
        for line in [
            "",
            "not json",
            r#"{"status":"pass"}"#,
            r#"{"path":"a","status":"maybe"}"#,
        ] {
            assert!(parse_result(line).is_err(), "accepted {line:?}");
        }
    }
}
