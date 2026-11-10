//! LaTeX rendering for [`canlab_core::Expr`].
//!
//! The printer is one recursive walk with an explicit precedence argument, in
//! the same spirit as the plain-text `Display` in `canlab-core`. Two rules shape
//! almost every decision here:
//!
//! 1. **Parenthesize only what the grammar requires.** LaTeX braces already
//!    group, so `\frac{x + 1}{2}` and `\sqrt{x + 1}` need no `\left(…\right)`;
//!    a sum in an exponent, or a product used as a power base, does.
//! 2. **Signs are not factors.** Canonical form encodes `a - b` as
//!    `a + (-1)·b`, so the sign has to be hoisted out of the numeric
//!    coefficient before anything is written; otherwise every subtraction
//!    would come out as `a + -b`.
//!
//! Output is deliberately plain — no `\displaystyle`, no styling macros, and
//! no surrounding `$`. The caller owns the delimiters, which is what lets the
//! same string drop into inline math, display math, or a Markdown table cell.

use canlab_core::expr::{Expr, ExprNode, Hold};
use canlab_core::number::Number;
use canlab_core::symbol::SymbolId;

/// Render `expr` as LaTeX math, without surrounding delimiters.
///
/// Delimiters are the caller's job: `$…$` inline, `\[…\]` in display mode.
/// Emitting them here would make composition (a cell inside an `align`
/// environment, a label inside a TikZ node) impossible to get right.
pub fn to_latex(expr: &Expr) -> String {
    let mut out = String::new();
    write_node(expr, &mut out, PREC_ADD);
    out
}

// Binding strengths, lowest first. A node is wrapped in `\left(…\right)` when
// its strength is below the minimum its context allows — that comparison, and
// nothing else, decides where parentheses appear.
const PREC_ADD: u8 = 1;
const PREC_MUL: u8 = 2;
const PREC_POW: u8 = 3;
const PREC_ATOM: u8 = 4;

/// The visible multiplication sign, used only where juxtaposition would be
/// misread (`2\cdot 3`). The trailing space is cosmetic — TeX spaces binary
/// operators itself — but keeps the source form readable.
const CDOT: &str = "\\cdot ";

/// Precedence of a node when it appears as a subexpression.
///
/// The two deviations from the tree's shape are deliberate:
/// * a negative number prints with a leading `-`, which binds like a
///   subtraction, so it must be delimited (`\left(-2\right)^{3}`) unless it
///   stands at the start of its context;
/// * a rational prints as `\frac{a}{b}`, which binds like a division — the
///   same choice `canlab-core`'s plain-text printer makes.
fn prec(node: &ExprNode) -> u8 {
    match node {
        ExprNode::Add(_) | ExprNode::Hold(_) => PREC_ADD,
        ExprNode::Mul(_) => PREC_MUL,
        ExprNode::Pow(..) => PREC_POW,
        ExprNode::Num(n) if n.is_negative() => PREC_ADD,
        ExprNode::Num(Number::Rat(..)) => PREC_MUL,
        ExprNode::Num(_) | ExprNode::Sym(_) | ExprNode::Fn(..) => PREC_ATOM,
    }
}

/// Does `node` need `\left(…\right)` when the context allows `min_prec`?
fn needs_parens(node: &ExprNode, min_prec: u8) -> bool {
    if prec(node) < min_prec {
        return true;
    }
    // A leading minus also appears on products such as `-3x`; inside another
    // product it has to be delimited, or `-3x` followed by `y` reads as a
    // subtraction rather than a factor.
    min_prec >= PREC_MUL && starts_with_minus(node)
}

fn starts_with_minus(node: &ExprNode) -> bool {
    match node {
        ExprNode::Num(n) => n.is_negative(),
        ExprNode::Mul(factors) => factors
            .first()
            .is_some_and(|factor| starts_with_minus(factor)),
        _ => false,
    }
}

/// Would the printed form of `node` begin with a digit?
///
/// Only used to decide between juxtaposition and `\cdot`; a factor that leads
/// with a digit must never be glued directly to the preceding number.
fn starts_with_digit(node: &ExprNode) -> bool {
    match node {
        ExprNode::Num(n) => !n.is_negative(),
        ExprNode::Sym(s) => s.name().chars().next().is_some_and(|c| c.is_ascii_digit()),
        _ => false,
    }
}

/// The single recursive formatter. `min_prec` is the weakest subexpression the
/// surrounding context can absorb without parentheses.
fn write_node(node: &ExprNode, out: &mut String, min_prec: u8) {
    let parens = needs_parens(node, min_prec);
    if parens {
        out.push_str("\\left(");
    }

    match node {
        ExprNode::Num(n) => write_number(n, out),
        ExprNode::Sym(s) => write_symbol(*s, out),
        ExprNode::Add(terms) => write_add(terms, out),
        ExprNode::Mul(factors) => write_mul(factors, out),
        ExprNode::Pow(base, exp) => write_pow(base, exp, out),
        ExprNode::Fn(name, args) => write_function(*name, args, out),
        ExprNode::Hold(hold) => write_hold(hold, out),
    }

    if parens {
        out.push_str("\\right)");
    }
}

// ---------------------------------------------------------------------------
// Numbers
// ---------------------------------------------------------------------------

fn write_number(number: &Number, out: &mut String) {
    match number {
        Number::Int(value) => out.push_str(&value.to_string()),
        Number::Rat(num, den) => {
            // The sign belongs to the fraction, not to its numerator:
            // `-\frac{3}{4}`, never `\frac{-3}{4}`.
            if *num < 0 {
                out.push('-');
            }
            out.push_str("\\frac{");
            out.push_str(&num.unsigned_abs().to_string());
            out.push_str("}{");
            out.push_str(&den.to_string());
            out.push('}');
        }
        Number::Float(value) => write_float(*value, out),
    }
}

fn write_float(value: f64, out: &mut String) {
    if value.is_nan() {
        out.push_str("\\mathrm{NaN}");
        return;
    }
    if value.is_infinite() {
        if value.is_sign_negative() {
            out.push('-');
        }
        out.push_str("\\infty");
        return;
    }
    let text = value.to_string();
    out.push_str(&text);
    // `Display` prints `2` for `2.0`. The whole point of `Number::Float` is
    // that it is not the exact integer, so keep the decimal point visible.
    if !text.contains('.') && !text.contains('e') && !text.contains('E') {
        out.push_str(".0");
    }
}

// ---------------------------------------------------------------------------
// Sums
// ---------------------------------------------------------------------------

fn write_add(terms: &[Expr], out: &mut String) {
    if terms.is_empty() {
        // Canonical form never produces this; raw constructors can.
        out.push('0');
        return;
    }

    for (index, term) in terms.iter().enumerate() {
        let (coeff, base) = split_coeff(term);
        let negative = coeff.is_negative();

        if index == 0 {
            if negative {
                out.push('-');
            }
        } else if negative {
            out.push_str(" - ");
        } else {
            out.push_str(" + ");
        }

        // The separator already carries the sign, so only the magnitude is
        // printed from here on.
        let magnitude = if negative { coeff.neg() } else { coeff };
        write_add_term(&magnitude, &base, out);
    }
}

fn write_add_term(coeff: &Number, base: &Expr, out: &mut String) {
    if base.is_one() {
        write_number(coeff, out);
        return;
    }
    if coeff.is_one() {
        write_node(base, out, PREC_MUL);
        return;
    }
    write_number(coeff, out);
    // `2\cdot 3` but `2x` and `\frac{1}{2}x`: a digit against a digit (or
    // another number) needs an explicit operator to stay readable.
    if coefficient_needs_cdot(base) {
        out.push_str(CDOT);
    }
    write_node(base, out, PREC_MUL);
}

fn coefficient_needs_cdot(base: &ExprNode) -> bool {
    matches!(base, ExprNode::Num(_)) || starts_with_digit(base)
}

/// Split a term into its numeric coefficient and everything else.
///
/// This mirrors the crate-private helper in `canlab-core`, which is where the
/// canonical invariant "a `Mul` may lead with a `Num` coefficient" is
/// established. Duplicating the split here keeps the printer independent of
/// `canlab-core` internals while depending only on that documented invariant.
fn split_coeff(term: &Expr) -> (Number, Expr) {
    match &**term {
        ExprNode::Num(n) => (n.clone(), ExprNode::int(1)),
        ExprNode::Mul(factors) if !factors.is_empty() => match &*factors[0] {
            ExprNode::Num(n) => {
                let rest = &factors[1..];
                let base = match rest.len() {
                    0 => ExprNode::int(1),
                    1 => rest[0].clone(),
                    _ => ExprNode::mul_raw(rest.to_vec()),
                };
                (n.clone(), base)
            }
            _ => (Number::one(), term.clone()),
        },
        _ => (Number::one(), term.clone()),
    }
}

// ---------------------------------------------------------------------------
// Products
// ---------------------------------------------------------------------------

fn write_mul(factors: &[Expr], out: &mut String) {
    if factors.is_empty() {
        // Canonical form never produces this; raw constructors can.
        out.push('1');
        return;
    }

    // `-1·x` is `-x`, never `-1x`: a unit coefficient is a sign, not a factor.
    let mut first = 0;
    if factors.len() > 1
        && matches!(&*factors[0], ExprNode::Num(n) if n.is_negative() && n.abs().is_one())
    {
        out.push('-');
        first = 1;
    }

    for (index, factor) in factors.iter().enumerate().skip(first) {
        if index > first {
            out.push_str(mul_separator(&factors[index - 1], factor));
        }
        // A leading signed coefficient prints `-3x`, not `\left(-3\right)x`,
        // so the first factor is exempt from the product's precedence. An
        // `Add` or an integral there still has to be delimited.
        let first_is_loose =
            index == 0 && !matches!(&**factor, ExprNode::Add(_) | ExprNode::Hold(_));
        let min_prec = if first_is_loose { PREC_ADD } else { PREC_MUL };
        write_node(factor, out, min_prec);
    }
}

/// The gap between two adjacent factors.
///
/// `2x` and `x y` are juxtaposition; `2\cdot 3` needs the operator because two
/// bare numbers side by side would read as one number.
fn mul_separator(prev: &ExprNode, next: &ExprNode) -> &'static str {
    if matches!(next, ExprNode::Num(_)) || starts_with_digit(next) {
        CDOT
    } else if matches!(prev, ExprNode::Num(_)) {
        ""
    } else {
        " "
    }
}

// ---------------------------------------------------------------------------
// Powers, roots and reciprocals
// ---------------------------------------------------------------------------

fn write_pow(base: &Expr, exp: &Expr, out: &mut String) {
    if let ExprNode::Num(n) = &**exp {
        if let Some(root) = unit_root(n) {
            write_radical(base, root, out);
            return;
        }
        if let Number::Int(exponent) = n {
            if *exponent < 0 {
                write_reciprocal(base, exponent.unsigned_abs(), out);
                return;
            }
        }
    }

    write_node(base, out, PREC_POW);
    out.push_str("^{");
    write_exponent(exp, out);
    out.push('}');
}

/// `1/n` with `n >= 2`, i.e. an exponent that a radical prints better than a
/// superscript.
fn unit_root(exponent: &Number) -> Option<u32> {
    match exponent {
        Number::Rat(1, den) if *den >= 2 && *den <= i128::from(u32::MAX) => Some(*den as u32),
        _ => None,
    }
}

fn write_radical(base: &Expr, root: u32, out: &mut String) {
    out.push_str("\\sqrt");
    if root != 2 {
        out.push('[');
        out.push_str(&root.to_string());
        out.push(']');
    }
    // Braces group, so a sum inside needs no parentheses.
    out.push('{');
    write_node(base, out, PREC_ADD);
    out.push('}');
}

fn write_reciprocal(base: &Expr, power: u128, out: &mut String) {
    out.push_str("\\frac{1}{");
    if power == 1 {
        // `x^{-1}` has no exponent left to show; `\frac{1}{x}` is the whole
        // denominator already.
        write_node(base, out, PREC_ADD);
    } else {
        write_node(base, out, PREC_POW);
        out.push_str("^{");
        out.push_str(&power.to_string());
        out.push('}');
    }
    out.push('}');
}

/// Render the exponent inside `^{…}`.
///
/// The braces already group, so nothing inside them needs parentheses of its
/// own: `x^{-2}`, `x^{a + b}` and `x^{-\frac{1}{2}}` all read correctly, and
/// `x^{\left(-2\right)}` is just noise. This is why the call passes
/// [`PREC_ADD`] — the weakest context — rather than a precedence that would
/// reach for parentheses.
fn write_exponent(exp: &Expr, out: &mut String) {
    if let ExprNode::Num(n) = &**exp {
        write_number(n, out);
    } else {
        write_node(exp, out, PREC_ADD);
    }
}

// ---------------------------------------------------------------------------
// Symbols, functions and held constructs
// ---------------------------------------------------------------------------

fn write_symbol(symbol: SymbolId, out: &mut String) {
    let name = symbol.name();
    if let Some(command) = greek_command(&name) {
        out.push_str(command);
    } else if name.chars().count() == 1 {
        out.push_str(&name);
    } else {
        out.push_str("\\mathrm{");
        out.push_str(&name);
        out.push('}');
    }
}

/// Greek letters must become commands: `\pi` is a glyph, `pi` would be read as
/// the product `p·i`. Anything longer than one character that is not in this
/// table falls back to `\mathrm`, which at least keeps it upright and grouped.
fn greek_command(name: &str) -> Option<&'static str> {
    Some(match name {
        "alpha" => "\\alpha",
        "beta" => "\\beta",
        "gamma" => "\\gamma",
        "delta" => "\\delta",
        "epsilon" => "\\epsilon",
        "zeta" => "\\zeta",
        "eta" => "\\eta",
        "theta" => "\\theta",
        "iota" => "\\iota",
        "kappa" => "\\kappa",
        "lambda" => "\\lambda",
        "mu" => "\\mu",
        "nu" => "\\nu",
        "xi" => "\\xi",
        "pi" => "\\pi",
        "rho" => "\\rho",
        "sigma" => "\\sigma",
        "tau" => "\\tau",
        "upsilon" => "\\upsilon",
        "phi" => "\\phi",
        "chi" => "\\chi",
        "psi" => "\\psi",
        "omega" => "\\omega",
        _ => return None,
    })
}

fn write_function(name: SymbolId, args: &[Expr], out: &mut String) {
    let function = name.as_str();
    match function_command(&function) {
        Some(command) => out.push_str(command),
        // An unknown name still has to look like a function, not a product of
        // its letters.
        None => {
            out.push_str("\\operatorname{");
            out.push_str(&function);
            out.push('}');
        }
    }

    out.push_str("\\left(");
    for (index, arg) in args.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        // The delimiters group the argument, so an inner sum needs no parens.
        write_node(arg, out, PREC_ADD);
    }
    out.push_str("\\right)");
}

/// Function names with a dedicated LaTeX command. `asin`/`acos`/`atan` are the
/// spellings some front ends use; they print as the standard `\arcsin` family.
fn function_command(name: &str) -> Option<&'static str> {
    Some(match name {
        "sin" => "\\sin",
        "cos" => "\\cos",
        "tan" => "\\tan",
        "cot" => "\\cot",
        "sec" => "\\sec",
        "csc" => "\\csc",
        "arcsin" | "asin" => "\\arcsin",
        "arccos" | "acos" => "\\arccos",
        "arctan" | "atan" => "\\arctan",
        "sinh" => "\\sinh",
        "cosh" => "\\cosh",
        "tanh" => "\\tanh",
        "ln" => "\\ln",
        "log" => "\\log",
        "exp" => "\\exp",
        _ => return None,
    })
}

fn write_hold(hold: &Hold, out: &mut String) {
    match hold {
        Hold::Integral { body, var } => {
            out.push_str("\\int ");
            // The differential closes the integrand, so a sum must be grouped.
            write_node(body, out, PREC_MUL);
            out.push_str("\\, d");
            write_symbol(*var, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::to_latex;
    use canlab_core::expr::{Expr, ExprNode, Hold, int, ratio, sym};
    use canlab_core::number::Number;
    use canlab_core::symbol::SymbolId;

    // Helpers build raw (unnormalized) trees so each test can pin one printing
    // rule exactly; the normalized-form tests use `canlab_core::add`/`mul`.
    fn x() -> Expr {
        sym("x")
    }

    fn y() -> Expr {
        sym("y")
    }

    fn sum(terms: Vec<Expr>) -> Expr {
        ExprNode::add_raw(terms)
    }

    fn product(factors: Vec<Expr>) -> Expr {
        ExprNode::mul_raw(factors)
    }

    fn power(base: Expr, exp: Expr) -> Expr {
        ExprNode::pow_raw(base, exp)
    }

    fn call(name: &str, args: Vec<Expr>) -> Expr {
        ExprNode::fn_raw(SymbolId::intern(name), args)
    }

    fn float(value: f64) -> Expr {
        ExprNode::num(Number::float(value))
    }

    #[test]
    fn simple_sum() {
        assert_eq!(to_latex(&sum(vec![x(), int(1)])), "x + 1");
    }

    #[test]
    fn subtraction_is_not_plus_a_negative() {
        // `x + (-1)·y` must print as `x - y`, never `x + -y`.
        let e = sum(vec![x(), product(vec![int(-1), y()])]);
        assert_eq!(to_latex(&e), "x - y");
    }

    #[test]
    fn negative_constant_renders_as_subtraction() {
        assert_eq!(to_latex(&sum(vec![x(), int(-5)])), "x - 5");
    }

    #[test]
    fn sum_can_lead_with_a_minus() {
        let e = sum(vec![product(vec![int(-3), x()]), int(1)]);
        assert_eq!(to_latex(&e), "-3x + 1");
    }

    #[test]
    fn nested_fractions() {
        let e = sum(vec![ratio(1, 2), product(vec![ratio(2, 3), x()])]);
        assert_eq!(to_latex(&e), "\\frac{1}{2} + \\frac{2}{3}x");
    }

    #[test]
    fn fraction_inside_a_denominator() {
        let e = power(ratio(2, 3), int(-1));
        assert_eq!(to_latex(&e), "\\frac{1}{\\frac{2}{3}}");
    }

    #[test]
    fn square_of_a_symbol() {
        assert_eq!(to_latex(&power(x(), int(2))), "x^{2}");
    }

    #[test]
    fn square_root() {
        assert_eq!(to_latex(&power(x(), ratio(1, 2))), "\\sqrt{x}");
    }

    #[test]
    fn square_root_of_a_sum() {
        // The braces of `\sqrt{…}` group, so no `\left(…\right)` is needed.
        let e = power(sum(vec![power(x(), int(2)), int(1)]), ratio(1, 2));
        assert_eq!(to_latex(&e), "\\sqrt{x^{2} + 1}");
    }

    #[test]
    fn cube_root() {
        assert_eq!(to_latex(&power(x(), ratio(1, 3))), "\\sqrt[3]{x}");
    }

    #[test]
    fn fourth_root_of_a_sum() {
        let e = power(sum(vec![power(x(), int(2)), int(1)]), ratio(1, 4));
        assert_eq!(to_latex(&e), "\\sqrt[4]{x^{2} + 1}");
    }

    #[test]
    fn negative_power_becomes_a_fraction() {
        assert_eq!(to_latex(&power(x(), int(-2))), "\\frac{1}{x^{2}}");
    }

    #[test]
    fn inverse_power_drops_the_exponent() {
        assert_eq!(to_latex(&power(x(), int(-1))), "\\frac{1}{x}");
    }

    #[test]
    fn rational_coefficient_product() {
        let e = product(vec![ratio(1, 2), x()]);
        assert_eq!(to_latex(&e), "\\frac{1}{2}x");
    }

    #[test]
    fn integer_coefficient_product() {
        assert_eq!(to_latex(&product(vec![int(2), x()])), "2x");
    }

    #[test]
    fn product_of_two_numbers_uses_cdot() {
        assert_eq!(to_latex(&product(vec![int(2), int(3)])), "2\\cdot 3");
    }

    #[test]
    fn product_of_two_symbols_uses_juxtaposition() {
        assert_eq!(to_latex(&product(vec![x(), y()])), "x y");
    }

    #[test]
    fn coefficient_times_a_radical() {
        let e = product(vec![int(2), power(x(), ratio(1, 2))]);
        assert_eq!(to_latex(&e), "2\\sqrt{x}");
    }

    #[test]
    fn sum_in_exponent_needs_no_parentheses() {
        let e = power(x(), sum(vec![int(1), y()]));
        assert_eq!(to_latex(&e), "x^{1 + y}");
    }

    #[test]
    fn sum_inside_product_is_parenthesized() {
        let e = product(vec![int(2), sum(vec![x(), int(1)])]);
        assert_eq!(to_latex(&e), "2\\left(x + 1\\right)");
    }

    #[test]
    fn product_as_power_base_is_parenthesized() {
        let e = power(product(vec![int(2), x()]), int(2));
        assert_eq!(to_latex(&e), "\\left(2x\\right)^{2}");
    }

    #[test]
    fn rational_as_power_base_is_parenthesized() {
        let e = power(ratio(1, 2), int(2));
        assert_eq!(to_latex(&e), "\\left(\\frac{1}{2}\\right)^{2}");
    }

    #[test]
    fn negative_number_as_power_base_is_parenthesized() {
        let e = power(int(-2), int(3));
        assert_eq!(to_latex(&e), "\\left(-2\\right)^{3}");
    }

    #[test]
    fn sine_function() {
        assert_eq!(to_latex(&call("sin", vec![x()])), "\\sin\\left(x\\right)");
    }

    #[test]
    fn logarithm_and_exponential() {
        let e = sum(vec![call("ln", vec![x()]), call("exp", vec![y()])]);
        assert_eq!(to_latex(&e), "\\ln\\left(x\\right) + \\exp\\left(y\\right)");
    }

    #[test]
    fn asin_alias_renders_as_arcsin() {
        assert_eq!(
            to_latex(&call("asin", vec![x()])),
            "\\arcsin\\left(x\\right)"
        );
    }

    #[test]
    fn unknown_function_uses_operatorname() {
        let e = call("frobnicate", vec![x()]);
        assert_eq!(to_latex(&e), "\\operatorname{frobnicate}\\left(x\\right)");
    }

    #[test]
    fn function_argument_with_sum_needs_no_extra_parens() {
        let e = call("sin", vec![sum(vec![x(), int(1)])]);
        assert_eq!(to_latex(&e), "\\sin\\left(x + 1\\right)");
    }

    #[test]
    fn pi_and_greek_letters() {
        assert_eq!(to_latex(&sym("pi")), "\\pi");
        let cases = [
            ("alpha", "\\alpha"),
            ("beta", "\\beta"),
            ("theta", "\\theta"),
            ("lambda", "\\lambda"),
            ("mu", "\\mu"),
            ("sigma", "\\sigma"),
            ("omega", "\\omega"),
            ("phi", "\\phi"),
            ("delta", "\\delta"),
        ];
        for (name, expected) in cases {
            assert_eq!(to_latex(&sym(name)), expected, "greek symbol {name}");
        }
    }

    #[test]
    fn single_and_multi_character_symbols() {
        assert_eq!(to_latex(&sym("x")), "x");
        assert_eq!(to_latex(&sym("velocity")), "\\mathrm{velocity}");
    }

    #[test]
    fn float_literals() {
        assert_eq!(to_latex(&float(2.5)), "2.5");
        // `2.0` must not collapse into the exact integer `2`.
        assert_eq!(to_latex(&float(2.0)), "2.0");
        assert_eq!(to_latex(&float(-1.5)), "-1.5");
        assert_eq!(to_latex(&product(vec![float(0.5), x()])), "0.5x");
    }

    #[test]
    fn unevaluated_integral() {
        let e = ExprNode::hold(Hold::Integral {
            body: power(x(), int(2)),
            var: SymbolId::intern("x"),
        });
        assert_eq!(to_latex(&e), "\\int x^{2}\\, dx");
    }

    #[test]
    fn integral_of_a_sum_is_grouped() {
        let e = ExprNode::hold(Hold::Integral {
            body: sum(vec![x(), int(1)]),
            var: SymbolId::intern("x"),
        });
        assert_eq!(to_latex(&e), "\\int \\left(x + 1\\right)\\, dx");
    }

    #[test]
    fn deeply_nested_expression() {
        // 1 / (2x + sqrt(x + 1))^2
        let root = power(sum(vec![x(), int(1)]), ratio(1, 2));
        let base = sum(vec![product(vec![int(2), x()]), root]);
        let e = power(base, int(-2));
        assert_eq!(
            to_latex(&e),
            "\\frac{1}{\\left(2x + \\sqrt{x + 1}\\right)^{2}}"
        );
    }

    #[test]
    fn quadratic_with_fractional_coefficients() {
        let e = sum(vec![
            product(vec![ratio(1, 2), power(x(), int(2))]),
            product(vec![int(-3), x()]),
            int(1),
        ]);
        assert_eq!(to_latex(&e), "\\frac{1}{2}x^{2} - 3x + 1");
    }

    #[test]
    fn product_of_trig_functions_uses_juxtaposition() {
        let e = product(vec![call("sin", vec![x()]), call("cos", vec![x()])]);
        assert_eq!(to_latex(&e), "\\sin\\left(x\\right) \\cos\\left(x\\right)");
    }

    #[test]
    fn normalized_polynomial_prints_canonically() {
        // Normalization sorts the constant first; the printer must follow the
        // tree rather than reorder it.
        let e = canlab_core::add(vec![
            canlab_core::mul(vec![int(2), power(x(), int(2))]),
            canlab_core::mul(vec![int(3), x()]),
            int(-1),
        ]);
        assert_eq!(to_latex(&e), "-1 + 2x^{2} + 3x");
    }

    #[test]
    fn normalized_square_is_a_power() {
        let e = canlab_core::mul(vec![x(), x()]);
        assert_eq!(to_latex(&e), "x^{2}");
    }

    #[test]
    fn empty_raw_collections_fall_back_to_identities() {
        // Canonical form never produces these; raw constructors can.
        assert_eq!(to_latex(&sum(Vec::new())), "0");
        assert_eq!(to_latex(&product(Vec::new())), "1");
    }

    #[test]
    fn multiple_function_arguments_are_comma_separated() {
        assert_eq!(
            to_latex(&call("log", vec![x(), y()])),
            "\\log\\left(x, y\\right)"
        );
    }

    #[test]
    fn negative_product_as_a_factor_is_delimited() {
        // `y -2x` would read as a subtraction; only canonical trees put the
        // coefficient first, so the printer must defend against this shape.
        let e = product(vec![y(), product(vec![int(-2), x()])]);
        assert_eq!(to_latex(&e), "y \\left(-2x\\right)");
    }

    #[test]
    fn sum_as_the_first_factor_is_delimited() {
        let e = product(vec![sum(vec![x(), int(1)]), int(2)]);
        assert_eq!(to_latex(&e), "\\left(x + 1\\right)\\cdot 2");
    }

    #[test]
    fn non_finite_floats_get_latex_spellings() {
        assert_eq!(to_latex(&float(f64::INFINITY)), "\\infty");
        assert_eq!(to_latex(&float(f64::NEG_INFINITY)), "-\\infty");
        assert_eq!(to_latex(&float(f64::NAN)), "\\mathrm{NaN}");
    }
}
