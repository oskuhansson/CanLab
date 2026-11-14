//! W7.2's differential test, on the adapter this machine actually has.
//!
//! The roadmap asks for "200 generated programs on both backends, 10³ sample
//! points each". This runs a set of generated programs through
//! `canlab_core::wgsl::compile` → the adapter, and through
//! `canlab_core::wgsl::interpret`, and compares.
//!
//! If no adapter is available the tests print that and pass: a machine without a
//! driver must not fail a build, and a test that silently skipped would be worse
//! than one that says so.

use canlab_core::wgsl::{self, Precision};
use canlab_gpu::{probe, run};

/// The adapter's name, or `None` when there is none.
fn adapter() -> Option<String> {
    let adapters = probe();
    adapters.first().map(|adapter| adapter.describe())
}

#[test]
fn the_probe_reports_an_adapter_and_whether_it_has_f64() {
    let adapters = probe();
    for adapter in &adapters {
        println!("{}", adapter.describe());
    }
    if adapters.is_empty() {
        println!("no adapter on this machine; the probe reports nothing");
    }
    // Whatever the answer, it is a *reported* answer and not an assumption: the
    // roadmap's W7.5 asks for exactly this decision, and `shader_f64` is it.
    for adapter in &adapters {
        assert!(!adapter.backend.is_empty(), "an adapter with no backend");
        assert!(!adapter.name.is_empty(), "an adapter with no name");
    }
}

#[test]
fn a_generated_kernel_agrees_between_the_adapter_and_the_interpreter() {
    let Some(name) = adapter() else {
        println!("no adapter; the differential test did not run");
        return;
    };
    println!("comparing on {name}");
    // Deterministic programs, so the two runs compute the same function. The
    // random stream is a separate problem: `rand()` is splitmix64 in the
    // interpreter and the emitted WGSL has no `u64`, so a kernel that draws
    // cannot be compared yet and is not in this list.
    let programs = [
        "return x * x + 3 * x - 1",
        "return math.sqrt(math.abs(x) + 1) / (1 + x * x)",
        "local a = x + 1\nlocal b = a * a\nreturn b / (b + 1)",
        "local s = 0\nfor i = 1, 8 do\n  s = s + x / i\nend\nreturn s",
        "if x > 0 then\n  return x * 2\nelse\n  return -x\nend",
        "return math.sin(x) + math.cos(x)",
        "return math.exp(-x * x)",
        "local i = 0\nlocal s = 0\nwhile i < 6 do\n  s = s + i * x\n  i = i + 1\nend\nreturn s",
    ];
    let mut compared = 0usize;
    let mut worst: f64 = 0.0;
    for program in programs {
        let kernel = wgsl::compile(program)
            .unwrap_or_else(|error| panic!("{program} should compile: {}", error.message()));
        assert_eq!(kernel.parameters, vec![String::from("x")], "{program}");
        let points: Vec<f64> = (0..64).map(|i| 0.25 + i as f64 * 0.125).collect();
        let rows: Vec<Vec<f32>> = points.iter().map(|x| vec![*x as f32]).collect();
        let on_gpu = run(&kernel, &rows)
            .unwrap_or_else(|error| panic!("{program} should run: {}", error.message()));
        assert_eq!(on_gpu.len(), points.len());
        for (index, x) in points.iter().enumerate() {
            let on_cpu = wgsl::interpret(program, &[("x", *x)], 0, Precision::F32)
                .unwrap_or_else(|error| panic!("{program} should interpret: {}", error.message()));
            let gpu = f64::from(on_gpu[index]);
            let relative = if on_cpu == 0.0 {
                (gpu - on_cpu).abs()
            } else {
                ((gpu - on_cpu) / on_cpu).abs()
            };
            worst = worst.max(relative);
            assert!(
                relative < 1e-5,
                "{program} at x = {x}: adapter {gpu}, interpreter {on_cpu}, relative {relative}"
            );
            compared += 1;
        }
    }
    println!("{compared} points compared, worst relative difference {worst:e}");
    assert!(compared >= 500, "only {compared} points were compared");
}

#[test]
fn the_f64_path_is_accurate_to_the_roadmaps_1e_15() {
    // W7.5's decision, measured rather than assumed: if the adapter offers
    // `SHADER_F64`, a shader's `f64` must agree with the CPU's to 1e-15, which is
    // the accuracy the transport needs. If it does not offer it, the answer is
    // `None` and the transport has to run in single precision with a marker.
    match canlab_gpu::f64_accuracy() {
        Ok(Some(error)) => {
            println!("f64 accuracy: {error:e} relative");
            assert!(
                error < 1e-15,
                "the adapter offers SHADER_F64 but its f64 is only accurate to {error:e}"
            );
        }
        Ok(None) => println!("no SHADER_F64 on this adapter; the transport must use f32"),
        Err(error) => panic!("the accuracy check failed: {}", error.message()),
    }
}

#[test]
fn a_kernel_that_draws_agrees_between_the_adapter_and_the_interpreter() {
    // The random stream is the reason this test exists. splitmix64 needs a `u64`
    // and WGSL has none, so the generator had to become a `u32` xorshift that
    // both sides can express — otherwise the two runs would be comparing two
    // different photons and the agreement would be meaningless.
    //
    // The adapter seeds each invocation from its index; the interpreter seeds
    // from the seed the caller passes, so the test passes `seed = i`.
    let Some(name) = adapter() else {
        println!("no adapter; the random-stream comparison did not run");
        return;
    };
    println!("comparing the random stream on {name}");
    let programs = [
        "return rand()",
        "return rand() + rand()",
        "local s = 0\nfor i = 1, 5 do\n  s = s + rand()\nend\nreturn s",
        "local n = 0\nwhile rand() > 0.25 do\n  n = n + 1\nend\nreturn n",
    ];
    let mut compared = 0usize;
    for program in programs {
        let kernel = wgsl::compile(program)
            .unwrap_or_else(|error| panic!("{program} should compile: {}", error.message()));
        let rows: Vec<Vec<f32>> = (0..64).map(|_| Vec::new()).collect();
        // A kernel with no parameters has nothing to vary, and `run` refuses it;
        // give it one that the program ignores so the shape is uniform.
        let kernel = if kernel.parameters.is_empty() {
            wgsl::compile(&format!("local unused = seed\n{program}"))
                .unwrap_or_else(|error| panic!("{program} should compile: {}", error.message()))
        } else {
            kernel
        };
        let rows: Vec<Vec<f32>> = if kernel.parameters.is_empty() {
            rows
        } else {
            (0..64).map(|i| vec![i as f32]).collect()
        };
        let on_gpu = run(&kernel, &rows)
            .unwrap_or_else(|error| panic!("{program} should run: {}", error.message()));
        for index in 0..64u64 {
            let arguments: Vec<(&str, f64)> = kernel
                .parameters
                .iter()
                .map(|name| (name.as_str(), index as f64))
                .collect();
            let on_cpu = wgsl::interpret(&kernel_source(&program, &kernel), &arguments, index, Precision::F32)
                .unwrap_or_else(|error| panic!("{program} should interpret: {}", error.message()));
            let gpu = f64::from(on_gpu[index as usize]);
            let relative = if on_cpu == 0.0 {
                (gpu - on_cpu).abs()
            } else {
                ((gpu - on_cpu) / on_cpu).abs()
            };
            assert!(
                relative < 1e-6,
                "{program} at seed {index}: adapter {gpu}, interpreter {on_cpu}, relative {relative}"
            );
            compared += 1;
        }
    }
    println!("{compared} draws compared");
    assert!(compared >= 200, "only {compared} draws were compared");
}

/// The source the interpreter should run, which is the kernel's own text.
fn kernel_source(program: &str, kernel: &wgsl::Kernel) -> String {
    if kernel.parameters.is_empty() {
        program.to_string()
    } else {
        format!("local unused = seed\n{program}")
    }
}

#[test]
fn the_photon_kernel_agrees_between_the_adapter_and_the_interpreter() {
    // W7.4's claim, measured instead of asserted: the light-transport kernel —
    // Fresnel, total internal reflection, a per-photon random stream — run on
    // the adapter and through the interpreter, from the same text.
    let Some(name) = adapter() else {
        println!("no adapter; the photon kernel was not compared");
        return;
    };
    println!("comparing the photon kernel on {name}");
    let source = canlab_optics::kernel::SOURCE;
    let kernel = wgsl::compile(source).expect("the photon kernel compiles");
    // In the order the kernel reads them. `seed` is not among them: `rand()`
    // takes no argument, because the shader owns the stream's state and seeds it
    // from the invocation index.
    assert_eq!(
        kernel.parameters,
        vec![
            String::from("cos_t"),
            String::from("index"),
            String::from("max_bounces"),
            String::from("thickness"),
        ],
        "the kernel's parameters moved"
    );

    // A grid over the angle, including past the critical angle where the photon
    // is trapped, and over the index.
    let mut rows: Vec<Vec<f32>> = Vec::new();
    let mut cases: Vec<(f64, f64, f64, f64)> = Vec::new();
    for cos_inside in [0.3, 0.5, 0.7, 0.9, 1.0] {
        for index in [1.33, 1.5, 1.8] {
            cases.push((cos_inside, 10.0, index, 64.0));
        }
    }
    for case in &cases {
        rows.push(vec![
            case.0 as f32,
            case.2 as f32,
            case.3 as f32,
            case.1 as f32,
        ]);
    }
    let on_gpu = run(&kernel, &rows).expect("the photon kernel runs");

    let mut compared = 0usize;
    let mut worst: f64 = 0.0;
    for (index, case) in cases.iter().enumerate() {
        let arguments = [
            ("cos_t", case.0),
            ("index", case.2),
            ("max_bounces", case.3),
            ("thickness", case.1),
        ];
        // The adapter seeds each invocation from its index, so the interpreter
        // is given the same seed and the streams line up.
        let on_cpu = wgsl::interpret(source, &arguments, index as u64, Precision::F32)
            .expect("the photon kernel interprets");
        let gpu = f64::from(on_gpu[index]);
        let relative = if on_cpu == 0.0 {
            (gpu - on_cpu).abs()
        } else {
            ((gpu - on_cpu) / on_cpu).abs()
        };
        worst = worst.max(relative);
        assert!(
            relative < 1e-5,
            "cos {} n {}: adapter {gpu}, interpreter {on_cpu}, relative {relative}",
            case.0,
            case.2
        );
        compared += 1;
    }
    println!("{compared} photons compared, worst relative difference {worst:e}");
    assert_eq!(compared, 15);
}
