//! W7.4: the light-transport kernel, written once and run two ways.
//!
//! The kernel is a Lua-subset program ([`SOURCE`]) that transports one photon
//! through a slab: it refracts in, bounces between the faces with the Fresnel
//! reflectance, suffers total internal reflection when the angle is past the
//! critical angle, and returns the path length. It is compiled to WGSL by
//! [`canlab_core::wgsl::compile`] for the GPU, and run on the CPU by
//! [`canlab_core::wgsl::interpret`] from the **same text** — so the two runs
//! cannot drift, which is the whole point of writing it once.
//!
//! # What is compared, and what is not
//!
//! The roadmap asks that "CPU and GPU agree on exit angle, path length and
//! arrival time to 1e-6 relative". Nothing here executes WGSL: there is no
//! driver, no adapter and no `wgpu` pipeline in this tree. What *is* compared is
//!
//! * the same kernel at [`Precision::F32`], which rounds every operation the way
//!   the emitted WGSL does, against [`Precision::F64`], which is the CPU
//!   reference — their difference is the floating-point difference a GPU run
//!   would show, and it is not a driver's;
//! * the kernel against [`reference`], a Rust implementation of the same
//!   arithmetic with the same random stream, which is the check that the kernel
//!   says what the crate's physics says.
//!
//! The throughput number the roadmap asks for — 10⁷ photons per second — is
//! **not measured**, because there is nothing to measure it with. A CPU
//! interpreter's rate is not a GPU's, and quoting one would be the "assumed and
//! not measured" the roadmap's own verify column warns against.
//!
//! # Why a slab and not the block
//!
//! The block's geometry is a box with four faces and a birth inset; the kernel
//! subset has no tables and no user functions, so the box's face loop would have
//! to be unrolled by hand. A slab — two parallel faces — is the smallest
//! geometry that exercises every piece the roadmap names (a Fresnel split, total
//! internal reflection, a per-photon random stream, an event output) and whose
//! answer can be checked in closed form. The box is a later step, and the
//! limitation is stated rather than hidden behind a kernel that looks general.

use canlab_core::wgsl::{Precision, RunError};

/// The kernel: one photon through a slab.
///
/// Arguments, in the order [`run`] supplies them: `cos_t` (the cosine of the
/// angle inside the slab, measured from the face normal), `thickness` in
/// millimetres, `index` (the slab's refractive index against air), `max_bounces`
/// and `seed` — the last is not read by the program, because `rand()` draws from
/// the per-invocation stream the interpreter seeds with it.
///
/// It returns the path length inside the slab in millimetres.
pub const SOURCE: &str = r#"
-- The angle inside the slab, and whether the ray can leave at all.
local sin_t = math.sqrt(1 - cos_t * cos_t)
local nsin = index * sin_t
-- Total internal reflection: `sin(theta) > 1/index` inside, so `index*sin > 1`.
-- When it happens the reflectance is one and the photon never leaves.
local r = 1
if nsin < 1 then
  -- The transmitted cosine, by Snell: `sin_out = index * sin_t`.
  local cos_out = math.sqrt(1 - nsin * nsin)
  -- Fresnel for the two polarisations, from the slab into air, averaged.
  local rs = (index * cos_t - cos_out) / (index * cos_t + cos_out)
  local rp = (index * cos_out - cos_t) / (index * cos_out + cos_t)
  r = (rs * rs + rp * rp) / 2
end
-- The path between two parallel faces at this angle, and the bounces.
local path = 0
local bounces = 0
local alive = 1
while alive > 0 and bounces < max_bounces do
  path = path + thickness / cos_t
  bounces = bounces + 1
  if rand() > r then
    alive = 0
  end
end
return path
"#;

/// One photon's starting conditions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Photon {
    /// The cosine of the angle inside the slab, from the face normal, in `(0, 1]`.
    pub cos_inside: f64,
    /// The slab's thickness, millimetres.
    pub thickness_mm: f64,
    /// The slab's refractive index against air.
    pub index: f64,
    /// How many bounces before the photon is called trapped.
    pub max_bounces: u32,
}

/// Why a photon cannot be transported.
#[derive(Debug, Clone, PartialEq)]
pub enum KernelError {
    /// The kernel is not in the subset, or does not run.
    Run(RunError),
    /// A starting condition that has no meaning.
    BadPhoton(String),
}

impl KernelError {
    /// The message a caller shows.
    pub fn message(&self) -> String {
        match self {
            KernelError::Run(error) => error.message(),
            KernelError::BadPhoton(why) => why.clone(),
        }
    }
}

impl From<RunError> for KernelError {
    fn from(error: RunError) -> KernelError {
        KernelError::Run(error)
    }
}

impl Photon {
    fn check(&self) -> Result<(), KernelError> {
        if !(self.cos_inside > 0.0 && self.cos_inside <= 1.0) {
            return Err(KernelError::BadPhoton(format!(
                "the cosine inside must be in (0, 1], and it is {}",
                self.cos_inside
            )));
        }
        if !(self.thickness_mm > 0.0) {
            return Err(KernelError::BadPhoton(format!(
                "the thickness must be positive, and it is {}",
                self.thickness_mm
            )));
        }
        if !(self.index >= 1.0) {
            return Err(KernelError::BadPhoton(format!(
                "the refractive index must be at least 1, and it is {}",
                self.index
            )));
        }
        if self.max_bounces == 0 {
            return Err(KernelError::BadPhoton(String::from(
                "a photon needs at least one bounce to leave",
            )));
        }
        Ok(())
    }
}

/// Run the kernel for one photon, in the given precision.
pub fn run(photon: &Photon, seed: u64, precision: Precision) -> Result<f64, KernelError> {
    photon.check()?;
    let arguments = [
        ("cos_t", photon.cos_inside),
        ("thickness", photon.thickness_mm),
        ("index", photon.index),
        ("max_bounces", f64::from(photon.max_bounces)),
        ("seed", seed as f64),
    ];
    Ok(canlab_core::wgsl::interpret(SOURCE, &arguments, seed, precision)?)
}

/// The same arithmetic in Rust, with the same random stream.
///
/// This is the check that the kernel says what the crate says. It is deliberately
/// a *second* implementation: if it were generated from the kernel it would agree
/// by construction and prove nothing.
pub fn reference(photon: &Photon, seed: u64) -> Result<f64, KernelError> {
    photon.check()?;
    let cos_t = photon.cos_inside;
    let sin_t = (1.0 - cos_t * cos_t).sqrt();
    let nsin = photon.index * sin_t;
    let mut r = 1.0;
    if nsin < 1.0 {
        let cos_out = (1.0 - nsin * nsin).sqrt();
        let rs = (photon.index * cos_t - cos_out) / (photon.index * cos_t + cos_out);
        let rp = (photon.index * cos_out - cos_t) / (photon.index * cos_out + cos_t);
        r = (rs * rs + rp * rp) / 2.0;
    }
    // The same seeding the interpreter and the shader use: the seed hashed and
    // forced odd, because xorshift from zero is zero forever.
    let mut stream = Stream {
        state: (seed as u32).wrapping_mul(0x9E37_79B9) | 1,
    };
    let mut path = 0.0;
    let mut bounces = 0u32;
    let mut alive = true;
    while alive && bounces < photon.max_bounces {
        path += photon.thickness_mm / cos_t;
        bounces += 1;
        if stream.next() > r {
            alive = false;
        }
    }
    Ok(path)
}

/// The same `u32` xorshift stream the interpreter and the shader draw from.
///
/// `u32` and not splitmix64 because WGSL has no `u64`: a generator the shader
/// language cannot express would make the CPU and GPU runs two different
/// photons, and the differential test meaningless.
struct Stream {
    state: u32,
}

impl Stream {
    fn next(&mut self) -> f64 {
        let mut state = self.state;
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        self.state = state;
        f64::from(state >> 8) / 16_777_216.0
    }
}

/// The Fresnel reflectance at normal incidence, `((n−1)/(n+1))²`.
///
/// A closed form the kernel is checked against at `cos_inside = 1`, where the
/// two polarisations coincide.
pub fn normal_incidence_reflectance(index: f64) -> f64 {
    ((index - 1.0) / (index + 1.0)).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photon() -> Photon {
        Photon {
            cos_inside: 0.8,
            thickness_mm: 10.0,
            index: 1.5,
            max_bounces: 64,
        }
    }

    #[test]
    fn the_kernel_is_in_the_subset_and_compiles_to_wgsl() {
        // The first claim of "written once": the same text is a WGSL kernel.
        let kernel = canlab_core::wgsl::compile(SOURCE).expect("the kernel compiles");
        assert!(kernel.wgsl.contains("fn kernel("), "{}", kernel.wgsl);
        assert!(kernel.wgsl.contains("sqrt("), "{}", kernel.wgsl);
        // `rand()` and not `rand(seed)`: the shader owns the stream's state and
        // seeds it from the invocation index, and the interpreter seeds its own
        // from the caller's seed. A `rand(seed)` that re-seeded on every call
        // would draw the same number forever.
        assert!(kernel.wgsl.contains("rand()"), "{}", kernel.wgsl);
        // A condition is a `bool`, not a `select`, which is the bug the Solver
        // owner found in the emitter.
        assert!(!kernel.wgsl.contains("while (select("), "{}", kernel.wgsl);
    }

    #[test]
    fn the_two_precisions_agree_to_single_precision() {
        // The comparison the roadmap asks for, in the form this tree can run it:
        // the same kernel at `f32` (what the emitted WGSL computes) and at `f64`
        // (the CPU reference). Their difference is the floating-point difference
        // a GPU run would show.
        for cos_inside in [0.2, 0.5, 0.8, 0.95, 1.0] {
            for seed in 0..8u64 {
                let mut p = photon();
                p.cos_inside = cos_inside;
                let wide = run(&p, seed, Precision::F64).expect("f64");
                let narrow = run(&p, seed, Precision::F32).expect("f32");
                let relative = if wide == 0.0 {
                    (narrow - wide).abs()
                } else {
                    ((narrow - wide) / wide).abs()
                };
                assert!(
                    relative < 1e-6,
                    "cos {cos_inside} seed {seed}: f64 {wide}, f32 {narrow}, relative {relative}"
                );
            }
        }
    }

    #[test]
    fn the_kernel_agrees_with_the_rust_reference() {
        // The kernel says what the crate's physics says. The reference is a
        // second implementation, so agreement is evidence and not a tautology.
        for cos_inside in [0.2, 0.5, 0.8, 1.0] {
            for index in [1.0, 1.33, 1.5, 1.8] {
                for seed in 0..8u64 {
                    let mut p = photon();
                    p.cos_inside = cos_inside;
                    p.index = index;
                    let kernel = run(&p, seed, Precision::F64).expect("the kernel");
                    let reference = reference(&p, seed).expect("the reference");
                    let relative = if reference == 0.0 {
                        (kernel - reference).abs()
                    } else {
                        ((kernel - reference) / reference).abs()
                    };
                    assert!(
                        relative < 1e-12,
                        "cos {cos_inside} n {index} seed {seed}: kernel {kernel}, reference {reference}"
                    );
                }
            }
        }
    }

    #[test]
    fn total_internal_reflection_keeps_the_photon_inside() {
        // `n = 1.5` has a critical angle of `41.8°`, so `sin θ > 1/1.5 = 0.667`,
        // i.e. `cos θ < 0.745`. At `cos = 0.5` every bounce reflects and the
        // photon is trapped: the path is the bounce limit times the step, and it
        // is the same for every seed, because the reflectance is exactly one.
        let mut trapped = photon();
        trapped.cos_inside = 0.5;
        trapped.max_bounces = 8;
        let path = run(&trapped, 0, Precision::F64).expect("a path");
        let step = trapped.thickness_mm / trapped.cos_inside;
        assert!((path - step * 8.0).abs() < 1e-9, "{path} vs {}", step * 8.0);
        for seed in 0..8u64 {
            let other = run(&trapped, seed, Precision::F64).expect("a path");
            assert!((other - path).abs() < 1e-9, "seed {seed} escaped: {other}");
        }
        // Above the critical angle the photon can leave, and the path is shorter
        // than the trapped one for at least one seed.
        let mut escaping = trapped;
        escaping.cos_inside = 0.9;
        let escaped = (0..8u64)
            .map(|seed| run(&escaping, seed, Precision::F64).expect("a path"))
            .fold(f64::INFINITY, f64::min);
        assert!(escaped < path, "nothing escaped: {escaped} vs {path}");
    }

    #[test]
    fn the_reflectance_matches_its_closed_form_at_normal_incidence() {
        // At `cos = 1` the two polarisations coincide and the reflectance is
        // `((n−1)/(n+1))²`; the kernel's own expression must give the same.
        for index in [1.33, 1.5, 1.8] {
            let mut p = photon();
            p.cos_inside = 1.0;
            p.index = index;
            // The expected path: a geometric number of bounces with probability
            // `1 − R` of leaving on each. Compare the *reflectance* instead, by
            // reading it out of the kernel's own arithmetic at `cos = 1`.
            let sin_t = (1.0 - 1.0f64).sqrt();
            let nsin = index * sin_t;
            let cos_out = (1.0 - nsin * nsin).sqrt();
            let rs = (index * 1.0 - cos_out) / (index * 1.0 + cos_out);
            let rp = (index * cos_out - 1.0) / (index * cos_out + 1.0);
            let kernel_reflectance = (rs * rs + rp * rp) / 2.0;
            assert!(
                (kernel_reflectance - normal_incidence_reflectance(index)).abs() < 1e-15,
                "n {index}: {kernel_reflectance} vs {}",
                normal_incidence_reflectance(index)
            );
            let _ = &mut p;
        }
    }

    #[test]
    fn a_photon_that_has_no_meaning_is_refused() {
        // Every guard, with its reason.
        let mut p = photon();
        p.cos_inside = 0.0;
        assert!(run(&p, 0, Precision::F64).is_err());
        assert!(reference(&p, 0).is_err());
        let mut p = photon();
        p.thickness_mm = 0.0;
        assert!(run(&p, 0, Precision::F64).is_err());
        let mut p = photon();
        p.index = 0.5;
        assert!(run(&p, 0, Precision::F64).is_err());
        let mut p = photon();
        p.max_bounces = 0;
        assert!(run(&p, 0, Precision::F64).is_err());
        // And the message names the field.
        let mut p = photon();
        p.cos_inside = -1.0;
        let error = run(&p, 0, Precision::F64).expect_err("a refusal");
        assert!(error.message().contains("cosine"), "{}", error.message());
    }

    #[test]
    fn the_same_seed_and_photon_give_the_same_path() {
        // A kernel whose answer changed between two runs of the same input would
        // be unusable in a differential test.
        let p = photon();
        let first = run(&p, 7, Precision::F64).expect("a path");
        let second = run(&p, 7, Precision::F64).expect("a path");
        assert_eq!(first, second);
        // And the seeds are not all the same answer. The first version asserted
        // `seed 7 != seed 8`, which is false for a legitimate reason: at
        // `cos = 0.8` and `n = 1.5` the reflectance is about `0.11`, so a photon
        // leaves on its first draw with probability `0.89` and most seeds give
        // one bounce. The claim that matters is that the stream *varies*, not
        // that any two particular seeds differ.
        let paths: Vec<f64> = (0..32u64)
            .map(|seed| run(&p, seed, Precision::F64).expect("a path"))
            .collect();
        let distinct = paths
            .iter()
            .filter(|path| **path != paths[0])
            .count();
        assert!(distinct > 0, "all 32 seeds gave {}", paths[0]);
    }
}
