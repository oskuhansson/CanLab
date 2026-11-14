use canlab_optics::block::Face;
use canlab_optics::detector::simulate;
use canlab_optics::{Block, Optical, SiPM, Tropical, Vec3};

fn main() {
    let block = Block::new(Tropical::fits_in(20.0, 10.0).expect("fits"));
    let n = 1.84;
    let optical = Optical::block_in_air(n, 10_000.0);
    let faces = [Face::Top, Face::Bottom, Face::Side(0), Face::Side(2)];
    let sensors: [SiPM; 4] = faces.map(SiPM::typical);
    let mut state = 11u64;
    let mut random = move || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((state >> 11) as f64) / (1u64 << 53) as f64
    };
    let start = Vec3::new(0.0, 0.0, -20.0);
    let end = Vec3::new(0.0, 0.0, 15.0);
    for (label, p) in [("start", start), ("end", end), ("mid", Vec3::new(0.0, 0.0, -2.5))] {
        println!("{label} {p:?} contains={} sd={}", block.contains(p), block.outline.signed_distance(p.x, p.y, p.z));
    }
    println!("block = {block:?}");
    let event = simulate(
        &block,
        &optical,
        &sensors,
        Vec3::new(0.0, 0.0, -20.0),
        Vec3::new(0.0, 0.0, 1.0),
        15.0,
        1.0,
        &mut random,
    )
    .expect("simulate");
    println!(
        "born {} trapped {} absorbed {} lost {} missed {} detected {}",
        event.photons_born,
        event.trapped,
        event.absorbed,
        event.lost,
        event.missed,
        event.detected_count()
    );
}
