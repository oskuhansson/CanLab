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
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use canlab_core::{Expr, ExprNode, Hold, Program, SymbolId, expr::node};
use canlab_corpus::run::{Provenance, RowResult, RunReport, run_batched};
use canlab_corpus::{LoadError, Outcome, Row, batch, fetch, load_rows, report};
use canlab_parse::parse_latex;
use canlab_plot::{SampleOptions, SvgOptions, sample, to_svg};
use canlab_solver::{NativeSolver, Solver};
use canlab_tex::to_latex;

mod bench;

const USAGE: &str = "\
canlab — a small LaTeX-native computer algebra system

USAGE:
    canlab                          start the REPL
    canlab eval   <latex>           parse and canonicalize, print plain text + LaTeX
    canlab tex    <latex>           print LaTeX only
    canlab plot   <latex> [opts]    render a plot to SVG
    canlab int    <latex> [opts]    integrate (uses SymPy when it is installed)
    canlab solve-all <latex> [opts] solve <latex> = 0 and print the solution set
    canlab solver [--probe]         report the SymPy backend, or test it
    canlab bench  [--write]         measure the evaluator, the sampler and the tracer
    canlab corpus run [opts]        run the exam corpus and write the coverage report
    canlab corpus fetch [name]      download the cited datasets (olympiadbench, ugphysics, scibench)

OPTIONS:
    -o, --out <file>             output file for plot (default: plot.svg)
        --xmin <f> --xmax <f>    plot domain (default: -10 10)
    -v, --var <name>             variable for integration or solve-all (default: guess)
        --probe                  really start the worker and make it answer
        --write                  (bench) write docs/bench.md instead of stdout
        --tag <tag>              (corpus run) only capabilities equal to `tag` or starting `tag.`
        --batch <n>              (corpus run) rows per child process (default: 50)
        --update                 (corpus run) re-fetch the datasets and check `hf:` rows

CORPUS ENVIRONMENT:
    CANLAB_CORPUS_HOME=<dir>     cache root (default: %LOCALAPPDATA%\\canlab\\corpus)
    CANLAB_CORPUS_MIRROR=<dir>   read a local mirror instead of the network

REPL COMMANDS:
    :help  :quit  :tex  :plot  :int  :solve-all  :solver
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
            return ExitCode::SUCCESS;
        }
        "corpus" => return corpus_command(&args[1..]),
        "bench" => return bench_command(&args[1..]),
        "eval" => eval_command(&args[1..]),
        "tex" => tex_command(&args[1..]),
        "plot" => plot_command(&args[1..]),
        "int" | "integrate" => integrate_command(&args[1..]),
        "solve-all" => solve_all_command(&args[1..]),
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

/// `canlab solve-all` — solve `<latex> = 0` and print the whole solution set.
///
/// The argument is the left-hand side with zero on the right, the same
/// convention `int` uses for its integrand: `x^2 - 4` means `x^2 - 4 = 0`.
fn solve_all_command(args: &[String]) -> Result<(), String> {
    let source = args.first().ok_or("solve-all needs a LaTeX expression")?;
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
            other => return Err(format!("unknown solve-all option `{other}`")),
        }
        i += 1;
    }

    let var = match var {
        Some(v) => v,
        None => {
            let mut vars = Vec::new();
            free_symbols(&expr, &mut vars);
            *vars.first().ok_or("no variable to solve for")?
        }
    };

    // The answer type, not a list of roots: "no solution", "every number", a
    // family and an interval all have a LaTeX form, and this is the one place
    // the CLI can show all of them.
    match backend().solve_all(&expr, var) {
        Ok(solutions) => {
            println!("{}", solutions.to_latex());
            Ok(())
        }
        Err(err) => Err(format!(
            "the solver could not give a solution set ({err}); the equation is\n  {} = 0",
            to_latex(&expr)
        )),
    }
}

// ---------------------------------------------------------------------------
// The backend
// ---------------------------------------------------------------------------

/// The strongest backend this machine can run, and the name the report records.
///
/// SymPy is the real thing but it needs a Python installation; the native
/// backend needs nothing and covers school algebra. Choosing here rather than at
/// each call site means the REPL, `int`, `solve-all`, `corpus` and the GUI all
/// make the same choice.
fn backend_choice() -> (Box<dyn Solver>, &'static str) {
    match canlab_solver::sympy::open() {
        Ok(worker) => (Box::new(worker), "sympy"),
        Err(_) => (Box::new(NativeSolver), "native"),
    }
}

fn backend() -> Box<dyn Solver> {
    backend_choice().0
}

// ---------------------------------------------------------------------------
// canlab bench (W0.3)
// ---------------------------------------------------------------------------

fn bench_command(args: &[String]) -> ExitCode {
    let mut write = false;
    for argument in args {
        match argument.as_str() {
            "--write" => write = true,
            other => {
                eprintln!("error: unknown bench option `{other}`");
                return ExitCode::from(2);
            }
        }
    }
    match bench::run(write) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

// ---------------------------------------------------------------------------
// canlab corpus (W0.4)
// ---------------------------------------------------------------------------

/// Where a run keeps the generated report. Fixed by `docs/corpus.md`.
const REPORT: &str = "docs/abimix-coverage.md";

fn corpus_command(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("run") => corpus_run(&args[1..]),
        Some("fetch") => corpus_fetch(&args[1..]),
        // Internal: the child half of `--batch`. Not in the help text because
        // nobody types it; the parent starts it.
        Some("exec") => corpus_exec(&args[1..]),
        Some(other) => {
            eprintln!("error: unknown corpus command `{other}`\n\n{USAGE}");
            ExitCode::from(2)
        }
        None => {
            eprintln!("error: `canlab corpus` needs `run`, `fetch` or `exec`\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn corpus_run(args: &[String]) -> ExitCode {
    let mut tag: Option<String> = None;
    let mut batch_size = batch::DEFAULT_BATCH;
    let mut update = false;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--tag" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    eprintln!("error: --tag needs a capability");
                    return ExitCode::from(2);
                };
                tag = Some(value.clone());
            }
            "--batch" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    eprintln!("error: --batch needs a count");
                    return ExitCode::from(2);
                };
                match value.parse::<usize>() {
                    Ok(count) if count >= 1 => batch_size = count,
                    _ => {
                        eprintln!("error: --batch needs a positive whole number, not {value:?}");
                        return ExitCode::from(2);
                    }
                }
            }
            "--update" => update = true,
            other => {
                eprintln!("error: unknown corpus run option `{other}`");
                return ExitCode::from(2);
            }
        }
        index += 1;
    }

    let cases = Path::new("tests/cases");
    let (mut rows, mut load_errors) = load_rows(cases);
    for error in &load_errors {
        eprintln!("load error: {error}");
    }

    if let Some(tag) = &tag {
        rows.retain(|row| canlab_corpus::row::matches_tag(&row.capability, tag));
        if rows.is_empty() {
            eprintln!("error: no row matched --tag {tag:?}");
            return ExitCode::from(2);
        }
    } else if rows.is_empty() && load_errors.is_empty() {
        eprintln!("error: no rows under {}", cases.display());
        return ExitCode::from(2);
    }

    let (_solver, backend) = backend_choice();
    let generated = bench::utc_now();

    // `--update` is a provenance check, never a rewrite: an expectation that
    // disagrees with upstream is a failed row, not a corrected one.
    let mut provenance = Provenance::Unchecked;
    let mut drift: Vec<(PathBuf, String)> = Vec::new();
    if update {
        let source = corpus_source();
        match fetch::check(&rows, source.as_ref(), &fetch::default_cache_root(), &generated) {
            Ok(outcome) => {
                for (path, detail) in &outcome.missing {
                    eprintln!("provenance: {}: {detail}", canlab_corpus::row::path_text(&path));
                }
                if outcome.missing.is_empty() && outcome.drift.is_empty() {
                    provenance = Provenance::Checked;
                } else if outcome.missing.is_empty() {
                    provenance = Provenance::Checked;
                    drift = outcome.drift;
                } else {
                    provenance = Provenance::Failed(format!(
                        "{} cited row(s) were not found upstream",
                        outcome.missing.len()
                    ));
                }
            }
            Err(reason) => {
                eprintln!("error: --update could not check the datasets: {reason}");
                return ExitCode::from(2);
            }
        }
    }

    let command = match batch::ExecCommand::current_exe() {
        Ok(command) => command.with_backend(backend),
        Err(reason) => {
            eprintln!("error: {reason}");
            return ExitCode::from(2);
        }
    };
    let outcomes = run_batched(&rows, &command, batch_size, batch::DEFAULT_DEADLINE);

    let mut ran: Vec<(Row, Outcome)> = Vec::new();
    for (row, result) in rows.into_iter().zip(outcomes) {
        match result {
            RowResult::Outcome(outcome) => ran.push((row, outcome)),
            RowResult::LoadError(reason) => load_errors.push(LoadError {
                path: row.path.clone(),
                reason,
            }),
        }
    }

    for (path, detail) in drift {
        if let Some((_, outcome)) = ran.iter_mut().find(|(row, _)| row.path == path)
            && outcome.is_pass()
        {
            *outcome = Outcome::Fail {
                got: detail,
                reason: Some(String::from(
                    "--update: the upstream answer disagrees with `expect`",
                )),
            };
        }
    }

    for (row, outcome) in &ran {
        println!("{}", row_line(row, outcome));
    }

    let report = RunReport {
        rows: ran,
        load_errors,
        backend: backend.to_string(),
        tag,
        provenance,
    };
    if let Err(message) = report::write(&report, &generated, Path::new(REPORT)) {
        eprintln!("error: {message}");
        return ExitCode::FAILURE;
    }
    println!(
        "{} row(s): {} passed, {} failed, {} refusal row(s); report {REPORT}",
        report.rows.len(),
        report.passes(),
        report.failures(),
        report.refusals()
    );
    ExitCode::from(report.exit_code())
}

/// One line per row: status, file, expected, got.
fn row_line(row: &Row, outcome: &Outcome) -> String {
    let status = match outcome {
        Outcome::Pass {
            unit_unchecked: true,
            ..
        } => "PASS (unit unchecked)",
        Outcome::Pass { .. } => "PASS",
        Outcome::Fail { .. } => "FAIL",
    };
    let mut line = format!(
        "{status}  {}  expected {}  got {}",
        canlab_corpus::row::path_text(&row.path),
        row.expect.describe(),
        outcome.got()
    );
    if let Outcome::Fail {
        reason: Some(reason), ..
    } = outcome
    {
        line.push_str(&format!(" ({reason})"));
    }
    line
}

fn corpus_fetch(args: &[String]) -> ExitCode {
    let datasets = match args {
        [] => fetch::Dataset::ALL.to_vec(),
        [name] => match fetch::Dataset::parse(name) {
            Some(dataset) => vec![dataset],
            None => {
                eprintln!(
                    "error: unknown dataset {name:?}; choose olympiadbench, ugphysics or scibench"
                );
                return ExitCode::from(2);
            }
        },
        _ => {
            eprintln!("error: `canlab corpus fetch` takes one dataset name, or none for all three");
            return ExitCode::from(2);
        }
    };

    let source = corpus_source();
    let cache = fetch::default_cache_root();
    let generated = bench::utc_now();
    match fetch::fetch_with(source.as_ref(), &cache, &datasets, &generated) {
        Ok(fetched) => {
            for dataset in fetched {
                println!(
                    "fetched {} {}: {} file(s), {} bytes -> {}",
                    dataset.dataset.name(),
                    dataset.revision,
                    dataset.files.len(),
                    dataset.bytes(),
                    dataset.directory.display()
                );
            }
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::from(2)
        }
    }
}

/// The child half of a batch: read paths, run them, write one JSON line each.
fn corpus_exec(args: &[String]) -> ExitCode {
    let pinned = match args {
        [] => None,
        [flag, name] if flag.as_str() == "--backend" => Some(name.as_str()),
        _ => {
            eprintln!("error: `canlab corpus exec` takes only `--backend native|sympy`");
            return ExitCode::from(2);
        }
    };
    let solver: Box<dyn Solver> = match pinned {
        None => backend_choice().0,
        Some("native") => Box::new(NativeSolver),
        Some("sympy") => match canlab_solver::sympy::open() {
            Ok(worker) => Box::new(worker),
            Err(reason) => {
                eprintln!("error: the parent asked for SymPy but it is unavailable: {reason}");
                return ExitCode::from(2);
            }
        },
        Some(other) => {
            eprintln!("error: unknown backend {other:?}; choose native or sympy");
            return ExitCode::from(2);
        }
    };
    match batch::exec_loop(&*solver) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::from(2)
        }
    }
}

/// A local mirror when the environment names one, HTTPS otherwise. The mirror
/// is what lets the fetch be tested, and what an offline checkout can use.
fn corpus_source() -> Box<dyn fetch::Source> {
    match std::env::var_os("CANLAB_CORPUS_MIRROR") {
        Some(root) => Box::new(fetch::LocalSource::new(PathBuf::from(root))),
        None => Box::new(fetch::HttpSource::curl()),
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
                "solve-all" => report(solve_all_command(&[argument.to_string()])),
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
