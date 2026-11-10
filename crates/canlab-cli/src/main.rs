//! `canlab` — command-line front-end and REPL.
//!
//! Everything interesting lives in the library crates; this binary is the
//! place where the pipeline is finally visible end to end:
//!
//! ```text
//! LaTeX text -> parser -> Expr -> (printer | solver | sampler) -> text/SVG
//! ```
//!
//! Two modes: one-shot subcommands for scripting, and a REPL for exploring.
//! The REPL is deliberately thin so the same code paths are exercised in both.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use canlab_core::{Expr, ExprNode, Hold, Program, SymbolId, expr::node};
use canlab_parse::parse_latex;
use canlab_plot::{SampleOptions, SvgOptions, sample, to_svg};
use canlab_solver::{NativeSolver, Solver};
use canlab_tex::to_latex;

const USAGE: &str = "\
canlab — a small LaTeX-native computer algebra system

USAGE:
    canlab                          start the REPL
    canlab eval   <latex>           parse and canonicalize, print plain text + LaTeX
    canlab tex    <latex>           print LaTeX only
    canlab plot   <latex> [opts]    render a plot to SVG
    canlab int    <latex> [opts]    integrate (uses SymPy when it is installed)
    canlab solver [--probe]         report the SymPy backend, or test it

OPTIONS:
    -o, --out <file>             output file for plot (default: plot.svg)
        --xmin <f> --xmax <f>    plot domain (default: -10 10)
    -v, --var <name>             variable for integration (default: guess)
        --probe                  really start the worker and make it answer

REPL COMMANDS:
    :help  :quit  :tex  :plot  :int  :solver
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        return match repl() {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("error: {err}");
                ExitCode::FAILURE
            }
        };
    }

    let result = match args[0].as_str() {
        "help" | "-h" | "--help" => {
            print!("{USAGE}");
            Ok(())
        }
        "eval" => eval_command(&args[1..]),
        "tex" => tex_command(&args[1..]),
        "plot" => plot_command(&args[1..]),
        "int" | "integrate" => integrate_command(&args[1..]),
        "solver" => solver_command(&args[1..]),
        other => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

// ---------------------------------------------------------------------------
// One-shot commands
// ---------------------------------------------------------------------------

fn eval_command(args: &[String]) -> Result<(), String> {
    let source = args.first().ok_or("eval needs a LaTeX expression")?;
    let expr = parse_or_err(source)?;
    println!("{}", node(&expr));
    println!("{}", to_latex(&expr));
    Ok(())
}

fn tex_command(args: &[String]) -> Result<(), String> {
    let source = args.first().ok_or("tex needs a LaTeX expression")?;
    let expr = parse_or_err(source)?;
    println!("{}", to_latex(&expr));
    Ok(())
}

fn plot_command(args: &[String]) -> Result<(), String> {
    let source = args.first().ok_or("plot needs a LaTeX expression")?;
    let expr = parse_or_err(source)?;

    let mut out = PathBuf::from("plot.svg");
    let mut x_min = -10.0_f64;
    let mut x_max = 10.0_f64;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--out" => {
                i += 1;
                out = PathBuf::from(args.get(i).ok_or("--out needs a path")?);
            }
            "--xmin" => {
                i += 1;
                x_min = parse_float(args.get(i), "--xmin")?;
            }
            "--xmax" => {
                i += 1;
                x_max = parse_float(args.get(i), "--xmax")?;
            }
            other => return Err(format!("unknown plot option `{other}`")),
        }
        i += 1;
    }

    if !(x_min < x_max) {
        return Err("--xmin must be less than --xmax".into());
    }

    let mut vars = Vec::new();
    free_symbols(&expr, &mut vars);
    match vars.len() {
        0 => return Err("that expression has no variable to plot".into()),
        1 => {}
        n => return Err(format!("plot needs exactly one variable, found {n}")),
    }

    let program = Program::compile(&expr, &vars)
        .ok_or("this expression cannot be evaluated numerically")?;
    let curve = sample(
        &program,
        &SampleOptions {
            x_min,
            x_max,
            ..SampleOptions::default()
        },
    );

    let (y_min, y_max) = vertical_extent(&curve).ok_or("the function has no finite values in that range")?;
    let svg = to_svg(
        &curve,
        &SvgOptions {
            width: 900,
            height: 600,
            x_range: (x_min, x_max),
            y_range: (y_min, y_max),
            stroke: "#1e88e5".to_string(),
            show_axes: true,
        },
    );

    std::fs::write(&out, svg).map_err(|e| format!("could not write {}: {e}", out.display()))?;
    println!(
        "wrote {} ({} segment{})",
        out.display(),
        curve.segments.len(),
        if curve.segments.len() == 1 { "" } else { "s" }
    );
    Ok(())
}

fn integrate_command(args: &[String]) -> Result<(), String> {
    let source = args.first().ok_or("int needs a LaTeX expression")?;
    let expr = parse_or_err(source)?;

    let mut var: Option<SymbolId> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-v" | "--var" => {
                i += 1;
                let name = args.get(i).ok_or("--var needs a name")?;
                var = Some(SymbolId::intern(name));
            }
            other => return Err(format!("unknown int option `{other}`")),
        }
        i += 1;
    }

    let var = match var {
        Some(v) => v,
        None => {
            let mut vars = Vec::new();
            free_symbols(&expr, &mut vars);
            *vars.first().ok_or("no variable to integrate with respect to")?
        }
    };

    // The best backend the machine offers: SymPy when it is installed, the native
    // one otherwise. Declining is still a normal answer, and the unevaluated
    // integral is printed either way.
    let solver = backend();
    match solver.integrate(&expr, var) {
        Ok(result) => {
            println!("{}", to_latex(&result));
            Ok(())
        }
        Err(err) => Err(format!(
            "the solver could not integrate this ({err}); the unevaluated form is\n  {}",
            to_latex(&ExprNode::hold(Hold::Integral {
                body: expr,
                var
            }))
        )),
    }
}

// ---------------------------------------------------------------------------
// The backend
// ---------------------------------------------------------------------------

/// The strongest backend this machine can run.
///
/// SymPy is the real thing but it needs a Python installation; the native backend
/// needs nothing and covers school algebra. Choosing here rather than at each call
/// site means the REPL, `int` and the GUI all make the same choice.
fn backend() -> Box<dyn Solver> {
    match canlab_solver::sympy::open() {
        Ok(worker) => Box::new(worker),
        Err(_) => Box::new(NativeSolver),
    }
}

/// `canlab solver` — what the SymPy backend situation is, and optionally a real
/// round trip through the worker.
fn solver_command(args: &[String]) -> Result<(), String> {
    if args.iter().any(|arg| arg == "--probe") {
        return canlab_solver::probe().map(|message| println!("{message}"));
    }
    println!("{}", canlab_solver::discovery().describe());
    println!("The native backend is always available: linear and quadratic equations,");
    println!("polynomial calculus, Gaussian elimination for linear systems.");
    Ok(())
}

// ---------------------------------------------------------------------------
// REPL
// ---------------------------------------------------------------------------

fn repl() -> Result<(), String> {
    println!("canlab — type LaTeX, or :help. Ctrl-D or :quit to leave.");
    let stdin = io::stdin();
    let mut line = String::new();

    loop {
        print!("canlab> ");
        io::stdout().flush().map_err(|e| e.to_string())?;
        line.clear();
        if stdin.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            println!();
            return Ok(());
        }
        let input = line.trim();
        if input.is_empty() {
            continue;
        }

        if let Some(rest) = input.strip_prefix(':') {
            let (command, argument) = match rest.split_once(char::is_whitespace) {
                Some((c, a)) => (c, a.trim()),
                None => (rest, ""),
            };
            match command {
                "q" | "quit" | "exit" => return Ok(()),
                "h" | "help" => print!("{USAGE}"),
                "tex" => report(tex_command(&[argument.to_string()])),
                "plot" => report(plot_command(&[argument.to_string()])),
                "int" | "integrate" => report(integrate_command(&[argument.to_string()])),
                "solver" => report(solver_command(&[argument.to_string()])),
                other => eprintln!("unknown command `:{other}` (try :help)"),
            }
            continue;
        }

        match parse_latex(input) {
            Ok(expr) => {
                println!("  plain  {}", node(&expr));
                println!("  latex  {}", to_latex(&expr));
            }
            Err(err) => eprintln!("  parse error: {err}"),
        }
    }
}

fn report(result: Result<(), String>) {
    if let Err(message) = result {
        eprintln!("  {message}");
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_or_err(source: &str) -> Result<Expr, String> {
    parse_latex(source).map_err(|e| e.to_string())
}

fn parse_float(value: Option<&String>, flag: &str) -> Result<f64, String> {
    value
        .ok_or_else(|| format!("{flag} needs a number"))?
        .parse::<f64>()
        .map_err(|_| format!("{flag} needs a number"))
}

/// Collect free variables in first-seen order, ignoring known constants.
fn free_symbols(expr: &Expr, out: &mut Vec<SymbolId>) {
    match &**expr {
        ExprNode::Num(_) => {}
        ExprNode::Sym(s) => {
            let name = s.as_str();
            let is_constant = matches!(name.as_str(), "pi" | "e" | "tau");
            if !is_constant && !out.contains(s) {
                out.push(*s);
            }
        }
        ExprNode::Add(items) | ExprNode::Mul(items) | ExprNode::Fn(_, items) => {
            for item in items {
                free_symbols(item, out);
            }
        }
        ExprNode::Pow(base, exponent) => {
            free_symbols(base, out);
            free_symbols(exponent, out);
        }
        ExprNode::Hold(hold) => match &**hold {
            Hold::Integral { body, .. } => free_symbols(body, out),
        },
    }
}

/// Vertical extent of the plot, padded and made non-degenerate so a constant
/// function still renders as a line rather than a divide-by-zero.
fn vertical_extent(curve: &canlab_plot::Curve) -> Option<(f64, f64)> {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for segment in &curve.segments {
        for point in segment {
            let y = point[1];
            if y.is_finite() {
                min = min.min(y);
                max = max.max(y);
            }
        }
    }
    if !min.is_finite() || !max.is_finite() {
        return None;
    }
    let span = max - min;
    let pad = if span.abs() < 1e-9 { 1.0 } else { span * 0.1 };
    Some((min - pad, max + pad))
}
