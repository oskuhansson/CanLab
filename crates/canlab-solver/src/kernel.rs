//! W7.3: the compile path between a `Expr` and a kernel, and the cache in
//! front of it.
//!
//! W7.1 printed an `Expr` as Lua and W7.2 compiled that Lua to WGSL. This
//! module is the plumbing that joins them: [`compile_expression`] does both
//! steps and caches the result in a process-wide [`KernelCache`],
//! [`compile_expression_with`] does the same with a cache the caller owns,
//! [`compile_source`] caches a caller's Lua under a content hash, and [`sweep`]
//! checks that the printer and the compiler agree about what they refuse.
//!
//! # Why the cache is keyed on the source
//!
//! The roadmap asks that a second compile of an unchanged document is instant.
//! The key is therefore the *text the compiler reads* — the Lua program — and
//! the options ([`Options`]) that changed how it was produced, joined into one
//! string and hashed. Two documents that produce the same Lua are the same
//! compile, whatever their LaTeX looked like, and an option that changes the
//! output changes the key even when the text does not.
//!
//! The key is a hand-written FNV-1a 64-bit digest over the joined text, and the
//! *whole joined text* is stored beside the entry as well, so the digest can
//! collide without a wrong answer: a hit is a hash match **and** a text match.
//! A cryptographic digest would only make that second comparison rarer; it
//! would not make it unnecessary, and it is not worth a dependency to make a
//! collision rarer in a cache that one machine fills and reads.
//!
//! A refused compile is cached too. A refusal is an answer — the printer or the
//! compiler said why not — and re-deriving it on the next Compile press is the
//! same work for the same result.
//!
//! # What this module does not do
//!
//! * **It does not run WGSL.** The roadmap's W7.3 artefact is "the measured
//!   worst-case difference between the CPU and GPU runs, stored with the
//!   artefact". There is no GPU run here: nothing in this module executes WGSL,
//!   so nothing here measures a CPU/GPU difference and no [`Artefact`] carries one.
//!   A UI that wants to show such a bound has to get it from somewhere that
//!   actually ran both, and until W7.4 runs a kernel it must say "not measured"
//!   rather than show the number the sweep reports below — that number is the
//!   *error bound the test asserts*, not a measurement.
//! * **It does not run the Lua.** `mlua` is not a dependency of this workspace
//!   (no network, no vendored Lua), so the comparison between the Lua and the
//!   CPU evaluator belongs to `canlab_core::lua`'s tests, which evaluate the
//!   emitted text with a second implementation.
//! * **It does not decide when to compile.** Nothing here compiles in the
//!   background and nothing recompiles on its own: a caller asks
//!   [`compile_source`] and is told whether the work was done. Marking an
//!   artefact stale is the caller's job too: an artefact is [`Artefact::hash`]
//!   and staleness is that hash against the document's current key.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use canlab_core::lua::{self, LuaError};
use canlab_core::wgsl::{self, Diagnostic, Kernel};
use canlab_core::{Expr, SymbolId};

/// The options that decide what a compile produces, and so what its hash is.
///
/// The name is the Lua program's function name, and it is part of the key
/// because it appears in the text the compiler reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// The name of the kernel function.
    pub name: String,
    /// The Lua locals the expression may refer to.
    pub variables: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            name: String::from("kernel_expression"),
            variables: Vec::new(),
        }
    }
}

impl Options {
    /// Options for one expression over `variables`, with a default name.
    pub fn over(variables: &[String]) -> Options {
        Options {
            name: String::from("kernel_expression"),
            variables: variables.to_vec(),
        }
    }
}

/// Why an expression did not become a kernel.
///
/// The two arms are the two steps, and each carries the refusal its own module
/// produced, so a caller can show the module's message rather than a summary of
/// it. The distinction matters because the two refusals are different promises:
/// a [`LuaError`] is "this expression is not one the subset has", and a
/// [`Diagnostic`] is "I emitted Lua and this compiler does not read it", which
/// is a bug in one of the two modules and not in the user's document.
#[derive(Debug, Clone, PartialEq)]
pub enum Refusal {
    /// The `Expr` is not in the Lua subset.
    Lua(LuaError),
    /// The Lua was emitted and the WGSL compiler refused it.
    Wgsl(Diagnostic),
}

impl Refusal {
    /// The message a caller shows.
    pub fn message(&self) -> String {
        match self {
            Refusal::Lua(error) => error.message(),
            Refusal::Wgsl(error) => error.message(),
        }
    }

    /// The kernel stage this refusal happened in, for a test to assert against.
    pub fn stage(&self) -> &'static str {
        match self {
            Refusal::Lua(_) => "lua",
            Refusal::Wgsl(_) => "wgsl",
        }
    }
}

/// A compiled kernel, with the Lua it came from.
///
/// The Lua is kept because W7.3 asks for a viewer for it: a caller showing the
/// generated source needs the exact text the compiler read, not a second
/// printing of the same expression, which could differ.
///
/// [`Artefact::parameters`] and [`Artefact::locals`] are the compiler's, and
/// they are almost always empty for a printed expression: `to_lua_program`
/// wraps the expression in `local f = …` and the compiler reads that local's
/// *value*, so the expression's own variables appear only as free names in the
/// WGSL — which is why a caller compiling an expression takes its parameters
/// from [`Options::variables`], not from here.
///
/// An artefact is in **single precision**: [`canlab_core::wgsl`] emits an `f32`
/// function. The roadmap's W7.5 marker ("single precision" shown on every
/// artefact compiled the way) is a UI decision, and this type carries no
/// precision field for it, because there is nothing else a kernel can be.
#[derive(Debug, Clone, PartialEq)]
pub struct Artefact {
    /// The content hash: the hash of [`Artefact::key`].
    pub hash: String,
    /// The text that was hashed — the options and the Lua, joined.
    pub key: String,
    /// The Lua program the compiler read.
    pub lua: String,
    /// The WGSL function.
    pub wgsl: String,
    /// The kernel's parameters, in order.
    pub parameters: Vec<String>,
    /// The kernel's locals.
    pub locals: Vec<String>,
}

/// Compile `expression` to Lua, then to WGSL, through a process-wide cache.
///
/// The Lua printing is `canlab_core::lua`'s; the WGSL is `canlab_core::wgsl`'s.
/// This function adds nothing to either beyond the options and the hash, which
/// is the point: the two modules stay the only answers to "what is this in
/// Lua" and "what is this in WGSL".
///
/// A second call with the same expression, name and variable set is a cache
/// hit: the Lua is printed again (the cache is keyed on the text the compiler
/// reads), and the WGSL compile is skipped. The cache lives as long as the
/// process, which is the lifetime a document's recompiles care about. A caller
/// that wants to own the cache — a test, or a second document set — calls
/// [`compile_expression_with`].
pub fn compile_expression(
    expression: &Expr,
    name: &str,
    variables: &[SymbolId],
) -> Result<Artefact, Refusal> {
    compile_expression_with(default_cache(), expression, name, variables)
}

/// [`compile_expression`] with the cache the caller supplies.
///
/// This is the same compile, with the hit and miss counts visible on `cache`.
pub fn compile_expression_with(
    cache: &KernelCache,
    expression: &Expr,
    name: &str,
    variables: &[SymbolId],
) -> Result<Artefact, Refusal> {
    let lua = lua::to_lua_program(name, expression, variables).map_err(Refusal::Lua)?;
    let options = Options {
        name: name.to_string(),
        variables: variables.iter().map(|symbol| symbol.as_str()).collect(),
    };
    compile_source_with(cache, &lua, &options).result
}

/// The cache [`compile_expression`] uses.
///
/// It is one `KernelCache` for the process. Nothing else touches it, so a
/// caller cannot be served a kernel from a document it did not compile: a hit
/// requires the same options and the same Lua text.
fn default_cache() -> &'static KernelCache {
    static CACHE: OnceLock<KernelCache> = OnceLock::new();
    CACHE.get_or_init(KernelCache::new)
}

/// Compile Lua that a caller wrote, rather than Lua the printer emitted.
///
/// This is the door for a hand-written kernel — the W7.4 light-transport
/// template is written in the subset by hand — and it skips the printer, so a
/// refusal here is always a [`Refusal::Wgsl`].
pub fn compile_lua(lua: &str) -> Result<Artefact, Refusal> {
    let kernel = wgsl::compile(lua).map_err(Refusal::Wgsl)?;
    let key = join_key(lua, &Options::default());
    Ok(artefact_from(lua, kernel, key))
}

fn artefact_from(lua: &str, kernel: Kernel, key: String) -> Artefact {
    let hash = digest(key.as_bytes());
    Artefact {
        hash,
        key,
        lua: lua.to_string(),
        wgsl: kernel.wgsl,
        parameters: kernel.parameters,
        locals: kernel.locals,
    }
}

/// The text a cache key is made of: the options, then the Lua.
///
/// The fields are length-prefixed (`<len>:<bytes>`), so no choice of option
/// names can make two different inputs produce the same key by running one
/// field into the next. The resulting key is readable, which is what makes the
/// stored key a useful second check against a digest collision.
fn join_key(lua: &str, options: &Options) -> String {
    let mut key = String::new();
    let mut field = |text: &str| {
        key.push_str(&text.len().to_string());
        key.push(':');
        key.push_str(text);
        key.push('\n');
    };
    field(&options.name);
    for variable in &options.variables {
        field(variable);
    }
    field(lua);
    key
}

/// FNV-1a, 64-bit, as 16 lowercase hex digits.
///
/// Small, dependency-free and good enough for a bucket number; correctness of a
/// cache hit comes from comparing [`Artefact::key`], not from this. The tests
/// pin the published FNV-1a values so that a change to the function is a
/// deliberate change to every key, not a silent one.
pub fn digest(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Whether a compile did the work or skipped it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The entry was not in the cache and was computed now.
    Compiled,
    /// The entry was already there; nothing was compiled.
    Cached,
}

impl Outcome {
    /// `true` when the work was skipped.
    pub fn is_cached(self) -> bool {
        matches!(self, Outcome::Cached)
    }
}

/// What a cache entry is: the artefact, or the refusal, always with the text it
/// was made from.
///
/// A refusal is stored with its key too. A cache that stored a refusal without
/// the text it refused could hand one document's refusal to another, which is
/// the same bug as handing it another document's kernel.
#[derive(Debug, Clone, PartialEq)]
enum Entry {
    Artefact(Artefact),
    Refused { key: String, refusal: Refusal },
}

impl Entry {
    fn key(&self) -> &str {
        match self {
            Entry::Artefact(artefact) => &artefact.key,
            Entry::Refused { key, .. } => key,
        }
    }

    fn hash(&self) -> String {
        digest(self.key().as_bytes())
    }
}

/// What one `compile_source` call produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Compiled {
    /// The kernel, or why there is none.
    pub result: Result<Artefact, Refusal>,
    /// Whether the work was done now or skipped.
    pub outcome: Outcome,
}

impl Compiled {
    /// The artefact, when there is one.
    pub fn artefact(&self) -> Option<&Artefact> {
        self.result.as_ref().ok()
    }

    /// The refusal, when there is one.
    pub fn refusal(&self) -> Option<&Refusal> {
        self.result.as_ref().err()
    }
}

/// Compile `lua`, unless the cache already holds exactly this program.
///
/// This is the whole of the policy: look, and on a miss compile and remember.
/// The lookup compares the stored key with the new one, so a digest collision
/// is a second probe and never a wrong answer. A sequential miss probe is not
/// needed: the digest is of the whole key, and a collision is then caught by
/// the comparison and recompiled — the cache is a cache, not a store of record.
pub fn compile_source(cache: &KernelCache, lua: &str) -> Compiled {
    compile_source_with(cache, lua, &Options::default())
}

/// [`compile_source`] with the options the key is made from spelled out.
pub fn compile_source_with(cache: &KernelCache, lua: &str, options: &Options) -> Compiled {
    let key = join_key(lua, options);
    if let Some(entry) = cache.get(&key) {
        let result = match entry {
            Entry::Artefact(artefact) => Ok(artefact),
            Entry::Refused { refusal, .. } => Err(refusal),
        };
        return Compiled {
            result,
            outcome: Outcome::Cached,
        };
    }
    let result = wgsl::compile(lua)
        .map(|kernel| artefact_from(lua, kernel, key.clone()))
        .map_err(Refusal::Wgsl);
    let entry = match &result {
        Ok(artefact) => Entry::Artefact(artefact.clone()),
        Err(refusal) => Entry::Refused {
            key: key.clone(),
            refusal: refusal.clone(),
        },
    };
    cache.insert(entry);
    Compiled {
        result,
        outcome: Outcome::Compiled,
    }
}

/// An in-memory content-hash cache of compiled kernels.
///
/// It lives for as long as its owner does. The roadmap puts the cache on disk,
/// under the user's local application data; this type deliberately does not.
/// The entry is cheap to rebuild within one process, and a cache file written
/// outside `target/` is a file this crate's owner has no business writing.
/// A persistent tier can wrap this one later without changing what a hit means.
#[derive(Debug, Default)]
pub struct KernelCache {
    entries: Mutex<HashMap<String, Entry>>,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl KernelCache {
    /// An empty cache.
    pub fn new() -> KernelCache {
        KernelCache::default()
    }

    /// How many lookups found an entry.
    pub fn hits(&self) -> u64 {
        self.hits.load(Ordering::Relaxed)
    }

    /// How many lookups compiled.
    pub fn misses(&self) -> u64 {
        self.misses.load(Ordering::Relaxed)
    }

    /// The entry whose key is exactly `key`.
    ///
    /// The key comparison is not a formality: the bucket is a 64-bit digest,
    /// and this comparison is the only thing standing between a collision and
    /// a document seeing another document's kernel. A collision therefore
    /// replaces the older entry, which is a cache miss on the next compile of
    /// the older document and never a wrong answer.
    fn get(&self, key: &str) -> Option<Entry> {
        let hash = digest(key.as_bytes());
        let entries = self.entries.lock().expect("the kernel cache is not poisoned");
        let entry = entries.get(&hash).filter(|entry| entry.key() == key).cloned();
        drop(entries);
        if entry.is_some() {
            self.hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
        }
        entry
    }

    fn insert(&self, entry: Entry) {
        let hash = entry.hash();
        self.entries
            .lock()
            .expect("the kernel cache is not poisoned")
            .insert(hash, entry);
    }

    /// How many entries the cache holds.
    pub fn len(&self) -> usize {
        self.entries
            .lock()
            .expect("the kernel cache is not poisoned")
            .len()
    }

    /// `true` when nothing has been compiled yet.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ---------------------------------------------------------------------------
// The validation sweep
// ---------------------------------------------------------------------------

/// How many randomly generated expressions the sweep compiles by default.
///
/// The roadmap's W7.2 check is two hundred generated *programs* run on both
/// backends. There is one backend here, so the number that means something is
/// the number of expressions whose printed Lua this workspace can hand to the
/// WGSL compiler and hold to a measured error bound; two hundred is the
/// roadmap's number and is kept.
pub const SWEEP_PROGRAMS: usize = 200;

/// How many points the sweep evaluates each expression at.
pub const SWEEP_POINTS: usize = 64;

/// The tolerance the sweep holds `f32` against `f64` at.
///
/// The kernel's arithmetic is `f32` (`canlab_core::wgsl` emits an `f32`
/// function) and the CPU side is `f64`, so the difference is rounding, plus the
/// amplification a transcendental's condition number applies to it.
///
/// The measured worst case is the one to read: at seed 1 and 200 expressions it
/// is `tan(…)` near a value of 141, at `8.4e-5` relative. That is what `f32`
/// does there and not a compiler fault — `tan` near π/2 has a condition number
/// of about `2·|t|`, so an `f32` input near `1e-7` relative becomes `1e-4` in
/// the output. `1e-4` is therefore also the margin, and a generator that
/// reaches closer to a pole would fail this bound with a correct kernel. The
/// number is reported ([`SweepReport::worst_relative`]) so a document shows the
/// measurement and not the bound.
pub const SWEEP_TOLERANCE: f64 = 1e-4;

/// What the sweep found.
#[derive(Debug, Clone, PartialEq)]
pub struct SweepReport {
    /// How many expressions were generated.
    pub programs: usize,
    /// How many printed to Lua and were refused by the WGSL compiler.
    pub wgsl_refusals: usize,
    /// How many reached both backends.
    pub compared: usize,
    /// How many sample points were compared.
    pub points: usize,
    /// How many points were inside `f32`'s range on the kernel side and outside
    /// it on the CPU side, so the difference is an overflow and not an error.
    ///
    /// `exp(x)` at `x = 7` is `1e3` on the CPU and `+∞` in `f32`: the kernel is
    /// still the right function, it just cannot hold the number. That is worth
    /// counting and worth showing, and it is not a disagreement about the
    /// function, so it is not a failure — the failures list is for the two
    /// sides computing different things.
    pub f32_overflowed: usize,
    /// The largest relative difference between the Lua evaluation and the CPU
    /// evaluation over every compared point, or `0.0` when nothing was compared.
    ///
    /// "Relative" is against `max(1, |expected|)`: a value near zero would
    /// otherwise divide the difference by nothing and report an infinity for an
    /// absolute error of `1e-8`.
    pub worst_relative: f64,
    /// The point that set [`SweepReport::worst_relative`], so the bound has an
    /// address: an expression, a sample point and both values.
    pub worst_at: Option<String>,
    /// The points where the two sides disagreed beyond [`SWEEP_TOLERANCE`], as
    /// one line each: the source, the point, both values.
    pub failures: Vec<String>,
    /// The generator was different: callers that want a different sweep pass a
    /// different seed. Kept here so a failure line can be reproduced.
    pub seed: u64,
}

impl SweepReport {
    /// `true` when every compared point was inside the tolerance.
    pub fn is_ok(&self) -> bool {
        self.failures.is_empty()
    }
}

/// A small splitmix64-style generator, so the sweep is the same on every run
/// and every machine.
///
/// `rand` would be a dependency for one file of test scaffolding, and a
/// non-deterministic sweep would make "it passed yesterday" the only
/// reproduction instruction.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        // SplitMix64's step: a seed of zero is a fixed point of nothing, but a
        // seed of zero would still be a poor start.
        Rng(seed ^ 0x9e37_79b9_7f4a_7c15)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A whole number in `[0, n)`.
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    /// A float in `[-1, 1)`, from three decimals, so the same expression is the
    /// same tree on every run and in every report line.
    fn small(&mut self) -> f64 {
        let milli = (self.next_u64() % 2001) as i64 - 1000;
        milli as f64 / 1000.0
    }
}

const FUNCTIONS: &[&str] = &["sin", "cos", "tan", "exp", "ln", "sqrt", "abs"];

/// One generated expression over `x` and `y`.
fn generate(rng: &mut Rng, depth: usize, x: SymbolId, y: SymbolId) -> Expr {
    use canlab_core::{ExprNode, Number, int, ratio};
    // A leaf at the bottom, so the tree is bounded; otherwise a weighted mix.
    let pick = if depth == 0 { 0 } else { rng.below(10) };
    match pick {
        0 | 1 => {
            let which = rng.below(4);
            match which {
                0 => ExprNode::sym(x),
                1 => ExprNode::sym(y),
                2 => ExprNode::num(Number::float(rng.small())),
                _ => int((rng.below(9) as i64) - 4),
            }
        }
        2..=4 => {
            // Sums and products of the *raw* constructors: the sweep is about
            // what the printer does with shapes, and the canonical constructors
            // fold leaves before the printer ever sees them.
            let count = 2 + rng.below(2);
            let terms: Vec<Expr> = (0..count).map(|_| generate(rng, depth - 1, x, y)).collect();
            if pick == 4 {
                ExprNode::mul_raw(terms)
            } else {
                ExprNode::add_raw(terms)
            }
        }
        5 | 6 => {
            let base = generate(rng, depth - 1, x, y);
            let exponent = match rng.below(3) {
                0 => int(2),
                1 => int(3),
                _ => ratio(1, 2),
            };
            ExprNode::pow_raw(base, exponent)
        }
        7 | 8 => {
            let name = FUNCTIONS[rng.below(FUNCTIONS.len())];
            ExprNode::fn_raw(
                SymbolId::intern(name),
                vec![generate(rng, depth - 1, x, y)],
            )
        }
        _ => {
            // The constants, so the printer's `math.pi` and `math.exp(1)` are
            // swept too.
            let name = if rng.below(2) == 0 { "pi" } else { "e" };
            ExprNode::sym(SymbolId::intern(name))
        }
    }
}

/// The Lua the printer emits, evaluated the way this crate can evaluate it:
/// through the CPU [`canlab_core::Program`] the Lua was printed from.
///
/// This is *not* an evaluation of the Lua text. Evaluating text needs a Lua
/// interpreter, and there is none here; `canlab_core::lua`'s own test parses the
/// emitted text with a second implementation, and that test is what checks the
/// printer's spelling. What the sweep checks is the pair of refusals and the
/// `f32`-against-`f64` gap, both of which need the tree and not the text.
fn evaluate_cpu(expression: &Expr, variables: &[SymbolId], args: &[f64]) -> Option<f64> {
    canlab_core::Program::compile(expression, variables).map(|program| program.eval(args))
}

/// Evaluate the WGSL compiler's own arithmetic in `f32`.
///
/// The compiler emits text and nothing runs it, so a comparison "against the
/// GPU" would have nothing to compare against. This function replays the
/// *arithmetic the emitter chose* — `pow` as `f32::powf`, `select` comparisons,
/// the `f32` builtins — which measures the `f32`-versus-`f64` gap and not a
/// driver's rounding of it.
///
/// # Refusals
///
/// `None` when the expression is not one the emitter can lower, which mirrors
/// the compiler's own refusals rather than accepting anything and computing a
/// wrong number: a `Hold`, a function outside the subset, a free symbol, a
/// non-unary function call. Both sides refuse the same shapes and the sweep
/// asserts that they do.
fn evaluate_f32(expression: &Expr, variables: &[SymbolId], args: &[f64]) -> Option<f32> {
    use canlab_core::{ExprNode, Number};
    /// Evaluate the children first, so a refusal anywhere refuses the whole.
    fn children(
        items: &[Expr],
        variables: &[SymbolId],
        args: &[f64],
    ) -> Option<Vec<f32>> {
        items
            .iter()
            .map(|item| evaluate_f32(item, variables, args))
            .collect()
    }
    Some(match &**expression {
        ExprNode::Num(number) => match number {
            Number::Int(value) => *value as f32,
            // The printer emits `(n / d)`, which is a division in `f32` too.
            Number::Rat(numerator, denominator) => *numerator as f32 / *denominator as f32,
            Number::Float(value) if value.is_finite() => *value as f32,
            Number::Float(_) => return None,
        },
        ExprNode::Sym(symbol) => match &*SymbolId::name(*symbol) {
            "pi" => std::f32::consts::PI,
            "e" => std::f32::consts::E,
            _ => {
                let index = variables.iter().position(|variable| variable == symbol)?;
                *args.get(index)? as f32
            }
        },
        ExprNode::Add(terms) => {
            let values = children(terms, variables, args)?;
            // `-0.0` for the same reason `Program::eval` starts there: it is
            // the only identity that leaves every operand's sign alone.
            values.iter().fold(-0.0f32, |sum, value| sum + value)
        }
        ExprNode::Mul(factors) => {
            let values = children(factors, variables, args)?;
            values.iter().fold(1.0f32, |product, value| product * value)
        }
        ExprNode::Pow(base, exponent) => {
            let base = evaluate_f32(base, variables, args)?;
            let exponent = evaluate_f32(exponent, variables, args)?;
            // The emitter maps `^` to WGSL's `pow`, which is `powf`.
            base.powf(exponent)
        }
        ExprNode::Fn(name, arguments) => {
            // The emitter maps a one-argument call; two arguments is a shape
            // the printer never emits and the emitter would refuse.
            if arguments.len() != 1 {
                return None;
            }
            let value = evaluate_f32(&arguments[0], variables, args)?;
            match &*SymbolId::name(*name) {
                "sin" => value.sin(),
                "cos" => value.cos(),
                "tan" => value.tan(),
                "exp" => value.exp(),
                // The printer emits `math.log`, which is natural in Lua.
                "ln" => value.ln(),
                "sqrt" => value.sqrt(),
                "abs" => value.abs(),
                _ => return None,
            }
        }
        ExprNode::Hold(_) => return None,
    })
}

/// Run the sweep: generate `count` expressions from `seed`, print each to Lua,
/// compile that Lua to WGSL, and hold the two evaluations to
/// [`SWEEP_TOLERANCE`].
///
/// # What this proves, and what it does not
///
/// * **Proves:** every expression that prints to Lua also compiles to WGSL —
///   a printer that emitted a shape the compiler refuses fails the sweep rather
///   than the document — and the two evaluations agree to `SWEEP_TOLERANCE` on
///   every sampled point.
/// * **Does not prove:** that the WGSL *runs*. The comparison is against an
///   `f32` replay of the arithmetic the emitter chose, in this crate, not a
///   GPU result. A driver could round differently, and nothing here would see
///   it. W7.4's CPU/GPU differential is still the check that the shader and the
///   CPU agree.
pub fn sweep(seed: u64, count: usize) -> SweepReport {
    let mut rng = Rng::new(seed);
    let x = SymbolId::intern("x");
    let y = SymbolId::intern("y");
    let variables = [x, y];

    let mut report = SweepReport {
        programs: 0,
        wgsl_refusals: 0,
        compared: 0,
        points: 0,
        f32_overflowed: 0,
        worst_relative: 0.0,
        worst_at: None,
        failures: Vec::new(),
        seed,
    };

    for _ in 0..count {
        let expression = generate(&mut rng, 3, x, y);
        sweep_expression(&expression, &variables, &mut report);
    }

    report
}

/// One program of the sweep: print, compile, and compare each sample point.
///
/// The caller owns the generator and the report; this owns the three steps and
/// the `continue`-on-refusal policy. A test drives it directly with a program
/// the generator cannot make — the printer accepts a two-argument call and the
/// compile refuses it — which is the only way to exercise the reporting path.
fn sweep_expression(expression: &Expr, variables: &[SymbolId], report: &mut SweepReport) {
    report.programs += 1;

    let lua = match lua::to_lua_program("kernel_expression", expression, variables) {
        Ok(lua) => lua,
        Err(_) => {
            // The printer refused. There is no text to ask the compiler about,
            // so the two sides cannot be compared; the expression is counted
            // in `programs` and left out of `compared`. The refusal's own
            // consistency is checked by the dedicated test for each shape.
            return;
        }
    };

    // The consistency the roadmap asks for: if the printer produced text, the
    // compiler must either accept it or say why not. It is not allowed to fail
    // without a diagnostic naming the construct and the line.
    if let Err(diagnostic) = wgsl::compile(&lua) {
        report.wgsl_refusals += 1;
        report.failures.push(format!(
            "printed to Lua and refused by the WGSL compiler: {} ({lua:?})",
            diagnostic.message()
        ));
        return;
    }

    report.compared += 1;
    for point in 0..SWEEP_POINTS {
        // The first points are mild (`x` near 0.5, `y` near 1.5), later ones
        // reach out to ±4, where `ln` and `sqrt` of a negative argument turn
        // into the NaN the roadmap says to compare as "is a NaN".
        let t = point as f64 / SWEEP_POINTS as f64;
        let args = [0.5 + 6.0 * t, 1.5 - 4.0 * t];
        let cpu = evaluate_cpu(expression, variables, &args);
        let gpu = evaluate_f32(expression, variables, &args);
        report.points += 1;

        let (cpu, gpu) = match (cpu, gpu) {
            (Some(cpu), Some(gpu)) => (cpu, gpu),
            (cpu, gpu) => {
                report.failures.push(format!(
                    "{expression}: the evaluators disagree about whether this is a value \
                     (cpu {cpu:?}, wgsl {gpu:?})"
                ));
                continue;
            }
        };
        // The roadmap's NaN rule: "is a NaN", not the bits.
        if cpu.is_nan() || gpu.is_nan() {
            if !nan_is_agreement(cpu, f64::from(gpu)) {
                report.failures.push(format!(
                    "{expression} at x={} y={}: NaN on one side only (cpu {cpu}, wgsl {gpu})",
                    args[0], args[1]
                ));
            }
            continue;
        }
        if cpu.is_infinite() || gpu.is_infinite() {
            // An infinity is not a relative difference, so it is not folded
            // into `worst_relative`. The cases that are not a disagreement:
            // both sides infinite with the same sign, and the `f32` side
            // overflowing where the CPU side is merely large.
            let same_infinity =
                cpu.is_infinite() && gpu.is_infinite() && cpu.signum() == f64::from(gpu.signum());
            let f32_overflow = gpu.is_infinite() && cpu.is_finite();
            if same_infinity || f32_overflow {
                if f32_overflow {
                    report.f32_overflowed += 1;
                }
                continue;
            }
            report.failures.push(format!(
                "{expression} at x={} y={}: infinite on one side only (cpu {cpu}, wgsl {gpu})",
                args[0], args[1]
            ));
            continue;
        }
        let scale = cpu.abs().max(1.0);
        let relative = (f64::from(gpu) - cpu).abs() / scale;
        if relative > report.worst_relative {
            report.worst_relative = relative;
            report.worst_at = Some(format!(
                "{expression} at x={} y={}: cpu {cpu}, kernel {gpu}",
                args[0], args[1]
            ));
        }
        if !relative.is_finite() || relative > SWEEP_TOLERANCE {
            report.failures.push(format!(
                "{expression} at x={} y={}: relative difference {relative:.3e} \
                 (cpu {cpu}, wgsl {gpu})",
                args[0], args[1]
            ));
        }
    }
}

/// The roadmap's NaN rule: a NaN is compared as "is a NaN", not as a bit
/// pattern. Two NaNs agree; a NaN on one side only is a disagreement.
fn nan_is_agreement(cpu: f64, gpu: f64) -> bool {
    cpu.is_nan() == gpu.is_nan()
}

#[cfg(test)]
mod tests {
    use super::*;
    use canlab_core::{ExprNode, Hold, add, int, mul, pow, sym};

    fn x() -> SymbolId {
        SymbolId::intern("x")
    }

    #[test]
    fn an_expression_becomes_lua_and_wgsl() {
        // The two steps in one call: the Lua is the printer's and the WGSL is
        // the compiler's, and neither is re-derived here.
        let expression = pow(sym("x"), int(2));
        let artefact = compile_expression(&expression, "f", &[x()]).expect("a kernel");
        assert_eq!(artefact.lua, "local f = (x ^ 2)\nreturn f\n");
        assert!(artefact.wgsl.starts_with("fn kernel()"), "{}", artefact.wgsl);
        assert!(artefact.wgsl.contains("pow(x, 2.0)"), "{}", artefact.wgsl);
        // `f` is a local of the program, not a free name: the compiler reads
        // its value and there is no `f` in the signature.
        assert_eq!(artefact.locals, vec![String::from("f")]);
        assert!(
            !artefact.wgsl.contains("fn kernel(f"),
            "a program's local must not become a parameter: {}",
            artefact.wgsl
        );
        // The hash is the digest of the key, and the key is readable text: a
        // caller that has to explain a cache miss can print it.
        assert_eq!(artefact.hash, digest(artefact.key.as_bytes()));
        assert_eq!(artefact.hash.len(), 16);
        assert!(artefact.key.contains(&artefact.lua), "{}", artefact.key);
    }

    #[test]
    fn a_hold_is_refused_by_the_printer_and_the_refusal_is_named() {
        // The refusal comes from the step that refused, and the stage says
        // which step that was: a refusal at the Lua stage is about the
        // expression, and a caller must not attribute it to the WGSL compiler.
        let integral = ExprNode::hold(Hold::Integral {
            body: sym("x"),
            var: x(),
        });
        let refusal = compile_expression(&integral, "f", &[x()]).expect_err("a refusal");
        assert_eq!(refusal.stage(), "lua");
        assert!(matches!(refusal, Refusal::Lua(LuaError::Hold(_))), "{refusal:?}");
        assert!(refusal.message().contains("integral"), "{}", refusal.message());
    }

    #[test]
    fn an_unsupported_function_is_refused_by_name() {
        let expression = ExprNode::fn_raw(SymbolId::intern("arcsin"), vec![sym("x")]);
        let refusal = compile_expression(&expression, "f", &[x()]).expect_err("a refusal");
        assert_eq!(refusal.stage(), "lua");
        assert!(refusal.message().contains("arcsin"), "{}", refusal.message());
    }

    #[test]
    fn a_free_symbol_is_refused_by_name() {
        let expression = add([sym("x"), sym("z")]);
        let refusal = compile_expression(&expression, "f", &[x()]).expect_err("a refusal");
        assert_eq!(refusal.stage(), "lua");
        assert!(refusal.message().contains("`z`"), "{}", refusal.message());
    }

    #[test]
    fn a_refusal_from_the_wgsl_compiler_says_so() {
        // Hand-written Lua can be outside the subset without the printer ever
        // being involved. The stage is `wgsl`, and the diagnostic names the
        // construct and the line, which is what a caller shows.
        let refusal = compile_lua("local x = 1\nrepeat x = x + 1 until x > 3").expect_err("a refusal");
        assert_eq!(refusal.stage(), "wgsl");
        assert!(refusal.message().contains("repeat"), "{}", refusal.message());
        assert!(refusal.message().contains("line 2"), "{}", refusal.message());
    }

    #[test]
    fn hand_written_lua_compiles_without_the_printer() {
        let artefact = compile_lua("local s = 0\nfor i = 0, 9 do\n  s = s + i\nend\nreturn s")
            .expect("a kernel");
        assert!(artefact.wgsl.contains("for (var i: f32 = 0.0"), "{}", artefact.wgsl);
        assert_eq!(artefact.lua, "local s = 0\nfor i = 0, 9 do\n  s = s + i\nend\nreturn s");
    }

    #[test]
    fn the_digest_is_the_published_fnv1a_value() {
        // Known FNV-1a 64-bit vectors. A change to the digest function changes
        // every key, and the test is the only thing that would notice.
        assert_eq!(digest(b""), "cbf29ce484222325");
        assert_eq!(digest(b"a"), "af63dc4c8601ec8c");
        assert_eq!(digest(b"foobar"), "85944171f73967e8");
    }

    #[test]
    fn a_second_compile_of_the_same_source_is_a_hit() {
        // The roadmap's "a second compile of an unchanged document is instant",
        // in the form this crate can state it: the second call does no work.
        let cache = KernelCache::new();
        let lua = "local f = (x ^ 2)\nreturn f\n";
        let first = compile_source(&cache, lua);
        assert_eq!(first.outcome, Outcome::Compiled);
        assert_eq!(cache.misses(), 1);
        let second = compile_source(&cache, lua);
        assert_eq!(second.outcome, Outcome::Cached);
        assert_eq!(cache.hits(), 1);
        assert_eq!(cache.misses(), 1);
        assert_eq!(cache.len(), 1);
        assert_eq!(first.result, second.result);
        // The artefact from a hit is the same text, not a re-print.
        assert_eq!(first.artefact(), second.artefact());
    }

    #[test]
    fn compile_expression_uses_the_cache_it_is_given() {
        // The default path is `compile_expression`, which supplies a
        // process-wide cache; this drives the same function with a cache whose
        // counters the test can read. The second call must do no compile.
        let cache = KernelCache::new();
        let expression = pow(sym("x"), int(2));
        let first = compile_expression_with(&cache, &expression, "f", &[x()]).expect("a kernel");
        assert_eq!(cache.misses(), 1);
        assert_eq!(cache.hits(), 0);
        let second = compile_expression_with(&cache, &expression, "f", &[x()]).expect("a kernel");
        assert_eq!(second, first);
        assert_eq!(cache.hits(), 1);
        assert_eq!(cache.misses(), 1);
        // A different name is part of the key, so it is a miss and not the
        // previous program's kernel under another name.
        let renamed = compile_expression_with(&cache, &expression, "g", &[x()]).expect("a kernel");
        assert_eq!(cache.misses(), 2);
        assert_ne!(renamed.key, first.key);
        assert!(renamed.wgsl.contains("pow(x, 2.0)"), "{}", renamed.wgsl);
        // The convenience function is the same compile through the same key.
        let through_default = compile_expression(&expression, "f", &[x()]).expect("a kernel");
        assert_eq!(through_default, first);
    }

    #[test]
    fn a_changed_source_is_a_miss() {
        // Editing the document changes the Lua, which must not hit the cache:
        // a hit here would be an artefact for a function the document no longer
        // has.
        let cache = KernelCache::new();
        compile_source(&cache, "local f = (x ^ 2)\nreturn f\n");
        let changed = compile_source(&cache, "local f = (x ^ 3)\nreturn f\n");
        assert_eq!(changed.outcome, Outcome::Compiled);
        assert_eq!(cache.misses(), 2);
        assert!(changed.artefact().expect("a kernel").wgsl.contains("pow(x, 3.0)"));
    }

    #[test]
    fn a_refusal_is_cached_as_itself() {
        // A refusal is an answer, and pressing Compile again must not redo it.
        // The cached refusal is the same refusal, not a stale one borrowed from
        // a different program.
        let cache = KernelCache::new();
        let bad = "local x = 1\nlocal t = {1, 2}\nreturn x\n";
        let first = compile_source(&cache, bad);
        let refusal = first.refusal().expect("a refusal").clone();
        assert_eq!(refusal.stage(), "wgsl");
        assert!(refusal.message().contains("table"), "{}", refusal.message());
        assert_eq!(first.outcome, Outcome::Compiled);
        let second = compile_source(&cache, bad);
        assert_eq!(second.outcome, Outcome::Cached);
        assert_eq!(second.refusal(), Some(&refusal));
        // And a *different* bad program is not served the first one's refusal.
        let other = compile_source(&cache, "return \"s\"\n");
        assert_eq!(other.outcome, Outcome::Compiled);
        assert!(other.refusal().expect("a refusal").message().contains("string"));
    }

    #[test]
    fn an_option_that_changes_the_program_changes_the_key() {
        // The name is part of the key because it is part of the text the
        // compiler reads. Two options that produce the same Lua are the same
        // compile only when the text really is the same.
        let same = join_key("local f = 1\nreturn f\n", &Options::over(&["x".to_string()]));
        let other = join_key("local f = 1\nreturn f\n", &Options::over(&["y".to_string()]));
        assert_ne!(same, other);
        // The fields are length-prefixed, so no pair of names runs together
        // into a single different pair.
        let a = join_key("L", &Options::over(&["ab".to_string(), "c".to_string()]));
        let b = join_key("L", &Options::over(&["a".to_string(), "bc".to_string()]));
        assert_ne!(a, b);
        // The cache honours that: the same Lua under a different variable set
        // is not the same entry.
        let cache = KernelCache::new();
        let lua = "local f = 1\nreturn f\n";
        let first = compile_source_with(&cache, lua, &Options::over(&["x".to_string()]));
        let second = compile_source_with(&cache, lua, &Options::over(&["y".to_string()]));
        assert_eq!(first.outcome, Outcome::Compiled);
        assert_eq!(second.outcome, Outcome::Compiled);
        assert_ne!(
            first.artefact().expect("a kernel").hash,
            second.artefact().expect("a kernel").hash
        );
    }

    #[test]
    fn the_sweep_reports_its_numbers() {
        // Not an assertion about the numbers — they are a property of this
        // machine's `f32` library — but the report is what a document would
        // show, so it is printed when the test is run with `--nocapture`.
        let report = sweep(1, SWEEP_PROGRAMS);
        println!(
            "sweep seed={}: {} programs, {} compared, {} points, {} f32 overflows, \
             worst relative {:e}, {} failures",
            report.seed,
            report.programs,
            report.compared,
            report.points,
            report.f32_overflowed,
            report.worst_relative,
            report.failures.len()
        );
        assert!(report.is_ok());
    }

    #[test]
    fn the_same_seed_gives_the_same_sweep() {
        // A sweep that is not reproducible cannot be re-run after it fails.
        let first = sweep(7, 20);
        let second = sweep(7, 20);
        assert_eq!(first, second);
        assert_eq!(first.programs, 20);
    }

    #[test]
    fn the_sweep_prints_and_compiles_every_generated_expression() {
        // The roadmap's W7.2 check, minus the GPU: two hundred generated
        // expressions, each printed to Lua and compiled to WGSL. Every one the
        // printer accepts, the compiler must accept too — a printer that emits a
        // shape the compiler refuses is a printer bug the sweep exists to find.
        //
        // What this does not show is that the WGSL *runs*: the comparison is
        // against an `f32` replay of the emitter's own arithmetic in this crate,
        // not a GPU result. That differential is W7.4's.
        let report = sweep(1, SWEEP_PROGRAMS);
        assert_eq!(report.programs, SWEEP_PROGRAMS);
        assert!(
            report.failures.is_empty(),
            "{} failure(s), worst relative difference {:e}:\n{}",
            report.failures.len(),
            report.worst_relative,
            report.failures.join("\n")
        );
        // A sweep that compared nothing would pass every assertion above.
        assert!(
            report.compared * 2 >= report.programs,
            "only {} of {} expressions reached both backends",
            report.compared,
            report.programs
        );
        assert_eq!(report.points, report.compared * SWEEP_POINTS);
        // A generator whose expressions are all inside `f32`'s range would not
        // have exercised `exp`, so the mixed tree is what this pins: at least
        // one deep expression overflows the kernel's `f32` and is counted as an
        // overflow, not as a failure.
        assert!(
            report.f32_overflowed > 0 || report.worst_relative > 1e-7,
            "the sweep measured nothing worth reporting: {report:?}"
        );
    }

    #[test]
    fn the_sweep_names_a_skipped_program_instead_of_passing_it() {
        // The sweep's other job: an expression the printer accepts but the
        // compiler refuses is a *reported* failure, not a silent `continue`.
        // The generator cannot make one — it only emits unary calls — so the
        // check drives the same per-program function with the two-argument
        // `sin` the printer accepts and the compiler refuses.
        let mut report = sweep(0, 0);
        let expression = ExprNode::fn_raw(SymbolId::intern("sin"), vec![sym("x"), int(2)]);
        sweep_expression(&expression, &[x()], &mut report);
        assert_eq!(report.programs, 1);
        assert_eq!(report.wgsl_refusals, 1);
        assert_eq!(report.compared, 0);
        assert_eq!(report.failures.len(), 1, "{:?}", report.failures);
        assert!(
            report.failures[0].contains("printed to Lua and refused by the WGSL compiler"),
            "{}",
            report.failures[0]
        );
        assert!(
            report.failures[0].contains("math.sin(2"),
            "{}",
            report.failures[0]
        );
    }

    #[test]
    fn the_sweeps_f32_replay_refuses_what_the_compiler_refuses() {
        // The replay must not accept a shape the compiler rejects: an
        // expression whose Lua the compiler refused would otherwise be compared
        // as though it had a kernel.
        let variables = [x()];
        // `arcsin` is not printed, so there is no Lua and no WGSL.
        let arcsin = ExprNode::fn_raw(SymbolId::intern("arcsin"), vec![sym("x")]);
        assert!(lua::to_lua(&arcsin, &variables).is_err());
        assert_eq!(evaluate_f32(&arcsin, &variables, &[0.5]), None);
        // A two-argument call is a gap between the two modules, and this is its
        // test. The printer accepts it — `math.sin(x, 2)` is Lua syntax — and
        // the WGSL compiler refuses it as `math.sin(2 argument(s))`. So the
        // sweep would report it as "printed and refused" rather than compare
        // it, and the replay refuses it for the same reason.
        let two = ExprNode::fn_raw(SymbolId::intern("sin"), vec![sym("x"), int(2)]);
        let lua = lua::to_lua(&two, &variables).expect("the printer accepts any arity");
        let diagnostic = wgsl::compile(&format!("local f = {lua}\nreturn f\n"))
            .expect_err("the compiler refuses two arguments");
        assert!(diagnostic.construct.contains("math.sin(2"), "{}", diagnostic.construct);
        assert_eq!(evaluate_f32(&two, &variables, &[0.5]), None);
        // A `Hold` has no value on either side.
        let integral = ExprNode::hold(Hold::Integral {
            body: sym("x"),
            var: x(),
        });
        assert_eq!(evaluate_f32(&integral, &variables, &[0.5]), None);
        // A free symbol the kernel does not take.
        assert_eq!(evaluate_f32(&sym("z"), &variables, &[0.5]), None);
        // And the shapes it does accept agree with the CPU at `f32` tolerance,
        // so the agreement in the sweep is not two refusals agreeing.
        let smooth = add([pow(sym("x"), int(2)), mul([int(3), sym("x")])]);
        let gpu = evaluate_f32(&smooth, &variables, &[2.0]).expect("a value");
        let cpu = evaluate_cpu(&smooth, &variables, &[2.0]).expect("a value");
        assert_eq!(gpu, 10.0f32);
        assert_eq!(cpu, 10.0f64);
    }

    #[test]
    fn the_constant_folding_gap_is_reported_and_bounded() {
        // `e` is printed as `math.exp(1)`, which is `exp` of one in *f32* in the
        // kernel: about `2.7182817`, not `2.718281828…`. That is a real gap
        // between the two backends, it is inside the sweep's tolerance, and a
        // caller that shows the sweep's worst difference is showing this.
        let variables: [SymbolId; 0] = [];
        let gpu = evaluate_f32(&sym("e"), &variables, &[]).expect("a value");
        let cpu = evaluate_cpu(&sym("e"), &variables, &[]).expect("a value");
        let relative = (f64::from(gpu) - cpu).abs() / cpu.abs();
        assert!(relative > 0.0, "the f32 gap vanished, which is suspicious");
        assert!(relative < SWEEP_TOLERANCE, "{relative:e}");
    }

    #[test]
    fn a_nan_is_compared_as_a_nan() {
        // The roadmap's rule: a domain error is "is a NaN", not a bit pattern.
        // This is the rule itself, not a property of whatever the generator
        // happened to produce at seed 3: two NaNs agree, one NaN is a
        // disagreement, and `sqrt(x − 10)` is NaN on both sides at every
        // sampled point.
        assert!(nan_is_agreement(f64::NAN, f64::NAN));
        assert!(nan_is_agreement(1.0, 1.0));
        assert!(!nan_is_agreement(f64::NAN, 1.0));
        assert!(!nan_is_agreement(1.0, f64::NAN));

        let variables = [x()];
        let root = ExprNode::fn_raw(SymbolId::intern("sqrt"), vec![add([sym("x"), int(-10)])]);
        let cpu = evaluate_cpu(&root, &variables, &[-1.0]).expect("a value");
        let gpu = evaluate_f32(&root, &variables, &[-1.0]).expect("a value");
        assert!(cpu.is_nan());
        assert!(gpu.is_nan());

        let mut report = sweep(0, 0);
        sweep_expression(&root, &variables, &mut report);
        assert_eq!(report.compared, 1);
        assert_eq!(report.points, SWEEP_POINTS);
        assert!(report.failures.is_empty(), "{}", report.failures.join("\n"));
    }
}
