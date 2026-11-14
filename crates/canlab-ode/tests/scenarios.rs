//! W6.4: the three physics scenarios, each against a closed form or an
//! invariant.
//!
//! The roadmap asks for three scenarios as corpus files. The corpus is a
//! document-to-number format — an equation is solved and its answer compared —
//! and an ODE scenario is not that: it is an integration compared with a closed
//! form over a range of parameters. Putting one in the corpus would mean a
//! `document` field that is not a document. These are tests instead, and each
//! one fails when its *physics* is wrong rather than when a number is retyped:
//!
//! * **air** — a projectile with `v²` drag against the `tanh` closed form, and
//!   the terminal velocity it approaches. A sign error in the drag, or a missing
//!   `m`, moves the answer far outside the tolerance.
//! * **sound** — the speed of sound from the temperature, and the Doppler shift
//!   of a moving source. Both are one-line formulas with a known value.
//! * **water** — the 1D wave equation, checked against d'Alembert's solution and
//!   for energy conservation over `10⁴` steps.
//!
//! # Why the water scenario is not an ODE
//!
//! The wave equation is second order in *both* time and space, and the
//! integrators in this crate are for first-order systems in time. The scenario
//! is therefore a finite-difference scheme written out in the test, with its own
//! stability condition, and the ODE crate is not involved. That is the honest
//! shape: the roadmap put it here because it is a *scenario*, not because the
//! ODE crate can integrate it.

#[cfg(test)]
mod tests {
    use canlab_ode::{rk4, Trace};

    /// A projectile with quadratic drag: `v' = −g − (k/m)·v·|v|`.
    ///
    /// The closed form for a *falling* body from rest is
    /// `v(t) = −v_t·tanh(g·t/v_t)` with `v_t = √(mg/k)`, and the distance fallen
    /// is `y(t) = −(v_t²/g)·ln(cosh(g·t/v_t))`. The test integrates the
    /// equation and compares with those, which is a check of the *physics*: a
    /// sign error in the drag makes the body accelerate instead of approach a
    /// terminal speed.
    fn projectile(g: f64, mass: f64, k: f64, t_end: f64, h: f64) -> Trace {
        let drag = k / mass;
        rk4(
            &move |_t, v| -g - drag * v * v.abs(),
            0.0,
            0.0,
            t_end,
            h,
        )
        .expect("a trace")
    }

    #[test]
    fn a_falling_body_reaches_its_terminal_velocity() {
        // `g = 9.81`, `m = 1`, `k = 0.01`: `v_t = √(9.81/0.01) = 31.32 m/s`.
        let (g, mass, k) = (9.81f64, 1.0f64, 0.01f64);
        let terminal = (g * mass / k).sqrt();
        assert!((terminal - 31.32).abs() < 0.01, "{terminal}");
        // Long enough to be within a per mille of terminal.
        let trace = projectile(g, mass, k, 60.0, 0.001);
        let speed = trace.last().expect("a point").y.abs();
        assert!(
            (speed - terminal).abs() < 1e-3 * terminal,
            "the speed is {speed}, the terminal velocity is {terminal}"
        );
    }

    #[test]
    fn the_quadratic_drag_trajectory_matches_the_tanh_closed_form() {
        // The roadmap's check. `v(t) = −v_t·tanh(g·t/v_t)` at several times.
        let (g, mass, k) = (9.81f64, 1.0f64, 0.01f64);
        let terminal = (g * mass / k).sqrt();
        let trace = projectile(g, mass, k, 5.0, 0.0005);
        for t in [0.5, 1.0, 2.0, 3.0, 5.0] {
            let exact = -terminal * (g * t / terminal).tanh();
            let computed = trace.at(t).expect("a value");
            assert!(
                (computed - exact).abs() < 1e-6 * terminal,
                "at t = {t}: {computed} vs {exact}"
            );
        }
    }

    #[test]
    fn the_distance_fallen_matches_its_closed_form() {
        // `y(t) = −(v_t²/g)·ln(cosh(g·t/v_t))`. The velocity is integrated, so
        // the distance is a second integration — done here in closed form from
        // the velocity trace, which is the check that the velocity is right at
        // *every* time and not only at the end.
        let (g, mass, k) = (9.81f64, 1.0f64, 0.01f64);
        let terminal = (g * mass / k).sqrt();
        let trace = projectile(g, mass, k, 4.0, 0.0005);
        // Trapezoid over the trace.
        let mut distance = 0.0;
        for pair in trace.points.windows(2) {
            distance += 0.5 * (pair[1].t - pair[0].t) * (pair[0].y + pair[1].y);
        }
        let exact = -(terminal * terminal / g) * ((g * 4.0 / terminal).cosh()).ln();
        assert!(
            (distance - exact).abs() < 1e-4 * exact.abs(),
            "distance {distance} vs {exact}"
        );
    }

    #[test]
    fn the_speed_of_sound_follows_the_temperature() {
        // `c = 331.3·√(1 + T/273.15)` m/s. At `T = 20 °C` that is `343.2 m/s`,
        // the number a textbook quotes.
        let speed = |t: f64| 331.3 * (1.0 + t / 273.15).sqrt();
        assert!((speed(0.0) - 331.3).abs() < 1e-9);
        assert!((speed(20.0) - 343.2).abs() < 0.05, "{}", speed(20.0));
        // And it rises with temperature: 10 K warmer is about 6 m/s faster.
        assert!(speed(30.0) > speed(20.0));
        let difference = speed(30.0) - speed(20.0);
        assert!((difference - 5.9).abs() < 0.2, "{difference}");
    }

    #[test]
    fn the_doppler_shift_of_a_moving_source_is_the_closed_form() {
        // A source moving towards a stationary observer: `f' = f·c/(c − v)`.
        // At `f = 440 Hz`, `c = 343 m/s`, `v = 34.3 m/s` (a tenth of the speed
        // of sound), `f' = 440·10/9 = 488.9 Hz`.
        let observed = |f: f64, c: f64, v: f64| f * c / (c - v);
        let shifted = observed(440.0f64, 343.0f64, 34.3f64);
        assert!((shifted - 488.89).abs() < 0.01, "{shifted}");
        // Away from the observer it is lower, and the two are not symmetric:
        // `f·c/(c+v)` is `440·10/11 = 400 Hz`, not `440 − (488.9 − 440)`.
        let receding: f64 = 440.0 * 343.0 / (343.0 + 34.3);
        assert!((receding - 400.0).abs() < 0.01, "{receding}");
        assert!(
            (shifted - 440.0).abs() > (receding - 440.0).abs(),
            "approaching shifts more than receding, which is the asymmetry"
        );
    }

    #[test]
    fn the_wave_equation_matches_dalemberts_solution() {
        // The 1D wave equation `u_tt = c²·u_xx` on `[0, 1]` with fixed ends,
        // by the explicit scheme `u^{n+1}_i = 2u^n_i − u^{n−1}_i +
        // (c·dt/dx)²(u^n_{i+1} − 2u^n_i + u^n_{i−1})`. The initial shape is
        // `sin(πx)`, whose exact solution is `sin(πx)·cos(πct)` — one standing
        // mode, so the comparison is exact and the scheme's own error is the
        // only discrepancy.
        const POINTS: usize = 100;
        const C: f64 = 1.0;
        let dx = 1.0 / POINTS as f64;
        // The Courant number must be at most 1 for the scheme to be stable.
        let courant = 0.5;
        let dt = courant * dx / C;
        let steps = 10_000;
        let mut previous: Vec<f64> = (0..=POINTS)
            .map(|i| (std::f64::consts::PI * i as f64 * dx).sin())
            .collect();
        let mut current = previous.clone();
        // The first step from rest: `u^1 = u^0 + dt·v^0 + ½(c·dt/dx)²·Δu`.
        let factor = (C * dt / dx).powi(2);
        let mut next = vec![0.0; POINTS + 1];
        for i in 1..POINTS {
            next[i] = previous[i] + 0.5 * factor * (previous[i + 1] - 2.0 * previous[i] + previous[i - 1]);
        }
        previous = current.clone();
        current = next;
        for _ in 1..steps {
            let mut next = vec![0.0; POINTS + 1];
            for i in 1..POINTS {
                next[i] = 2.0 * current[i] - previous[i]
                    + factor * (current[i + 1] - 2.0 * current[i] + current[i - 1]);
            }
            previous = std::mem::replace(&mut current, next);
        }
        let t = steps as f64 * dt;
        let mut worst: f64 = 0.0;
        // The exact and computed arrays are indexed together, so the index
        // addresses both.
        #[allow(clippy::needless_range_loop)]
        for i in 0..=POINTS {
            let exact = (std::f64::consts::PI * i as f64 * dx).sin() * (std::f64::consts::PI * C * t).cos();
            worst = worst.max((current[i] - exact).abs());
        }
        assert!(worst < 0.02, "the largest deviation from d'Alembert is {worst}");
    }

    #[test]
    fn the_wave_equations_energy_does_not_drift() {
        // The roadmap's check: energy drift below 0.1 % over `10⁴` steps. The
        // discrete energy is `Σ ½((u^{n+1}−u^n)/dt)² + ½c²((u_{i+1}−u_i)/dx)²`,
        // which the scheme conserves to the rounding of the arithmetic.
        const POINTS: usize = 100;
        const C: f64 = 1.0;
        let dx = 1.0 / POINTS as f64;
        let courant = 0.5;
        let dt = courant * dx / C;
        let energy = |u: &[f64], previous: &[f64]| -> f64 {
            let mut kinetic = 0.0;
            let mut potential = 0.0;
            for i in 1..POINTS {
                kinetic += 0.5 * ((u[i] - previous[i]) / dt).powi(2);
                potential += 0.5 * C * C * ((u[i + 1] - u[i]) / dx).powi(2);
            }
            (kinetic + potential) * dx
        };
        let mut previous: Vec<f64> = (0..=POINTS)
            .map(|i| (std::f64::consts::PI * i as f64 * dx).sin())
            .collect();
        let factor = (C * dt / dx).powi(2);
        let mut next = vec![0.0; POINTS + 1];
        for i in 1..POINTS {
            next[i] = previous[i] + 0.5 * factor * (previous[i + 1] - 2.0 * previous[i] + previous[i - 1]);
        }
        let mut current = next;
        let initial = energy(&current, &previous);
        for _ in 1..10_000 {
            let mut next = vec![0.0; POINTS + 1];
            for i in 1..POINTS {
                next[i] = 2.0 * current[i] - previous[i]
                    + factor * (current[i + 1] - 2.0 * current[i] + current[i - 1]);
            }
            previous = std::mem::replace(&mut current, next);
        }
        let final_energy = energy(&current, &previous);
        let drift = ((final_energy - initial) / initial).abs();
        assert!(drift < 1e-3, "the energy drifted by {drift}");
    }

    #[test]
    fn a_wrong_drag_sign_fails_the_terminal_velocity_check() {
        // The test that makes the others meaningful: if the drag's sign were
        // flipped, the body would accelerate without bound and the terminal
        // velocity check would fail. This is the *same* comparison the first
        // test makes, against a deliberately wrong model, and it must fail.
        let (g, mass, k) = (9.81f64, 1.0f64, 0.01f64);
        let terminal = (g * mass / k).sqrt();
        let drag = k / mass;
        let wrong = rk4(
            &move |_t, v| -g + drag * v * v.abs(),
            0.0,
            0.0,
            60.0,
            0.001,
        );
        // The wrong sign does not merely miss the terminal velocity: it
        // *diverges*, and the integrator says so with a `NotFinite` error rather
        // than a trace. Either outcome shows that the check has teeth — a
        // failure is a stronger one than a wrong number — so the test accepts
        // both and asserts that the correct model does neither.
        match wrong {
            Err(_) => {}
            Ok(trace) => {
                let speed = trace.last().expect("a point").y.abs();
                assert!(
                    (speed - terminal).abs() > 1e-3 * terminal,
                    "the wrong sign passed the check: {speed} vs {terminal}"
                );
            }
        }
        // And the right sign does not diverge.
        assert!(projectile(g, mass, k, 60.0, 0.001).points.len() > 1);
    }
}
