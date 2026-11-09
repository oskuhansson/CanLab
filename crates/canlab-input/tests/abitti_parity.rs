//! Parity with the live Abitti editor.
//!
//! Every expectation in this file was **observed**, not invented: the keystroke
//! script in the first column was typed into the real MathQuill field at
//! <https://math-demo.abitti.fi/> (build "Päivitetty 5.11.2026") with Playwright,
//! and the second column is the LaTeX that editor reported afterwards.
//!
//! The scripts use the same notation the observation harness used:
//!
//! | Token | Key |
//! |---|---|
//! | `[BS]` | Backspace |
//! | `[DEL]` | Delete |
//! | `[LEFT]` `[RIGHT]` `[UP]` `[DOWN]` | arrows |
//!
//! To regenerate: open the demo, press `Ctrl-E` to get a formula field, and for
//! each script call `field.latex('')`, type the keys, and read `field.latex()`.
//!
//! ## Known and deliberate divergences
//!
//! These are *not* asserted, because reproducing them would make the model
//! worse. Each is a MathQuill internal quirk rather than a behaviour a student
//! would want:
//!
//! * `\int_0` is `\int_{_{0}}^{ }` there — the integral owns hidden sub/sup
//!   blocks and the `_` nests inside them. Here `\int` is a plain token, so it
//!   reads `\int_0`. The palette's `\int_{X}^{X}` still gives the full form.
//! * `(]` is `\left(\right]` there: a closer of a *different* family can
//!   harden a ghost. Here a mismatched closer opens its own pair. Rejecting a
//!   typo is friendlier than building mixed delimiters.
//! * `\text{x}` / `\mathbb{Z}` typed with braces put literal braces inside the
//!   command there. Here those are still unknown-command fallbacks; the palette
//!   provides `roman` and the blackboard letters as structured snippets.

use canlab_input::MathField;

/// Type a keystroke script into a fresh field.
fn typed(script: &str) -> MathField {
    let mut field = MathField::new();
    run(&mut field, script);
    field
}

fn run(field: &mut MathField, script: &str) {
    let mut chars = script.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '[' {
            field.insert_char(c);
            continue;
        }
        let mut token = String::new();
        for c in chars.by_ref() {
            if c == ']' {
                break;
            }
            token.push(c);
        }
        match token.as_str() {
            "BS" => field.backspace(),
            "DEL" => field.delete_forward(),
            "LEFT" => {
                field.move_left();
            }
            "RIGHT" => {
                field.move_right();
            }
            "UP" => {
                field.move_up();
            }
            "DOWN" => {
                field.move_down();
            }
            other => {
                // A literal bracket in the script, e.g. the `[x]` case.
                field.insert_char('[');
                for c in other.chars() {
                    field.insert_char(c);
                }
                field.insert_char(']');
            }
        }
    }
}

/// The observed corpus, as `(keys, latex)`.
fn corpus() -> &'static [(&'static str, &'static str)] {
    &[
        // ---------------------------------------------------------- plain typing
        ("x+1", "x+1"),
        ("2x", "2x"),
        ("x2", "x2"),
        ("x\\cdot y", "x\\cdot y"),
        ("2*3", "2\\cdot3"),
        ("sin x", "\\sin\\ x"),
        ("<= ", "\\le\\ "),
        (">= ", "\\ge\\ "),
        ("<", "<"),
        (">", ">"),
        ("=", "="),
        ("!=", "!="),
        ("->", "->"),
        ("=>", "=>"),
        ("+-", "+-"),
        (",", "{,}"),
        ("'", "'"),
        (";", ";"),
        (":", ":"),
        ("%", "\\%"),
        ("#", "\\#"),
        ("&", "\\&"),
        ("$", "\\$"),
        ("~", "\\sim"),
        ("x~y", "x\\sim y"),
        ("\\ ", "\\ "),
        // -------------------------------------------------------- implicit names
        ("sinx", "\\sin x"),
        ("sin2", "\\sin2"),
        ("xsin", "x\\sin"),
        ("arcsin", "\\arcsin"),
        ("pi", "pi"),
        ("sqrt", "sqrt"),
        // ------------------------------------------------------------ fractions
        ("1/2", "\\frac{1}{2}"),
        ("1/2x", "\\frac{1}{2x}"),
        ("1+2/", "1+\\frac{2}{ }"),
        ("xy/", "\\frac{xy}{ }"),
        ("/", "\\frac{ }{ }"),
        ("1/2/", "\\frac{1}{\\frac{2}{ }}"),
        ("a/b", "\\frac{a}{b}"),
        // --------------------------------------------------------------- scripts
        ("x^2", "x^2"),
        ("x^2y", "x^{2y}"),
        ("x_1", "x_1"),
        ("^", "^{ }"),
        ("_", "_{ }"),
        ("x^", "x^{ }"),
        ("x_", "x_{ }"),
        ("x_i^2", "x_{i^2}"),
        // -------------------------------------------------------------- brackets
        ("(", "\\left(\\right)"),
        ("()", "\\left(\\right)"),
        ("(x", "\\left(x\\right)"),
        ("(x)", "\\left(x\\right)"),
        (")", "\\left(\\right)"),
        ("x)", "\\left(x\\right)"),
        ("x+1)", "\\left(x+1\\right)"),
        ("1+2)", "\\left(1+2\\right)"),
        ("(x)(y)", "\\left(x\\right)\\left(y\\right)"),
        ("f(x)", "f\\left(x\\right)"),
        ("sin(x)", "\\sin\\left(x\\right)"),
        ("((x))", "\\left(\\left(x\\right)\\right)"),
        ("()]", "\\left[\\left(\\right)\\right]"),
        ("(x))", "\\left(\\left(x\\right)\\right)"),
        ("[x]", "\\left[x\\right]"),
        ("[[", "\\left[\\left[\\right]\\right]"),
        ("|x|", "\\left|x\\right|"),
        ("|x", "\\left|x\\right|"),
        ("|", "\\left|\\right|"),
        ("{x}", "\\left\\{x\\right\\}"),
        ("x^{2}", "x^{\\left\\{2\\right\\}}"),
        ("x^{2y}", "x^{\\left\\{2y\\right\\}}"),
        // --------------------------------------------------------------- accents
        ("\\bar", "\\bar"),
        ("\\bar ", "\\bar{ }"),
        ("\\bar x", "\\bar{x}"),
        ("\\bar xy", "\\bar{xy}"),
        ("\\bar12", "\\bar{12}"),
        ("\\bar 12", "\\bar{12}"),
        ("\\bar x^2", "\\bar{x^2}"),
        ("\\bar{12}", "\\bar{\\left\\{12\\right\\}}"),
        ("\\vec x", "\\vec{x}"),
        ("\\vec ab", "\\vec{ab}"),
        ("\\hat x", "\\hat{x}"),
        ("\\tilde x", "\\tilde{x}"),
        ("\\dot x", "\\dot{x}"),
        ("\\underline x", "\\underline{x}"),
        ("\\overline x", "\\overline{x}"),
        ("\\overline xy", "\\overline{xy}"),
        ("\\overrightarrow x", "\\overrightarrow{x}"),
        ("\\overleftarrow x", "\\overleftarrow{x}"),
        // Not commands in Abitti either: they come back as \text{…}.
        ("\\ddot x", "\\text{ddot}x"),
        ("\\widehat xy", "\\text{widehat}xy"),
        ("\\overbrace x", "\\text{overbrace}x"),
        // ------------------------------------------------------------ structures
        ("\\sqrt x", "\\sqrt{x}"),
        ("\\sqrt2", "\\sqrt{2}"),
        ("\\sqrt 2", "\\sqrt{2}"),
        ("\\sqrt{2}", "\\sqrt{\\left\\{2\\right\\}}"),
        ("\\frac 1 2", "\\frac{1\\ 2}{ }"),
        ("\\frac12", "\\frac{12}{ }"),
        ("\\frac{1}2", "\\frac{\\left\\{1\\right\\}2}{ }"),
        ("\\binom 1 2", "\\binom{1\\ 2}{ }"),
        ("\\int", "\\int"),
        ("\\sum", "\\sum"),
        ("\\lim", "\\lim"),
        ("\\lim x", "\\lim x"),
        ("\\lim_x", "\\lim_x"),
        ("\\log", "\\log"),
        ("\\ln", "\\ln"),
        // ------------------------------------------------------ command entry
        ("\\alpha", "\\alpha"),
        ("\\alpha ", "\\alpha"),
        ("\\alpha2", "\\alpha2"),
        ("\\alphax", "\\alphax"),
        ("\\alphaa", "\\alphaa"),
        ("\\alp ", "\\text{alp}"),
        ("\\varepsilon", "\\varepsilon"),
        ("\\Gamma", "\\Gamma"),
        ("\\Sigma", "\\Sigma"),
        ("\\ ", "\\ "),
        // ------------------------------------------- rewritten Unicode input
        ("\u{00bd}", "\\frac{1}{2}"),
        ("\u{00b2}", "^2"),
        ("\u{00b3}", "^3"),
        ("\u{00d7}", "\\times"),
        ("\u{03c0}", "\\pi"),
        ("\u{2264}", "\\le"),
        // …and the ones that stay literal, which is the easy thing to get wrong
        ("\u{00b0}", "\u{00b0}"),
        ("\u{2030}", "\u{2030}"),
        ("\u{00b7}", "\u{00b7}"),
        ("\u{221e}", "\u{221e}"),
        ("\u{03b1}", "\u{03b1}"),
        // ------------------------------------------------------------ backspace
        ("(x)[LEFT][BS]", "\\left(\\right)"),
        ("(x)[LEFT][LEFT][BS]", "x"),
        ("1/2[BS]", "\\frac{1}{ }"),
        ("1/2[BS][BS]", "1"),
        ("x^2[BS]", "x^{ }"),
        ("x^2[BS][BS]", "x"),
        ("\\alpha[BS]", "\\alph"),
        ("\\alpha[BS][BS]", "\\alp"),
        ("\\alpha[BS][BS][BS]", "\\al"),
        ("\\alp[BS][BS]", "\\a"),
        ("\\frac[BS]", "\\fra"),
        ("\\sqrt[BS]", "\\sqr"),
        ("\\alpha [BS]", ""),
        // --------------------------------------------------------- delete/arrows
        ("123[DEL]", "123"),
        ("123[LEFT][LEFT][DEL]", "13"),
        ("(x)[DEL]", "\\left(x\\right)"),
        ("ab[LEFT][LEFT][RIGHT]", "ab"),
    ]
}

#[test]
fn the_model_reproduces_the_observed_abitti_corpus() {
    let mut failures = Vec::new();
    for (script, expected) in corpus() {
        let actual = typed(script).to_latex();
        if actual != *expected {
            failures.push(format!(
                "keys {script:?}\n    Abitti: {expected:?}\n    CanLab: {actual:?}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} observed cases differ:\n\n{}",
        failures.len(),
        corpus().len(),
        failures.join("\n")
    );
}

#[test]
fn backspace_after_a_closed_bracket_deletes_the_pair() {
    // Abitti turns the closing `)` back into a ghost here, which is invisible in
    // the LaTeX (`\left(x\right)` either way) and only shows as a grey paren.
    // Pressing Backspace and having seemingly nothing happen is worse than
    // removing the bracket, so this diverges on purpose.
    let mut field = typed("(x)");
    field.backspace();
    assert_eq!(field.to_latex(), "");
}

#[test]
fn the_corpus_is_not_trivially_small() {
    // A parity test that quietly shrank to ten cases would still pass.
    assert!(corpus().len() >= 100, "only {} cases", corpus().len());
}
