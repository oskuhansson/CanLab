//! LaTeX math → [`cas_core::Expr`].
//!
//! `cas-parse` turns the LaTeX a student types into the canonical expression
//! tree the rest of the CAS works with. The parser itself does not evaluate
//! anything: it builds the tree through `cas-core`'s normalizing constructors,
//! so the result is already canonical (and, for example, a literal
//! `\frac{1}{0}` is rejected rather than turned into a `NaN`).
//!
//! # Grammar
//!
//! The parser is precedence climbing. Binding powers, loosest first:
//!
//! | construct | binding power |
//! |---|---|
//! | `+`, `-` | 10 |
//! | `*`, `\cdot`, `\times`, `\div`, `/` | 20 |
//! | implicit multiplication (`2x`) | 30 |
//! | prefix `-`, prefix `+` | 35 |
//! | `^`, `_` | 50 (right-associative) |
//!
//! # Conventions
//!
//! These are the decisions a reader of the source should not have to
//! rediscover. Each one is covered by a test in this module.
//!
//! * **Implicit multiplication binds tighter than `/`.** `1/2x` is
//!   `1/(2x)`, not `(1/2)x`. `\frac{1}{2}x` *is* `(1/2)x`, because `\frac` is
//!   a single atom.
//! * **A function consumes the product that follows it.** `\sin 2x` is
//!   `sin(2*x)`, while `\sin x + 1` is `sin(x) + 1`: the argument stops at the
//!   `+`. A braced or parenthesized argument is taken as a whole, so
//!   `\sin(x)^2` is `(sin x)^2` and `\sin x^2` is `sin(x^2)`.
//! * **`\sin^2 x` is `(sin x)^2`.** The superscript belongs to the function
//!   application, exactly as LaTeX renders it.
//! * **Decimals are exact.** `3.5` is the rational `7/2`, never a float.
//! * **Subscripts are parsed and discarded.** `x_1` is `x` and `\log_2 x` is
//!   `log(x)`. This is a documented limitation: [`cas_core::Expr`] has no node
//!   for an indexed symbol yet, and rejecting the syntax would make
//!   `\int_0^1`-style input fail before the integrator ever sees it.
//! * **`!` is rejected.** There is no factorial node, and silently dropping
//!   the operator would be worse than saying so.
//! * **`\frac` arguments are one atom or one braced group.** `\frac{1}{2}` and
//!   `\frac1x` both work; `\frac12` does not, because the lexer reads `12` as
//!   a single number. LaTeX would split it into two digits, but guessing a
//!   split inside a number would be surprising, so braces are required for
//!   multi-digit arguments.
//! * **Bare multi-letter identifiers are split into single letters.** `xy` is
//!   `x*y`, which is how LaTeX reads a run of variables; the known function
//!   names (`sin`, `ln`, …) are the exception and stay functions.
//!
//! # Errors
//!
//! Every failure is a [`ParseError`] carrying a message and a byte offset into
//! the source. The parser never panics, whatever the input.
//!
//! # Examples
//!
//! ```
//! let expr = cas_parse::parse_latex(r"\frac{1}{2}x + 1").unwrap();
//! assert_eq!(expr.to_string(), "1 + (1/2)*x");
//!
//! let err = cas_parse::parse_latex(r"\frac{1}").unwrap_err();
//! assert_eq!(err.position, 8);
//! ```

use std::fmt;

mod lexer;
mod parser;

/// The Abitti formula palette: which glyph inserts which LaTeX.
///
/// Data, not logic: it exists so the GUI can render the keypad students know,
/// and so tests can assert that the parser accepts everything the palette can
/// produce. Transcribed from `digabi/rich-text-editor` (MIT); see `NOTICE`.
pub mod palette;

/// A parse failure: what went wrong and where.
///
/// `position` is a byte offset into the source, so callers can point at the
/// offending token (or slice the source for a caret diagnostic). It is always
/// `<= src.len()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// Human-readable description, e.g. ``unknown command `\foo` ``.
    pub message: String,
    /// Byte offset of the token that caused the failure.
    pub position: usize,
}

impl ParseError {
    /// Build an error at `position`. Kept public so other crates in the
    /// workspace can report their own LaTeX-level problems in the same shape.
    pub fn new(message: impl Into<String>, position: usize) -> Self {
        ParseError { message: message.into(), position }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (at byte {})", self.message, self.position)
    }
}

impl std::error::Error for ParseError {}

/// Parse a LaTeX math expression into a canonical [`cas_core::Expr`].
///
/// The result is already normalized: sums and products are flattened and
/// sorted, numeric constants are folded, and `\sqrt` is a `Pow` with exponent
/// `1/2`. See the [module documentation](self) for the parsing conventions.
///
/// # Examples
///
/// ```
/// let expr = cas_parse::parse_latex("2x + 1").unwrap();
/// assert_eq!(expr.to_string(), "1 + 2*x");
/// ```
///
/// Errors carry the byte offset of the problem:
///
/// ```
/// let err = cas_parse::parse_latex(r"\foo{x}").unwrap_err();
/// assert_eq!(err.position, 0);
/// assert!(err.message.contains("unknown command"));
/// ```
pub fn parse_latex(src: &str) -> Result<cas_core::Expr, ParseError> {
    parser::parse(src)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cas_core::{add, int, mul, pow, sym, Expr};

    fn parse(src: &str) -> Expr {
        parse_latex(src).unwrap_or_else(|err| panic!("{src:?} should parse: {err}"))
    }

    fn parse_err(src: &str) -> ParseError {
        match parse_latex(src) {
            Ok(expr) => panic!("{src:?} should not parse, got {}", expr.to_string()),
            Err(err) => err,
        }
    }

    // -- core constructs ----------------------------------------------------

    #[test]
    fn parses_polynomial_with_power() {
        assert_eq!(parse("x^2+1").to_string(), "1 + x^2");
    }

    #[test]
    fn parses_simple_fraction() {
        assert_eq!(parse(r"\frac{1}{2}").to_string(), "1/2");
    }

    #[test]
    fn parses_implicit_multiplication() {
        assert_eq!(parse("2x").to_string(), "2*x");
        // A command is an atom too: `2\pi` is `2*pi`.
        assert_eq!(parse(r"2\pi").to_string(), "2*pi");
        // Adjacent groups multiply. Structural comparison: `cas-core`'s plain
        // text rendering drops the parentheses around a leading `Add` factor,
        // so the string would read misleadingly.
        assert_eq!(
            parse("(x+1)(x+2)"),
            mul([add([int(1), sym("x")]), add([int(2), sym("x")])])
        );
    }

    #[test]
    fn parses_quadratic_in_canonical_order() {
        assert_eq!(parse("3x^2+2x-1").to_string(), "-1 + 2*x + 3*x^2");
    }

    #[test]
    fn parses_braced_exponent() {
        // Canonical `Add` sorts the constant first, hence `1 + n`.
        assert_eq!(parse("x^{n+1}").to_string(), "x^(1 + n)");
    }

    #[test]
    fn parses_exponential() {
        assert_eq!(parse("e^x").to_string(), "e^x");
    }

    #[test]
    fn parses_nested_groups() {
        assert_eq!(parse("((x))").to_string(), "x");
        assert_eq!(parse(r"\left(x+1\right)^2").to_string(), "(1 + x)^2");
    }

    #[test]
    fn parses_deeply_nested_radicals() {
        assert_eq!(
            parse(r"\sqrt{1+\sqrt{x}}").to_string(),
            "(1 + x^(1/2))^(1/2)"
        );
    }

    // -- conventions --------------------------------------------------------

    #[test]
    fn function_consumes_the_following_product() {
        // The convention required by the crate: `\sin 2x` is `sin(2*x)`.
        assert_eq!(parse(r"\sin 2x").to_string(), "sin(2*x)");
        // ... but it stops at a sum.
        assert_eq!(parse(r"\sin x + 1").to_string(), "1 + sin(x)");
    }

    #[test]
    fn division_binds_looser_than_implicit_multiplication() {
        // `1/2x` must be `1/(2x)`. Division is stored as a negative power, so
        // the canonical rendering of `1/(2x)` is `(2*x)^(-1)`.
        let parsed = parse("1/2x");
        assert_eq!(
            parsed,
            mul([int(1), pow(mul([int(2), sym("x")]), int(-1))]),
            "1/2x must be 1/(2x), not (1/2)x"
        );
        assert_eq!(parsed.to_string(), "(2*x)^-1");
    }

    #[test]
    fn explicit_division_is_left_associative_and_implicit_is_not_swallowed() {
        // Contrast: `1/2*x` is `(1/2)*x`, and `\frac{1}{2}x` is `(1/2)*x`.
        assert_eq!(parse("1/2*x").to_string(), "1/2*x");
        assert_eq!(parse(r"\frac{1}{2}x").to_string(), "1/2*x");
        assert_eq!(parse(r"\frac{2}{3}x").to_string(), "2/3*x");
    }

    #[test]
    fn function_power_attaches_to_the_application() {
        assert_eq!(parse(r"\sin^2 x").to_string(), "sin(x)^2");
        assert_eq!(parse(r"\sin(x)^2").to_string(), "sin(x)^2");
        // `\left(` is just a parenthesis, so it behaves the same way.
        assert_eq!(parse(r"\sin\left(x\right)^2").to_string(), "sin(x)^2");
        // Without the superscript on the name, `^` belongs to the argument.
        assert_eq!(parse(r"\sin x^2").to_string(), "sin(x^2)");
    }

    #[test]
    fn subscripts_are_parsed_and_discarded() {
        assert_eq!(parse("x_1").to_string(), "x");
        assert_eq!(parse("x_1 + x_2").to_string(), "2*x");
        assert_eq!(parse("x_{i+1}^2").to_string(), "x^2");
        assert_eq!(parse(r"\log_2 x").to_string(), "log(x)");
    }

    // -- numbers, symbols, functions ---------------------------------------

    #[test]
    fn decimals_are_exact_rationals() {
        assert_eq!(parse("3.5").to_string(), "7/2");
        assert_eq!(parse("0.25").to_string(), "1/4");
        assert_eq!(parse("-3.5").to_string(), "-7/2");
    }

    #[test]
    fn greek_commands_become_symbols() {
        assert_eq!(parse(r"\pi").to_string(), "pi");
        assert_eq!(parse(r"\alpha").to_string(), "alpha");
        // Two distinct symbols: canonical ordering follows interning order, so
        // compare structurally instead of pinning the term order.
        assert_eq!(parse(r"\alpha + \beta"), add([sym("alpha"), sym("beta")]));
    }

    #[test]
    fn square_roots_are_half_powers() {
        assert_eq!(parse(r"\sqrt{x}").to_string(), "x^(1/2)");
        // `cas-core` only evaluates roots that are exact; sqrt(8) stays put.
        assert_eq!(parse(r"\sqrt{8}").to_string(), "8^(1/2)");
    }

    #[test]
    fn nth_root_uses_the_index_as_the_exponent() {
        assert_eq!(parse(r"\sqrt[3]{x}").to_string(), "x^(1/3)");
        assert_eq!(parse(r"\sqrt[3]{8}").to_string(), "2");
    }

    #[test]
    fn fraction_commands_are_aliases() {
        assert_eq!(parse(r"\dfrac{3}{4}").to_string(), "3/4");
        assert_eq!(parse(r"\tfrac{1}{2}").to_string(), "1/2");
        assert_eq!(parse(r"\frac{1}{2}").to_string(), "1/2");
    }

    #[test]
    fn unbraced_fraction_arguments_are_single_tokens() {
        // LaTeX reads `\frac12` as `\frac{1}{2}` because each digit is a token.
        // Our lexer reads `12` as one number, so the two-token form would have
        // to be guessed; we require braces instead of guessing.
        let err = parse_err(r"\frac12");
        assert!(err.message.contains("denominator"), "{}", err.message);
        assert_eq!(err.position, 7);
        // A single-letter argument works without braces: `\frac1x` is 1/x.
        assert_eq!(parse(r"\frac1x").to_string(), "x^-1");
    }

    #[test]
    fn named_functions_parse_with_and_without_backslash() {
        assert_eq!(parse(r"\sin(x)").to_string(), "sin(x)");
        assert_eq!(parse("sin(x)").to_string(), "sin(x)");
        assert_eq!(parse(r"\cos x").to_string(), "cos(x)");
        assert_eq!(parse(r"\exp x").to_string(), "exp(x)");
        assert_eq!(parse(r"\arcsin x").to_string(), "arcsin(x)");
        assert_eq!(parse(r"\ln e").to_string(), "ln(e)");
    }

    #[test]
    fn abs_becomes_a_function_node() {
        assert_eq!(parse(r"\abs{x}").to_string(), "abs(x)");
        assert_eq!(parse(r"\abs x").to_string(), "abs(x)");
    }

    #[test]
    fn multi_letter_identifiers_split_into_letters() {
        assert_eq!(parse("xy"), mul([sym("x"), sym("y")]));
        assert_eq!(parse("2xy"), mul([int(2), sym("x"), sym("y")]));
    }

    // -- operators ----------------------------------------------------------

    #[test]
    fn multiplication_commands() {
        assert_eq!(parse(r"2\cdot 3").to_string(), "6");
        assert_eq!(parse(r"2\times x").to_string(), "2*x");
        assert_eq!(parse(r"6\div 2").to_string(), "3");
        assert_eq!(parse("x/2").to_string(), "1/2*x");
    }

    #[test]
    fn unary_signs() {
        assert_eq!(parse("+x").to_string(), "x");
        assert_eq!(parse("--x").to_string(), "x");
        assert_eq!(parse("-(2+3)").to_string(), "-5");
        assert_eq!(parse("-x^2"), mul([int(-1), pow(sym("x"), int(2))]));
    }

    #[test]
    fn whitespace_is_ignored() {
        assert_eq!(parse("  x  +  1 ").to_string(), "1 + x");
        assert_eq!(parse(r"\sin  x").to_string(), "sin(x)");
    }

    #[test]
    fn left_right_brackets_group() {
        assert_eq!(parse(r"\left[\frac{1}{2}\right]").to_string(), "1/2");
        assert_eq!(
            parse(r"\left(x+1\right)\left(x+1\right)").to_string(),
            "(1 + x)^2"
        );
    }

    // -- errors -------------------------------------------------------------

    #[test]
    fn missing_denominator_is_an_error_at_the_failing_token() {
        let err = parse_err(r"\frac{1}");
        assert_eq!(err.position, 8);
        assert!(err.message.contains("denominator"), "message was {}", err.message);
    }

    #[test]
    fn missing_exponent_is_an_error_at_the_caret() {
        let err = parse_err("x^");
        assert_eq!(err.position, 1);
        assert!(err.message.contains("exponent"), "message was {}", err.message);
    }

    #[test]
    fn unknown_command_reports_its_position() {
        let err = parse_err(r"\foo{x}");
        assert_eq!(err.position, 0);
        assert!(err.message.contains(r"\foo"), "message was {}", err.message);
    }

    #[test]
    fn unclosed_brace_points_at_the_opening_brace() {
        let err = parse_err("{x");
        assert_eq!(err.position, 0);
        assert!(err.message.contains("unclosed"), "message was {}", err.message);

        let err = parse_err(r"\frac{1}{2");
        assert_eq!(err.position, 8);
    }

    #[test]
    fn stray_closing_paren_is_an_error() {
        let err = parse_err("x)");
        assert_eq!(err.position, 1);
        assert!(err.message.contains("after the expression"), "message was {}", err.message);
    }

    #[test]
    fn empty_input_is_an_error() {
        let err = parse_err("");
        assert_eq!(err.position, 0);
        assert!(err.message.contains("end of input"), "message was {}", err.message);
    }

    #[test]
    fn dangling_operator_is_an_error_at_end_of_input() {
        let err = parse_err("x +");
        assert_eq!(err.position, 3);
        assert!(err.message.contains("end of input"), "message was {}", err.message);
    }

    #[test]
    fn factorial_is_rejected_rather_than_dropped() {
        let err = parse_err("5!");
        assert_eq!(err.position, 1);
        assert!(err.message.contains("factorial"), "message was {}", err.message);
    }

    #[test]
    fn division_by_literal_zero_is_rejected() {
        let err = parse_err(r"\frac{1}{0}");
        assert_eq!(err.position, 0);
        assert!(err.message.contains("division by zero"), "message was {}", err.message);

        let err = parse_err("1/0");
        assert_eq!(err.position, 1);
    }

    #[test]
    fn mismatched_left_right_is_an_error() {
        let err = parse_err(r"\left(x\right]");
        assert_eq!(err.position, 7);
        assert!(err.message.contains("mismatched"), "message was {}", err.message);
    }

    #[test]
    fn unclosed_left_right_is_an_error() {
        let err = parse_err(r"\left(x+1");
        assert_eq!(err.position, 0);
        assert!(err.message.contains("unclosed"), "message was {}", err.message);
    }

    #[test]
    fn missing_function_argument_is_an_error() {
        let err = parse_err(r"\sin");
        assert_eq!(err.position, 0);
        assert!(err.message.contains("argument"), "message was {}", err.message);
    }

    #[test]
    fn missing_radicand_is_an_error() {
        let err = parse_err(r"\sqrt[3]");
        assert_eq!(err.position, 8);
        assert!(err.message.contains("radicand"), "message was {}", err.message);
    }

    #[test]
    fn unknown_characters_are_errors_with_positions() {
        let err = parse_err("x @ y");
        assert_eq!(err.position, 2);
        let err = parse_err("x + π");
        assert_eq!(err.position, 4);
    }

    #[test]
    fn division_by_identical_terms_is_cancelled_by_the_normalizer() {
        // The parser does not evaluate; it just hands the tree to `cas-core`,
        // whose normalizer folds `x * x^(-1)` into `1`. Documented so the
        // result is not mistaken for a parser-side simplification.
        assert_eq!(parse(r"\frac{x}{x}").to_string(), "1");
        assert_eq!(parse(r"\frac{1}{x}").to_string(), "x^-1");
    }

    #[test]
    fn no_input_panics() {
        // Anything can be thrown at the parser; it must answer with `Ok` or
        // `Err`, never a panic.
        let inputs = [
            "",
            " ",
            r"\",
            "}{",
            "x^{",
            r"\frac{}{}",
            r"\frac",
            r"\sqrt",
            r"\sqrt[]",
            "1..2",
            r"\left",
            r"\left(",
            r"\right)",
            "()",
            "[]",
            "x_",
            "x^^2",
            "\\",
            "!!!",
            ",",
            r"\sin^",
            r"\sin^2",
            "x_{i,j}",
            "π",
            "x=1",
            r"\frac{1}{2}^",
            "2 3 4",
            r"\abs",
            r"\foo",
            "x\u{0}",
        ];
        for input in inputs {
            let _ = parse_latex(input);
        }
    }

    #[test]
    fn fuzz_never_panics() {
        // A deterministic pseudo-random sweep over the token alphabet. The
        // parser may say `Ok` or `Err`; it must not panic, and every `Err` must
        // carry an in-bounds position.
        const ALPHABET: &[char] = &[
            'x', 'y', '1', '2', '0', '.', '+', '-', '*', '/', '^', '_', '!', '(', ')', '[', ']',
            '{', '}', ',', '\\', 'a', 's', 'n', 't', 'c', 'o', 'r', 'f', 'p', 'q', ' ', 'π',
        ];
        let mut seed: u64 = 0x5DEE_CE66_D1CE_1234;
        for _ in 0..2000 {
            let len = (seed % 12) as usize;
            let mut text = String::new();
            for _ in 0..len {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                text.push(ALPHABET[(seed >> 33) as usize % ALPHABET.len()]);
            }
            match parse_latex(&text) {
                Ok(_) => {}
                Err(err) => assert!(
                    err.position <= text.len(),
                    "position {} out of bounds for {text:?}",
                    err.position
                ),
            }
        }
    }
}
