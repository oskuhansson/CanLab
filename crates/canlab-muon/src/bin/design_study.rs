//! Five designs, one table, and the trade each one makes.
//!
//! ```text
//! cargo run --release -p canlab-muon --bin design_study
//! ```
//!
//! The columns are the constraints a team actually trades: what it costs, what it
//! weighs, how many sensors it needs, how much light it gets, whether the sensor
//! fills up, what angle it can measure, and how many muons the flight gives it to
//! measure with.
//!
//! # Reading the table
//!
//! * **`angle`** is the angular uncertainty in degrees, and `-` means the design
//!   has no direction at all. A number in parentheses is a *geometric estimate*
//!   that no simulation in this crate has checked.
//! * **`sat`** is `yes` when the light fills the sensor. It is not automatically
//!   bad: a tracker reads out which pixel fired, so a clipped pulse is still a
//!   hit. The column that matters is `matters`, which is `yes` only where the
//!   pulse height is the measurement.
//! * **`muons`** is the crossings a 100 s flight gives through the plane. It is
//!   the same for two designs with the same plane, because the flux does not care
//!   what is behind it.
//!
//! # What the table says
//!
//! The counter is the cheapest and has no direction. The Cherenkov timing design
//! has four sensors, a timer and a bound of tens of thousands of degrees: it is
//! the most expensive way to learn nothing. The coarse tracker is the cheapest
//! design that measures a direction, and the fine tracker measures four times
//! better for four times the sensors and three times the budget. The ring imager
//! is the idea worth checking next and the one number here that is not checked.

use canlab_muon::designs::{AngleSource, Design};
use canlab_muon::mission::Mission;

fn main() {
    let mission = Mission::standard();
    println!(
        "MISSION — can {:.0} x D{:.0} mm, detector budget {:.0}-{:.0} EUR, mass share {:.1} g",
        mission.can.height_mm,
        mission.can.radius_mm * 2.0,
        mission.detector_budget_eur.0,
        mission.detector_budget_eur.1,
        mission.total_mass_g.1 * 0.15,
    );
    println!(
        "The can's cross-section is {:.1} cm², and a 100 s flight gives the muons in the last column.\n",
        mission.can.cross_section_mm2() / 100.0,
    );

    println!(
        "{:<18} {:>7} {:>7} {:>4} {:>9} {:>5} {:>8} {:>10} {:>9} {:>7} {:>7} {:>9}",
        "design", "EUR", "g", "ch", "pe/plane", "sat", "matters", "angle deg", "source", "cross", "accept", "directed"
    );
    for design in Design::all() {
        let metrics = design.metrics();
        // Parentheses for anything that is not a bound: a formula and an
        // estimate are not measurements and the table must not look like they are.
        let angle = match (metrics.angle_deg, metrics.angle_source) {
            (None, _) => "-".to_string(),
            (Some(angle), AngleSource::Bound) => format!("{angle:.1}"),
            (Some(angle), _) => format!("({angle:.1})"),
        };
        println!(
            "{:<18} {:>7.0} {:>7.1} {:>4} {:>9.0} {:>5} {:>8} {:>10} {:>9} {:>7.0} {:>7.2} {:>9.1}",
            design.name,
            metrics.cost_eur,
            metrics.detector_g,
            metrics.channels,
            metrics.photoelectrons,
            if metrics.saturates { "yes" } else { "no" },
            if metrics.saturation_matters { "yes" } else { "no" },
            angle,
            metrics.angle_source.tag(),
            metrics.crossings_per_flight,
            metrics.acceptance,
            metrics.muons_per_flight,
        );
    }
    println!(
        "\n  cross = muons through the first plane in 100 s; accept = the fraction that reach the second;\n  \
         directed = muons with a direction, which is cross x accept."
    );
    println!(
        "\n  source: bound = a Cramer-Rao limit checked against a Monte Carlo of the estimator;\n  \
         formula = a closed form for its model, with no simulated hits behind it;\n  \
         estimate = geometry only. Parentheses mark anything that is not a bound."
    );

    println!("\nDIMENSIONS — what each design is, in millimetres");
    println!("{:<18} {:>8} {:>10} {:>9} {:>9} {:>8}", "design", "shape", "plane mm", "area cm2", "stack mm", "fits");
    for design in Design::all() {
        let shape = match design.shape {
            canlab_muon::designs::PlaneShape::Rectangle => "rect",
            canlab_muon::designs::PlaneShape::Disc => "disc",
        };
        let plane = match design.shape {
            canlab_muon::designs::PlaneShape::Rectangle => {
                format!("{:.0}x{:.0}", design.width_mm, design.height_mm)
            }
            canlab_muon::designs::PlaneShape::Disc => format!("D{:.0}", design.width_mm),
        };
        let fits = match design.fits(&mission.can) {
            Ok(()) => "yes".to_string(),
            Err(reason) => format!("NO: {reason}"),
        };
        println!(
            "{:<18} {:>8} {:>10} {:>9.1} {:>9.0} {:>8}",
            design.name,
            shape,
            plane,
            design.area_mm2() / 100.0,
            design.stack_height_mm(),
            fits,
        );
    }

    println!("\nAUDIT — every requirement each design breaks");
    for design in Design::all() {
        let violations = design.audit(&mission);
        if violations.is_empty() {
            println!("  {:<18} passes", design.name);
        } else {
            println!("  {:<18} {}", design.name, violations.join("; "));
        }
    }

    // The two numbers the whole comparison turns on, in one place.
    let counter = Design::counter().metrics();
    let coarse = Design::coarse_tracker().metrics();
    let short = Design::short_tracker().metrics();
    println!(
        "\nThe trade in one line: a counter costs {:.0} EUR, counts {:.0} muons and measures \
         no direction. A {:.0} EUR tracker with a 20 mm gap measures {:.0}° from {:.0} muons; \
         the same tracker with a 40 mm gap measures {:.0}° from {:.0} muons.",
        counter.cost_eur,
        counter.muons_per_flight,
        short.cost_eur,
        short.angle_deg.unwrap_or(f64::NAN),
        short.muons_per_flight,
        coarse.angle_deg.unwrap_or(f64::NAN),
        coarse.muons_per_flight,
    );
    println!(
        "\nThe gap buys the angle and costs the sample: a muon at 30 degrees drifts \
         {:.0} mm sideways over a 40 mm gap, which is most of the plane's width. That \
         is why the coarse tracker's 11.7 degrees comes with only 7 muons behind it.",
        40.0 * (30.0f64).to_radians().tan(),
    );
}
