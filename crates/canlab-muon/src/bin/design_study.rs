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

use canlab_muon::designs::Design;
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
        "{:<18} {:>7} {:>7} {:>4} {:>9} {:>5} {:>8} {:>10} {:>8}",
        "design", "EUR", "g", "ch", "pe/plane", "sat", "matters", "angle deg", "muons"
    );
    for design in Design::all() {
        let metrics = design.metrics();
        let angle = match metrics.angle_deg {
            None => "-".to_string(),
            Some(angle) if !metrics.angle_is_checked => format!("({angle:.1})"),
            Some(angle) => format!("{angle:.1}"),
        };
        println!(
            "{:<18} {:>7.0} {:>7.1} {:>4} {:>9.0} {:>5} {:>8} {:>10} {:>8.1}",
            design.name,
            metrics.cost_eur,
            metrics.detector_g,
            metrics.channels,
            metrics.photoelectrons,
            if metrics.saturates { "yes" } else { "no" },
            if metrics.saturation_matters { "yes" } else { "no" },
            angle,
            metrics.muons_per_flight,
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
    let fine = Design::fine_tracker().metrics();
    println!(
        "\nThe trade in one line: a counter costs {:.0} EUR and measures no direction; \
         a {:.0} EUR tracker measures {:.0}°; the {:.0} EUR tracker measures {:.0}°.",
        counter.cost_eur,
        coarse.cost_eur,
        coarse.angle_deg.unwrap_or(f64::NAN),
        fine.cost_eur,
        fine.angle_deg.unwrap_or(f64::NAN),
    );
    println!(
        "The gap between the tracker's planes is air and costs nothing but height, \
         so the resolution is bought with pixels, not with precision."
    );
}
