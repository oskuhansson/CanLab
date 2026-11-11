//! `canlab bench` — W0.3's measurement tool.
//!
//! The point of the file it writes is a diff: a 20% regression should be
//! visible, so every number names its parameters and every measurement a
//! measurement that could not be taken says `n/a`. It never says `0` for a
//! number it did not take.
//!
//! The numbers are wall-clock, best-of-N, in this process. That is honest but
//! limited: it is the same kind of measurement the plan asks for, not a
//! benchmark harness, and it is not comparable across machines.

use std::hint::black_box;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use canlab_core::{Expr, ExprNode, Program, SymbolId};
use canlab_parse::parse_latex;
use canlab_plot::{ImplicitOptions, SampleOptions, sample, sample_implicit};

/// One row of the table.
struct Measurement {
    name: &'static str,
    value: Option<f64>,
    unit: &'static str,
    detail: String,
}

fn unmeasured(name: &'static str, unit: &'static str, why: impl Into<String>) -> Measurement {
    Measurement {
        name,
        value: None,
        unit,
        detail: why.into(),
    }
}

/// Run the measurements and print or write the table.
pub fn run(write: bool) -> Result<(), String> {
    let measurements = vec![
        implicit_tracer(),
        program_eval(),
        program_eval_batch(),
        plot_sample(),
    ];
    let profile = if cfg!(debug_assertions) {
        "debug profile"
    } else {
        "release profile"
    };
    let text = render(
        &measurements,
        &utc_now(),
        &format!("{} — built in the {profile}", cpu()),
    );

    if write {
        std::fs::create_dir_all("docs")
            .map_err(|error| format!("could not create docs/: {error}"))?;
        std::fs::write("docs/bench.md", &text)
            .map_err(|error| format!("could not write docs/bench.md: {error}"))?;
        println!("wrote docs/bench.md");
    } else {
        print!("{text}");
    }
    Ok(())
}

fn implicit_tracer() -> Measurement {
    const NAME: &str = "implicit tracer, 240×240, `x^2 + y^2 = 1`";
    const UNIT: &str = "ms/rebuild";
    let Some(expression) = parse_latex("x^2 + y^2 - 1").ok() else {
        return unmeasured(NAME, UNIT, "the equation did not parse");
    };
    let x = SymbolId::intern("x");
    let y = SymbolId::intern("y");
    let Some(program) = Program::compile(&expression, &[x, y]) else {
        return unmeasured(NAME, UNIT, "the equation did not compile to a program");
    };
    let options = ImplicitOptions {
        resolution: 240,
        x_min: -2.0,
        x_max: 2.0,
        y_min: -2.0,
        y_max: 2.0,
    };
    let trace = || sample_implicit(|x, y| program.eval(&[x, y]), &options);

    let curve = trace();
    if curve.is_empty() {
        return unmeasured(NAME, UNIT, "the tracer returned no curve");
    }
    black_box(&curve);

    let rebuilds = 20;
    let mut best = f64::INFINITY;
    for _ in 0..rebuilds {
        let start = Instant::now();
        let curve = trace();
        black_box(&curve);
        best = best.min(start.elapsed().as_secs_f64() * 1_000.0);
    }
    Measurement {
        name: NAME,
        value: Some(best),
        unit: UNIT,
        detail: format!(
            "best of {rebuilds} rebuilds on [-2, 2]²; {} points on the curve",
            curve.point_count()
        ),
    }
}

/// The 20-node expression W0.3 names. `docs/bench.md` is a measurement, not a
/// fixture, but the shape has to be stated so a diff means something.
const TWENTY_NODES: &str = r"\sin x + \cos x + x^2 + \frac{1}{x+1} + 2x + x^3 + 1";

fn twenty_node_expression() -> Option<Expr> {
    let expression = parse_latex(TWENTY_NODES).ok()?;
    (node_count(&expression) == 20).then_some(expression)
}

fn program_eval() -> Measurement {
    const NAME: &str = "`Program::eval`, 20-node expression";
    const UNIT: &str = "ns/call";
    let Some(expression) = twenty_node_expression() else {
        return unmeasured(
            NAME,
            UNIT,
            "the 20-node expression did not parse into 20 nodes",
        );
    };
    let x = SymbolId::intern("x");
    let Some(program) = Program::compile(&expression, &[x]) else {
        return unmeasured(NAME, UNIT, "the expression did not compile");
    };

    let value = 0.7;
    black_box(program.eval(&[value]));
    let calls_per_round = 1_000_000u64;
    let rounds = 5;
    let mut best = f64::INFINITY;
    for _ in 0..rounds {
        let start = Instant::now();
        for _ in 0..calls_per_round {
            black_box(program.eval(&[value]));
        }
        let nanos = start.elapsed().as_nanos() as f64;
        best = best.min(nanos / calls_per_round as f64);
    }
    Measurement {
        name: NAME,
        value: Some(best),
        unit: UNIT,
        detail: format!("best of {rounds} × {calls_per_round} calls"),
    }
}

fn program_eval_batch() -> Measurement {
    const NAME: &str = "`Program::eval_batch`, one row";
    const UNIT: &str = "ns/row";
    let Some(expression) = twenty_node_expression() else {
        return unmeasured(
            NAME,
            UNIT,
            "the 20-node expression did not parse into 20 nodes",
        );
    };
    let x = SymbolId::intern("x");
    let Some(program) = Program::compile(&expression, &[x]) else {
        return unmeasured(NAME, UNIT, "the expression did not compile");
    };

    let rows = 1_000_000usize;
    let mut args = vec![0.0; rows];
    for (index, slot) in args.iter_mut().enumerate() {
        *slot = index as f64 / rows as f64;
    }
    let mut out = vec![0.0; rows];

    let rounds = 5;
    let mut best = f64::INFINITY;
    for _ in 0..rounds {
        let start = Instant::now();
        program.eval_batch(&args, 1, &mut out);
        let nanos = start.elapsed().as_nanos() as f64;
        black_box(&out);
        best = best.min(nanos / rows as f64);
    }
    Measurement {
        name: NAME,
        value: Some(best),
        unit: UNIT,
        detail: format!("best of {rounds} × {rows} rows"),
    }
}

fn plot_sample() -> Measurement {
    const NAME: &str = "`canlab_plot::sample`, 2000-point range";
    const UNIT: &str = "ms";
    let Some(expression) = parse_latex(r"\sin x + 0.1x").ok() else {
        return unmeasured(NAME, UNIT, "the expression did not parse");
    };
    let x = SymbolId::intern("x");
    let Some(program) = Program::compile(&expression, &[x]) else {
        return unmeasured(NAME, UNIT, "the expression did not compile");
    };
    let options = SampleOptions {
        x_min: -10.0,
        x_max: 10.0,
        resolution: 2000,
        ..SampleOptions::default()
    };
    let curve = sample(&program, &options);
    if curve.is_empty() {
        return unmeasured(NAME, UNIT, "the sampler returned no curve");
    }
    black_box(&curve);

    let rounds = 20;
    let mut best = f64::INFINITY;
    for _ in 0..rounds {
        let start = Instant::now();
        let curve = sample(&program, &options);
        black_box(&curve);
        best = best.min(start.elapsed().as_secs_f64() * 1_000.0);
    }
    Measurement {
        name: NAME,
        value: Some(best),
        unit: UNIT,
        detail: format!(
            "best of {rounds} samples; {} points",
            curve.point_count()
        ),
    }
}

fn node_count(expression: &Expr) -> usize {
    match &**expression {
        ExprNode::Num(_) | ExprNode::Sym(_) => 1,
        ExprNode::Add(items) | ExprNode::Mul(items) | ExprNode::Fn(_, items) => {
            1 + items.iter().map(node_count).sum::<usize>()
        }
        ExprNode::Pow(base, exponent) => 1 + node_count(base) + node_count(exponent),
        ExprNode::Hold(_) => 1,
    }
}

fn render(measurements: &[Measurement], generated: &str, cpu: &str) -> String {
    let mut out = String::new();
    out.push_str("# Benchmarks\n\n");
    out.push_str(&format!(
        "Generated {generated} by `canlab bench` on {cpu}.\n\n"
    ));
    out.push_str(
        "Wall-clock measurements, best of N, taken in this process on this machine. They are not \
         comparable across machines. A measurement that could not be taken reads `n/a`, never `0`.\n\n",
    );
    out.push_str("| measurement | value | detail |\n|---|---:|---|\n");
    for measurement in measurements {
        let value = match measurement.value {
            Some(value) => format!("{} {}", format_value(value), measurement.unit),
            None => String::from("n/a"),
        };
        out.push_str(&format!(
            "| {} | {value} | {} |\n",
            measurement.name,
            measurement.detail.replace('|', "\\|")
        ));
    }
    out
}

/// Enough digits that a real measurement never reads as `0`.
fn format_value(value: f64) -> String {
    if value >= 100.0 {
        format!("{value:.0}")
    } else if value >= 10.0 {
        format!("{value:.1}")
    } else if value >= 1.0 {
        format!("{value:.2}")
    } else if value >= 0.01 {
        format!("{value:.3}")
    } else {
        format!("{value:.4}")
    }
}

/// The current time, in UTC, without a date-time dependency.
pub(crate) fn utc_now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let days = (seconds / 86_400) as i64;
    let remainder = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        remainder / 3_600,
        (remainder % 3_600) / 60,
        remainder % 60
    )
}

/// Howard Hinnant's days-to-civil-date, with day 0 at 1970-01-01.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = (z - era * 146_097) as u64;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// Which CPU the OS says this is, without a new dependency.
fn cpu() -> String {
    let mut parts = Vec::new();
    if let Ok(identifier) = std::env::var("PROCESSOR_IDENTIFIER") {
        let identifier = identifier.trim();
        if !identifier.is_empty() {
            parts.push(identifier.to_string());
        }
    }
    if let Ok(count) = std::env::var("NUMBER_OF_PROCESSORS") {
        parts.push(format!("{count} logical processors"));
    }
    parts.push(format!("{} target", std::env::consts::ARCH));
    format!("{} (read from the OS)", parts.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_expression_really_has_twenty_nodes() {
        let expression = parse_latex(TWENTY_NODES).expect("parsed");
        assert_eq!(node_count(&expression), 20);
        assert!(twenty_node_expression().is_some());
    }

    #[test]
    fn utc_dates_are_computed_from_the_epoch() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(59), (1970, 3, 1));
        // 2000-03-01 is 11017 days after the epoch.
        assert_eq!(civil_from_days(11_017), (2000, 3, 1));
        // 2024-02-29 is a leap day.
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }

    #[test]
    fn a_failed_measurement_is_n_a_and_never_zero() {
        let measurements = vec![
            unmeasured("a measurement", "ms", "the tool refused"),
            Measurement {
                name: "a taken measurement",
                value: Some(1.25),
                unit: "ns",
                detail: String::new(),
            },
        ];
        let text = render(&measurements, "2026-11-11 12:00 UTC", "a CPU");
        assert!(text.contains("| a measurement | n/a | the tool refused |"));
        assert!(text.contains("| a taken measurement | 1.25 ns |"));
        assert!(!text.contains("0 ms"));
        assert!(text.contains("2026-11-11 12:00 UTC"));
        assert!(text.contains("a CPU"));
    }

    #[test]
    fn pipes_in_a_detail_do_not_break_the_table() {
        let text = render(
            &[Measurement {
                name: "x",
                value: None,
                unit: "ms",
                detail: String::from("a | b"),
            }],
            "now",
            "cpu",
        );
        assert!(text.contains("a \\| b"));
    }

    #[test]
    fn the_cpu_line_is_never_empty() {
        let line = cpu();
        assert!(line.contains("target"), "{line}");
    }
}
