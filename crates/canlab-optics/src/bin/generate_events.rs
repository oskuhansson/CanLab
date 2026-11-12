//! Write reference events for the muon-detector simulation, computed by this
//! crate's transport rather than by a second copy of it.
//!
//! ```text
//! cargo run -p canlab-optics --bin generate_events -- <spec.json> <events.json>
//! ```
//!
//! `events.json` is a **regression fixture**: the simulation that consumes it
//! must reproduce these photons, because they were produced by the optics crate
//! the CAS itself uses. If the two disagree, one of them has drifted from the
//! physics — and this is the file that says which numbers to compare.
//!
//! # What is modelled
//!
//! One muon crosses the block. Each photon leaves the Cherenkov cone at the
//! cone angle for the block's index, and travels inside the block:
//!
//! 1. it meets a face, and Fresnel decides whether it escapes;
//! 2. if it does not, it reflects inside (total internal reflection, which for a
//!    block in air at any angle above the critical angle is every bounce), and
//!    the path continues from the face;
//! 3. `absorbed` reports a photon that is still trapped after `MAX_BOUNCES`,
//!    with the path it travelled. That is the honest record of light the
//!    sensors never see.
//!
//! A rough block only ever has total internal reflection at angles above the
//! critical one; below it, part of the light escapes and this fixture records
//! that photon as leaving through the face it met.
//!
//! Time of flight is `path·n/c` in the block, and the fixture carries it: the
//! whole study is about picoseconds and time is the one quantity with no other
//! home.

use std::io::Write;

use canlab_optics::block::Face;
use canlab_optics::{Block, Cherenkov, Tropical, Vec3, fresnel};

const SPEED_OF_LIGHT_M_PER_S: f64 = 299_792_458.0;
const MAX_BOUNCES: usize = 64;

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let spec_path = args.next().unwrap_or_else(|| "sim/muon/spec.json".into());
    let out_path = args.next().unwrap_or_else(|| "sim/muon/events.json".into());
    let spec = std::fs::read_to_string(&spec_path)?;
    let radius = number(&spec, "radius_mm").unwrap_or(20.0);
    let thickness = number(&spec, "thickness_mm").unwrap_or(10.0);
    let index = number(&spec, "index").unwrap_or(1.84);
    let outside = number(&spec, "index_outside").unwrap_or(1.0);
    let block = Block::new(Tropical::fits_in(radius, thickness).expect("the spec's geometry"));
    let medium = Cherenkov::new(index, 0.0).expect("the spec's index");
    let theta = medium.angle_at_unit_beta();

    let mut events: Vec<String> = Vec::new();
    let mut written = 0;
    // Twelve muons, each one a photon launched around the cone. A single muon
    // sprays many photons in reality; the fixture needs directions, not counts.
    for muon in 0..12 {
        let azimuth = std::f64::consts::TAU * muon as f64 / 12.0;
        let direction = Vec3::new(
            theta.sin() * azimuth.cos(),
            theta.sin() * azimuth.sin(),
            theta.cos(),
        )
        .normalized()
        .expect("unit");
        // The photon starts *inside* the block. Starting it on the bottom face
        // made `Block::exit` skip that face (it refuses the plane a ray starts
        // on, so a bouncing photon cannot re-hit its own wall) and the first
        // recorded exit was off the surface entirely — which the fixture test
        // caught by checking the exit's signed distance.
        let start = Vec3::new(0.0, 0.0, -thickness / 2.0 + 0.05);
        let mut position = start;
        let mut heading = direction;
        let mut path = 0.0;
        let mut escaped = None;
        let mut bounced = 0;
        for _ in 0..MAX_BOUNCES {
            let Some(exit) = block.exit(position, heading) else {
                break;
            };
            if exit.distance <= 1e-9 {
                // The photon is on the face it is leaving; move it off rather
                // than counting a zero-length step as a bounce.
                position = exit.point + normal_epsilon(exit.normal);
                continue;
            }
            path += exit.distance;
            let outcome = fresnel::interface(
                heading,
                exit.normal,
                outside,
                index,
                fresnel::Polarisation::S,
            )
            .expect("valid rays");
            if outcome.total_internal_reflection {
                bounced += 1;
                // The photon reflects: a new heading, and a position nudged off
                // the face *along that new heading*. Stepping along the old
                // heading is what a first version did, and since the ray
                // continues straight while never leaving the plane, the next
                // exit was computed from a point that was not on the surface.
                heading = heading.reflect(exit.normal).normalized().expect("unit");
                position = exit.point + heading.scale(1e-6);
                continue;
            }
            escaped = Some(exit);
            break;
        }
        let face = escaped.map(|exit| match exit.face {
            Face::Top => "top",
            Face::Bottom => "bottom",
            Face::Side(_) => "side",
        });
        let exit_point = escaped.map(|exit| exit.point);
        // The time the fixture reports is not the sum of bounces — light
        // travels in straight lines between them, and a photon that took three
        // rebound steps arrived along the same net displacement as one that
        // took none. `path` is the *optical* path length (distance travelled in
        // the medium), `time` is the *net* flight time: straight-line
        // displacement times the group index. Reporting the bounce sum as the
        // time would make the fixture's own physics wrong, which a first draft
        // did.
        let displacement = exit_point
            .map(|p| (p - start).length())
            .unwrap_or(path);
        let to_seconds = displacement * 1e-3 * index / SPEED_OF_LIGHT_M_PER_S;
        events.push(format!(
            "  {{\"muon\": {muon}, \"azimuth_deg\": {:.3}, \"theta_deg\": {:.5}, \"entry_mm\": [0.000, 0.000, {:.3}], \"exit_mm\": {}, \"face\": {}, \"path_mm\": {:.6}, \"net_mm\": {:.6}, \"time_ps\": {:.6}, \"bounces\": {bounced}}}\n",
            azimuth.to_degrees(),
            theta.to_degrees(),
            -thickness / 2.0,
            match exit_point {
                Some(p) => format!("[{:.6}, {:.6}, {:.6}]", p.x, p.y, p.z),
                None => String::from("null"),
            },
            match face {
                Some(name) => format!("\"{name}\""),
                None => String::from("null"),
            },
            path,
            displacement,
            to_seconds * 1e12,
        ));
    }
    // Joined with a comma and a newline each: the first version appended a
    // trailing newline into one `String`, which produced an array with no
    // commas between its objects and a fixture no JSON reader could open.
    let events = events.join(",\n");
    let document = format!(
        "{{\n  \"schema\": \"cansat-muon-events/1\",\n  \"parameters\": {{\"radius_mm\": {radius}, \"thickness_mm\": {thickness}, \"index\": {index}, \"index_outside\": {outside}, \"theta_deg\": {theta_deg}}},\n  \"events\": [\n{events}\n  ]\n}}\n",
        theta_deg = theta.to_degrees(),
    );
    let mut file = std::fs::File::create(&out_path)?;
    file.write_all(document.as_bytes())?;
    println!("wrote {out_path}: {written} events, cone at {:.2} degrees", theta.to_degrees());
    Ok(())
}

/// A nudge off a face, along its outward normal.
fn normal_epsilon(normal: Vec3) -> Vec3 {
    normal.scale(1e-7)
}

/// Read a top-level number from the spec's flat JSON, without a JSON parser.
fn number(document: &str, key: &str) -> Option<f64> {
    let start = document.find(&format!("\"{key}\""))? + key.len() + 2;
    let rest = &document[start..];
    let colon = rest.find(':')? + 1;
    let value = rest[colon..].trim_start();
    let end = value.find(|c: char| c == ',' || c == '}' || c == '\n')?;
    value[..end].trim().parse().ok()
}
