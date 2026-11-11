//! The wire format between the CAS and a symbolic backend.
//!
//! # Why not LaTeX
//!
//! LaTeX is a *presentation* format. It loses the tree — `\frac{a}{b}` and
//! `a b^{-1}` print differently and mean the same thing, `x^2` versus
//! `x^{2}` depends on the printer, and the parse back is a research problem.
//! A solver needs the tree exactly as the core built it, including which
//! `Fn` was called and which `SymbolId` an integral binds.
//!
//! # Why JSON
//!
//! Every plausible backend — Python, Julia, a Node service — parses JSON with
//! its standard library, and JSON maps one-to-one onto the [`Expr`] tree:
//! objects for tagged nodes, arrays for `Add`/`Mul` operands. It also degrades
//! well: a human can read a wire dump and see the bug.
//!
//! # Shape
//!
//! ```text
//! {"Num":"3/2"}                          exact rational
//! {"Num":"2"}                            exact integer
//! {"Num":"1.5"}                          approximate
//! {"Sym":"x"}
//! {"Add":[...]} {"Mul":[...]}
//! {"Pow":{"base":{...},"exp":{...}}}
//! {"Fn":{"name":"sin","args":[...]}}
//! {"Integral":{"body":{...},"var":"x"}}
//! ```
//!
//! Numbers are **strings**, never JSON numbers. JSON has no rational type and
//! its number grammar conflates `2` and `2.0`, which is precisely the
//! distinction [`canlab_core::Number`] exists to keep.
//!
//! # Solution sets
//!
//! A `solve_all` answer wraps that tree in an explicit tag, because a list of
//! roots and a periodic family are different answers and a decoder must never
//! guess between them:
//!
//! ```text
//! {"kind":"none"}                         no solutions
//! {"kind":"all"}                          every value solves it
//! {"kind":"values","values":[<expr>,…]}   a finite set
//! {"kind":"family","base":<expr>,"period":<expr>,"param":"k"}
//! {"kind":"inequality","var":"x","intervals":[
//!     {"from":<expr>|null,"from_open":true,"to":<expr>|null,"to_open":false}]}
//! ```
//!
//! An interval end is an expression, or `null` for the unbounded end. Openness
//! is a separate boolean because `[1, 3)` and `(1, 3]` are different sets built
//! from the same two endpoints; an infinite end is always open, and encoding it
//! as closed is rejected rather than silently repaired.
//!
//! # Hand-written on purpose
//!
//! The workspace has no network and therefore no `serde`. A parser for the
//! subset above is ~300 lines; a dependency would have been 30 transitive
//! crates. The parser never panics on malformed input: every failure becomes
//! [`SolverError::BadOutput`] with a byte offset, because the input comes from
//! another process and may be anything at all.

use canlab_core::{Expr, ExprNode, Hold, Number, SymbolId, add, mul, normalize, pow};

use crate::SolverError;
use crate::solutions::{Bound, Solutions};

/// Maximum container nesting accepted by the parser.
///
/// The decoder is recursive, so unbounded nesting is a stack-overflow vector:
/// a hostile or buggy worker could send `[[[[…` and take the editor down. 256
/// is far beyond any real expression and far below the stack limit.
const MAX_DEPTH: usize = 256;

/// A parsed JSON value.
///
/// Numbers keep their literal text instead of an `f64`: the protocol only ever
/// puts numbers in the envelope (`"id"`, `"order"`), and re-emitting the
/// original text makes a round trip byte-for-byte lossless.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Json {
    /// `null`
    Null,
    /// `true` / `false`
    Bool(bool),
    /// A number, as written.
    Num(String),
    /// A string, already unescaped.
    Str(String),
    /// An array.
    Array(Vec<Json>),
    /// An object. Members keep their order so re-encoding is predictable.
    Object(Vec<(String, Json)>),
}

impl Json {
    /// The string payload, if this is a string.
    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(text) => Some(text),
            _ => None,
        }
    }

    /// The boolean payload, if this is `true` or `false`.
    pub(crate) fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// The elements, if this is an array.
    pub(crate) fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }

    /// The members, if this is an object.
    pub(crate) fn as_object(&self) -> Option<&[(String, Json)]> {
        match self {
            Json::Object(members) => Some(members),
            _ => None,
        }
    }

    /// Look up `key` in this object; `None` for a non-object or a missing key.
    pub(crate) fn member(&self, key: &str) -> Option<&Json> {
        member_of(self.as_object()?, key)
    }
}

/// Look up `key` in an already-destructured object.
fn member_of<'a>(members: &'a [(String, Json)], key: &str) -> Option<&'a Json> {
    members
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse exactly one JSON value out of `src`. Whitespace at either end is
/// allowed; anything else left over is an error.
pub(crate) fn parse(src: &str) -> Result<Json, SolverError> {
    let mut parser = Parser::new(src);
    let value = parser.value(0)?;
    parser.skip_whitespace();
    if parser.pos != parser.bytes.len() {
        return parser.fail("trailing characters after the JSON value");
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            bytes: src.as_bytes(),
            pos: 0,
        }
    }

    fn fail<T>(&self, message: impl std::fmt::Display) -> Result<T, SolverError> {
        Err(SolverError::BadOutput(format!(
            "{message} at byte {}",
            self.pos
        )))
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, SolverError> {
        if depth > MAX_DEPTH {
            return self.fail("JSON nested too deeply");
        }
        self.skip_whitespace();
        match self.peek() {
            None => self.fail("unexpected end of input"),
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => self.string().map(Json::Str),
            Some(b't') => {
                self.literal("true")?;
                Ok(Json::Bool(true))
            }
            Some(b'f') => {
                self.literal("false")?;
                Ok(Json::Bool(false))
            }
            Some(b'n') => {
                self.literal("null")?;
                Ok(Json::Null)
            }
            Some(b'-') | Some(b'0'..=b'9') => self.number(),
            Some(_) => self.fail("unexpected character"),
        }
    }

    fn literal(&mut self, word: &str) -> Result<(), SolverError> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(())
        } else {
            self.fail(format!("expected `{word}`"))
        }
    }

    fn number(&mut self) -> Result<Json, SolverError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => self.digits(),
            _ => return self.fail("malformed number"),
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return self.fail("malformed number: no digit after '.'");
            }
            self.digits();
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return self.fail("malformed number: empty exponent");
            }
            self.digits();
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| SolverError::BadOutput("number is not valid UTF-8".into()))?;
        Ok(Json::Num(text.to_string()))
    }

    fn digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, SolverError> {
        self.pos += 1; // '['
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Json::Array(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Json::Array(items));
                }
                _ => return self.fail("expected ',' or ']' in array"),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, SolverError> {
        self.pos += 1; // '{'
        let mut members: Vec<(String, Json)> = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Object(members));
        }
        loop {
            self.skip_whitespace();
            if self.peek() != Some(b'"') {
                return self.fail("expected a quoted object key");
            }
            let key = self.string()?;
            self.skip_whitespace();
            if self.peek() != Some(b':') {
                return self.fail("expected ':' after object key");
            }
            self.pos += 1;
            let value = self.value(depth + 1)?;
            members.push((key, value));
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Object(members));
                }
                _ => return self.fail("expected ',' or '}' in object"),
            }
        }
    }

    fn string(&mut self) -> Result<String, SolverError> {
        if self.peek() != Some(b'"') {
            return self.fail("expected a string");
        }
        self.pos += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let byte = match self.peek() {
                Some(byte) => byte,
                None => return self.fail("unterminated string"),
            };
            self.pos += 1;
            match byte {
                b'"' => break,
                b'\\' => {
                    let escape = match self.peek() {
                        Some(escape) => escape,
                        None => return self.fail("unterminated escape sequence"),
                    };
                    self.pos += 1;
                    match escape {
                        b'"' => out.push(b'"'),
                        b'\\' => out.push(b'\\'),
                        b'/' => out.push(b'/'),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0c),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let ch = self.unicode_escape()?;
                            let mut buffer = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
                        }
                        _ => return self.fail("unknown escape sequence"),
                    }
                }
                0x00..=0x1f => return self.fail("raw control character in string"),
                byte => out.push(byte),
            }
        }
        String::from_utf8(out)
            .map_err(|_| SolverError::BadOutput("string is not valid UTF-8".into()))
    }

    /// Four hex digits of a `\uXXXX` escape.
    fn hex4(&mut self) -> Result<u32, SolverError> {
        let mut value = 0u32;
        for _ in 0..4 {
            let digit = match self.peek() {
                Some(b'0'..=b'9') => self.bytes[self.pos] - b'0',
                Some(b'a'..=b'f') => self.bytes[self.pos] - b'a' + 10,
                Some(b'A'..=b'F') => self.bytes[self.pos] - b'A' + 10,
                Some(_) => return self.fail("non-hex digit in \\u escape"),
                None => return self.fail("truncated \\u escape"),
            };
            self.pos += 1;
            value = value * 16 + digit as u32;
        }
        Ok(value)
    }

    /// The rest of a `\uXXXX` escape, including surrogate pairs.
    fn unicode_escape(&mut self) -> Result<char, SolverError> {
        let first = self.hex4()?;
        if (0xd800..=0xdbff).contains(&first) {
            if self.peek() != Some(b'\\') {
                return self.fail("unpaired high surrogate");
            }
            self.pos += 1;
            if self.peek() != Some(b'u') {
                return self.fail("unpaired high surrogate");
            }
            self.pos += 1;
            let second = self.hex4()?;
            if !(0xdc00..=0xdfff).contains(&second) {
                return self.fail("invalid surrogate pair");
            }
            let code = 0x1_0000 + ((first - 0xd800) << 10) + (second - 0xdc00);
            return char::from_u32(code)
                .ok_or_else(|| SolverError::BadOutput("invalid code point".into()));
        }
        if (0xdc00..=0xdfff).contains(&first) {
            return self.fail("unpaired low surrogate");
        }
        char::from_u32(first).ok_or_else(|| SolverError::BadOutput("invalid code point".into()))
    }
}

// ---------------------------------------------------------------------------
// Serialisation
// ---------------------------------------------------------------------------

/// Render `text` as a JSON string literal, quotes included.
pub(crate) fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    write_string_body(text, &mut out);
    out.push('"');
    out
}

fn write_string_body(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
}

/// Re-render a parsed value as compact JSON.
pub(crate) fn compact(value: &Json) -> String {
    let mut out = String::new();
    write_compact(value, &mut out);
    out
}

fn write_compact(value: &Json, out: &mut String) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(true) => out.push_str("true"),
        Json::Bool(false) => out.push_str("false"),
        Json::Num(text) => out.push_str(text),
        Json::Str(text) => out.push_str(&quote(text)),
        Json::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_compact(item, out);
            }
            out.push(']');
        }
        Json::Object(members) => {
            out.push('{');
            for (index, (key, item)) in members.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&quote(key));
                out.push(':');
                write_compact(item, out);
            }
            out.push('}');
        }
    }
}

// ---------------------------------------------------------------------------
// Expression <-> JSON
// ---------------------------------------------------------------------------

/// Encode `e` as the tagged JSON of this module.
///
/// The output is compact (no spaces) and deterministic: the same tree always
/// produces the same bytes, which lets the tests assert on exact strings and
/// lets a transcript be diffed.
pub fn encode(e: &Expr) -> String {
    let mut out = String::new();
    write_expr(e, &mut out);
    out
}

fn write_expr(e: &Expr, out: &mut String) {
    match &**e {
        ExprNode::Num(number) => {
            out.push_str("{\"Num\":");
            out.push_str(&quote(&number_text(number)));
            out.push('}');
        }
        ExprNode::Sym(symbol) => {
            out.push_str("{\"Sym\":");
            out.push_str(&quote(&symbol.as_str()));
            out.push('}');
        }
        ExprNode::Add(terms) => write_tagged_list("Add", terms, out),
        ExprNode::Mul(factors) => write_tagged_list("Mul", factors, out),
        ExprNode::Pow(base, exp) => {
            out.push_str("{\"Pow\":{\"base\":");
            write_expr(base, out);
            out.push_str(",\"exp\":");
            write_expr(exp, out);
            out.push_str("}}");
        }
        ExprNode::Fn(name, args) => {
            out.push_str("{\"Fn\":{\"name\":");
            out.push_str(&quote(&name.as_str()));
            out.push_str(",\"args\":[");
            for (index, arg) in args.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_expr(arg, out);
            }
            out.push_str("]}}");
        }
        ExprNode::Hold(hold) => match &**hold {
            Hold::Integral { body, var } => {
                out.push_str("{\"Integral\":{\"body\":");
                write_expr(body, out);
                out.push_str(",\"var\":");
                out.push_str(&quote(&var.as_str()));
                out.push_str("}}");
            }
        },
    }
}

fn write_tagged_list(tag: &str, items: &[Expr], out: &mut String) {
    out.push_str("{\"");
    out.push_str(tag);
    out.push_str("\":[");
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        write_expr(item, out);
    }
    out.push_str("]}");
}

/// The text of a number inside `{"Num": … }`.
fn number_text(number: &Number) -> String {
    match number {
        Number::Int(value) => value.to_string(),
        Number::Rat(numerator, denominator) => format!("{numerator}/{denominator}"),
        Number::Float(value) => float_text(*value),
    }
}

/// Decimal text that always looks like a float.
///
/// [`f64`]'s `Display` writes `2` for `2.0`, which would decode back as an
/// exact `Number::Int(2)` and quietly lose the "approximate" flag. Appending
/// `.0` keeps the distinction visible on the wire.
fn float_text(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-inf".to_string()
        } else {
            "inf".to_string()
        };
    }
    let mut text = value.to_string();
    if !text.contains('.') && !text.contains('e') && !text.contains('E') {
        text.push_str(".0");
    }
    text
}

/// Parse the payload of `{"Num": … }`.
///
/// Accepted: `p/q`, integer text, decimal text (anything with `.`, `e` or `E`),
/// and the three non-finite floats `NaN`, `inf`, `-inf`. An integer too large
/// for `i128` is rejected rather than quietly turned into an approximation:
/// the wire is the one place where exactness can still be preserved, so it is
/// not the place to start rounding.
fn parse_number_text(text: &str) -> Result<Number, SolverError> {
    if text.is_empty() {
        return Err(SolverError::BadOutput("empty number literal".into()));
    }
    if let Some((numerator, denominator)) = text.split_once('/') {
        if denominator.contains('/') {
            return Err(SolverError::BadOutput(format!(
                "invalid rational literal {text:?}"
            )));
        }
        let numerator: i128 = numerator
            .parse()
            .map_err(|_| SolverError::BadOutput(format!("invalid rational literal {text:?}")))?;
        let denominator: i128 = denominator
            .parse()
            .map_err(|_| SolverError::BadOutput(format!("invalid rational literal {text:?}")))?;
        if denominator == 0 {
            return Err(SolverError::BadOutput(format!(
                "zero denominator in {text:?}"
            )));
        }
        return Ok(Number::ratio(numerator, denominator));
    }
    if text.contains('.') || text.contains('e') || text.contains('E') {
        return text
            .parse::<f64>()
            .map(Number::float)
            .map_err(|_| SolverError::BadOutput(format!("invalid float literal {text:?}")));
    }
    if let Ok(value) = text.parse::<i128>() {
        return Ok(Number::int(value));
    }
    match text {
        "NaN" | "nan" => Ok(Number::float(f64::NAN)),
        "inf" | "+inf" | "Infinity" | "+Infinity" => Ok(Number::float(f64::INFINITY)),
        "-inf" | "-Infinity" => Ok(Number::float(f64::NEG_INFINITY)),
        _ => Err(SolverError::BadOutput(format!(
            "invalid number literal {text:?}"
        ))),
    }
}

/// Decode one tagged-JSON expression.
///
/// The result is normalized, so a worker cannot smuggle in a non-canonical
/// tree: `{"Add":[]}` decodes to `0` and `{"Mul":[]}` to `1`. Key order inside
/// `Pow`, `Fn` and `Integral` does not matter; whitespace does not matter.
pub fn decode(text: &str) -> Result<Expr, SolverError> {
    let value = parse(text)?;
    let expr = expr_from_value(&value)?;
    Ok(normalize(&expr))
}

/// Decode a JSON array of expressions — the shape `solve` answers with.
pub fn decode_array(text: &str) -> Result<Vec<Expr>, SolverError> {
    let value = parse(text)?;
    let items = value
        .as_array()
        .ok_or_else(|| SolverError::BadOutput("expected a JSON array of expressions".into()))?;
    items
        .iter()
        .map(|item| expr_from_value(item).map(|expr| normalize(&expr)))
        .collect()
}

fn expr_from_value(value: &Json) -> Result<Expr, SolverError> {
    let members = value
        .as_object()
        .ok_or_else(|| SolverError::BadOutput("expression must be a JSON object".into()))?;
    // Exactly one tag: `{"Num":"2","Sym":"x"}` is not an expression, it is a
    // disagreement, and guessing which half to believe is how protocols rot.
    let (tag, body) = match members {
        [(tag, body)] => (tag.as_str(), body),
        _ => {
            return Err(SolverError::BadOutput(
                "an expression object must have exactly one tag key".into(),
            ));
        }
    };

    match tag {
        "Num" => {
            let text = body
                .as_str()
                .ok_or_else(|| SolverError::BadOutput("\"Num\" must hold a string".into()))?;
            Ok(ExprNode::num(parse_number_text(text)?))
        }
        "Sym" => {
            let name = body
                .as_str()
                .ok_or_else(|| SolverError::BadOutput("\"Sym\" must hold a string".into()))?;
            Ok(ExprNode::sym(SymbolId::intern(name)))
        }
        "Add" => Ok(add(expr_list(body, "\"Add\"")?)),
        "Mul" => Ok(mul(expr_list(body, "\"Mul\"")?)),
        "Pow" => {
            let fields = object_body(body, "Pow")?;
            let base = member_of(fields, "base")
                .ok_or_else(|| SolverError::BadOutput("Pow is missing \"base\"".into()))?;
            let exp = member_of(fields, "exp")
                .ok_or_else(|| SolverError::BadOutput("Pow is missing \"exp\"".into()))?;
            Ok(pow(expr_from_value(base)?, expr_from_value(exp)?))
        }
        "Fn" => {
            let fields = object_body(body, "Fn")?;
            let name = member_of(fields, "name")
                .and_then(Json::as_str)
                .ok_or_else(|| SolverError::BadOutput("Fn is missing a string \"name\"".into()))?;
            let args = member_of(fields, "args")
                .ok_or_else(|| SolverError::BadOutput("Fn is missing \"args\"".into()))?;
            Ok(ExprNode::fn_raw(
                SymbolId::intern(name),
                expr_list(args, "\"args\"")?,
            ))
        }
        "Integral" => {
            let fields = object_body(body, "Integral")?;
            let inner = member_of(fields, "body")
                .ok_or_else(|| SolverError::BadOutput("Integral is missing \"body\"".into()))?;
            let var = member_of(fields, "var")
                .and_then(Json::as_str)
                .ok_or_else(|| {
                    SolverError::BadOutput("Integral is missing a string \"var\"".into())
                })?;
            Ok(ExprNode::hold(Hold::Integral {
                body: expr_from_value(inner)?,
                var: SymbolId::intern(var),
            }))
        }
        other => Err(SolverError::BadOutput(format!(
            "unknown expression tag {other:?}"
        ))),
    }
}

fn object_body<'a>(body: &'a Json, tag: &str) -> Result<&'a [(String, Json)], SolverError> {
    body.as_object()
        .ok_or_else(|| SolverError::BadOutput(format!("{tag} must hold a JSON object")))
}

fn expr_list(value: &Json, tag: &str) -> Result<Vec<Expr>, SolverError> {
    let items = value.as_array().ok_or_else(|| {
        SolverError::BadOutput(format!("{tag} must hold an array of expressions"))
    })?;
    items.iter().map(expr_from_value).collect()
}

// ---------------------------------------------------------------------------
// Solution sets <-> JSON
// ---------------------------------------------------------------------------

/// Encode a whole answer, preserving which kind of answer it is.
///
/// The output is compact and deterministic, like [`encode`]: the same
/// [`Solutions`] always produces the same bytes, so a transcript can be diffed
/// and an older client can be told exactly what changed.
pub fn encode_solutions(solutions: &Solutions) -> String {
    match solutions {
        Solutions::None => String::from(r#"{"kind":"none"}"#),
        Solutions::All => String::from(r#"{"kind":"all"}"#),
        Solutions::Values(values) => {
            let mut out = String::from(r#"{"kind":"values","values":["#);
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_expr(value, &mut out);
            }
            out.push_str("]}");
            out
        }
        Solutions::Family {
            base,
            period,
            param,
        } => {
            let mut out = String::from(r#"{"kind":"family","base":"#);
            write_expr(base, &mut out);
            out.push_str(r#","period":"#);
            write_expr(period, &mut out);
            out.push_str(r#","param":"#);
            out.push_str(&quote(&param.as_str()));
            out.push('}');
            out
        }
        Solutions::Inequality { var, intervals } => {
            let mut out = String::from(r#"{"kind":"inequality","var":"#);
            out.push_str(&quote(&var.as_str()));
            out.push_str(r#","intervals":["#);
            for (index, (lower, upper)) in intervals.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(r#"{"from":"#);
                write_bound(lower, &mut out);
                out.push_str(r#","from_open":"#);
                out.push_str(if lower.is_open() { "true" } else { "false" });
                out.push_str(r#","to":"#);
                write_bound(upper, &mut out);
                out.push_str(r#","to_open":"#);
                out.push_str(if upper.is_open() { "true" } else { "false" });
                out.push('}');
            }
            out.push_str("]}");
            out
        }
    }
}

/// One interval end: the expression, or `null` for the infinite end.
fn write_bound(bound: &Bound, out: &mut String) {
    match bound {
        Bound::NegInfinity | Bound::PosInfinity => out.push_str("null"),
        Bound::Finite { value, .. } => write_expr(value, out),
    }
}

/// Decode one tagged solution set.
///
/// Strict where guessing would change the answer: the `kind` tag is required,
/// every member that tag promises must be present and of the right type, and an
/// infinite end may not be marked closed. The result is normalized, like
/// [`decode`], so a worker cannot smuggle in a non-canonical value.
pub fn decode_solutions(text: &str) -> Result<Solutions, SolverError> {
    let value = parse(text)?;
    let members = value.as_object().ok_or_else(|| {
        SolverError::BadOutput("a solution set must be a JSON object with a \"kind\"".into())
    })?;
    let kind = member_of(members, "kind")
        .and_then(Json::as_str)
        .ok_or_else(|| {
            SolverError::BadOutput("a solution set is missing a string \"kind\"".into())
        })?;

    match kind {
        "none" => Ok(Solutions::None),
        "all" => Ok(Solutions::All),
        "values" => {
            let values = member_of(members, "values")
                .ok_or_else(|| SolverError::BadOutput("\"values\" is missing".into()))?;
            let values = expr_list(values, "\"values\"")?
                .iter()
                .map(normalize)
                .collect();
            Ok(Solutions::Values(values))
        }
        "family" => {
            let base = member_of(members, "base")
                .ok_or_else(|| SolverError::BadOutput("family is missing \"base\"".into()))?;
            let period = member_of(members, "period")
                .ok_or_else(|| SolverError::BadOutput("family is missing \"period\"".into()))?;
            let param = member_of(members, "param")
                .and_then(Json::as_str)
                .ok_or_else(|| {
                    SolverError::BadOutput("family is missing a string \"param\"".into())
                })?;
            Ok(Solutions::Family {
                base: normalize(&expr_from_value(base)?),
                period: normalize(&expr_from_value(period)?),
                param: SymbolId::intern(param),
            })
        }
        "inequality" => {
            let var = member_of(members, "var")
                .and_then(Json::as_str)
                .ok_or_else(|| {
                    SolverError::BadOutput("inequality is missing a string \"var\"".into())
                })?;
            let intervals = member_of(members, "intervals")
                .and_then(Json::as_array)
                .ok_or_else(|| {
                    SolverError::BadOutput("inequality is missing an \"intervals\" array".into())
                })?;
            let mut parsed = Vec::with_capacity(intervals.len());
            for interval in intervals {
                let fields = object_body(interval, "an inequality interval")?;
                let from = member_of(fields, "from").ok_or_else(|| {
                    SolverError::BadOutput("an interval is missing \"from\"".into())
                })?;
                let to = member_of(fields, "to").ok_or_else(|| {
                    SolverError::BadOutput("an interval is missing \"to\"".into())
                })?;
                let from_open = member_of(fields, "from_open")
                    .and_then(Json::as_bool)
                    .ok_or_else(|| {
                        SolverError::BadOutput(
                            "an interval is missing a boolean \"from_open\"".into(),
                        )
                    })?;
                let to_open = member_of(fields, "to_open")
                    .and_then(Json::as_bool)
                    .ok_or_else(|| {
                        SolverError::BadOutput(
                            "an interval is missing a boolean \"to_open\"".into(),
                        )
                    })?;
                parsed.push((lower_bound(from, from_open)?, upper_bound(to, to_open)?));
            }
            Ok(Solutions::Inequality {
                var: SymbolId::intern(var),
                intervals: parsed,
            })
        }
        other => Err(SolverError::BadOutput(format!(
            "unknown solution-set kind {other:?}"
        ))),
    }
}

/// Decode the lower end of an interval: `null` means unbounded below.
fn lower_bound(value: &Json, open: bool) -> Result<Bound, SolverError> {
    match value {
        Json::Null if open => Ok(Bound::NegInfinity),
        // `(-∞, 3]` is the only reading of a null from-end; a closed -∞ is a
        // disagreement between the two fields, not something to repair.
        Json::Null => Err(SolverError::BadOutput(
            "an interval cannot be closed at negative infinity".into(),
        )),
        finite => Ok(Bound::Finite {
            value: normalize(&expr_from_value(finite)?),
            open,
        }),
    }
}

/// Decode the upper end of an interval: `null` means unbounded above.
fn upper_bound(value: &Json, open: bool) -> Result<Bound, SolverError> {
    match value {
        Json::Null if open => Ok(Bound::PosInfinity),
        Json::Null => Err(SolverError::BadOutput(
            "an interval cannot be closed at positive infinity".into(),
        )),
        finite => Ok(Bound::Finite {
            value: normalize(&expr_from_value(finite)?),
            open,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canlab_core::{int, ratio, sym};

    fn x() -> Expr {
        sym("x")
    }

    #[test]
    fn encodes_an_integer() {
        assert_eq!(encode(&int(2)), r#"{"Num":"2"}"#);
    }

    #[test]
    fn encodes_a_rational() {
        assert_eq!(encode(&ratio(3, 2)), r#"{"Num":"3/2"}"#);
    }

    #[test]
    fn encodes_a_float_with_a_decimal_point() {
        assert_eq!(
            encode(&ExprNode::num(Number::float(1.5))),
            r#"{"Num":"1.5"}"#
        );
        // 2.0 must not come back as the exact integer 2.
        assert_eq!(
            encode(&ExprNode::num(Number::float(2.0))),
            r#"{"Num":"2.0"}"#
        );
    }

    #[test]
    fn encodes_a_symbol() {
        assert_eq!(encode(&x()), r#"{"Sym":"x"}"#);
    }

    #[test]
    fn encodes_2x_plus_1_exactly() {
        let e = add([mul([int(2), x()]), int(1)]);
        let text = encode(&e);
        assert_eq!(
            text,
            r#"{"Add":[{"Num":"1"},{"Mul":[{"Num":"2"},{"Sym":"x"}]}]}"#
        );
        assert_eq!(decode(&text).expect("round trip"), e);
    }

    #[test]
    fn encodes_x_squared_exactly() {
        let e = pow(x(), int(2));
        let text = encode(&e);
        assert_eq!(text, r#"{"Pow":{"base":{"Sym":"x"},"exp":{"Num":"2"}}}"#);
        assert_eq!(decode(&text).expect("round trip"), e);
    }

    #[test]
    fn encodes_sin_x_exactly() {
        let e = normalize(&ExprNode::fn_raw(SymbolId::intern("sin"), vec![x()]));
        let text = encode(&e);
        assert_eq!(text, r#"{"Fn":{"name":"sin","args":[{"Sym":"x"}]}}"#);
        assert_eq!(decode(&text).expect("round trip"), e);
    }

    #[test]
    fn round_trips_an_unevaluated_integral() {
        let e = ExprNode::hold(Hold::Integral {
            body: mul([int(2), x()]),
            var: SymbolId::intern("x"),
        });
        let text = encode(&e);
        assert_eq!(
            text,
            r#"{"Integral":{"body":{"Mul":[{"Num":"2"},{"Sym":"x"}]},"var":"x"}}"#
        );
        assert_eq!(decode(&text).expect("integral decodes"), e);
    }

    #[test]
    fn round_trips_scalars() {
        for e in [
            int(-7),
            ratio(1, 3),
            ExprNode::num(Number::float(-0.25)),
            x(),
        ] {
            let text = encode(&e);
            assert_eq!(decode(&text).expect("scalar decodes"), e, "text={text}");
        }
    }

    #[test]
    fn non_finite_floats_survive_the_wire() {
        let nan = decode(r#"{"Num":"NaN"}"#).expect("NaN decodes");
        assert!(nan.as_number().expect("number").to_f64().is_nan());

        let inf = decode(r#"{"Num":"-inf"}"#).expect("-inf decodes");
        assert_eq!(inf.as_number().expect("number").to_f64(), f64::NEG_INFINITY);

        assert_eq!(encode(&inf), r#"{"Num":"-inf"}"#);
    }

    #[test]
    fn an_integer_too_large_for_the_exact_type_is_rejected_not_rounded() {
        let huge = format!(r#"{{"Num":"{}"}}"#, "9".repeat(60));
        assert!(matches!(decode(&huge), Err(SolverError::BadOutput(_))));
    }

    #[test]
    fn round_trips_a_nested_fraction() {
        // 1/(x + 1)
        let e = pow(add([x(), int(1)]), int(-1));
        let text = encode(&e);
        assert_eq!(
            text,
            r#"{"Pow":{"base":{"Add":[{"Num":"1"},{"Sym":"x"}]},"exp":{"Num":"-1"}}}"#
        );
        assert_eq!(decode(&text).expect("fraction decodes"), e);
    }

    #[test]
    fn round_trips_a_larger_expression() {
        let e = add([
            mul([int(3), pow(x(), int(2))]),
            mul([ratio(-1, 2), x()]),
            ExprNode::num(Number::float(0.5)),
        ]);
        let text = encode(&e);
        assert_eq!(decode(&text).expect("expression decodes"), e);
    }

    #[test]
    fn decoding_normalizes_to_the_core_canonical_form() {
        // The worker is allowed to send terms in any order; the core decides.
        let text = r#"{"Add":[{"Sym":"x"},{"Num":"2"},{"Sym":"x"}]}"#;
        assert_eq!(decode(text).expect("decodes").to_string(), "2 + 2*x");
    }

    #[test]
    fn decoding_tolerates_key_order_and_whitespace() {
        let text = "  { \"Pow\" : { \"exp\" : { \"Num\" : \"2\" } ,\n\t\"base\" : { \"Sym\" : \"x\" } } }  ";
        assert_eq!(decode(text).expect("decodes"), pow(x(), int(2)));
    }

    #[test]
    fn decoding_handles_escapes_and_unicode() {
        let text = r#"{"Sym":"a\"b\\c\u00b5"}"#;
        let e = decode(text).expect("decodes");
        assert_eq!(e.as_symbol().expect("symbol").as_str(), "a\"b\\c\u{b5}");
        // And the encoder escapes it again.
        assert_eq!(encode(&e), r#"{"Sym":"a\"b\\cµ"}"#);
    }

    #[test]
    fn decoding_a_surrogate_pair_yields_one_character() {
        let e = decode(r#"{"Sym":"\ud83d\ude00"}"#).expect("decodes");
        assert_eq!(e.as_symbol().expect("symbol").as_str(), "\u{1f600}");
    }

    #[test]
    fn malformed_json_is_bad_output_and_never_a_panic() {
        let cases = [
            "",
            "   ",
            r#"{"Num":"2""#,                   // truncated
            r#"{"Num":}"#,                     // missing value
            r#"{"Num":2}"#,                    // wrong type: number, not string
            r#"{"Add":7}"#,                    // wrong type: array expected
            r#"{"Sym":[]}"#,                   // wrong type
            r#"{"Num":"abc"}"#,                // not a number
            r#"{"Num":"1/0"}"#,                // zero denominator
            r#"{"Unknown":1}"#,                // unknown tag
            r#"{"Num":"1","Sym":"x"}"#,        // two tags
            r#"{"Pow":{"base":{"Sym":"x"}}}"#, // missing exp
            r#"{"Fn":{"args":[]}}"#,           // missing name
            r#"{"Integral":{"var":"x"}}"#,     // missing body
            r#"[]"#,                           // not an object
            r#"{"Num":"2"} trailing"#,         // trailing garbage
            "{\"Num\":\"2\u{0}\"}",            // raw control character inside a string
        ];
        for case in cases {
            let error = decode(case).expect_err(&format!("{case:?} must be rejected"));
            assert!(
                matches!(error, SolverError::BadOutput(_)),
                "{case:?} produced {error:?}"
            );
        }
    }

    #[test]
    fn deeply_nested_input_is_rejected_instead_of_overflowing_the_stack() {
        let text = "[".repeat(10_000);
        assert!(matches!(decode(&text), Err(SolverError::BadOutput(_))));
    }

    #[test]
    fn decode_array_reads_a_solution_set() {
        let solutions = decode_array(r#"[{"Num":"1"},{"Num":"-1"}]"#).expect("decodes");
        assert_eq!(solutions, vec![int(1), int(-1)]);
    }

    #[test]
    fn decode_array_rejects_a_scalar() {
        assert!(matches!(
            decode_array(r#"{"Num":"1"}"#),
            Err(SolverError::BadOutput(_))
        ));
    }

    #[test]
    fn encoding_is_stable() {
        let exprs = [
            int(4),
            ratio(-2, 5),
            ExprNode::num(Number::float(1.25)),
            add([mul([int(2), x()]), int(1)]),
            pow(x(), ratio(1, 2)),
            normalize(&ExprNode::fn_raw(
                SymbolId::intern("cos"),
                vec![mul([int(2), x()])],
            )),
            ExprNode::hold(Hold::Integral {
                body: normalize(&ExprNode::fn_raw(SymbolId::intern("sin"), vec![x()])),
                var: SymbolId::intern("x"),
            }),
        ];
        for e in exprs {
            let once = encode(&e);
            let twice = encode(&decode(&once).expect("decodes"));
            assert_eq!(once, twice);
        }
    }

    #[test]
    fn json_value_parser_covers_every_value_kind() {
        let value =
            parse(r#"{"a":null,"b":true,"c":false,"d":[1,-2.5e3,"s"],"e":{}}"#).expect("parses");
        let object = value.as_object().expect("object");
        assert_eq!(object.len(), 5);
        assert_eq!(value.member("a"), Some(&Json::Null));
        assert_eq!(value.member("b"), Some(&Json::Bool(true)));
        assert_eq!(value.member("missing"), None);
        assert_eq!(value.member("e"), Some(&Json::Object(Vec::new())));
        assert_eq!(
            value
                .member("d")
                .and_then(Json::as_array)
                .map(|items| items.len()),
            Some(3)
        );
        // Re-encoding a parsed value is compact and preserves number text.
        assert_eq!(
            compact(&value),
            r#"{"a":null,"b":true,"c":false,"d":[1,-2.5e3,"s"],"e":{}}"#
        );
    }

    /// The five answer kinds, built once so the encoding and the round trip
    /// cannot drift apart.
    fn every_kind() -> Vec<Solutions> {
        vec![
            Solutions::None,
            Solutions::All,
            Solutions::Values(vec![int(2), int(-2)]),
            Solutions::Family {
                base: mul([ratio(1, 6), sym("pi")]),
                period: mul([int(2), sym("pi")]),
                param: SymbolId::intern("k"),
            },
            Solutions::Inequality {
                var: SymbolId::intern("x"),
                intervals: vec![
                    (
                        Bound::Finite {
                            value: int(-1),
                            open: true,
                        },
                        Bound::Finite {
                            value: int(3),
                            open: false,
                        },
                    ),
                    (
                        Bound::Finite {
                            value: int(4),
                            open: false,
                        },
                        Bound::PosInfinity,
                    ),
                ],
            },
        ]
    }

    #[test]
    fn solution_sets_round_trip_every_kind() {
        for solutions in every_kind() {
            let text = encode_solutions(&solutions);
            let decoded = decode_solutions(&text).expect(&format!("{text} decodes"));
            assert_eq!(decoded, solutions, "text={text}");
            // A decoded answer encodes to the same bytes: the wire form is
            // stable, not merely readable.
            assert_eq!(encode_solutions(&decoded), text);
        }
    }

    #[test]
    fn solution_sets_encode_with_an_explicit_tag() {
        assert_eq!(encode_solutions(&Solutions::None), r#"{"kind":"none"}"#);
        assert_eq!(encode_solutions(&Solutions::All), r#"{"kind":"all"}"#);
        assert_eq!(
            encode_solutions(&Solutions::Values(vec![int(2), int(-2)])),
            r#"{"kind":"values","values":[{"Num":"2"},{"Num":"-2"}]}"#
        );
        assert_eq!(
            encode_solutions(&Solutions::Family {
                base: mul([ratio(1, 6), sym("pi")]),
                period: mul([int(2), sym("pi")]),
                param: SymbolId::intern("k"),
            }),
            r#"{"kind":"family","base":{"Mul":[{"Num":"1/6"},{"Sym":"pi"}]},"period":{"Mul":[{"Num":"2"},{"Sym":"pi"}]},"param":"k"}"#
        );
        assert_eq!(
            encode_solutions(&Solutions::Inequality {
                var: SymbolId::intern("x"),
                intervals: vec![
                    (
                        Bound::Finite {
                            value: int(-1),
                            open: true,
                        },
                        Bound::Finite {
                            value: int(3),
                            open: false,
                        },
                    ),
                    (
                        Bound::Finite {
                            value: int(4),
                            open: false,
                        },
                        Bound::PosInfinity
                    ),
                ],
            }),
            r#"{"kind":"inequality","var":"x","intervals":[{"from":{"Num":"-1"},"from_open":true,"to":{"Num":"3"},"to_open":false},{"from":{"Num":"4"},"from_open":false,"to":null,"to_open":true}]}"#
        );
    }

    #[test]
    fn a_truncated_or_mis_tagged_solution_set_is_bad_output() {
        // Every case pairs the wire text with a fragment of the reason the
        // decoder gives: a refusal that cannot say why is as bad as a panic,
        // and a decoder that guesses is worse than both.
        let cases: &[(&str, &str)] = &[
            ("", "unexpected end of input"),
            ("   ", "unexpected end of input"),
            // Truncated.
            (
                r#"{"kind":"values","values":[{"Num":"1"}"#,
                "expected ',' or ']'",
            ),
            (r#"{"kind":"values"}"#, "\"values\" is missing"),
            (r#"{"kind":"values","values":{}}"#, "array of expressions"),
            (r#"{"kind":"values","values":[7]}"#, "must be a JSON object"),
            // A family with a piece missing.
            (
                r#"{"kind":"family","base":{"Num":"1"}}"#,
                "missing \"period\"",
            ),
            (
                r#"{"kind":"family","base":{"Num":"1"},"period":{"Num":"2"},"param":7}"#,
                "string \"param\"",
            ),
            // An inequality whose pieces disagree.
            (r#"{"kind":"inequality","var":"x"}"#, "\"intervals\" array"),
            (
                r#"{"kind":"inequality","var":"x","intervals":[{"from":{"Num":"1"},"from_open":true,"to":{"Num":"2"}}]}"#,
                "boolean \"to_open\"",
            ),
            (
                r#"{"kind":"inequality","var":"x","intervals":[{"from":null,"from_open":false,"to":{"Num":"2"},"to_open":false}]}"#,
                "negative infinity",
            ),
            (
                r#"{"kind":"inequality","var":"x","intervals":[{"from":{"Num":"1"},"from_open":"yes","to":{"Num":"2"},"to_open":false}]}"#,
                "boolean \"from_open\"",
            ),
            (
                r#"{"kind":"inequality","var":"x","intervals":[7]}"#,
                "must hold a JSON object",
            ),
            // Mis-tagged, or not tagged at all.
            (r#"{"kind":"surprise"}"#, "unknown solution-set kind"),
            (r#"{"values":[]}"#, "missing a string \"kind\""),
            (r#"{"kind":7}"#, "missing a string \"kind\""),
            // The old `solve` array is not a tagged answer.
            (r#"[{"Num":"1"}]"#, "must be a JSON object"),
            (r#"{"kind":"none"} trailing"#, "trailing characters"),
        ];
        for (case, reason) in cases {
            let error = decode_solutions(case).expect_err(&format!("{case:?} must be rejected"));
            assert!(
                matches!(error, SolverError::BadOutput(_)),
                "{case:?} produced {error:?}"
            );
            let message = error.to_string();
            assert!(
                message.contains(reason),
                "{case:?} refused with {message:?}, expected {reason:?}"
            );
        }
    }
}
