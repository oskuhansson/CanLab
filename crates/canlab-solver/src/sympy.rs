//! The real backend: `python3 worker.py` behind a pipe.
//!
//! SymPy is a Python library, so the solver is a child process. That is a
//! feature, not a workaround:
//!
//! * a symbolic backend that segfaults, allocates unboundedly or hangs takes
//!   down its own process, and the editor survives with a
//!   [`SolverError::Backend`];
//! * `std::process::Child::kill` is the only portable way to stop a runaway
//!   computation, which is what makes [`SolverError::Timeout`] an actual
//!   guarantee rather than a hopeful `select`;
//! * the protocol is one line in, one line out, so the boundary is auditable.
//!
//! # Threads, because `std` has no read timeout
//!
//! The standard library offers no deadline on `read`. Rather than pull in a
//! crate (there is no network here), the [`Session`] is moved onto a thread of
//! its own and the main thread waits on a channel with `recv_timeout`. On a
//! miss the child is killed — which closes its stdout and unblocks the reader
//! thread with EOF — and the whole pipe is dropped. A late answer can therefore
//! never be mistaken for the answer to the next request.
//!
//! # After a failure
//!
//! A timed-out or crashed worker is *not* restarted automatically. Silently
//! respawning would make a pathological expression look like a one-off hiccup
//! and could loop for ever; the caller decides, and calls [`SympyWorker::restart`].

use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use canlab_core::{Expr, SymbolId};

use crate::json;
use crate::session::Session;
use crate::{Solver, SolverError};

/// How long one request may take before the worker is killed.
///
/// Thirty seconds is long enough for a hard integral and short enough that a
/// hung backend does not look like a frozen application.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// One queued request for the session thread.
struct Job {
    op: String,
    payload: String,
}

/// The thread that owns the [`Session`]: it turns jobs into lines and lines
/// back into answers.
///
/// It stops after the first fatal answer, because every later answer on a dead
/// pipe would be a lie.
fn serve<R, W>(
    mut session: Session<R, W>,
    jobs: Receiver<Job>,
    answers: Sender<Result<String, SolverError>>,
) where
    R: BufRead,
    W: Write,
{
    while let Ok(job) = jobs.recv() {
        let answer = session.request(&job.op, &job.payload);
        let fatal = matches!(
            &answer,
            Err(SolverError::Timeout) | Err(SolverError::Backend(_))
        );
        if answers.send(answer).is_err() {
            break; // nobody is listening any more
        }
        if fatal {
            break;
        }
    }
}

/// Wait for one answer, mapping a missed deadline to [`SolverError::Timeout`].
fn recv_answer(
    answers: &Receiver<Result<String, SolverError>>,
    timeout: Duration,
) -> Result<String, SolverError> {
    match answers.recv_timeout(timeout) {
        Ok(answer) => answer,
        Err(RecvTimeoutError::Timeout) => Err(SolverError::Timeout),
        Err(RecvTimeoutError::Disconnected) => {
            Err(SolverError::Backend("worker thread exited".into()))
        }
    }
}

/// A live child process plus the thread that talks to it.
struct Pipe {
    /// `None` for a pipe built over in-memory streams in the tests.
    child: Option<Child>,
    jobs: Option<Sender<Job>>,
    answers: Receiver<Result<String, SolverError>>,
    thread: Option<JoinHandle<()>>,
}

impl Pipe {
    fn start(program: &str, script: &Path) -> io::Result<Self> {
        let mut child = Command::new(program)
            .arg(script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // A worker that logs to a pipe nobody drains blocks for ever once
            // the pipe buffer fills. Losing the traceback is the cheaper loss.
            .stderr(Stdio::null())
            .spawn()?;

        let stdin = child.stdin.take().ok_or_else(|| missing_pipe("stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| missing_pipe("stdout"))?;

        let session = Session::new(BufReader::new(stdout), stdin);
        Ok(Self::with_session(Some(child), session))
    }

    /// Move `session` onto its own thread and hand back the channels.
    fn with_session<R, W>(child: Option<Child>, session: Session<R, W>) -> Self
    where
        R: BufRead + Send + 'static,
        W: Write + Send + 'static,
    {
        let (job_tx, job_rx) = mpsc::channel();
        let (answer_tx, answer_rx) = mpsc::channel();
        let thread = thread::spawn(move || serve(session, job_rx, answer_tx));

        Self {
            child,
            jobs: Some(job_tx),
            answers: answer_rx,
            thread: Some(thread),
        }
    }

    /// A pipe that talks to `reader`/`writer` instead of a process.
    ///
    /// This is what lets the tests drive [`SympyWorker`] end to end — request
    /// bodies, ids, decoding — without a Python interpreter anywhere near it.
    #[cfg(test)]
    fn in_memory<R, W>(reader: R, writer: W) -> Self
    where
        R: BufRead + Send + 'static,
        W: Write + Send + 'static,
    {
        Self::with_session(None, Session::new(reader, writer))
    }

    fn exchange(&self, op: &str, payload: &str, timeout: Duration) -> Result<String, SolverError> {
        let jobs = self
            .jobs
            .as_ref()
            .ok_or_else(|| SolverError::Backend("worker channel is closed".into()))?;
        jobs.send(Job {
            op: op.to_string(),
            payload: payload.to_string(),
        })
        .map_err(|_| SolverError::Backend("worker thread exited".into()))?;
        recv_answer(&self.answers, timeout)
    }
}

impl Drop for Pipe {
    fn drop(&mut self) {
        // Kill first: the reader thread is most likely blocked in `read_line`,
        // and closing the child's end is what makes that read return EOF.
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        // Releasing the job sender lets the thread leave its loop once the
        // in-flight request has been settled.
        self.jobs.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn missing_pipe(which: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::BrokenPipe,
        format!("the child process has no {which} pipe"),
    )
}

/// A solver backed by an external worker process, normally `python3 worker.py`.
///
/// All requests are serialised through an internal mutex: the protocol is one
/// question at a time, and `Solver` hands out `&self`.
pub struct SympyWorker {
    program: String,
    script: PathBuf,
    timeout: Duration,
    /// `None` after a timeout or a crash, until [`SympyWorker::restart`].
    pipe: Mutex<Option<Pipe>>,
}

impl SympyWorker {
    /// Start `program` with `script` as its only argument.
    ///
    /// The worker is not probed: a wrong interpreter fails on the first
    /// request with [`SolverError::Backend`], which is the same path a crash
    /// takes and therefore the one path worth keeping exercised.
    pub fn spawn(program: &str, script: &Path) -> io::Result<Self> {
        let pipe = Pipe::start(program, script)?;
        Ok(Self {
            program: program.to_string(),
            script: script.to_path_buf(),
            timeout: DEFAULT_TIMEOUT,
            pipe: Mutex::new(Some(pipe)),
        })
    }

    /// Replace the per-request deadline.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Kill the current worker, if any, and start a fresh one.
    ///
    /// Call this after [`SolverError::Timeout`] or [`SolverError::Backend`];
    /// until then every request fails fast instead of touching a dead pipe.
    pub fn restart(&mut self) -> io::Result<()> {
        let mut guard = self.lock();
        *guard = None;
        let pipe = Pipe::start(&self.program, &self.script)?;
        *guard = Some(pipe);
        Ok(())
    }

    /// Whether a worker is currently attached.
    pub fn is_running(&self) -> bool {
        self.lock().is_some()
    }

    fn lock(&self) -> MutexGuard<'_, Option<Pipe>> {
        // A panic elsewhere must not poison the solver for good: the state we
        // keep behind this lock is a process handle, and it is still valid.
        self.pipe
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn call(&self, op: &str, payload: &str) -> Result<String, SolverError> {
        let mut guard = self.lock();
        let pipe = match guard.as_mut() {
            Some(pipe) => pipe,
            None => {
                return Err(SolverError::Backend(
                    "worker is not running; restart it".into(),
                ));
            }
        };
        let answer = pipe.exchange(op, payload, self.timeout);
        if matches!(&answer, Err(error) if error.is_fatal()) {
            // The pipe cannot be trusted any more: a late answer would be read
            // as the answer to the *next* request.
            *guard = None;
        }
        answer
    }
}

impl Solver for SympyWorker {
    fn simplify(&self, e: &Expr) -> Result<Expr, SolverError> {
        decode_one(&self.call("simplify", &payload_expr(e))?)
    }

    fn diff(&self, e: &Expr, var: SymbolId) -> Result<Expr, SolverError> {
        decode_one(&self.call("diff", &payload_expr_var(e, var))?)
    }

    fn integrate(&self, e: &Expr, var: SymbolId) -> Result<Expr, SolverError> {
        decode_one(&self.call("integrate", &payload_expr_var(e, var))?)
    }

    fn solve(&self, eq: &Expr, var: SymbolId) -> Result<Vec<Expr>, SolverError> {
        let answer = self.call("solve", &payload_solve(eq, var))?;
        json::decode_array(&answer)
    }

    fn limit(&self, e: &Expr, var: SymbolId, at: &Expr) -> Result<Expr, SolverError> {
        decode_one(&self.call("limit", &payload_limit(e, var, at))?)
    }

    fn series(
        &self,
        e: &Expr,
        var: SymbolId,
        at: &Expr,
        order: usize,
    ) -> Result<Expr, SolverError> {
        decode_one(&self.call("series", &payload_series(e, var, at, order))?)
    }
}

fn decode_one(answer: &str) -> Result<Expr, SolverError> {
    json::decode(answer)
}

// ---------------------------------------------------------------------------
// Request bodies
//
// These are the JSON objects `Session::request` splices into the envelope.
// They are free functions so the tests can pin the exact bytes without a
// process.
// ---------------------------------------------------------------------------

/// `{"expr":<encoded>}`
fn payload_expr(e: &Expr) -> String {
    format!("{{\"expr\":{}}}", json::encode(e))
}

/// `{"expr":<encoded>,"var":"x"}`
fn payload_expr_var(e: &Expr, var: SymbolId) -> String {
    format!(
        "{{\"expr\":{},\"var\":{}}}",
        json::encode(e),
        json::quote(&var.as_str())
    )
}

/// `{"eq":<encoded>,"var":"x"}`
fn payload_solve(eq: &Expr, var: SymbolId) -> String {
    format!(
        "{{\"eq\":{},\"var\":{}}}",
        json::encode(eq),
        json::quote(&var.as_str())
    )
}

/// `{"expr":<encoded>,"var":"x","at":<encoded>}`
fn payload_limit(e: &Expr, var: SymbolId, at: &Expr) -> String {
    format!(
        "{{\"expr\":{},\"var\":{},\"at\":{}}}",
        json::encode(e),
        json::quote(&var.as_str()),
        json::encode(at)
    )
}

/// `{"expr":<encoded>,"var":"x","at":<encoded>,"order":N}`
fn payload_series(e: &Expr, var: SymbolId, at: &Expr, order: usize) -> String {
    format!(
        "{{\"expr\":{},\"var\":{},\"at\":{},\"order\":{}}}",
        json::encode(e),
        json::quote(&var.as_str()),
        json::encode(at),
        order
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use canlab_core::{int, pow, sym};
    use std::io::Cursor;

    impl SympyWorker {
        /// A worker in the state it reaches after a fatal error: no pipe.
        fn dead() -> Self {
            Self {
                program: "canlab-solver-no-such-interpreter".to_string(),
                script: PathBuf::from("worker.py"),
                timeout: Duration::from_millis(20),
                pipe: Mutex::new(None),
            }
        }

        /// A worker whose "process" is a canned transcript.
        fn scripted(responses: &'static str, request_log: SharedBuffer) -> Self {
            Self {
                program: "python3".to_string(),
                script: PathBuf::from("worker.py"),
                timeout: Duration::from_secs(5),
                pipe: Mutex::new(Some(Pipe::in_memory(
                    Cursor::new(responses.as_bytes()),
                    request_log,
                ))),
            }
        }
    }

    /// A writer that keeps everything so the test can assert on it.
    #[derive(Clone, Default)]
    struct SharedBuffer(std::sync::Arc<Mutex<Vec<u8>>>);

    impl Write for SharedBuffer {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl SharedBuffer {
        fn text(&self) -> String {
            let guard = self
                .0
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            String::from_utf8(guard.clone()).expect("requests are UTF-8")
        }
    }

    #[test]
    fn a_live_pipe_round_trips_a_request_and_its_answer() {
        let log = SharedBuffer::default();
        let worker = SympyWorker::scripted(
            "{\"id\":1,\"ok\":{\"Mul\":[{\"Num\":\"1/3\"},{\"Sym\":\"x\"}]}}\n",
            log.clone(),
        );

        let answer = worker
            .integrate(&pow(sym("x"), int(2)), SymbolId::intern("x"))
            .expect("the fake worker answers");
        assert_eq!(answer.to_string(), "1/3*x");
        assert_eq!(
            log.text(),
            "{\"id\":1,\"op\":\"integrate\",\"expr\":{\"Pow\":{\"base\":{\"Sym\":\"x\"},\"exp\":{\"Num\":\"2\"}}},\"var\":\"x\"}\n"
        );
        assert!(worker.is_running(), "a successful request keeps the pipe");
    }

    #[test]
    fn solve_reads_the_answer_array() {
        let worker = SympyWorker::scripted(
            "{\"id\":1,\"ok\":[{\"Num\":\"1\"},{\"Num\":\"-1\"}]}\n",
            SharedBuffer::default(),
        );
        let solutions = worker
            .solve(&pow(sym("x"), int(2)), SymbolId::intern("x"))
            .expect("two roots");
        assert_eq!(solutions, vec![int(1), int(-1)]);
    }

    #[test]
    fn a_declined_request_leaves_the_worker_alive() {
        let worker = SympyWorker::scripted(
            "{\"id\":1,\"err\":\"not_implemented\"}\n",
            SharedBuffer::default(),
        );
        assert_eq!(
            worker.integrate(&sym("x"), SymbolId::intern("x")),
            Err(SolverError::NotImplemented)
        );
        assert!(worker.is_running());
    }

    #[test]
    fn a_backend_failure_retires_the_worker() {
        let worker = SympyWorker::scripted(
            "{\"id\":1,\"err\":\"ZeroDivisionError: division by zero\"}\n",
            SharedBuffer::default(),
        );
        assert_eq!(
            worker.limit(&sym("x"), SymbolId::intern("x"), &int(0)),
            Err(SolverError::Backend(
                "ZeroDivisionError: division by zero".into()
            ))
        );
        assert!(!worker.is_running(), "a failed pipeline must not be reused");
        assert!(matches!(
            worker.simplify(&int(1)),
            Err(SolverError::Backend(_))
        ));
    }

    #[test]
    fn the_worker_fits_the_solver_contract() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SympyWorker>();

        let worker: std::sync::Arc<dyn Solver> = std::sync::Arc::new(SympyWorker::dead());
        let clone = std::sync::Arc::clone(&worker);
        let handle = std::thread::spawn(move || clone.simplify(&int(1)));
        assert!(handle.join().expect("thread finishes").is_err());
    }

    #[test]
    fn restarting_after_a_failure_reports_a_missing_interpreter() {
        let mut worker = SympyWorker::dead();
        match worker.restart() {
            Ok(()) => panic!("there is no such interpreter; restart must fail"),
            Err(error) => assert_eq!(error.kind(), io::ErrorKind::NotFound),
        }
        assert!(!worker.is_running());
    }

    #[test]
    fn spawning_a_missing_program_fails_without_panicking() {
        match SympyWorker::spawn("canlab-solver-no-such-interpreter", Path::new("worker.py")) {
            Ok(worker) => panic!("spawn must fail, got running={}", worker.is_running()),
            Err(error) => assert_eq!(error.kind(), io::ErrorKind::NotFound),
        }
    }

    #[test]
    fn a_dead_worker_reports_backend_errors_for_every_operation() {
        let worker = SympyWorker::dead();
        let x = sym("x");
        let results: [Result<(), SolverError>; 6] = [
            worker.simplify(&x).map(drop),
            worker.diff(&x, SymbolId::intern("x")).map(drop),
            worker.integrate(&x, SymbolId::intern("x")).map(drop),
            worker.solve(&x, SymbolId::intern("x")).map(drop),
            worker.limit(&x, SymbolId::intern("x"), &int(0)).map(drop),
            worker
                .series(&x, SymbolId::intern("x"), &int(0), 3)
                .map(drop),
        ];
        for result in results {
            assert!(matches!(result, Err(SolverError::Backend(_))), "{result:?}");
        }
        assert!(!worker.is_running());
    }

    #[test]
    fn the_deadline_is_enforced_on_the_answer_channel() {
        let (answer_tx, answer_rx) = mpsc::channel::<Result<String, SolverError>>();
        assert_eq!(
            recv_answer(&answer_rx, Duration::from_millis(10)),
            Err(SolverError::Timeout)
        );

        answer_tx.send(Ok("{}".to_string())).expect("send");
        assert_eq!(
            recv_answer(&answer_rx, Duration::from_secs(1)),
            Ok("{}".to_string())
        );

        drop(answer_tx);
        assert!(matches!(
            recv_answer(&answer_rx, Duration::from_secs(1)),
            Err(SolverError::Backend(_))
        ));
    }

    #[test]
    fn the_reader_thread_answers_in_order_and_stops_at_eof() {
        let transcript = "{\"id\":1,\"ok\":{\"Num\":\"7\"}}\n";
        let session = Session::new(Cursor::new(transcript.as_bytes()), io::sink());
        let (job_tx, job_rx) = mpsc::channel();
        let (answer_tx, answer_rx) = mpsc::channel();
        let thread = thread::spawn(move || serve(session, job_rx, answer_tx));

        job_tx
            .send(Job {
                op: "simplify".into(),
                payload: String::new(),
            })
            .expect("queue the first request");
        assert_eq!(
            recv_answer(&answer_rx, Duration::from_secs(5)).expect("first answer"),
            "{\"Num\":\"7\"}"
        );

        // The reader is now at EOF: the thread must report it and stop, not
        // spin or silently answer the next request with the same line.
        job_tx
            .send(Job {
                op: "simplify".into(),
                payload: String::new(),
            })
            .expect("queue the second request");
        assert_eq!(
            recv_answer(&answer_rx, Duration::from_secs(5)),
            Err(SolverError::Backend("worker exited".into()))
        );

        thread.join().expect("the reader thread finishes");
    }

    #[test]
    fn request_bodies_are_well_formed_json_objects() {
        let x = sym("x");
        let squared = pow(x.clone(), int(2));
        let var = SymbolId::intern("x");

        assert_eq!(
            payload_expr(&squared),
            r#"{"expr":{"Pow":{"base":{"Sym":"x"},"exp":{"Num":"2"}}}}"#
        );
        assert_eq!(
            payload_expr_var(&squared, var),
            r#"{"expr":{"Pow":{"base":{"Sym":"x"},"exp":{"Num":"2"}}},"var":"x"}"#
        );
        assert_eq!(
            payload_solve(&squared, var),
            r#"{"eq":{"Pow":{"base":{"Sym":"x"},"exp":{"Num":"2"}}},"var":"x"}"#
        );
        assert_eq!(
            payload_limit(&squared, var, &int(0)),
            r#"{"expr":{"Pow":{"base":{"Sym":"x"},"exp":{"Num":"2"}}},"var":"x","at":{"Num":"0"}}"#
        );
        assert_eq!(
            payload_series(&squared, var, &int(0), 5),
            r#"{"expr":{"Pow":{"base":{"Sym":"x"},"exp":{"Num":"2"}}},"var":"x","at":{"Num":"0"},"order":5}"#
        );

        // Every body must stand on its own as a JSON object.
        for payload in [
            payload_expr(&squared),
            payload_expr_var(&squared, var),
            payload_solve(&squared, var),
            payload_limit(&squared, var, &int(0)),
            payload_series(&squared, var, &int(0), 5),
        ] {
            assert!(json::parse(&payload).is_ok(), "{payload}");
        }
    }

    #[test]
    fn malformed_worker_output_is_bad_output_not_a_panic() {
        for answer in ["", "not json", r#"{"Num":"2"} trailing"#, r#"{"Nope":{}}"#] {
            assert!(
                matches!(decode_one(answer), Err(SolverError::BadOutput(_))),
                "{answer:?}"
            );
        }
    }
}
