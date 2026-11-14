//! The 3D view of one detector event: the block, the sensors, a muon, its
//! Cherenkov cone, and the photons the transport actually traced.
//!
//! The picture is not a decoration drawn beside the numbers — it is the same
//! `canlab_optics::simulate_recording` call the study runs, with the photons'
//! polylines kept instead of thrown away. A picture and a number that disagree
//! would be two answers to one question, so there is one call and both read it.
//!
//! # The clock
//!
//! `time_ps` is picoseconds after the muon reaches the block's entry face. The
//! muon moves at `c`; a photon moves at `c/n`; a photon is born where the muon
//! was when it emitted it. So at a given clock the muon is a point on its track,
//! the photons are points on their polylines, and a sensor lights when the first
//! photon that reached it gets there.

use canlab_muon::timing_study::TimingDesign;
use canlab_optics::{Block, Event, Optical, PhotonRecord, SiPM, Vec3};
use canlab_scene::Camera;

/// The speed of light, millimetres per picosecond.
const C_MM_PER_PS: f64 = 0.299_792_458;

/// How far before the block the muon's track starts, millimetres.
const LEAD_IN_MM: f64 = 120.0;

/// How long the caller's path is, millimetres.
const PATH_MM: f64 = 240.0;

/// One simulated event, and the clock the view is at.
pub struct DetectorScene {
    /// The design that made it.
    pub design: TimingDesign,
    /// The block the transport used.
    pub block: Block,
    /// The four sensors.
    pub sensors: [SiPM; 4],
    /// Where the muon's track enters the caller's path.
    pub entry: Vec3,
    /// The muon's heading, a unit vector.
    pub heading: Vec3,
    /// Where the track enters the block.
    pub inside_start: Vec3,
    /// Where it leaves.
    pub inside_end: Vec3,
    /// Every photon's polyline.
    pub photons: Vec<PhotonRecord>,
    /// The photons' indices in a stable random order.
    ///
    /// The drawing shows the first `photon_limit` of these. A stride would show
    /// every *n*-th photon, which is a biased sample of the cone: the photons
    /// are born in order along the track, so every *n*-th is every *n*-th
    /// millimetre of the track. A shuffle is a random sample of the emission.
    pub order: Vec<usize>,
    /// How many photons the drawing shows.
    pub photon_limit: usize,
    /// The event's arrivals, for the timestamps and the sensor flashes.
    pub event: Event,
    /// The true heading, as the two horizontal components.
    pub truth: [f64; 2],
    /// Picoseconds after the muon reached `inside_start`.
    pub time_ps: f64,
    /// Whether the clock is running.
    pub playing: bool,
    /// Picoseconds per wall-clock second.
    pub speed_ps_per_s: f64,
    /// The seed the event was drawn with.
    pub seed: u64,
    /// The camera's azimuth, radians.
    pub azimuth: f64,
    /// The camera's elevation, radians.
    pub elevation: f64,
    /// How far the camera is pulled in or pushed out, as a multiplier on the
    /// distance that frames the block. `1.0` is the framing the block was fitted
    /// to.
    pub zoom: f64,
    /// Whether to draw the photons.
    pub show_photons: bool,
    /// Whether to draw the cone.
    pub show_cone: bool,
    /// How many photons reached a sensor face, for the header.
    pub detected: usize,
}

impl DetectorScene {
    /// Simulate one event for a design.
    pub fn new(design: TimingDesign, seed: u64) -> Option<DetectorScene> {
        let block = design.block()?;
        let optical = Optical::block_in_air(design.index, 10_000.0);
        let sensors = design.sensors();
        let mut rng = canlab_muon::Rng::new(seed);

        // A direction from the measured sky, and a point inside the block.
        let zenith = canlab_muon::MuonFlux::measured().sample_zenith(rng.uniform());
        let azimuth = rng.range(0.0, std::f64::consts::TAU);
        let heading = Vec3::new(
            zenith.sin() * azimuth.cos(),
            zenith.sin() * azimuth.sin(),
            -zenith.cos(),
        )
        .normalized()?;
        let inside = random_inside(&block, &mut rng)?;
        let entry = inside.sub(heading.scale(LEAD_IN_MM));

        let (event, photons) = canlab_optics::simulate_recording(
            &block,
            &optical,
            &sensors,
            entry,
            heading,
            PATH_MM,
            1.0,
            &mut || rng.uniform(),
        )
        .ok()?;
        let (inside_start, inside_end) = inside_segment(&block, entry, heading)?;
        let detected = photons
            .iter()
            .filter(|photon| photon.face.is_some_and(|face| sensors.iter().any(|s| s.face == face)))
            .count();
        // A stable shuffle, so the subset is random but does not change from
        // frame to frame — a subset that re-drew every frame would flicker.
        let mut order: Vec<usize> = (0..photons.len()).collect();
        for index in (1..order.len()).rev() {
            let swap = (rng.uniform() * (index + 1) as f64) as usize;
            order.swap(index, swap.min(index));
        }
        let photon_limit = photons.len().min(120);
        Some(DetectorScene {
            design,
            block,
            sensors,
            entry,
            heading,
            inside_start,
            inside_end,
            photons,
            order,
            photon_limit,
            event,
            truth: [heading.x, heading.y],
            time_ps: 0.0,
            playing: true,
            speed_ps_per_s: 20.0,
            seed,
            azimuth: 0.9,
            elevation: 0.35,
            zoom: 1.0,
            show_photons: true,
            show_cone: true,
            detected,
        })
    }

    /// The clock's end: the last photon's arrival, plus a margin.
    pub fn duration_ps(&self) -> f64 {
        let last = self
            .photons
            .iter()
            .map(|photon| self.birth_ps(photon) + photon.time_ps)
            .fold(0.0f64, f64::max);
        (last + 20.0).max(60.0)
    }

    /// The photons the drawing shows: a random subset of the emission.
    pub fn visible(&self) -> impl Iterator<Item = &PhotonRecord> {
        self.order
            .iter()
            .take(self.photon_limit.min(self.order.len()))
            .map(|index| &self.photons[*index])
    }

    /// When a photon was emitted, picoseconds after the muon reached the block.
    pub fn birth_ps(&self, photon: &PhotonRecord) -> f64 {
        let Some(birth) = photon.path.first().copied() else {
            return 0.0;
        };
        birth.sub(self.inside_start).dot(self.heading).max(0.0) / C_MM_PER_PS
    }

    /// Where the muon is at a clock time.
    pub fn muon_at(&self, time_ps: f64) -> Vec3 {
        self.inside_start.add(self.heading.scale(time_ps * C_MM_PER_PS))
    }

    /// Advance the clock, and stop it at the end.
    pub fn advance(&mut self, dt_s: f64) {
        if !self.playing {
            return;
        }
        self.time_ps += self.speed_ps_per_s * dt_s.max(0.0);
        let end = self.duration_ps();
        if self.time_ps >= end {
            self.time_ps = end;
            self.playing = false;
        }
    }

    /// Start the event again from the muon's entry.
    pub fn restart(&mut self) {
        self.time_ps = 0.0;
        self.playing = true;
    }

    /// The first time a sensor saw a photon, if it did.
    pub fn first_arrival_ps(&self, sensor: usize) -> Option<f64> {
        self.event.true_ps.get(sensor).and_then(|times| times.first().copied())
    }
}

/// A point strictly inside the block, by rejection over its bounding box.
fn random_inside(block: &Block, rng: &mut canlab_muon::Rng) -> Option<Vec3> {
    let half = block.thickness_mm() / 2.0;
    for _ in 0..256 {
        let point = Vec3::new(
            rng.range(-40.0, 40.0),
            rng.range(-60.0, 60.0),
            rng.range(-half, half),
        );
        if block.strictly_contains(point) {
            return Some(point);
        }
    }
    None
}

/// The stretch of the muon's path that lies inside the block.
fn inside_segment(block: &Block, entry: Vec3, heading: Vec3) -> Option<(Vec3, Vec3)> {
    const STEPS: usize = 4_000;
    let mut start = None;
    for step in 0..=STEPS {
        let point = entry.add(heading.scale(PATH_MM * step as f64 / STEPS as f64));
        if block.strictly_contains(point) {
            start = Some(point);
            break;
        }
    }
    let start = start?;
    let exit = block.exit(start, heading)?;
    Some((start, exit.point))
}

/// The scene's camera, framed on the **block**.
///
/// The target is the block's centre, not a point on the track: the track is
/// where the event happens, but the block is what the reader is looking at, and
/// an orbit around a point on the track swings the shape around a corner.
pub fn camera(scene: &DetectorScene, aspect: f64) -> Camera {
    let camera = Camera {
        azimuth: scene.azimuth,
        elevation: scene.elevation,
        field_of_view: 0.85,
        aspect,
        near: 0.5,
        far: 2_000.0,
        ..Camera::default()
    };
    let mut points = Vec::new();
    for top in [true, false] {
        for corner in corners(scene, top) {
            points.push(scene_point(corner));
        }
    }
    let fitted = camera.fit(&points);
    Camera {
        distance: fitted.distance * scene.zoom,
        ..fitted
    }
}

/// A point in the scene's own coordinates.
pub fn scene_point(point: Vec3) -> canlab_scene::Vec3 {
    canlab_scene::Vec3::new(point.x, point.y, point.z)
}

/// The block's corners at one end, as scene points.
pub fn corners(scene: &DetectorScene, top: bool) -> Vec<Vec3> {
    let half = scene.block.thickness_mm() / 2.0;
    let z = if top { half } else { -half };
    scene
        .block
        .outline
        .corners_mm()
        .iter()
        .map(|(x, y)| Vec3::new(*x, *y, z))
        .collect()
}


/// Draw the scene into `rect`, and turn it when the reader drags.
pub fn draw(scene: &mut DetectorScene, ui: &mut egui::Ui, rect: egui::Rect) -> egui::Response {
    use egui::{Color32, Pos2, Sense, Stroke};

    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    if response.dragged() {
        let delta = response.drag_delta();
        scene.azimuth -= f64::from(delta.x) * 0.01;
        scene.elevation = (scene.elevation + f64::from(delta.y) * 0.01).clamp(-1.5, 1.5);
    }
    if response.hovered() {
        let scroll = f64::from(ui.input(|input| input.smooth_scroll_delta.y));
        if scroll != 0.0 {
            scene.zoom = (scene.zoom * (1.0 - scroll * 0.0015)).clamp(0.3, 4.0);
        }
    }
    let painter = ui.painter_at(rect);
    let camera = camera(scene, f64::from(rect.width() / rect.height().max(1.0)));
    let to_screen = |point: Vec3| -> Option<Pos2> {
        let projected = camera.project(scene_point(point))?;
        let (x, y) = Camera::to_pixels(
            projected,
            f64::from(rect.width()),
            f64::from(rect.height()),
        );
        Some(rect.min + egui::vec2(x as f32, y as f32))
    };
    let accent = crate::theme::accent();
    let glass = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 10);
    let edge = Stroke::new(1.2, accent.gamma_multiply(0.8));

    // The block: its two ends and its walls, far first.
    let top = corners(scene, true);
    let bottom = corners(scene, false);
    let mut polygons: Vec<(f64, Vec<Pos2>)> = Vec::new();
    for polygon in [&top, &bottom] {
        let mut points = Vec::new();
        let mut depth = 0.0;
        for corner in polygon {
            if let Some(projected) = camera.project(scene_point(*corner)) {
                depth += projected.depth;
                if let Some(at) = to_screen(*corner) {
                    points.push(at);
                }
            }
        }
        if points.len() == polygon.len() {
            polygons.push((depth / polygon.len() as f64, points));
        }
    }
    for index in 0..top.len() {
        let next = (index + 1) % top.len();
        let quad = [top[index], top[next], bottom[next], bottom[index]];
        let mut points = Vec::new();
        let mut depth = 0.0;
        for corner in quad {
            if let Some(projected) = camera.project(scene_point(corner)) {
                depth += projected.depth;
                if let Some(at) = to_screen(corner) {
                    points.push(at);
                }
            }
        }
        if points.len() == 4 {
            polygons.push((depth / 4.0, points));
        }
    }
    polygons.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (_, points) in &polygons {
        painter.add(egui::Shape::convex_polygon(points.clone(), glass, Stroke::NONE));
    }
    // The edges, once each. A stroke on every face draws each shared edge
    // twice, and the double-drawn line shimmers as the camera turns — the
    // "flickering lines" a reader sees are one edge painted on top of itself.
    for index in 0..top.len() {
        let next = (index + 1) % top.len();
        for (a, b) in [
            (top[index], top[next]),
            (bottom[index], bottom[next]),
            (top[index], bottom[index]),
        ] {
            if let (Some(from), Some(to)) = (to_screen(a), to_screen(b)) {
                painter.line_segment([from, to], edge);
            }
        }
    }

    // The photons, up to where the clock has them.
    if scene.show_photons {
        // A random subset, not every *n*-th: the photons are born in order
        // along the track, so a stride samples the track's positions rather
        // than the emission. The subset is fixed per scene, so it does not
        // flicker as the clock runs.
        let photon_colour = Stroke::new(1.3, Color32::from_rgb(126, 198, 255));
        for photon in scene.visible() {
            let birth = scene.birth_ps(photon);
            if scene.time_ps < birth {
                continue;
            }
            let distance = (scene.time_ps - birth) * C_MM_PER_PS / scene.design.index;
            let mut travelled = 0.0;
            // From the birth, not from the first bounce: a polyline drawn from
            // `windows(2)` alone skips the segment that starts at the birth, and
            // every photon then appears to come into existence at a face.
            let mut previous = to_screen(photon.path[0]);
            for pair in photon.path.windows(2) {
                let segment = pair[1].sub(pair[0]).length();
                let take = (distance - travelled).clamp(0.0, segment);
                let end = pair[0].add(pair[1].sub(pair[0]).scale(if segment > 0.0 { take / segment } else { 0.0 }));
                if let Some(at) = to_screen(end) {
                    if let Some(from) = previous {
                        painter.line_segment([from, at], photon_colour);
                    }
                    previous = Some(at);
                } else {
                    previous = None;
                }
                travelled += segment;
                if travelled >= distance {
                    break;
                }
            }
        }
    }

    // The muon's track, and the muon at the clock.
    let track = Stroke::new(1.6, accent.gamma_multiply(0.55));
    let mut previous: Option<Pos2> = None;
    for point in [scene.entry, scene.inside_start, scene.inside_end] {
        match to_screen(point) {
            Some(at) => {
                if let Some(from) = previous {
                    painter.line_segment([from, at], track);
                }
                previous = Some(at);
            }
            None => previous = None,
        }
    }
    let muon = scene.muon_at(scene.time_ps);
    if let Some(at) = to_screen(muon) {
        painter.circle_filled(at, 4.5, accent);
        painter.circle_stroke(at, 7.0, Stroke::new(1.0, accent.gamma_multiply(0.5)));
    }

    // The Cherenkov cone, from the muon's position and stopped at the surface:
    // a cone drawn through the wall would be light that left the block.
    if scene.show_cone && scene.block.strictly_contains(muon) {
        let cone = (1.0 / (scene.design.index)).acos();
        let (right, up) = basis(scene.heading);
        if let Some(apex) = to_screen(muon) {
            for step in 0..12 {
                let phi = std::f64::consts::TAU * step as f64 / 12.0;
                let direction = scene
                    .heading
                    .scale(cone.cos())
                    .add(right.scale(cone.sin() * phi.cos()))
                    .add(up.scale(cone.sin() * phi.sin()));
                let end = scene
                    .block
                    .exit(muon, direction)
                    .map(|exit| exit.point)
                    .unwrap_or_else(|| muon.add(direction.scale(20.0)));
                if let Some(at) = to_screen(end) {
                    painter.line_segment(
                        [apex, at],
                        Stroke::new(1.4, Color32::from_rgb(120, 240, 255).gamma_multiply(0.8)),
                    );
                }
            }
        }
    }

    // The sensors: grey until the first photon arrives, then lit.
    for (index, sensor) in scene.sensors.iter().enumerate() {
        let position = scene.block.face_centre(sensor.face);
        let Some(at) = to_screen(position) else {
            continue;
        };
        let arrival = scene.first_arrival_ps(index);
        let lit = arrival.is_some_and(|time| scene.time_ps >= time);
        let colour = if lit {
            Color32::from_rgb(255, 196, 64)
        } else {
            Color32::from_gray(120)
        };
        painter.circle_filled(at, if lit { 6.0 } else { 4.0 }, colour);
        if lit {
            painter.circle_stroke(at, 10.0, Stroke::new(1.0, colour.gamma_multiply(0.5)));
        }
    }

    response
}

/// Two unit vectors perpendicular to a heading.
fn basis(heading: Vec3) -> (Vec3, Vec3) {
    let reference = if heading.z.abs() < 0.9 {
        Vec3::new(0.0, 0.0, 1.0)
    } else {
        Vec3::new(1.0, 0.0, 0.0)
    };
    let right = heading.cross(reference).normalized().unwrap_or(Vec3::new(1.0, 0.0, 0.0));
    let up = right.cross(heading).normalized().unwrap_or(Vec3::new(0.0, 1.0, 0.0));
    (right, up)
}

#[cfg(test)]
mod tests {
    use super::*;
    use canlab_muon::timing_study::Shape;

    fn hexagon() -> TimingDesign {
        TimingDesign {
            name: "hexagon",
            scale_mm: 28.0,
            thickness_mm: 30.0,
            index: 1.49,
            pde: 0.35,
            shape: Shape::Hexagon,
        }
    }

    #[test]
    fn the_photons_are_born_along_the_track() {
        // The bug this prevents: a photon drawn from a face instead of from the
        // track looks like a photon that came into existence at the edge.
        let scene = DetectorScene::new(hexagon(), 1).expect("a scene");
        let chord = scene.inside_start.sub(scene.inside_end).length();
        let mut low = f64::INFINITY;
        let mut high = f64::NEG_INFINITY;
        for photon in &scene.photons {
            let birth = photon.path.first().expect("a birth point");
            let along = birth.sub(scene.inside_start).dot(scene.heading);
            low = low.min(along);
            high = high.max(along);
        }
        assert!(low < 1.0, "the first photon is born {low} mm into a {chord} mm chord");
        assert!(
            high > chord - 1.0,
            "the last photon is born {high} mm into a {chord} mm chord"
        );
    }

    #[test]
    fn the_visible_photons_are_a_random_subset() {
        // The bug this prevents: a stride shows every *n*-th photon, and the
        // photons are born in order along the track, so every *n*-th is every
        // *n*-th millimetre of the track — a sample of positions, not of the
        // emission. The order must be a permutation and the subset must not be
        // the first ones.
        let scene = DetectorScene::new(hexagon(), 1).expect("a scene");
        let mut sorted = scene.order.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..scene.photons.len()).collect::<Vec<_>>());
        assert_eq!(scene.visible().count(), scene.photon_limit);
        let first: Vec<usize> = scene.order.iter().take(scene.photon_limit).copied().collect();
        let identity: Vec<usize> = (0..scene.photon_limit).collect();
        assert_ne!(first, identity, "the subset is the first photons, not a sample");
        // And the limit is the drawing's, not the scene's: every photon is
        // still in `photons`, so the timestamps are unaffected.
        assert_eq!(scene.photons.len(), sorted.len());
    }

    #[test]
    fn the_camera_looks_at_the_block() {
        // The bug this prevents: the orbit target was the track's midpoint,
        // which is a point on the track and not the block's centre, so the
        // camera swung around a corner of the shape.
        let scene = DetectorScene::new(hexagon(), 1).expect("a scene");
        let camera = camera(&scene, 1.5);
        let centre = scene.block.face_centre(canlab_optics::Face::Top);
        assert!((camera.target.x - centre.x).abs() < 1e-9);
        assert!((camera.target.y - centre.y).abs() < 1e-9);
        assert!(camera.target.z.abs() < 1e-9);
        // And the block fits: every corner is inside the frustum.
        for top in [true, false] {
            for corner in corners(&scene, top) {
                assert!(camera.contains(scene_point(corner)), "{corner:?} is off screen");
            }
        }
    }
}
