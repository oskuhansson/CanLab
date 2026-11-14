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
use canlab_solver::{NativeSolver, Solutions, Solver};
use canlab_tex::to_latex;

mod bench;

const USAGE: &str = "\
canlab — a small LaTeX-native computer algebra system

USAGE:
    canlab                          start the REPL
    canlab eval   <latex>           parse and canonicalize, print plain text + LaTeX
    canlab tex    <latex>           print LaTeX only
    canlab plot   <latex> [opts]    render a plot to SVG
    canlab sign   <latex> [opts]    the sign table of f' on an interval, shaded in the SVG
    canlab int    <latex> [opts]    integrate (uses SymPy when it is installed)
    canlab solve-all <latex> [opts] solve <latex> = 0 and print the solution set
    canlab compile <latex> [opts]   print the Lua and the WGSL a kernel would be compiled from
    canlab compile-sweep [opts]     the W7.3 validation sweep, compiled but not run on a GPU
    canlab solver [--probe]         report the SymPy backend, or test it
    canlab bench  [--write]         measure the evaluator, the sampler and the tracer
    canlab corpus run [opts]        run the exam corpus and write the coverage report
    canlab corpus fetch [name]      download the cited datasets (olympiadbench, ugphysics, scibench)

OPTIONS:
    -o, --out <file>             output file for plot (default: plot.svg)
        --xmin <f> --xmax <f>    plot domain (default: -10 10)
    -v, --var <name>             variable for integration or solve-all (default: guess)
        --lua-only               (compile) print the Lua only
        --wgsl-only              (compile) print the WGSL only
        --seed <n>               (compile-sweep) generator seed (default: 1)
        --count <n>              (compile-sweep) expressions to generate (default: 200)
        --probe                  really start the worker and make it answer
        --write                  (bench) write docs/bench.md instead of stdout
        --tag <tag>              (corpus run) only capabilities equal to `tag` or starting `tag.`
        --batch <n>              (corpus run) rows per child process (default: 50)
        --update                 (corpus run) re-fetch the datasets and check `hf:` rows

CORPUS ENVIRONMENT:
    CANLAB_CORPUS_HOME=<dir>     cache root (default: %LOCALAPPDATA%\\canlab\\corpus)
    CANLAB_CORPUS_MIRROR=<dir>   read a local mirror instead of the network

REPL COMMANDS:
    :help  :quit  :tex  :plot  :int  :solve-all  :solver  :compile
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
        "sign" => sign_command(&args[1..]),
        "surface" => surface_command(&args[1..]),
        "ode" => ode_command(&args[1..]),
        "int" | "integrate" => integrate_command(&args[1..]),
        "solve-all" => solve_all_command(&args[1..]),
        "compile" => compile_command(&args[1..]),
        "compile-sweep" => return compile_sweep_command(&args[1..]),
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
    // `parse_statement` and not `parse_latex`: a definite integral that is
    // numeric carries a one-sided error bound, and `parse_latex` collapses it to
    // a bare float because an `Expr` has nowhere to put it. `canlab eval
    // '\int_0^1 e^{-x^2} dx'` printed `0.746824132812427` with no marker, which
    // is exactly the "approximate and unmarked" outcome the integral module's
    // own header says the CAS is built against.
    let statement = canlab_parse::parse_statement(source).map_err(|e| e.to_string())?;
    let expr = statement.to_expr();
    println!("{}", node(&expr));
    println!("{}", to_latex(&expr));
    if let Some(error) = statement.error() {
        // The bound is one-sided and in the value's own units: the true value is
        // within `error` of the answer above.
        println!("approximate: within {error:e} of the true value");
    }
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
            bands: Vec::new(),
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

/// `canlab sign` — the sign table of `f'` on an interval, and the picture of it.
///
/// The table is printed as the pieces an exam writes (`increasing on (−1, 0)`,
/// `decreasing on (0, 2)`, …) and the same pieces are shaded in the SVG, so the
/// artefact and the picture cannot disagree: both come from one call to
/// [`canlab_parse::calculus::sign_table`].
/// `canlab surface <f(x,y)> [-o out.svg] [--xmin a --xmax b] [--cells n]
/// [--azimuth a --elevation e]`
///
/// Writes the surface as an SVG. This is not W8.4's viewport — that is planned on
/// Bevy, which does not fit on this machine — it is the smallest thing that makes
/// a mesh visible, using the same camera, projection and depth sort the viewport
/// will.
/// `canlab ode <line>... [--span a b] [--tolerance x] [-o out.svg]
/// [--azimuth a --elevation e]`
///
/// Integrates the system the lines describe and writes its trajectory as an SVG,
/// with the same camera and projection `canlab surface` uses — so a trajectory
/// and a surface drawn from one viewpoint line up. This is W8.5's "the ODE path
/// drawn in the same scene", from the command line rather than the panel.
fn ode_command(args: &[String]) -> Result<(), String> {
    let mut lines: Vec<String> = Vec::new();
    let mut out = PathBuf::from("ode.svg");
    let mut span = (0.0_f64, 10.0_f64);
    let mut tolerance = 1e-8_f64;
    let mut azimuth = 0.6_f64;
    let mut elevation = 0.5_f64;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--out" => {
                i += 1;
                out = PathBuf::from(args.get(i).ok_or("--out needs a path")?);
            }
            "--span" => {
                let from = parse_float(args.get(i + 1), "--span")?;
                let to = parse_float(args.get(i + 2), "--span")?;
                if !(from < to) {
                    return Err("--span needs an increasing pair".into());
                }
                span = (from, to);
                i += 2;
            }
            "--tolerance" => {
                i += 1;
                tolerance = parse_float(args.get(i), "--tolerance")?;
                if !(tolerance > 0.0) {
                    return Err("--tolerance must be positive".into());
                }
            }
            "--azimuth" => {
                i += 1;
                azimuth = parse_float(args.get(i), "--azimuth")?;
            }
            "--elevation" => {
                i += 1;
                elevation = parse_float(args.get(i), "--elevation")?;
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown ode option `{other}`"))
            }
            _ => lines.push(args[i].clone()),
        }
        i += 1;
    }
    if lines.is_empty() {
        return Err(
            "ode needs at least one line, like \"x' = y\" and \"x(0) = 1\"".into(),
        );
    }
    let system = canlab_ode::system::parse_system(&lines).map_err(|error| error.message())?;
    let trace = canlab_ode::system::dopri_system(&system, span.0, span.1, tolerance, 0.01)
        .map_err(|error| error.message())?;
    if trace.points.len() < 2 {
        return Err("the integration produced fewer than two points".into());
    }
    // The first three components, and `z = 0` for a system with fewer: a phase
    // portrait of `x' = y, y' = -x` is a curve in the plane, and putting it in
    // the plane is what a reader expects.
    let points: Vec<canlab_scene::Vec3> = trace
        .points
        .iter()
        .map(|point| {
            canlab_scene::Vec3::new(
                point.y.first().copied().unwrap_or(0.0),
                point.y.get(1).copied().unwrap_or(0.0),
                point.y.get(2).copied().unwrap_or(0.0),
            )
        })
        .collect();
    let camera = framing_points(&points, azimuth, elevation);
    let svg = canlab_scene::svg::path_svg(&points, &camera, &canlab_scene::svg::SvgOptions::default());
    std::fs::write(&out, svg)
        .map_err(|error| format!("could not write {}: {error}", out.display()))?;
    println!(
        "wrote {} ({} point(s), {} component(s))",
        out.display(),
        points.len(),
        system.dimension()
    );
    Ok(())
}

/// A camera that frames a list of points, from the angles the caller gives.
fn framing_points(
    points: &[canlab_scene::Vec3],
    azimuth: f64,
    elevation: f64,
) -> canlab_scene::Camera {
    let camera = canlab_scene::Camera {
        azimuth,
        elevation,
        aspect: 4.0 / 3.0,
        ..canlab_scene::Camera::default()
    };
    let mut fitted = camera.fit(points);
    fitted.distance *= 1.1;
    fitted
}

fn surface_command(args: &[String]) -> Result<(), String> {
    let source = args.first().ok_or("surface needs a LaTeX expression in x and y")?;
    let expr = parse_or_err(source)?;
    let mut out = PathBuf::from("surface.svg");
    let mut x_min = -2.0_f64;
    let mut x_max = 2.0_f64;
    let mut cells = 24usize;
    let mut azimuth = 0.6_f64;
    let mut elevation = 0.5_f64;
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
            "--cells" => {
                i += 1;
                cells = args
                    .get(i)
                    .ok_or("--cells needs a number")?
                    .parse()
                    .map_err(|_| String::from("--cells needs a whole number"))?;
            }
            "--azimuth" => {
                i += 1;
                azimuth = parse_float(args.get(i), "--azimuth")?;
            }
            "--elevation" => {
                i += 1;
                elevation = parse_float(args.get(i), "--elevation")?;
            }
            other => return Err(format!("unknown surface option `{other}`")),
        }
        i += 1;
    }
    if !(x_min < x_max) {
        return Err("--xmin must be less than --xmax".into());
    }
    if cells == 0 || cells > 400 {
        return Err("--cells must be between 1 and 400".into());
    }
    let x = canlab_core::SymbolId::intern("x");
    let y = canlab_core::SymbolId::intern("y");
    let mut vars = Vec::new();
    free_symbols(&expr, &mut vars);
    for var in &vars {
        if *var != x && *var != y {
            return Err(format!(
                "`{}` is not a variable of a surface; it is `z = f(x, y)`",
                var.as_str()
            ));
        }
    }
    let options = canlab_scene::surface::SurfaceOptions {
        x_range: (x_min, x_max),
        y_range: (x_min, x_max),
        x_cells: cells,
        y_cells: cells,
        pixel_error: 0.0,
        max_split: 0,
    };
    let mesh = canlab_scene::surface::mesh_surface(&expr, &options, &canlab_scene::Camera::default())
        .map_err(|error| error.message())?;
    if mesh.triangle_count() == 0 {
        return Err("the surface has no triangles: every corner was not a number".into());
    }
    let camera = canlab_scene::svg::framing_camera(&mesh, azimuth, elevation, 4.0 / 3.0);
    let svg = canlab_scene::svg::mesh_svg(&mesh, &camera, &canlab_scene::svg::SvgOptions::default());
    std::fs::write(&out, svg).map_err(|error| format!("could not write {}: {error}", out.display()))?;
    println!(
        "wrote {} ({} triangle(s), {} vertices)",
        out.display(),
        mesh.triangle_count(),
        mesh.vertex_count()
    );
    Ok(())
}

fn sign_command(args: &[String]) -> Result<(), String> {
    let source = args.first().ok_or("sign needs a LaTeX expression")?;
    let expr = parse_or_err(source)?;

    let mut out = PathBuf::from("sign.svg");
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
            other => return Err(format!("unknown sign option `{other}`")),
        }
        i += 1;
    }
    if !(x_min < x_max) {
        return Err("--xmin must be less than --xmax".into());
    }

    let mut vars = Vec::new();
    free_symbols(&expr, &mut vars);
    let var = match vars.len() {
        0 => return Err("that expression has no variable".into()),
        1 => vars[0],
        n => return Err(format!("sign needs exactly one variable, found {n}")),
    };

    let table = canlab_parse::calculus::sign_table(&expr, var, x_min, x_max)
        .map_err(|error| error.message())?;
    if table.is_empty() {
        return Err("the interval has no pieces".into());
    }
    // The words an exam uses, and the colour each direction is shaded with.
    let (up, down) = ("#c8e6c9", "#ffcdd2");
    let mut bands = Vec::new();
    for piece in &table {
        let (word, colour) = match piece.direction {
            1 => ("increasing", up),
            -1 => ("decreasing", down),
            _ => ("flat", "#eeeeee"),
        };
        println!("{word} on ({:.4}, {:.4})", piece.from, piece.to);
        bands.push((piece.from, piece.to, colour.to_string()));
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
    let (y_min, y_max) = vertical_extent(&curve)
        .ok_or("the function has no finite values in that range")?;
    let svg = to_svg(
        &curve,
        &SvgOptions {
            width: 900,
            height: 600,
            x_range: (x_min, x_max),
            y_range: (y_min, y_max),
            stroke: "#1e88e5".to_string(),
            show_axes: true,
            bands,
        },
    );
    std::fs::write(&out, svg).map_err(|e| format!("could not write {}: {e}", out.display()))?;
    println!("wrote {} ({} piece(s))", out.display(), table.len());
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
            // The candidates that were computed and rejected are the working,
            // not the answer: they are printed beside it and never inside it.
            for rejection in solutions.rejections_latex() {
                println!("  {rejection}");
            }
            // `to_latex` prints an approximation as its number, because only
            // the caller knows whether it is writing "x = 1.2207" or comparing
            // two answers. The CLI *is* writing an answer, so otherwise
            // `x^4 - x - 1` printed `-0.7244… \cup 1.2207…` and a reader had no
            // way to tell it from an exact value — the residual is the evidence
            // that it is not one, and it belongs beside the number.
            for (value, residual) in approximations_of(&solutions) {
                println!(
                    "x \u{2248} {value}  (f(x) = {residual:.1e} of the function's scale)"
                );
            }
            Ok(())
        }
        Err(err) => Err(format!(
            "the solver could not give a solution set ({err}); the equation is\n  {} = 0",
            to_latex(&expr)
        )),
    }
}

/// Every approximate member of an answer, outermost union first, as the number
/// to print and the residual that says how good the number is.
///
/// A `Solutions` keeps its approximations as leaves of the answer, so a union of
/// two numeric roots is two leaves and a union of a family and one is one leaf.
fn approximations_of(solutions: &Solutions) -> Vec<(String, f64)> {
    match solutions {
        Solutions::Approximation { value, residual } => {
            vec![(to_latex(value), *residual)]
        }
        Solutions::Union(members) => members.iter().flat_map(approximations_of).collect(),
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// canlab compile (W7.3)
// ---------------------------------------------------------------------------

/// `canlab compile` — the two viewers the command line needs.
///
/// The Lua is what `canlab_core::lua` printed; the WGSL is what
/// `canlab_core::wgsl` compiled that Lua to. Neither is re-derived here, and
/// neither is evaluated: nothing in this workspace runs WGSL yet, so the
/// command shows the source of a kernel and says nothing about its numbers.
///
/// A refusal is printed as the message of the step that refused — the printer
/// for a `Hold`, an unsupported function or a free symbol; the compiler for a
/// construct outside its subset — and the exit code is a failure, because a
/// compile that produced no kernel is not a success whatever the reason.
fn compile_command(args: &[String]) -> Result<(), String> {
    let mut latex: Option<&str> = None;
    let mut variables: Vec<SymbolId> = Vec::new();
    let mut lua_only = false;
    let mut wgsl_only = false;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "-v" | "--var" => {
                index += 1;
                let name = args.get(index).ok_or("--var needs a name")?;
                let symbol = SymbolId::intern(name);
                // A name given twice is still one local: `to_lua` looks it up by
                // symbol, and two identical entries are the same entry.
                if !variables.contains(&symbol) {
                    variables.push(symbol);
                }
            }
            "--lua-only" => lua_only = true,
            "--wgsl-only" => wgsl_only = true,
            // The one LaTeX argument. A second one is refused rather than
            // ignored: a command that prints one kernel while given two
            // expressions is printing the wrong one half the time.
            argument if argument.starts_with('-') && argument.len() > 1 => {
                return Err(format!("unknown compile option `{argument}`"));
            }
            argument => {
                if latex.is_some() {
                    return Err(format!(
                        "compile takes one LaTeX expression, and `{argument}` is a second"
                    ));
                }
                latex = Some(argument);
            }
        }
        index += 1;
    }

    if lua_only && wgsl_only {
        return Err("--lua-only and --wgsl-only ask for nothing to be printed".into());
    }
    let source = latex.ok_or("compile needs a LaTeX expression")?;
    let expr = parse_or_err(source)?;

    // With no `--var`, the variables are the expression's own free symbols, in
    // the order they first appear: the same guess `int` and `solve-all` make.
    if variables.is_empty() {
        free_symbols(&expr, &mut variables);
    }
    let name = "kernel_expression";

    if !wgsl_only {
        match canlab_core::lua::to_lua_program(name, &expr, &variables) {
            Ok(lua) => {
                println!("-- Lua");
                print!("{lua}");
            }
            Err(error) => {
                return Err(format!("\n-- Lua\nthe printer refused this: {}", error.message()));
            }
        }
    }
    if lua_only {
        return Ok(());
    }
    match canlab_solver::kernel::compile_expression(&expr, name, &variables) {
        Ok(artefact) => {
            println!(
                "-- WGSL (f32 function; the expression's variables: {})",
                if variables.is_empty() {
                    String::from("none")
                } else {
                    variables
                        .iter()
                        .map(|symbol| symbol.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            );
            print!("{}", artefact.wgsl);
            Ok(())
        }
        Err(refusal) => {
            // The Lua stage printing the expression already happened, unless
            // `--wgsl-only` skipped it, so the message repeats the refusal the
            // viewer above just showed: the two are printed together and a
            // reader who redirects one stream still sees why.
            Err(format!(
                "\n-- WGSL\nthe compiler refused this (at the {} step): {}",
                refusal.stage(),
                refusal.message()
            ))
        }
    }
}

/// `canlab compile-sweep` — the W7.3 validation check, as a command.
///
/// Prints the numbers the sweep measured and exits non-zero when any generated
/// expression broke the tolerance or was printed to Lua and refused by the
/// compiler. It says what it does not do on its own output, because a number
/// printed beside no caveat is a number a reader will trust.
fn compile_sweep_command(args: &[String]) -> ExitCode {
    let mut seed = 1u64;
    let mut count = canlab_solver::kernel::SWEEP_PROGRAMS;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--seed" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    eprintln!("error: --seed needs a whole number");
                    return ExitCode::from(2);
                };
                match value.parse() {
                    Ok(parsed) => seed = parsed,
                    Err(_) => {
                        eprintln!("error: --seed needs a whole number, not {value:?}");
                        return ExitCode::from(2);
                    }
                }
            }
            "--count" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    eprintln!("error: --count needs a whole number");
                    return ExitCode::from(2);
                };
                match value.parse::<usize>() {
                    Ok(parsed) if parsed >= 1 => count = parsed,
                    _ => {
                        eprintln!("error: --count needs a positive whole number, not {value:?}");
                        return ExitCode::from(2);
                    }
                }
            }
            other => {
                eprintln!("error: unknown compile-sweep option `{other}`");
                return ExitCode::from(2);
            }
        }
        index += 1;
    }

    let report = canlab_solver::kernel::sweep(seed, count);
    println!(
        "{} generated expression(s) at seed {seed}: {} printed to Lua and compiled to WGSL, \
         {} refused by the compiler",
        report.programs, report.compared, report.wgsl_refusals
    );
    println!(
        "{} sample point(s); worst relative difference {:e} against the {} bound",
        report.points,
        report.worst_relative,
        canlab_solver::kernel::SWEEP_TOLERANCE
    );
    if let Some(worst) = &report.worst_at {
        println!("  the worst point: {worst}");
    }
    println!(
        "{} point(s) where the kernel's f32 overflowed and the CPU's f64 did not",
        report.f32_overflowed
    );
    for failure in &report.failures {
        println!("FAIL {failure}");
    }
    println!(
        "This compares the CPU evaluation with an f32 replay of the arithmetic the WGSL \
         emitter chose. It does not run WGSL: the CPU/GPU differential is W7.4's."
    );
    if report.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
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
                "compile" => report(compile_command(&[argument.to_string()])),
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
            Hold::Accent { body, .. } => free_symbols(body, out),
            Hold::Matrix { rows } => {
                for row in rows {
                    for cell in row {
                        free_symbols(cell, out);
                    }
                }
            }
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
