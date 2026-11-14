//! The W7.5 probe: what this machine's adapter is, and how accurate its `f64` is.
//!
//! Run it with `cargo run -p canlab-gpu`. It prints the adapter, whether it
//! offers `SHADER_F64`, and the largest relative difference between a shader's
//! `f64` and the CPU's.

fn main() {
    let adapters = canlab_gpu::probe();
    if adapters.is_empty() {
        println!("no adapter on this machine");
        return;
    }
    for adapter in &adapters {
        println!("{}", adapter.describe());
    }
    match canlab_gpu::f64_accuracy() {
        Ok(Some(error)) => {
            println!("f64 accuracy: {error:e} relative (the roadmap asks for 1e-15)");
            if error < 1e-15 {
                println!("  -> f64 is available and accurate; the transport can run in double precision");
            } else {
                println!("  -> f64 is offered but not accurate enough; the transport needs f32 with a marker, or double-double");
            }
        }
        Ok(None) => println!(
            "f64 accuracy: not available — the adapter does not offer SHADER_F64"
        ),
        Err(error) => println!("f64 accuracy: {}", error.message()),
    }
}
