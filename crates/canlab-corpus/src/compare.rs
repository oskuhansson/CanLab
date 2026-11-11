//! Comparing an expected answer with the one the run produced.
//!
//! Three comparisons are all the format needs, and each has one rule the tests
//! pin down:
//!
//! * an `exact` answer is text with whitespace collapsed — **except** a finite
//!   set, which is compared as a set because `\left\{0, 8\right\}` and
//!   `\left\{8, 0\right\}` are the same answer;
//! * a system answer is compared by variable name, so the order of the
//!   assignments does not matter;
//! * a `numeric` answer is compared within its tolerance after both sides are
//!   rounded to `digits` significant figures, half-even.

/// Collapse runs of whitespace to one space and trim. This is "text with
/// whitespace collapsed" from the format, and nothing more: `0.5` is still not
/// `\frac{1}{2}`.
pub fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The elements of a finite set, if `text` is one.
///
/// `[]` is returned for `\left\{\right\}` and for `\varnothing`? No — the empty
/// set and the empty list are different forms in the format, and only the
/// brace form is a set here. `\varnothing` compares as text, which is what the
/// canonical printer emits for it.
fn set_elements(text: &str) -> Option<Vec<String>> {
    let text = text.trim();
    let inner = text
        .strip_prefix("\\left\\{")
        .and_then(|rest| rest.strip_suffix("\\right\\}"))
        .or_else(|| {
            text.strip_prefix("\\{")
                .and_then(|rest| rest.strip_suffix("\\}"))
        })?;

    let mut elements = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let mut chars = inner.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\\' => {
                // A command: copy it and the character it escapes. `\{` and
                // `\}` are literal braces, so they do not move the nesting
                // depth; `\frac` is copied as `\f` and its letters follow as
                // ordinary characters.
                current.push(character);
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            '{' => {
                depth += 1;
                current.push(character);
            }
            '}' => {
                depth -= 1;
                current.push(character);
            }
            ',' if depth == 0 => {
                elements.push(collapse(&current));
                current.clear();
            }
            other => current.push(other),
        }
    }
    elements.push(collapse(&current));
    Some(elements)
}

/// A system answer as `name = value` entries, if `text` has that shape.
fn assignments(text: &str) -> Option<Vec<(String, String)>> {
    if !text.contains('=') {
        return None;
    }
    let mut out = Vec::new();
    for part in text.split(';') {
        let (name, value) = part.split_once('=')?;
        let name = collapse(name);
        if name.is_empty() {
            return None;
        }
        out.push((name, collapse(value)));
    }
    out.sort();
    Some(out)
}

/// Compare an `exact` expectation with the answer text.
pub fn exact_matches(expected: &str, got: &str) -> bool {
    if let (Some(expected), Some(got)) = (set_elements(expected), set_elements(got)) {
        let mut expected = expected;
        let mut got = got;
        expected.sort();
        got.sort();
        return expected == got;
    }
    if expected.contains('=') || got.contains('=') {
        // A system is matched by variable name. If only one side is a set of
        // assignments, it is not the same answer and the text comparison below
        // says so with the two strings in the report.
        if let (Some(expected), Some(got)) = (assignments(expected), assignments(got)) {
            return expected == got;
        }
    }
    collapse(expected) == collapse(got)
}

/// Round to `digits` significant figures, ties to even.
///
/// `digits` counts significant figures, not decimal places: the physics row in
/// `docs/corpus.md` says `2.309…` is `2.3 to two significant figures`.
pub fn round_significant(value: f64, digits: u32) -> f64 {
    if value == 0.0 || !value.is_finite() {
        return value;
    }
    let digits = digits.max(1) as i32;
    let exponent = value.abs().log10().floor() as i32;
    let scale = 10f64.powi(digits - 1 - exponent);
    (value * scale).round_ties_even() / scale
}

/// Compare a numeric expectation, applying `digits` to both sides first.
pub fn numeric_matches(expected: f64, value: f64, tolerance: f64, digits: Option<u32>) -> bool {
    if !value.is_finite() {
        return false;
    }
    let (expected, value) = match digits {
        Some(digits) => (
            round_significant(expected, digits),
            round_significant(value, digits),
        ),
        None => (expected, value),
    };
    (expected - value).abs() <= tolerance
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_set_is_compared_as_a_set() {
        assert!(exact_matches(
            "\\left\\{0, 8\\right\\}",
            "\\left\\{8, 0\\right\\}"
        ));
        assert!(exact_matches(
            "\\left\\{1, \\frac{1}{2}\\right\\}",
            "\\left\\{\\frac{1}{2}, 1\\right\\}"
        ));
        assert!(!exact_matches(
            "\\left\\{0, 8\\right\\}",
            "\\left\\{0, 8, 1\\right\\}"
        ));
    }

    #[test]
    fn sets_and_other_forms_do_not_cross() {
        // A set is not its element, and a bracketed interval is not a set.
        assert!(!exact_matches("\\left\\{63\\right\\}", "63"));
        assert!(!exact_matches("(-1, 3]", "\\left\\{-1, 3\\right\\}"));
        assert!(exact_matches("(-1, 3]", "(-1, 3]"));
    }

    #[test]
    fn text_is_whitespace_collapsed_only() {
        assert!(exact_matches("\\frac{\\pi}{6} + 2k\\pi", "\\frac{\\pi}{6}  +  2k\\pi"));
        assert!(!exact_matches("0.5", "\\frac{1}{2}"));
        assert!(!exact_matches("63", "63.0"));
        assert!(exact_matches("\\varnothing", "\\varnothing"));
    }

    #[test]
    fn a_system_is_matched_by_name_not_order() {
        assert!(exact_matches("x = 3; y = 5", "y = 5; x = 3"));
        assert!(exact_matches("x = 3; y = 5", "x = 3; y = 5"));
        assert!(!exact_matches("x = 3; y = 5", "x = 3; y = 6"));
        assert!(!exact_matches("x = 3; y = 5", "x = 3"));
        // A set of assignments is not a plain expression answer.
        assert!(!exact_matches("x = 3; y = 5", "8"));
    }

    #[test]
    fn significant_figures_round_half_even() {
        assert_eq!(round_significant(2.309_401, 2), 2.3);
        assert_eq!(round_significant(2.35, 2), 2.4);
        assert_eq!(round_significant(2.25, 2), 2.2);
        assert_eq!(round_significant(0.012_345, 3), 0.0123);
        assert_eq!(round_significant(-2.35, 2), -2.4);
    }

    #[test]
    fn numeric_comparison_uses_the_tolerance() {
        assert!(numeric_matches(2.3, 2.309_401, 0.05, Some(2)));
        assert!(numeric_matches(2.3, 2.309_401, 0.01, None));
        assert!(!numeric_matches(2.3, 2.309_401, 0.001, None));
        assert!(!numeric_matches(2.3, f64::NAN, 1.0, None));
        assert!(!numeric_matches(2.3, f64::INFINITY, 1.0, None));
    }
}
