//! A small JSON reader and writer, because the batch protocol is line JSON.
//!
//! The alternative was a serde dependency for two fixed message shapes. The
//! protocol is ours on both ends, so the smallest honest implementation is a
//! parser that reads exactly JSON and a writer that escapes exactly what JSON
//! requires. It is not a general-purpose library: no duplicate-key policy, no
//! arbitrary-precision numbers, and a number is an `f64`.

use std::fmt::Write as _;

/// One JSON value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A number. The corpus protocol only ever writes integers, but a dataset
    /// answer column is JSON and its numbers arrive here as `f64`.
    Number(f64),
    /// A string.
    String(String),
    /// An array.
    Array(Vec<Value>),
    /// An object, in the order the document wrote its members.
    Object(Vec<(String, Value)>),
}

impl Value {
    /// The member named `key` of an object, if it has one.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(members) => members
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// The string, if this is a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(text) => Some(text),
            _ => None,
        }
    }

    /// The number, if this is a number.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(value) => Some(*value),
            _ => None,
        }
    }

    /// The array, if this is an array.
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    /// The object, if this is an object.
    pub fn as_object(&self) -> Option<&[(String, Value)]> {
        match self {
            Value::Object(members) => Some(members),
            _ => None,
        }
    }

    /// The boolean, if this is a boolean.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(value) => Some(*value),
            _ => None,
        }
    }
}

/// Parse one JSON document.
pub fn parse(source: &str) -> Result<Value, String> {
    let mut parser = Parser {
        bytes: source.as_bytes(),
        at: 0,
    };
    parser.space();
    let value = parser.value()?;
    parser.space();
    if parser.at != parser.bytes.len() {
        return Err(format!("trailing characters at byte {}", parser.at));
    }
    Ok(value)
}

/// Write one JSON document on one line.
pub fn encode(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

/// A JSON string literal, quotes included.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => {
            if number.is_finite() {
                // `{}` is the shortest round-tripping form in Rust, which is what
                // a manifest reader should get back.
                let _ = write!(out, "{number}");
            } else {
                // JSON has no NaN or infinity. Writing `null` is honest: the
                // reader sees "no value" rather than a number that is not one.
                out.push_str("null");
            }
        }
        Value::String(text) => out.push_str(&escape(text)),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Value::Object(members) => {
            out.push('{');
            for (index, (key, member)) in members.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&escape(key));
                out.push(':');
                write_value(out, member);
            }
            out.push('}');
        }
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.at += 1;
        Some(byte)
    }

    fn space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), String> {
        if self.bump() == Some(byte) {
            Ok(())
        } else {
            Err(format!("expected {:?} at byte {}", byte as char, self.at))
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        self.space();
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(Value::String),
            Some(b't') => self.keyword("true", Value::Bool(true)),
            Some(b'f') => self.keyword("false", Value::Bool(false)),
            Some(b'n') => self.keyword("null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(other) => Err(format!(
                "unexpected {:?} at byte {}",
                other as char, self.at
            )),
            None => Err(String::from("unexpected end of input")),
        }
    }

    fn keyword(&mut self, word: &str, value: Value) -> Result<Value, String> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(format!("expected `{word}` at byte {}", self.at))
        }
    }

    fn object(&mut self) -> Result<Value, String> {
        self.expect(b'{')?;
        let mut members = Vec::new();
        self.space();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Value::Object(members));
        }
        loop {
            self.space();
            let key = self.string()?;
            self.space();
            self.expect(b':')?;
            let value = self.value()?;
            members.push((key, value));
            self.space();
            match self.bump() {
                Some(b',') => continue,
                Some(b'}') => return Ok(Value::Object(members)),
                _ => return Err(format!("expected `,` or `}}` at byte {}", self.at)),
            }
        }
    }

    fn array(&mut self) -> Result<Value, String> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.space();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Value::Array(items));
        }
        loop {
            items.push(self.value()?);
            self.space();
            match self.bump() {
                Some(b',') => continue,
                Some(b']') => return Ok(Value::Array(items)),
                _ => return Err(format!("expected `,` or `]` at byte {}", self.at)),
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            match self.bump() {
                None => return Err(String::from("unterminated string")),
                Some(b'"') => return Ok(out),
                Some(b'\\') => match self.bump() {
                    Some(b'"') => out.push('"'),
                    Some(b'\\') => out.push('\\'),
                    Some(b'/') => out.push('/'),
                    Some(b'b') => out.push('\u{08}'),
                    Some(b'f') => out.push('\u{0c}'),
                    Some(b'n') => out.push('\n'),
                    Some(b'r') => out.push('\r'),
                    Some(b't') => out.push('\t'),
                    Some(b'u') => out.push(self.unicode_escape()?),
                    Some(other) => {
                        return Err(format!("bad escape \\{} at byte {}", other as char, self.at))
                    }
                    None => return Err(String::from("unterminated escape")),
                },
                Some(byte) if byte < 0x20 => {
                    return Err(format!("control character in string at byte {byte}"))
                }
                Some(byte) => {
                    // The input is `&str`, so a multi-byte character is valid
                    // UTF-8; collect the bytes and let `String` validate.
                    let start = self.at - 1;
                    let width = utf8_width(byte);
                    if start + width > self.bytes.len() {
                        return Err(String::from("truncated UTF-8 sequence"));
                    }
                    let text = std::str::from_utf8(&self.bytes[start..start + width])
                        .map_err(|_| String::from("invalid UTF-8 in string"))?;
                    out.push_str(text);
                    self.at = start + width;
                }
            }
        }
    }

    fn unicode_escape(&mut self) -> Result<char, String> {
        let first = self.hex4()?;
        // A surrogate pair is two \u escapes in JSON. A lone surrogate is not a
        // character and is refused rather than replaced with U+FFFD.
        if (0xd800..=0xdbff).contains(&first) {
            if self.bump() != Some(b'\\') || self.bump() != Some(b'u') {
                return Err(String::from("lone high surrogate"));
            }
            let second = self.hex4()?;
            if !(0xdc00..=0xdfff).contains(&second) {
                return Err(String::from("bad low surrogate"));
            }
            let code = 0x10000 + ((first - 0xd800) << 10) + (second - 0xdc00);
            char::from_u32(code).ok_or_else(|| String::from("invalid code point"))
        } else {
            char::from_u32(first).ok_or_else(|| String::from("invalid code point"))
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let mut value = 0u32;
        for _ in 0..4 {
            let byte = self.bump().ok_or("truncated \\u escape")?;
            let digit = (byte as char)
                .to_digit(16)
                .ok_or_else(|| format!("bad hex digit {} in escape", byte as char))?;
            value = value * 16 + digit;
        }
        Ok(value)
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.at += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.at += 1;
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.at])
            .map_err(|_| String::from("invalid number"))?;
        text.parse::<f64>()
            .map(Value::Number)
            .map_err(|_| format!("bad number `{text}`"))
    }
}

/// How many bytes the UTF-8 sequence starting with `first` occupies.
fn utf8_width(first: u8) -> usize {
    match first {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_protocol_shape() {
        let value = parse(r#"{"path":"a.toml","status":"pass","n":2,"ok":true}"#).unwrap();
        assert_eq!(value.get("path").and_then(Value::as_str), Some("a.toml"));
        assert_eq!(value.get("status").and_then(Value::as_str), Some("pass"));
        assert_eq!(value.get("n").and_then(Value::as_f64), Some(2.0));
        assert_eq!(value.get("ok").and_then(Value::as_bool), Some(true));
    }

    #[test]
    fn parses_arrays_and_escapes() {
        let value = parse(r#"["\u00e4", "\n", ["x"], {"k": null}]"#).unwrap();
        let items = value.as_array().unwrap();
        assert_eq!(items[0].as_str(), Some("ä"));
        assert_eq!(items[1].as_str(), Some("\n"));
        assert_eq!(items[2].as_array().unwrap().len(), 1);
        assert_eq!(items[3].get("k"), Some(&Value::Null));
    }

    #[test]
    fn a_surrogate_pair_is_one_character() {
        let value = parse(r#""\ud83d\ude00""#).unwrap();
        assert_eq!(value.as_str(), Some("😀"));
    }

    #[test]
    fn refuses_broken_documents() {
        for source in [
            "",
            "{",
            r#"{"a":}"#,
            r#"{"a":1,}"#,
            r#""unterminated"#,
            "nul",
            r#"{"a":1} x"#,
        ] {
            assert!(parse(source).is_err(), "accepted {source:?}");
        }
    }

    #[test]
    fn round_trips_every_value_kind() {
        let value = Value::Object(vec![
            ("s".into(), Value::String("a\"b\\c\n".into())),
            ("n".into(), Value::Number(-2.5)),
            ("a".into(), Value::Array(vec![Value::Bool(false), Value::Null])),
        ]);
        assert_eq!(parse(&encode(&value)).unwrap(), value);
    }

    #[test]
    fn a_non_finite_number_is_written_as_null() {
        // JSON cannot spell NaN; the reader must not be handed a number it
        // would interpret as a real one.
        assert_eq!(encode(&Value::Number(f64::NAN)), "null");
    }
}
