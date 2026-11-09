//! The request/response protocol, over any reader and writer.
//!
//! One JSON object per line, in both directions:
//!
//! ```text
//! -> {"id":1,"op":"integrate","expr":{"Pow":{…}},"var":"x"}
//! <- {"id":1,"ok":{"Mul":[…]}}
//! <- {"id":1,"err":"not_implemented"}
//! ```
//!
//! # Why a line, and why an id
//!
//! A line is the smallest unit a reader can wait on, so the child process can
//! be as dumb as `for line in sys.stdin`. The id makes the channel
//! self-checking: if the worker is restarted mid-flight, or writes a stray
//! `print()` from library code, the ids diverge and we learn about it on the
//! very next answer instead of silently attributing one expression's result to
//! another. A protocol without ids fails *plausibly*, which is worse than
//! failing loudly.
//!
//! # Why this is generic
//!
//! [`Session`] never touches a process. It is generic over [`BufRead`] and
//! [`Write`], which is what allows the whole state machine — success, decline,
//! timeout, EOF, garbage — to be tested from [`std::io::Cursor`] with no
//! Python, no subprocess and no sleeps. [`crate::sympy`] supplies the pipes;
//! the tests supply the script.
//!
//! # Payloads
//!
//! `request` takes the op-specific members as a JSON **object** (or the empty
//! string). They are spliced into the envelope verbatim, which keeps the wire
//! shape exactly `{"id":…,"op":…,<members>}` without this module having to know
//! that `integrate` wants `expr` and `var` while `series` also wants `order`.
//! The payload is parsed before it is sent: a malformed body is caught here,
//! not diagnosed later by a Python traceback.

use std::io::{BufRead, Write};

use crate::SolverError;
use crate::json::{self, Json};

/// How many empty lines in a row we tolerate before deciding the worker is
/// emitting noise rather than answers.
const MAX_BLANK_LINES: usize = 64;

/// A conversation with one backend.
///
/// The type is deliberately not `Clone`, and every request takes `&mut self`:
/// a session is a single ordered dialogue, and two requests in flight through
/// one pipe would interleave their answers.
pub struct Session<R: BufRead, W: Write> {
    reader: R,
    writer: W,
    next_id: u64,
}

impl<R: BufRead, W: Write> Session<R, W> {
    /// Start a session; the first request gets id 1.
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            reader,
            writer,
            next_id: 1,
        }
    }

    /// Send one request and read its answer.
    ///
    /// `payload` is a JSON object holding the op-specific members, or `""` for
    /// none. On success the returned string is the compact JSON of the `ok`
    /// value: an expression for most ops, an array of expressions for `solve`.
    ///
    /// A request that cannot be formed — malformed payload — is rejected
    /// before anything is written, so it does not consume an id. Once a line
    /// has gone out the id is spent even if the round trip then fails, so a
    /// retry can never be confused with the late answer to the abandoned
    /// request.
    pub fn request(&mut self, op: &str, payload: &str) -> Result<String, SolverError> {
        let members = payload_members(payload)?;
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);

        let mut line = String::with_capacity(48 + members.len());
        line.push_str("{\"id\":");
        line.push_str(&id.to_string());
        line.push_str(",\"op\":");
        line.push_str(&json::quote(op));
        if !members.is_empty() {
            line.push(',');
            line.push_str(members);
        }
        line.push('}');

        self.write_line(&line)?;
        self.read_answer(id)
    }

    /// Take the underlying reader and writer back.
    ///
    /// This is how a caller reclaims the pipes after the conversation, and how
    /// the tests inspect exactly what went out.
    pub fn into_inner(self) -> (R, W) {
        (self.reader, self.writer)
    }

    fn write_line(&mut self, line: &str) -> Result<(), SolverError> {
        self.writer
            .write_all(line.as_bytes())
            .map_err(|error| io_failure("writing a request", error))?;
        self.writer
            .write_all(b"\n")
            .map_err(|error| io_failure("writing a request", error))?;
        // The worker blocks on `readline`; without this flush the request sits
        // in our buffer and both sides wait for ever.
        self.writer
            .flush()
            .map_err(|error| io_failure("flushing a request", error))
    }

    fn read_answer(&mut self, id: u64) -> Result<String, SolverError> {
        let line = self.read_response_line()?;
        let value = json::parse(line.trim())?;
        if value.as_object().is_none() {
            return Err(SolverError::BadOutput(
                "response must be a JSON object".into(),
            ));
        }

        let announced = value
            .member("id")
            .and_then(number_id)
            .ok_or_else(|| SolverError::BadOutput("response is missing a numeric \"id\"".into()))?;
        if announced != id {
            return Err(SolverError::BadOutput(format!(
                "response id {announced} does not match request id {id}"
            )));
        }

        let ok = value.member("ok");
        let err = value.member("err");
        match (ok, err) {
            (Some(ok), None) => Ok(json::compact(ok)),
            (None, Some(Json::Str(code))) => Err(reported_error(code)),
            (None, Some(_)) => Err(SolverError::BadOutput("\"err\" must hold a string".into())),
            (Some(_), Some(_)) => Err(SolverError::BadOutput(
                "response has both \"ok\" and \"err\"".into(),
            )),
            (None, None) => Err(SolverError::BadOutput(
                "response has neither \"ok\" nor \"err\"".into(),
            )),
        }
    }

    fn read_response_line(&mut self) -> Result<String, SolverError> {
        let mut blank_lines = 0;
        loop {
            let mut line = String::new();
            let read = self
                .reader
                .read_line(&mut line)
                .map_err(|error| io_failure("reading a response", error))?;
            if read == 0 {
                return Err(SolverError::Backend("worker exited".into()));
            }
            if line.trim().is_empty() {
                blank_lines += 1;
                if blank_lines > MAX_BLANK_LINES {
                    return Err(SolverError::BadOutput(
                        "worker sent nothing but blank lines".into(),
                    ));
                }
                continue;
            }
            return Ok(line);
        }
    }
}

/// The members of `payload`, ready to be spliced into the envelope.
fn payload_members(payload: &str) -> Result<&str, SolverError> {
    let trimmed = payload.trim();
    if trimmed.is_empty() {
        return Ok("");
    }
    match json::parse(trimmed)? {
        Json::Object(members) if members.is_empty() => Ok(""),
        Json::Object(_) => trimmed
            .strip_prefix('{')
            .and_then(|inner| inner.strip_suffix('}'))
            .map(str::trim)
            .ok_or_else(|| SolverError::BadOutput("request payload is not a JSON object".into())),
        _ => Err(SolverError::BadOutput(
            "request payload must be a JSON object".into(),
        )),
    }
}

fn number_id(value: &Json) -> Option<u64> {
    match value {
        Json::Num(text) => text.parse().ok(),
        _ => None,
    }
}

/// Translate the worker's `err` code into our error type.
///
/// `not_implemented` is the one code that is a normal answer; `timeout` is the
/// worker's own deadline (distinct from ours, which fires before it can).
/// Anything else is the backend's problem text shown to the user verbatim.
fn reported_error(code: &str) -> SolverError {
    match code {
        "not_implemented" => SolverError::NotImplemented,
        "timeout" => SolverError::Timeout,
        message => SolverError::Backend(message.to_string()),
    }
}

fn io_failure(action: &str, error: std::io::Error) -> SolverError {
    SolverError::Backend(format!("{action} failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn session(responses: &str) -> Session<Cursor<&[u8]>, Vec<u8>> {
        Session::new(Cursor::new(responses.as_bytes()), Vec::new())
    }

    fn written(session: Session<Cursor<&[u8]>, Vec<u8>>) -> String {
        let (_, bytes) = session.into_inner();
        String::from_utf8(bytes).expect("requests are UTF-8")
    }

    #[test]
    fn request_writes_the_exact_line_and_returns_the_ok_payload() {
        let mut session = session("{\"id\":1,\"ok\":{\"Num\":\"2\"}}\n");
        let answer = session
            .request("simplify", r#"{"expr":{"Sym":"x"}}"#)
            .expect("simplify succeeds");
        assert_eq!(answer, r#"{"Num":"2"}"#);
        assert_eq!(
            written(session),
            "{\"id\":1,\"op\":\"simplify\",\"expr\":{\"Sym\":\"x\"}}\n"
        );
    }

    #[test]
    fn empty_payload_omits_the_trailing_comma() {
        let mut session = session("{\"id\":1,\"ok\":{\"Num\":\"0\"}}\n");
        session.request("ping", "").expect("ping succeeds");
        assert_eq!(written(session), "{\"id\":1,\"op\":\"ping\"}\n");
    }

    #[test]
    fn ids_increase_across_requests() {
        let mut session =
            session("{\"id\":1,\"ok\":{\"Num\":\"1\"}}\n{\"id\":2,\"ok\":{\"Num\":\"2\"}}\n");
        session.request("simplify", "").expect("first");
        session.request("simplify", "").expect("second");
        let lines = written(session);
        assert_eq!(
            lines,
            "{\"id\":1,\"op\":\"simplify\"}\n{\"id\":2,\"op\":\"simplify\"}\n"
        );
    }

    #[test]
    fn not_implemented_is_a_normal_answer() {
        let mut session = session("{\"id\":1,\"err\":\"not_implemented\"}\n");
        let error = session
            .request("integrate", r#"{"expr":{"Sym":"x"}}"#)
            .expect_err("the worker declined");
        assert_eq!(error, SolverError::NotImplemented);
        assert!(error.is_declined());
        assert!(!error.is_fatal());
    }

    #[test]
    fn timeout_is_mapped_to_timeout() {
        let mut session = session("{\"id\":1,\"err\":\"timeout\"}\n");
        let error = session.request("limit", "").expect_err("timed out");
        assert_eq!(error, SolverError::Timeout);
    }

    #[test]
    fn any_other_error_text_becomes_a_backend_error() {
        let mut session = session("{\"id\":1,\"err\":\"ZeroDivisionError: division by zero\"}\n");
        let error = session.request("solve", "").expect_err("failed");
        assert_eq!(
            error,
            SolverError::Backend("ZeroDivisionError: division by zero".into())
        );
    }

    #[test]
    fn a_mismatched_id_is_bad_output() {
        let mut session = session("{\"id\":7,\"ok\":{\"Num\":\"2\"}}\n");
        let error = session.request("simplify", "").expect_err("wrong id");
        assert!(matches!(error, SolverError::BadOutput(_)), "{error:?}");
        assert!(error.to_string().contains('7'));
    }

    #[test]
    fn eof_is_a_backend_error() {
        let mut session = session("");
        let error = session.request("simplify", "").expect_err("worker gone");
        assert_eq!(error, SolverError::Backend("worker exited".into()));
    }

    #[test]
    fn a_garbage_line_is_bad_output() {
        let mut session = session("Traceback (most recent call last):\n");
        let error = session.request("simplify", "").expect_err("garbage");
        assert!(matches!(error, SolverError::BadOutput(_)), "{error:?}");
    }

    #[test]
    fn a_response_without_ok_or_err_is_bad_output() {
        let mut session = session("{\"id\":1}\n");
        assert!(matches!(
            session.request("simplify", ""),
            Err(SolverError::BadOutput(_))
        ));
    }

    #[test]
    fn a_response_with_both_ok_and_err_is_bad_output() {
        let mut session = session("{\"id\":1,\"ok\":{\"Num\":\"2\"},\"err\":\"nope\"}\n");
        assert!(matches!(
            session.request("simplify", ""),
            Err(SolverError::BadOutput(_))
        ));
    }

    #[test]
    fn a_non_string_err_is_bad_output() {
        let mut session = session("{\"id\":1,\"err\":17}\n");
        assert!(matches!(
            session.request("simplify", ""),
            Err(SolverError::BadOutput(_))
        ));
    }

    #[test]
    fn blank_lines_before_an_answer_are_ignored() {
        let mut session = session("\n\r\n{\"id\":1,\"ok\":{\"Num\":\"5\"}}\n");
        assert_eq!(
            session.request("simplify", "").expect("answer"),
            r#"{"Num":"5"}"#
        );
    }

    #[test]
    fn a_solve_answer_can_be_an_array() {
        let mut session = session("{\"id\":1,\"ok\":[{\"Num\":\"1\"},{\"Num\":\"-1\"}]}\n");
        let answer = session.request("solve", "").expect("answer");
        assert_eq!(answer, r#"[{"Num":"1"},{"Num":"-1"}]"#);
    }

    #[test]
    fn a_malformed_payload_never_reaches_the_worker() {
        let mut session = session("{\"id\":1,\"ok\":{\"Num\":\"1\"}}\n");
        let error = session.request("simplify", "5").expect_err("not an object");
        assert!(matches!(error, SolverError::BadOutput(_)), "{error:?}");
        // Nothing was written, so the next request is still id 1 — and the
        // canned answer proves the worker never saw the rejected payload.
        session.request("simplify", "").expect("first real request");
        assert_eq!(written(session), "{\"id\":1,\"op\":\"simplify\"}\n");
    }
}
