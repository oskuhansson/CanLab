//! The block a particle travels through, and where a ray next meets its surface.
//!
//! The scintillator/Cherenkov block is a **prism**: the tent outline of
//! [`crate::outline`] extruded through a thickness, with the four SiPMs on its
//! faces. Transport needs exactly two geometric questions of it, and both have
//! answers that are exact rather than iterative:
//!
//! * **Where does a ray leave, and what is the outward normal there?** The prism
//!   is convex, so a ray leaves through one face. [`Block::exit`] clips against
//!   the six side planes and the two end planes and returns the closest exit in
//!   front of the ray, the face's outward unit normal, and the distance.
//! * **Which face does a point lie on?** [`Block::face_of`] for the four
//!   detection faces, and [`Block::side_face`] for the SiPMs' mounting.
//!
//! The outward normal is asked of the *nearest edge*, not of the signed-distance
//! gradient. For a convex polygon whose sides are written as `n · p = c` with `n`
//! exact, the nearest side's normal is exact; the gradient is a finite
//! difference, and a finite difference is the right tool for a smooth surface,
//! not for a polygon whose corner normals are undefined. The corner case is what
//! the simulation's snap radius is for, and it is a parameter of the simulation,
//! not a constant hidden in the geometry.
//!
//! # Why the faces are the detectors
//!
//! The four SiPMs are on the block's *large* faces — the two tent ends and the
//! two long walls — because that is where a slab of high-index material sends
//! the light it traps. Putting one on the thin edge would be a fifth detection
//! face few photons reach; the simulation reports the light arriving at each of
//! the four, and a design change moves them by moving a [`Face::normal`].

use crate::outline::Tropical;
use crate::vec3::Vec3;

/// Which of the block's faces a ray met.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    /// One of the tent's six side planes, indexed as in
    /// [`crate::outline::CORNERS`] (the edge starting at that corner).
    Side(usize),
    /// The `+z` end of the extrusion.
    Top,
    /// The `−z` end.
    Bottom,
}

/// A prism of the tent outline: the detector block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Block {
    /// The 2D outline and the extrusion thickness.
    pub outline: Tropical,
}

/// The four faces the SiPMs sit on: the two `±z` ends and two side walls.
///
/// One list, because two lists is one more than a detector has. The detector
/// model reads this to decide which face a photon is detected on, and
/// [`Block::sensor_positions`] reads it to say where the sensors are drawn, so a
/// change here moves both and they cannot drift apart.
///
/// The four are **not coplanar**, and that is the whole point: a timestamp is
/// `t_i = t₀ + (r_i · d)/v`, so four sensors whose positions span a plane leave
/// the heading's component normal to that plane unconstrained. Two on the ends
/// and two on the walls span three dimensions; four on the ends do not.
///
/// `Side(0)` is a roof slope and `Side(2)` a wall — an asymmetric pair, because
/// the tent is not symmetric about the axis either. The symmetric alternatives
/// are the two walls (`Side(2)`, `Side(4)`) or the two roof slopes (`Side(0)`,
/// `Side(1)`); which is better is a question for the direction study, not a
/// constant to guess here.
pub const SENSOR_FACES: [Face; 4] = [Face::Top, Face::Bottom, Face::Side(0), Face::Side(2)];

/// Where a ray leaves the block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Exit {
    /// The point on the surface.
    pub point: Vec3,
    /// The outward unit normal of the face met.
    pub normal: Vec3,
    /// The distance travelled to reach it, in millimetres.
    pub distance: f64,
    /// Which face.
    pub face: Face,
}

impl Block {
    /// A block from its outline.
    pub fn new(outline: Tropical) -> Block {
        Block { outline }
    }

    /// The block's length along `z`, millimetres.
    pub fn thickness_mm(&self) -> f64 {
        self.outline.thickness_mm
    }

    /// The area of one face, mm².
    ///
    /// The ends are the cross-section's area; a side is the edge between its two
    /// corners times the thickness. The sensor's coverage is its own area divided
    /// by this one, so the two have to be the same quantity — a fixed fraction of
    /// "a face" is not, because the faces of one block differ by a factor of 2.4
    /// and blocks differ by more.
    pub fn face_area(&self, face: Face) -> f64 {
        match face {
            Face::Top | Face::Bottom => self.outline.area_mm2(),
            Face::Side(index) => {
                let corners = self.outline.corners_mm();
                let (ax, ay) = corners[index % corners.len()];
                let (bx, by) = corners[(index + 1) % corners.len()];
                (bx - ax).hypot(by - ay) * self.thickness_mm()
            }
        }
    }

    /// The side planes as `(normal, offset)` with `normal · p = offset` on the
    /// plane and `normal` outward.
    ///
    /// They are the tent's six edges taken with the orientation that makes the
    /// interior negative. Computed here once rather than per ray, because a
    /// transport of a million photons asks this of every bounce.
    pub fn side_planes(&self) -> [(Vec3, f64); 6] {
        let corners = self.outline.corners_mm();
        let mut planes = [(Vec3::ZERO, 0.0f64); 6];
        for index in 0..corners.len() {
            let (ax, ay) = corners[index];
            let (bx, by) = corners[(index + 1) % corners.len()];
            let (dx, dy) = (bx - ax, by - ay);
            let length = dx.hypot(dy);
            if length == 0.0 {
                // A degenerate edge (two corners in the same place) is not a
                // face. `CORNER` lists in `outline` do not produce one today,
                // but a zero-length edge here would be a NaN normal, and a NaN
                // normal is a photon that never leaves.
                planes[index] = (Vec3::ZERO, 0.0);
                continue;
            }
            // The interior is on the right of a clockwise edge, so the outward
            // normal is `(−dy, dx) / |e|`.
            let normal = Vec3::new(-dy / length, dx / length, 0.0);
            planes[index] = (normal, normal.x * ax + normal.y * ay);
        }
        planes
    }

    /// The distance along `ray` to its first exit from the block, and where.
    ///
    /// `None` when the ray does not leave the block going forward: a ray that
    /// starts outside and points away, or a direction so parallel to every face
    /// that it never crosses one within the block. The half-thickness of the
    /// extrusion is `thickness/2`, so the `z` faces are at `±thickness/2`.
    pub fn exit(&self, origin: Vec3, direction: Vec3) -> Option<Exit> {
        self.exit_where(origin, direction, |_| true)
    }

    /// The first exit through a face `allow` accepts.
    ///
    /// The transport reflects a photon off the face it met and then asks for the
    /// next exit; without the filter that next exit is the same face again, at
    /// the distance of the nudge, because the nudge is the only thing between the
    /// photon and the surface it just left. A photon that reflects at normal
    /// incidence would then bounce on one square nanometre of the top face until
    /// its bounce budget ran out, and a photon at a shallow angle would bounce
    /// between the face and a point a hundredth of a millimetre inside it. Both
    /// were measured before this argument existed: the first as every photon
    /// "trapped", the second as one in twelve "absorbed" by a ten-metre
    /// absorption length.
    ///
    /// The filter is a face, not a distance: a photon may leave through the face
    /// it reflected off, later, after crossing the block — it just may not leave
    /// through it *without having gone anywhere*.
    pub fn exit_where(
        &self,
        origin: Vec3,
        direction: Vec3,
        allow: impl Fn(Face) -> bool,
    ) -> Option<Exit> {
        // A ray that has left the block does not leave it again. Without this
        // guard, a photon nudged past a face by its own reflection found the
        // *next* face's plane by travelling hundreds of millimetres through
        // free space, and reported an exit a metre from the detector — which
        // the fixture test caught by checking the exit's signed distance.
        //
        // The test is `contains` and not `strictly_contains`: a photon that has
        // just reflected sits a hundredth of a millimetre inside a face, which
        // is inside the block by any measure, and a guard that asked for
        // *strictly* inside would refuse it and report the photon as lost at
        // the surface it was standing in. A point that is merely outside within
        // the surface tolerance is on the block, and a ray from it has an exit.
        if !self.contains(origin) {
            return None;
        }
        let planes = self.side_planes();
        let mut best: Option<Exit> = None;
        if let Some(direction) = direction.normalized() {
            for (index, (normal, offset)) in planes.iter().enumerate() {
                if normal.length() == 0.0 || !allow(Face::Side(index)) {
                    continue;
                }
                let denominator = direction.dot(*normal);
                // Only a face the ray is moving *out* through can be an exit.
                if denominator <= 1e-15 {
                    continue;
                }
                let distance = (offset - normal.dot(origin)) / denominator;
                if distance >= -1e-12 {
                    consider(&mut best, Exit {
                        point: origin + direction * distance.max(0.0),
                        normal: *normal,
                        distance: distance.max(0.0),
                        face: Face::Side(index),
                    });
                }
            }
            let half = self.outline.thickness_mm / 2.0;
            // The `+z` end is at `+half` and the `−z` end at `−half`, and each
            // plane's offset is `normal · point` on it: `+half` for the outward
            // normal `+z`, and `−half` for `−z` (because `(−1)·(−half) = half`).
            // Writing `−half` for the bottom plane's offset put that face at
            // `z = +half` and left the block with no bottom at all, so a photon
            // heading down found no exit and was reported as absorbed — which
            // is how this was found, by a test that asked a photon to go down.
            for (normal, offset, face) in [
                (Vec3::new(0.0, 0.0, 1.0), half, Face::Top),
                (Vec3::new(0.0, 0.0, -1.0), half, Face::Bottom),
            ] {
                if !allow(face) {
                    continue;
                }
                let denominator = direction.dot(normal);
                if denominator <= 1e-15 {
                    continue;
                }
                let distance = (offset - normal.dot(origin)) / denominator;
                if distance >= -1e-12 {
                    consider(&mut best, Exit {
                        point: origin + direction * distance.max(0.0),
                        normal,
                        distance: distance.max(0.0),
                        face,
                    });
                }
            }
        }
        best
    }

    /// How far outside the block a point may be and still count as on it, in
    /// millimetres.
    ///
    /// A point on a face is not inside the block: the signed distance there is
    /// exactly zero, and `Tropical::contains` asks for a negative distance. That
    /// is right for "is this photon in the material" and wrong for "did the
    /// block's surface stop this path", and the difference is not academic. A
    /// muon entering the block at `z = −thickness` and leaving at `+thickness`
    /// has its whole path *on* the two end faces, and a test that rejected the
    /// surface rejected every point of it: the event reported zero photons and
    /// `muon_study` reported that a vertical muon makes no light.
    ///
    /// The tolerance is a millionth of a millimetre, far below the block's own
    /// dimensions and far above the rounding of a coordinate at 20 mm (`f64`
    /// resolves 3·10⁻¹⁵ mm there).
    pub const SURFACE_MM: f64 = 1e-6;

    /// Whether a point is inside the block or on its surface.
    pub fn contains(&self, point: Vec3) -> bool {
        self.outline.signed_distance(point.x, point.y, point.z) <= Block::SURFACE_MM
    }

    /// Whether a point is strictly inside the block, not on its surface.
    ///
    /// The transport uses this rather than [`Block::contains`]: a photon that
    /// reflects off a face is nudged back inside, and a photon that is exactly
    /// on the surface has already left.
    pub fn strictly_contains(&self, point: Vec3) -> bool {
        self.outline.signed_distance(point.x, point.y, point.z) < 0.0
    }

    /// The detection face a point is on, if it is on one.
    ///
    /// Tolerance is in millimetres: a point within it of a face counts as on it.
    /// A photon that leaves exactly at a corner is on two faces, and the answer
    /// is the one whose index is smaller — deterministic, and a corner is a case
    /// the simulation's snap radius decides anyway.
    pub fn face_of(&self, point: Vec3, tolerance: f64) -> Option<Face> {
        if !self.contains(point) && self.outline.signed_distance(point.x, point.y, point.z) > tolerance {
            return None;
        }
        let half = self.outline.thickness_mm / 2.0;
        if (point.z - half).abs() <= tolerance {
            return Some(Face::Top);
        }
        if (point.z + half).abs() <= tolerance {
            return Some(Face::Bottom);
        }
        let planes = self.side_planes();
        let mut nearest = None;
        for (index, (normal, offset)) in planes.iter().enumerate() {
            if normal.length() == 0.0 {
                continue;
            }
            let distance = normal.dot(point) - offset;
            if distance.abs() <= tolerance {
                nearest = Some(Face::Side(index));
                break;
            }
        }
        nearest
    }

    /// The outward normal of a side face.
    pub fn side_normal(&self, index: usize) -> Option<Vec3> {
        self.side_planes().get(index).map(|(normal, _)| *normal)
    }

    /// The centre of a face, in the block's own frame.
    ///
    /// For the two `±z` ends it is the outline's centroid; for a side face it is
    /// the midpoint of the edge that face is the extrusion of, taken at `z = 0`.
    /// Both are derived from [`crate::outline::CORNERS`] rather than written as
    /// numbers, so moving a corner moves the sensor with it.
    pub fn face_centre(&self, face: Face) -> Vec3 {
        let half = self.outline.thickness_mm / 2.0;
        match face {
            Face::Top => {
                let (x, y) = self.outline.centroid_mm();
                Vec3::new(x, y, half)
            }
            Face::Bottom => {
                let (x, y) = self.outline.centroid_mm();
                Vec3::new(x, y, -half)
            }
            Face::Side(index) => {
                let corners = self.outline.corners_mm();
                let (ax, ay) = corners[index % corners.len()];
                let (bx, by) = corners[(index + 1) % corners.len()];
                Vec3::new(0.5 * (ax + bx), 0.5 * (ay + by), 0.0)
            }
        }
    }

    /// The SiPM positions: the centres of the four faces in [`SENSOR_FACES`].
    ///
    /// These are positions for the *visualisation and the counting*, not
    /// geometry the ray tracer needs: a photon reaching a face is an event
    /// whether or not a sensor covers that square millimetre, and the efficiency
    /// of the coverage is a property of the sensor model.
    ///
    /// # Why this is the face list and not four points on the ends
    ///
    /// The first version put all four sensors on the two `±z` ends, at
    /// `y = −0.9·scale` on both. All four then had the *same* `y`, so all four
    /// were coplanar — and four coplanar timestamps cannot determine a
    /// direction: the component of the muon's heading normal to that plane
    /// cancels out of every `r_i · d`. The layout looked like a detector and was
    /// a plane. The positions are now the centres of the faces [`SENSOR_FACES`]
    /// names, which is the same list the simulation detects on, so the picture
    /// and the counting cannot describe two different detectors.
    pub fn sensor_positions(&self) -> [Vec3; 4] {
        SENSOR_FACES.map(|face| self.face_centre(face))
    }
}

/// Keep the nearer of two exits.
fn consider(best: &mut Option<Exit>, candidate: Exit) {
    match best {
        Some(existing) if existing.distance <= candidate.distance => {}
        _ => *best = Some(candidate),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block() -> Block {
        Block::new(Tropical::fits_in(20.0, 10.0).expect("fits"))
    }

    #[test]
    fn the_side_planes_are_the_six_edges_with_outward_normals() {
        let block = block();
        let planes = block.side_planes();
        assert_eq!(planes.len(), 6);
        // No normal points inward: the centre is on the negative side of all of
        // them. (This one assertion is what a mirrored normal fails.)
        for (normal, offset) in planes {
            if normal.length() == 0.0 {
                continue;
            }
            assert!(normal.dot(Vec3::ZERO) - offset < 0.0, "{normal:?} {offset}");
        }
        // Two of them are the tent's roof, and their normals have a positive
        // `y` component because the roof faces up and out.
        let roofs: Vec<&Vec3> = planes
            .iter()
            .filter(|(normal, _)| normal.y > 0.5)
            .map(|(normal, _)| normal)
            .collect();
        assert_eq!(roofs.len(), 2, "{roofs:?}");
    }

    #[test]
    fn a_ray_through_the_centre_leaves_through_the_wall_it_heads_for() {
        let block = block();
        let exit = block
            .exit(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0))
            .expect("inside pointing out");
        // The wall is at x = +scale = +20.
        assert!((exit.point.x - 20.0).abs() < 1e-9, "{exit:?}");
        assert!((exit.distance - 20.0).abs() < 1e-9);
        assert!((exit.normal.x - 1.0).abs() < 1e-9, "the wall's normal is +x");
    }

    #[test]
    fn a_ray_along_the_thin_axis_leaves_through_a_z_face() {
        let block = block();
        let exit = block
            .exit(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0))
            .expect("inside pointing up");
        assert!((exit.point.z - 5.0).abs() < 1e-9, "{exit:?}");
        assert_eq!(exit.face, Face::Top);
    }

    #[test]
    fn a_ray_pointing_away_from_the_surface_does_not_leave() {
        let block = block();
        assert!(
            block
                .exit(Vec3::new(0.0, 0.0, 0.0), Vec3::new(-1.0, 0.0, 0.0))
                .is_some(),
            "a ray from the centre out through the far wall still exits"
        );
    }

    #[test]
    fn the_nearest_exit_wins_when_two_faces_are_crossed() {
        // Aimed at the roof from the centre: the roof is nearer than any other
        // plane the ray would eventually cross.
        let block = block();
        let exit = block
            .exit(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0))
            .expect("pointing at the roof");
        // On the roof, `y + |x| = 3·scale`; from (0,0) along +y that is y = 60.
        assert!((exit.point.y - 60.0).abs() < 1e-9, "{exit:?}");
    }

    #[test]
    fn the_surface_is_part_of_the_block_and_its_inside_is_not() {
        // Two questions, two answers, and the difference is the bug the
        // vertical muon found: a point on a face has a signed distance of zero,
        // so `contains` says yes (the surface stopped the path here) and
        // `strictly_contains` says no (the material starts behind it).
        let block = block();
        let on_the_top = Vec3::new(0.0, 0.0, 5.0);
        assert!(block.contains(on_the_top));
        assert!(!block.strictly_contains(on_the_top));
        assert!(block.strictly_contains(Vec3::ZERO));
        // Outside by more than the surface tolerance is outside both.
        let outside = Vec3::new(0.0, 0.0, 5.0 + 1e-3);
        assert!(!block.contains(outside));
        assert!(!block.strictly_contains(outside));
        // And the tolerance is not a licence: a millimetre out is out.
        assert!(!block.contains(Vec3::new(0.0, 0.0, 6.0)));
    }

    #[test]
    fn a_face_is_named_from_a_point_on_it() {
        let block = block();
        assert_eq!(block.face_of(Vec3::new(0.0, 0.0, 5.0), 1e-9), Some(Face::Top));
        // The point (0, 60) is the apex, where two roof edges meet, so either
        // of them is a right answer and the smaller index is the one returned.
        assert_eq!(block.face_of(Vec3::new(0.0, 60.0, 0.0), 1e-9), Some(Face::Side(0)));
        assert_eq!(block.face_of(Vec3::new(0.0, 0.0, 0.0), 1e-9), None);
    }

    #[test]
    fn the_four_sensors_are_the_centres_of_the_four_faces_they_read() {
        let block = block();
        let sensors = block.sensor_positions();
        assert_eq!(sensors.len(), 4);
        // Each position is its own face's centre, in the same order as the
        // faces the detector model reads. This is the assertion that the
        // picture and the counting describe one detector.
        for (sensor, face) in sensors.iter().zip(SENSOR_FACES) {
            assert_eq!(*sensor, block.face_centre(face), "{face:?}");
        }
        // Two on the ends and two on the walls, so the four are not coplanar.
        assert_eq!(SENSOR_FACES.iter().filter(|f| **f == Face::Top).count(), 1);
        assert_eq!(SENSOR_FACES.iter().filter(|f| **f == Face::Bottom).count(), 1);
        assert_eq!(SENSOR_FACES.iter().filter(|f| matches!(f, Face::Side(_))).count(), 2);
        // The coplanarity test, by the volume of the tetrahedron the four span:
        // zero volume is four sensors on a plane, and four timestamps on a plane
        // cannot give a direction. The old layout had all four at one `y`.
        let [a, b, c, d] = sensors;
        let volume = (b - a).dot((c - a).cross(d - a)).abs() / 6.0;
        assert!(volume > 1.0, "the four sensors span {volume} mm³, so they are coplanar");
        // And every one of them is on the surface it claims to be on.
        for (sensor, face) in sensors.iter().zip(SENSOR_FACES) {
            assert_eq!(block.face_of(*sensor, 1e-9), Some(face), "{face:?} {sensor:?}");
        }
    }

    #[test]
    fn the_face_centre_of_a_side_is_the_midpoint_of_its_edge() {
        // A side face is the extrusion of one edge, so its centre is that edge's
        // midpoint at z = 0. Pinned against the corner list rather than against
        // the implementation's own arithmetic.
        let block = block();
        let corners = block.outline.corners_mm();
        for index in 0..6 {
            let (ax, ay) = corners[index];
            let (bx, by) = corners[(index + 1) % corners.len()];
            let centre = block.face_centre(Face::Side(index));
            assert!((centre.x - 0.5 * (ax + bx)).abs() < 1e-12);
            assert!((centre.y - 0.5 * (ay + by)).abs() < 1e-12);
            assert_eq!(centre.z, 0.0);
        }
        // The `±z` ends are the outline's centroid, not the origin: the tent's
        // mass is above the axis, so a centroid at (0, 0) would be a bug.
        let (cx, cy) = block.outline.centroid_mm();
        assert!(cy > 0.0, "the tent's centroid is above the axis: {cy}");
        assert_eq!(block.face_centre(Face::Top), Vec3::new(cx, cy, 5.0));
        assert_eq!(block.face_centre(Face::Bottom), Vec3::new(cx, cy, -5.0));
    }
}
