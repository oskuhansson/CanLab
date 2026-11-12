//! The fixture the detector simulation is checked against.
//!
//! `generate_events` writes `can-sat/sim/muon/events.json` from this crate's
//! transport. This test reads it back and checks that every event obeys the
//! physics the file claims: the cone angle is `arccos(1/n)`, the flight time is
//! the net displacement times the index over `c`, and every exit point is on
//! the block's surface. A fixture that is wrong is worse than no fixture,
//! because the consumer's tests then pin the simulation's *bugs*.
//!
//! The path is resolved from the crate, not from the working directory, so the
//! test runs from any directory.

use std::path::PathBuf;

use canlab_optics::{Block, Cherenkov, Tropical};

fn fixture_path() -> PathBuf {
    // crates/canlab-optics -> the workspace root -> can-sat, which is a sibling
    // of `cas` in this checkout.
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = crate_dir
        .ancestors()
        .find(|path| path.join("Cargo.toml").is_file() && path.join("crates").is_dir())
        .expect("the workspace root");
    root.parent()
        .expect("the checkout root")
        .join("can-sat/sim/muon/events.json")
}

/// Read the bracketed array that follows a key, as a string.
fn array<'a>(entry: &'a str, key: &str) -> Option<&'a str> {
    let start = entry.find(&format!("\"{key}\""))? + key.len() + 2;
    let rest = &entry[start..];
    let open = rest.find('[')?;
    let close = rest[open..].find(']')? + open;
    Some(&rest[open..=close])
}

/// Read a number after a key in the flat JSON the generator writes.
fn field<'a>(entry: &'a str, key: &str) -> Option<&'a str> {
    let start = entry.find(&format!("\"{key}\""))? + key.len() + 2;
    let rest = &entry[start..];
    let colon = rest.find(':')? + 1;
    let value = rest[colon..].trim_start();
    let end = value.find(|c: char| c == ',' || c == '}')?;
    Some(value[..end].trim().trim_matches('"'))
}

#[test]
fn every_generated_event_obeys_the_physics_it_claims() {
    let path = fixture_path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        // The fixture is generated, not committed data that the crate owns; a
        // checkout without `can-sat` is a checkout this test has nothing to
        // say about.
        eprintln!("skipping: no fixture at {}", path.display());
        return;
    };
    assert!(text.contains("\"cansat-muon-events/1\""), "schema tag");
    let index: f64 = field(&text, "index").expect("index").parse().expect("a number");
    let radius: f64 = field(&text, "radius_mm").expect("radius").parse().expect("a number");
    let thickness: f64 = field(&text, "thickness_mm").expect("thickness").parse().expect("a number");
    let block = Block::new(Tropical::fits_in(radius, thickness).expect("the fixture's geometry"));
    let cone = Cherenkov::new(index, 0.0).expect("index").angle_at_unit_beta();

    let mut events = 0;
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with('{') || !line.contains("\"muon\"") {
            continue;
        }
        events += 1;
        let theta: f64 = field(line, "theta_deg").expect("theta").parse().expect("a number");
        assert!(
            (theta.to_radians() - cone).abs() < 1e-6,
            "the cone angle in an event is {theta}, the index gives {}",
            cone.to_degrees()
        );
        let path_mm: f64 = field(line, "path_mm").expect("path").parse().expect("a number");
        assert!(path_mm > 0.0, "a photon that nowhere is not an event: {line}");
        if let Some(exit) = array(line, "exit_mm") {
            // The exit point must be on the surface: the signed distance is
            // within rounding of zero. The fixture writes it with six decimals.
            let numbers: Vec<f64> = exit
                .trim_start_matches('[')
                .trim_end_matches(']')
                .split(',')
                .map(|value| value.trim().parse().expect("a number"))
                .collect();
            assert_eq!(numbers.len(), 3, "{exit}");
            let distance = block
                .outline
                .signed_distance(numbers[0], numbers[1], numbers[2]);
            assert!(distance.abs() < 1e-4, "exit is not on the surface: {distance} in {line}");
        }
        if let Some(time_ps) = field(line, "time_ps") {
            let time: f64 = time_ps.parse().expect("a number");
            let net_mm: f64 = field(line, "net_mm").expect("net").parse().expect("a number");
            // t = net·n/c, so net = t·c/n. Two independent transcriptions of the
            // same constant agree here or the fixture's clock is wrong.
            let expected = net_mm * 1e-3 * index / 299_792_458.0 * 1e12;
            assert!(
                (time - expected).abs() < 1e-3,
                "time {time} ps but {expected} ps from the displacement"
            );
        }
    }
    assert!(events >= 10, "only {events} events in the fixture");
}
