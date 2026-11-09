//! Tokenizer for the LaTeX subset accepted by [`crate::parse_latex`].
//!
//! The lexer is deliberately dumb: it never decides what a token *means*, only
//! where it starts and what it is. That keeps every "this command does not
//! exist" or "this token cannot appear here" decision in the parser, where a
//! byte position can be attached to a useful message.
//!
//! Two choices matter downstream:
//!
//! * A backslash followed by letters becomes [`TokenKind::Command`] with the
//!   backslash stripped (`\frac` -> `Command("frac")`). An *unknown* command is
//!   not a lexer error; the parser rejects it with its position. A backslash
//!   followed by a non-letter (`\{`, `\,`) keeps that single character as the
//!   command name so the parser can say "unknown command `\{`" instead of
//!   failing here.
//! * Digits become one [`TokenKind::Number`] token, including the decimal
//!   point. The parser turns the text into an exact rational, so `3.5` is
//!   `7/2` and never a float.

use crate::ParseError;

/// One token and the byte offset where it starts.
///
/// Positions are byte offsets, not character indices, so a diagnostic can be
/// mapped back onto the source with `src[pos..]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub pos: usize,
}

/// The token alphabet. It is closed: anything not listed here is a lex error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    /// A run of digits with an optional decimal point: `42`, `3.5`, `.5`.
    Number(String),
    /// A run of ASCII letters: `x`, `xy`, `sin`.
    Ident(String),
    /// A backslash command with the backslash removed: `frac`, `cdot`.
    Command(String),
    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Caret,
    Underscore,
    Plus,
    Minus,
    Star,
    Slash,
    Bang,
    Comma,
    /// The synthetic token at `src.len()`; always the last token.
    Eof,
}

impl TokenKind {
    /// Human-readable form used inside error messages, e.g.
    /// ``unexpected command `\foo` ``.
    pub fn describe(&self) -> String {
        match self {
            TokenKind::Number(text) => format!("number `{text}`"),
            TokenKind::Ident(name) => format!("identifier `{name}`"),
            TokenKind::Command(name) => format!("command `\\{name}`"),
            TokenKind::Eof => "end of input".to_string(),
            TokenKind::LBrace => "`{`".to_string(),
            TokenKind::RBrace => "`}`".to_string(),
            TokenKind::LParen => "`(`".to_string(),
            TokenKind::RParen => "`)`".to_string(),
            TokenKind::LBracket => "`[`".to_string(),
            TokenKind::RBracket => "`]`".to_string(),
            TokenKind::Caret => "`^`".to_string(),
            TokenKind::Underscore => "`_`".to_string(),
            TokenKind::Plus => "`+`".to_string(),
            TokenKind::Minus => "`-`".to_string(),
            TokenKind::Star => "`*`".to_string(),
            TokenKind::Slash => "`/`".to_string(),
            TokenKind::Bang => "`!`".to_string(),
            TokenKind::Comma => "`,`".to_string(),
        }
    }
}

/// Tokenize `src`.
///
/// The returned vector is never empty: it always ends with exactly one
/// [`TokenKind::Eof`] token positioned at `src.len()`.
pub(crate) fn tokenize(src: &str) -> Result<Vec<Token>, ParseError> {
    let bytes = src.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        let start = i;
        match bytes[i] {
            b if b.is_ascii_whitespace() => i += 1,
            b'{' => {
                i += 1;
                tokens.push(Token { kind: TokenKind::LBrace, pos: start });
            }
            b'}' => {
                i += 1;
                tokens.push(Token { kind: TokenKind::RBrace, pos: start });
            }
            b'(' => {
                i += 1;
                tokens.push(Token { kind: TokenKind::LParen, pos: start });
            }
            b')' => {
                i += 1;
                tokens.push(Token { kind: TokenKind::RParen, pos: start });
            }
            b'[' => {
                i += 1;
                tokens.push(Token { kind: TokenKind::LBracket, pos: start });
            }
            b']' => {
                i += 1;
                tokens.push(Token { kind: TokenKind::RBracket, pos: start });
            }
            b'^' => {
                i += 1;
                tokens.push(Token { kind: TokenKind::Caret, pos: start });
            }
            b'_' => {
                i += 1;
                tokens.push(Token { kind: TokenKind::Underscore, pos: start });
            }
            b'+' => {
                i += 1;
                tokens.push(Token { kind: TokenKind::Plus, pos: start });
            }
            b'-' => {
                i += 1;
                tokens.push(Token { kind: TokenKind::Minus, pos: start });
            }
            b'*' => {
                i += 1;
                tokens.push(Token { kind: TokenKind::Star, pos: start });
            }
            b'/' => {
                i += 1;
                tokens.push(Token { kind: TokenKind::Slash, pos: start });
            }
            b'!' => {
                i += 1;
                tokens.push(Token { kind: TokenKind::Bang, pos: start });
            }
            b',' => {
                i += 1;
                tokens.push(Token { kind: TokenKind::Comma, pos: start });
            }
            b'\\' => {
                let (name, end) = lex_command(src, start)?;
                tokens.push(Token { kind: TokenKind::Command(name), pos: start });
                i = end;
            }
            b'0'..=b'9' => {
                let (text, end) = lex_number(src, start);
                tokens.push(Token { kind: TokenKind::Number(text), pos: start });
                i = end;
            }
            b'.' if starts_decimal(bytes, start) => {
                let (text, end) = lex_number(src, start);
                tokens.push(Token { kind: TokenKind::Number(text), pos: start });
                i = end;
            }
            b if b.is_ascii_alphabetic() => {
                let mut end = start;
                while end < bytes.len() && bytes[end].is_ascii_alphabetic() {
                    end += 1;
                }
                tokens.push(Token {
                    kind: TokenKind::Ident(src[start..end].to_string()),
                    pos: start,
                });
                i = end;
            }
            _ => {
                let Some(ch) = src[start..].chars().next() else {
                    // Unreachable: `start` is a valid boundary below `len`.
                    return Err(ParseError::new("invalid character boundary", start));
                };
                return Err(ParseError::new(format!("unexpected character `{ch}`"), start));
            }
        }
    }

    tokens.push(Token { kind: TokenKind::Eof, pos: src.len() });
    Ok(tokens)
}

/// A `.` only starts a number when a digit follows it; `x . y` is not a number.
fn starts_decimal(bytes: &[u8], start: usize) -> bool {
    matches!(bytes.get(start + 1), Some(next) if next.is_ascii_digit())
}

/// Scan a number literal starting at `start`.
///
/// The caller guarantees that `bytes[start]` is a digit or a `.` followed by a
/// digit, so this always consumes at least one byte and cannot fail.
fn lex_number(src: &str, start: usize) -> (String, usize) {
    let bytes = src.as_bytes();
    let mut i = start;
    let mut seen_dot = false;
    if bytes[i] == b'.' {
        seen_dot = true;
        i += 1;
    }
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            i += 1;
        } else if bytes[i] == b'.' && !seen_dot && starts_decimal(bytes, i) {
            seen_dot = true;
            i += 1;
        } else {
            break;
        }
    }
    (src[start..i].to_string(), i)
}

/// Scan a backslash command starting at the backslash in `start`.
fn lex_command(src: &str, start: usize) -> Result<(String, usize), ParseError> {
    let bytes = src.as_bytes();
    let mut i = start + 1;
    let name_start = i;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        i += 1;
    }
    if i > name_start {
        return Ok((src[name_start..i].to_string(), i));
    }

    // A backslash followed by a non-letter. Keep that one character as the
    // command name so the parser reports "unknown command `\{`" at this
    // position instead of the lexer guessing.
    let Some(ch) = src[i..].chars().next() else {
        return Err(ParseError::new("trailing backslash", start));
    };
    Ok((ch.to_string(), i + ch.len_utf8()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokenKind> {
        tokenize(src)
            .expect("test input should tokenize")
            .into_iter()
            .map(|token| token.kind)
            .collect()
    }

    #[test]
    fn tokenizes_a_small_expression() {
        assert_eq!(
            kinds("x^2 + 1"),
            vec![
                TokenKind::Ident("x".into()),
                TokenKind::Caret,
                TokenKind::Number("2".into()),
                TokenKind::Plus,
                TokenKind::Number("1".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn tokenizes_commands_without_the_backslash() {
        assert_eq!(
            kinds(r"\frac{1}{2}"),
            vec![
                TokenKind::Command("frac".into()),
                TokenKind::LBrace,
                TokenKind::Number("1".into()),
                TokenKind::RBrace,
                TokenKind::LBrace,
                TokenKind::Number("2".into()),
                TokenKind::RBrace,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn unknown_commands_are_still_tokens() {
        // The lexer must not decide that `\foo` is invalid; the parser owns
        // that error so the message can carry the position.
        assert_eq!(
            kinds(r"\foo"),
            vec![TokenKind::Command("foo".into()), TokenKind::Eof]
        );
        assert_eq!(
            kinds(r"\{"),
            vec![TokenKind::Command("{".into()), TokenKind::Eof]
        );
    }

    #[test]
    fn positions_are_byte_offsets() {
        let tokens = tokenize("  12 + x").expect("tokenizes");
        assert_eq!(tokens[0].pos, 2); // `12`
        assert_eq!(tokens[1].pos, 5); // `+`
        assert_eq!(tokens[2].pos, 7); // `x`
        assert_eq!(tokens[3].pos, 8); // eof
        assert_eq!(tokens[3].kind, TokenKind::Eof);
    }

    #[test]
    fn whitespace_is_skipped() {
        assert_eq!(
            kinds(" \t x \n + \r\n 1 "),
            vec![
                TokenKind::Ident("x".into()),
                TokenKind::Plus,
                TokenKind::Number("1".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn decimals_lex_as_one_token() {
        assert_eq!(
            kinds("3.5 + .25"),
            vec![
                TokenKind::Number("3.5".into()),
                TokenKind::Plus,
                TokenKind::Number(".25".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn letters_lex_as_one_ident() {
        assert_eq!(
            kinds("2xy"),
            vec![
                TokenKind::Number("2".into()),
                TokenKind::Ident("xy".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn unknown_character_reports_its_byte_position() {
        let err = tokenize("x + π").expect_err("π is outside the grammar");
        assert_eq!(err.position, 4);
        assert!(err.message.contains('π'), "message was {}", err.message);
    }

    #[test]
    fn trailing_backslash_is_an_error() {
        let err = tokenize(r"x + \").expect_err("a lone backslash is not a command");
        assert_eq!(err.position, 4);
    }

    #[test]
    fn empty_input_is_just_eof() {
        assert_eq!(kinds(""), vec![TokenKind::Eof]);
        assert_eq!(kinds("   "), vec![TokenKind::Eof]);
    }
}
