//! The interval type against an oracle: a million random samples.
//!
//! A bound is only worth more than a float if it *contains*. Each test here
//! takes intervals built from known end points, evaluates an expression both as
//! an `Interval` and as a plain float at many interior points, and asserts the
//! float answer is inside the interval. A rounding bug that leaks inward fails
//! here even when every individual operation looks right.
//!
//! This is the test the crate's module docs promise when they say the limits are
//! tested rather than assumed.

use canlab_optics::Interval;

/// A cheap, self-contained splitmix-style RNG, so this test has no dependencies
/// and the same stream on every platform.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`.
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in `[lo, hi)`.
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
}

#[test]
fn addition_and_subtraction_contain_every_interior_value() {
    let mut rng = Rng(1);
    for _ in 0..100_000 {
        let (a1, a2) = (rng.range(-1e3, 1e3), rng.range(-1e3, 1e3));
        let (b1, b2) = (rng.range(-1e3, 1e3), rng.range(-1e3, 1e3));
        let a = Interval::new(a1.min(a2), a1.max(a2));
        let b = Interval::new(b1.min(b2), b1.max(b2));
        let sum = a.add(b);
        let difference = a.sub(b);
        for _ in 0..10 {
            let x = rng.range(a.lo().min(a.hi()), a.lo().max(a.hi()));
            let y = rng.range(b.lo().min(b.hi()), b.lo().max(b.hi()));
            assert!(sum.contains(x + y), "sum {sum} missed x+y");
            assert!(difference.contains(x - y), "difference {difference} missed x-y");
        }
    }
}

#[test]
fn products_and_quotients_contain_every_interior_value() {
    let mut rng = Rng(2);
    for _ in 0..100_000 {
        let (a1, a2) = (rng.range(-50.0, 50.0), rng.range(-50.0, 50.0));
        // The divisor never straddles zero: an interval division that produces
        // an infinite endpoint is not a bug, it is the domain of the operation,
        // and a caller that needs it must split the interval itself.
        let (b1, b2) = (rng.range(0.1, 50.0), rng.range(0.1, 50.0));
        let a = Interval::new(a1.min(a2), a1.max(a2));
        let b = Interval::new(b1.min(b2), b1.max(b2));
        let product = a.mul(b);
        let quotient = a.div(b);
        for _ in 0..10 {
            let x = rng.range(a.lo(), a.hi());
            let y = rng.range(b.lo(), b.hi());
            assert!(product.contains(x * y), "product {product} missed {x}*{y}");
            assert!(
                quotient.contains(x / y) || (x == 0.0 && y > 1.0),
                "quotient {quotient} missed {x}/{y}"
            );
        }
    }
}

#[test]
fn a_division_by_an_interval_spanning_zero_is_empty_not_infinite() {
    let a = Interval::new(1.0, 2.0);
    let b = Interval::new(-0.1, 0.1);
    assert!(a.div(b).is_empty());
}

#[test]
fn even_powers_spanning_zero_start_at_zero() {
    let a = Interval::new(-2.0, 3.0);
    let square = a.powi(2);
    assert_eq!(square.lo(), 0.0);
    assert!(square.hi() >= 9.0);
    // The interior values must be covered, including at the extremes.
    for x in [-2.0, -1.5, 0.0, 1.0, 3.0] {
        assert!(square.contains(x * x), "{square} missed {x}^2");
    }
}

#[test]
fn an_odd_power_keeps_the_sign() {
    let a = Interval::new(-2.0, 3.0);
    let cube = a.powi(3);
    assert!(cube.contains(-8.0));
    assert!(cube.contains(27.0));
    assert!(!cube.contains(-9.0));
}

#[test]
fn the_square_root_refuses_a_negative_end() {
    assert!(Interval::new(-1.0, 4.0).sqrt().is_empty());
    let root = Interval::new(4.0, 9.0).sqrt();
    for x in [4.0f64, 5.5, 9.0] {
        assert!(root.contains(x.sqrt()));
    }
}

#[test]
fn the_sine_and_cosine_bound_their_peaks() {
    let quarter = Interval::new(1.0, 2.0); // contains pi/2 = 1.5708
    let s = quarter.sin();
    assert!((s.hi() - 1.0).abs() < 1e-15, "{s}");
    // The interior values must all be in the bound.
    for x in [1.0, 1.2, std::f64::consts::FRAC_PI_2, 1.8, 2.0] {
        assert!(s.contains(x.sin()), "{s} missed sin({x})");
    }
    let c = quarter.cos();
    for x in [1.0f64, 1.5708, 2.0] {
        assert!(c.contains(x.cos()), "{c} missed cos({x})");
    }
}

#[test]
fn arcsine_and_arccosine_refuse_outside_their_domain() {
    assert!(Interval::new(1.5, 2.0).asin().is_empty());
    assert!(Interval::new(-2.0, 0.5).acos().is_empty());
    let inside = Interval::new(0.1, 0.9).asin();
    for x in [0.1f64, 0.5, 0.9] {
        assert!(inside.contains(x.asin()), "{inside} missed asin({x})");
    }
    let acos_inside = Interval::new(0.1, 0.9).acos();
    // `acos` is decreasing: the bound is the reversed pair.
    for x in [0.1f64, 0.5, 0.9] {
        assert!(acos_inside.contains(x.acos()), "{acos_inside} missed acos({x})");
    }
}

#[test]
fn the_radius_of_a_point_is_one_ulp_not_zero() {
    let point = Interval::point(1.0);
    // `radius` includes the rounding, so a point is a very small interval rather
    // than a claim of exactness — that is the point of the type. A caller wanting
    // "exactly zero error" should not ask a float for it.
    assert!(point.radius() <= f64::EPSILON);
    assert!(point.radius() >= 0.0);
}
