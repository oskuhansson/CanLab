//! W7.1: an expression as Lua source, and the refusals that keep it honest.
//!
//! The GPU workstream (W7) compiles a *kernel* — a small Lua program — to WGSL.
//! The first step is turning an `Expr` the CAS already has into Lua that means
//! the same thing, and that is this module: a printer, not an interpreter.
//!
//! # The mapping, and why each part of it
//!
//! * **Numbers** — `2`, `1/2`, `0.5`. An exact rational is printed as a division
//!   (`1/2`) so that Lua's `f64` arithmetic keeps the value; a float is printed
//!   with its decimal point.
//! * **Symbols** — a variable is a Lua local; `pi` and `e` are `math.pi` and
//!   `math.exp(1)`. A symbol that is neither is a *refusal*, because a Lua
//!   program with a free variable has no meaning.
//! * **Operators** — `+ − * /` map directly; `^` is Lua's `^`, which is
//!   exponentiation. Unary minus is `-`.
//! * **Functions** — `sin`, `cos`, `tan`, `exp`, `ln`, `sqrt`, `abs` map to
//!   `math.sin` and friends. `ln` is `math.log`, which is Lua's natural
//!   logarithm.
//! * **A `Hold`** — an unevaluated integral or a limit — is a **named refusal**.
//!   It is not a number, and printing it as one would be the worst outcome.
//!
//! # What is refused, and why a refusal is the answer
//!
//! A function the subset cannot express (`arcsin`, `sinh`, a user-defined one),
//! a `Hold`, and a free symbol are each refused with the construct's name. The
//! alternative — printing something plausible — is a kernel that compiles and
//! computes a different function, which is the failure this project treats as
//! the worst kind.
//!
//! # What this module does not do
//!
//! It does not run the Lua. `mlua` is not a dependency of this workspace (no
//! network, no vendored Lua), so the agreement with [`canlab_core::Program`] is
//! checked by *evaluating the emitted text with a small interpreter written in
//! the test* — which is not Lua, but is a second implementation of the same
//! arithmetic, and a disagreement between the two is the bug this test exists
//! to find. The real `mlua` comparison is W7.3's job, on a machine that has it.

use crate::expr::{Expr, ExprNode};
use crate::number::Number;
use crate::symbol::SymbolId;

/// Why an expression cannot be printed as Lua.
#[derive(Debug, Clone, PartialEq)]
pub enum LuaError {
    /// A function the subset does not express.
    UnsupportedFunction(String),
    /// A construct with no value: an unevaluated integral.
    Hold(String),
    /// A symbol that is neither a variable nor a constant.
    FreeSymbol(String),
    /// A number that is not finite.
    NotFinite,
}

impl LuaError {
    /// The message a caller shows.
    pub fn message(&self) -> String {
        match self {
            LuaError::UnsupportedFunction(name) => {
                format!("`{name}` is not in the Lua subset this kernel compiler reads")
            }
            LuaError::Hold(what) => format!("`{what}` has no value to compile"),
            LuaError::FreeSymbol(name) => {
                format!("`{name}` is not a variable of this kernel and has no value")
            }
            LuaError::NotFinite => "a non-finite number has no Lua literal".to_string(),
        }
    }
}

/// Print `expression` as a Lua expression, with `variables` as its locals.
///
/// The result is an expression, not a statement: the caller assigns it. Every
/// symbol must be in `variables`, or be `pi` or `e`.
pub fn to_lua(expression: &Expr, variables: &[SymbolId]) -> Result<String, LuaError> {
    let mut out = String::new();
    write(expression, variables, &mut out)?;
    Ok(out)
}

/// A whole program: `local <name> = <expression>`.
pub fn to_lua_program(
    name: &str,
    expression: &Expr,
    variables: &[SymbolId],
) -> Result<String, LuaError> {
    let body = to_lua(expression, variables)?;
    Ok(format!("local {name} = {body}\nreturn {name}\n"))
}

fn write(expression: &Expr, variables: &[SymbolId], out: &mut String) -> Result<(), LuaError> {
    match &**expression {
        ExprNode::Num(number) => {
            out.push_str(&number_literal(number)?);
            Ok(())
        }
        ExprNode::Sym(symbol) => {
            let name = SymbolId::name(*symbol);
            match &*name {
                "pi" => out.push_str("math.pi"),
                "e" => out.push_str("math.exp(1)"),
                _ => {
                    if !variables.contains(symbol) {
                        return Err(LuaError::FreeSymbol(name.to_string()));
                    }
                    out.push_str(&name);
                }
            }
            Ok(())
        }
        ExprNode::Add(terms) => {
            if terms.is_empty() {
                out.push('0');
                return Ok(());
            }
            // Parenthesised as a whole: the caller may multiply it, and Lua's
            // precedence is not this printer's to rely on.
            out.push('(');
            for (index, term) in terms.iter().enumerate() {
                if index > 0 {
                    out.push_str(" + ");
                }
                write(term, variables, out)?;
            }
            out.push(')');
            Ok(())
        }
        ExprNode::Mul(factors) => {
            if factors.is_empty() {
                out.push('1');
                return Ok(());
            }
            out.push('(');
            for (index, factor) in factors.iter().enumerate() {
                if index > 0 {
                    out.push_str(" * ");
                }
                write(factor, variables, out)?;
            }
            out.push(')');
            Ok(())
        }
        ExprNode::Pow(base, exponent) => {
            // `^` is right-associative in Lua and so is a power in the CAS, but
            // the base is parenthesised anyway: `(-2)^2` and `-2^2` differ.
            out.push('(');
            write(base, variables, out)?;
            out.push_str(" ^ ");
            write(exponent, variables, out)?;
            out.push(')');
            Ok(())
        }
        ExprNode::Fn(name, args) => {
            let function = SymbolId::name(*name);
            let mapped = match &*function {
                "sin" => "math.sin",
                "cos" => "math.cos",
                "tan" => "math.tan",
                "exp" => "math.exp",
                // Lua's `math.log` is the natural logarithm; `math.log(x, 10)`
                // is the base-ten one. The CAS's `ln` is natural.
                "ln" => "math.log",
                "sqrt" => "math.sqrt",
                "abs" => "math.abs",
                "arcsin" | "arccos" | "arctan" | "sinh" | "cosh" | "tanh" => {
                    return Err(LuaError::UnsupportedFunction(function.to_string()))
                }
                other => return Err(LuaError::UnsupportedFunction(other.to_string())),
            };
            out.push_str(mapped);
            out.push('(');
            for (index, argument) in args.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                write(argument, variables, out)?;
            }
            out.push(')');
            Ok(())
        }
        ExprNode::Hold(hold) => {
            let what = match &**hold {
                crate::expr::Hold::Integral { .. } => "an unevaluated integral",
                // An accent is a decoration of a name, and a kernel computes
                // numbers; there is nothing to emit for `\vec{v}`.
                crate::expr::Hold::Accent { .. } => "an accent",
                // A matrix is not a number either.
                crate::expr::Hold::Matrix { .. } => "a matrix",
            };
            Err(LuaError::Hold(what.to_string()))
        }
    }
}

/// A number as a Lua literal.
///
/// An exact integer is itself; an exact rational is a division, so that Lua's
/// `f64` arithmetic gets the same value; a float keeps its decimal point so that
/// `2.0` does not become an integer in a language that has one.
fn number_literal(number: &Number) -> Result<String, LuaError> {
    match number {
        Number::Int(value) => Ok(value.to_string()),
        Number::Rat(numerator, denominator) => Ok(format!("({numerator} / {denominator})")),
        Number::Float(value) => {
            if !value.is_finite() {
                return Err(LuaError::NotFinite);
            }
            // A float that happens to be an integer still needs a point, or a
            // reader cannot tell it from an exact integer.
            let text = format!("{value}");
            if text.contains('.') || text.contains('e') || text.contains('E') {
                Ok(text)
            } else {
                Ok(format!("{text}.0"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{add, int, mul, pow, ratio, sym};

    /// Evaluate a Lua expression this module emitted, for the subset it emits.
    ///
    /// This is **not** Lua: it is a small recursive-descent evaluator for the
    /// exact grammar [`to_lua`] produces — parenthesised sums, products and
    /// powers, `math.*` calls, and numeric literals. It exists so that the
    /// agreement with `Program::eval` can be checked without `mlua`, and it is
    /// deliberately independent of the printer: it parses the *text*, so a
    /// printer that emits the wrong operator fails here.
    fn eval_lua(text: &str, variables: &[(SymbolId, f64)]) -> f64 {
        struct Parser<'a> {
            text: &'a str,
            position: usize,
            variables: &'a [(SymbolId, f64)],
        }
        impl Parser<'_> {
            fn skip(&mut self) {
                while self.text[self.position..].starts_with(' ') {
                    self.position += 1;
                }
            }
            fn peek(&mut self) -> Option<char> {
                self.skip();
                self.text[self.position..].chars().next()
            }
            fn eat(&mut self, want: char) -> bool {
                if self.peek() == Some(want) {
                    self.position += want.len_utf8();
                    true
                } else {
                    false
                }
            }
            fn expect(&mut self, want: char) {
                assert!(self.eat(want), "expected {want:?} at {}", self.position);
            }
            fn expression(&mut self) -> f64 {
                self.term()
            }
            fn term(&mut self) -> f64 {
                let mut value = self.factor();
                loop {
                    if self.eat('+') {
                        value += self.factor();
                    } else if self.eat('-') {
                        value -= self.factor();
                    } else if self.eat('*') {
                        value *= self.factor();
                    } else if self.eat('/') {
                        value /= self.factor();
                    } else {
                        return value;
                    }
                }
            }
            fn factor(&mut self) -> f64 {
                // A power binds tighter than a product: `x ^ 2` is one factor,
                // and the printer emits `(x ^ 2)` with the parentheses already
                // around it.
                let base = self.atom();
                if self.eat('^') {
                    return base.powf(self.factor());
                }
                base
            }
            fn atom(&mut self) -> f64 {
                if self.eat('(') {
                    let value = self.expression();
                    self.expect(')');
                    return value;
                }
                if self.eat('-') {
                    return -self.factor();
                }
                // A number.
                let start = self.position;
                while let Some(c) = self.text[self.position..].chars().next() {
                    if c.is_ascii_digit() || c == '.' {
                        self.position += 1;
                    } else {
                        break;
                    }
                }
                if self.position > start {
                    return self.text[start..self.position].parse().expect("a number");
                }
                // A name: a variable or `math.*`.
                let start = self.position;
                while let Some(c) = self.text[self.position..].chars().next() {
                    if c.is_alphanumeric() || c == '_' || c == '.' {
                        self.position += 1;
                    } else {
                        break;
                    }
                }
                let name = &self.text[start..self.position];
                // `math.pi` is a constant and takes no argument; it must be
                // checked before the `math.` function branch, or the parser
                // demands a `(` that is not there.
                if name == "math.pi" {
                    return std::f64::consts::PI;
                }
                if let Some(function) = name.strip_prefix("math.") {
                    self.expect('(');
                    let argument = self.expression();
                    self.expect(')');
                    return match function {
                        "sin" => argument.sin(),
                        "cos" => argument.cos(),
                        "tan" => argument.tan(),
                        "exp" => argument.exp(),
                        "log" => argument.ln(),
                        "sqrt" => argument.sqrt(),
                        "abs" => argument.abs(),
                        other => panic!("unknown math function {other}"),
                    };
                }
                let symbol = SymbolId::intern(name);
                self.variables
                    .iter()
                    .find(|(variable, _)| *variable == symbol)
                    .map(|(_, value)| *value)
                    .unwrap_or_else(|| panic!("free variable {name}"))
            }
        }
        let mut parser = Parser {
            text,
            position: 0,
            variables,
        };
        let value = parser.expression();
        assert_eq!(parser.position, text.len(), "trailing text in {text}");
        value
    }

    #[test]
    fn a_printed_expression_agrees_with_the_evaluator() {
        // The roadmap's check, in the form this workspace can run it: the Lua
        // text, evaluated by an independent parser, agrees with `Program::eval`
        // to `1e-10` — far tighter than the `1e-1` the roadmap asks for, because
        // both sides are `f64` and the arithmetic is the same arithmetic.
        let x = SymbolId::intern("x");
        let y = SymbolId::intern("y");
        let cases: Vec<Expr> = vec![
            add([pow(sym("x"), int(2)), mul([int(3), sym("y")]), int(-1)]),
            mul([ratio(1, 2), pow(sym("x"), int(2))]),
            add([
                ExprNode::fn_raw(SymbolId::intern("sin"), vec![sym("x")]),
                ExprNode::fn_raw(SymbolId::intern("cos"), vec![sym("y")]),
            ]),
            ExprNode::fn_raw(SymbolId::intern("exp"), vec![mul([int(-1), sym("x")])]),
            ExprNode::fn_raw(SymbolId::intern("sqrt"), vec![add([pow(sym("y"), int(2)), int(1)])]),
            ExprNode::fn_raw(SymbolId::intern("ln"), vec![sym("x")]),
            pow(add([sym("x"), int(1)]), int(3)),
            // `pi` and `e` as constants.
            mul([sym("pi"), sym("x")]),
        ];
        for expression in cases {
            let lua = to_lua(&expression, &[x, y]).expect("a Lua expression");
            let program =
                crate::Program::compile(&expression, &[x, y]).expect("a program");
            for (x_value, y_value) in [(0.5, 1.5), (2.0, 0.25), (1.0, 3.0)] {
                let expected = program.eval(&[x_value, y_value]);
                let computed = eval_lua(&lua, &[(x, x_value), (y, y_value)]);
                assert!(
                    (computed - expected).abs() < 1e-10,
                    "{lua}: {computed} vs {expected} at ({x_value}, {y_value})"
                );
            }
        }
    }

    #[test]
    fn the_operators_are_spelled_as_luas() {
        // A sum, a product and a power, each parenthesised.
        let x = SymbolId::intern("x");
        let expression = add([pow(sym("x"), int(2)), int(1)]);
        // The canonicaliser sorts the terms, so the order is `1 + x^2`; the
        // test asserts the *shape* and not the order.
        let lua = to_lua(&expression, &[x]).expect("a value");
        assert!(lua.contains("x ^ 2"), "{lua}");
        assert!(lua.starts_with('(') && lua.ends_with(')'), "{lua}");
        // A rational is a division, so Lua's `f64` gets the same value.
        let expression = ratio(1, 2);
        assert_eq!(to_lua(&expression, &[]).expect("a value"), "(1 / 2)");
        // A float keeps its point.
        let expression = ExprNode::num(Number::float(2.0));
        assert_eq!(to_lua(&expression, &[]).expect("a value"), "2.0");
        // `pi` and `e`.
        assert_eq!(to_lua(&sym("pi"), &[]).expect("a value"), "math.pi");
        assert_eq!(to_lua(&sym("e"), &[]).expect("a value"), "math.exp(1)");
    }

    #[test]
    fn a_hold_is_a_named_refusal() {
        // An unevaluated integral has no value, and the refusal names it.
        let integral = ExprNode::hold(crate::expr::Hold::Integral {
            body: sym("x"),
            var: SymbolId::intern("x"),
        });
        let error = to_lua(&integral, &[SymbolId::intern("x")]).expect_err("a refusal");
        assert!(matches!(error, LuaError::Hold(_)), "{error:?}");
        assert!(error.message().contains("integral"), "{}", error.message());
    }

    #[test]
    fn a_function_outside_the_subset_is_refused_by_name() {
        let x = SymbolId::intern("x");
        let expression = ExprNode::fn_raw(SymbolId::intern("arcsin"), vec![sym("x")]);
        let error = to_lua(&expression, &[x]).expect_err("a refusal");
        assert_eq!(
            error,
            LuaError::UnsupportedFunction(String::from("arcsin"))
        );
        assert!(error.message().contains("arcsin"), "{}", error.message());
        // A user-defined function the same way.
        let expression = ExprNode::fn_raw(SymbolId::intern("f"), vec![sym("x")]);
        assert!(matches!(
            to_lua(&expression, &[x]),
            Err(LuaError::UnsupportedFunction(_))
        ));
    }

    #[test]
    fn a_free_symbol_is_refused() {
        // `z` is not a variable of this kernel, so the program would have a
        // free variable and no meaning.
        let x = SymbolId::intern("x");
        let expression = add([sym("x"), sym("z")]);
        let error = to_lua(&expression, &[x]).expect_err("a refusal");
        assert_eq!(error, LuaError::FreeSymbol(String::from("z")));
    }

    #[test]
    fn a_whole_program_is_a_local_and_a_return() {
        let x = SymbolId::intern("x");
        let program = to_lua_program("f", &pow(sym("x"), int(2)), &[x]).expect("a program");
        assert!(program.starts_with("local f = "), "{program}");
        assert!(program.trim_end().ends_with("return f"), "{program}");
    }
}
