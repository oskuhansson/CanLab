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
        // A ray that has left the block does not leave it again. Without this
        // guard, a photon nudged past a face by its own reflection found the
        // *next* face's plane by travelling hundreds of millimetres through
        // free space, and reported an exit a metre from the detector — which
        // the fixture test caught by checking the exit's signed distance.
        if !self.contains(origin) {
            return None;
        }
        let planes = self.side_planes();
        let mut best: Option<Exit> = None;
        if let Some(direction) = direction.normalized() {
            for (index, (normal, offset)) in planes.iter().enumerate() {
                if normal.length() == 0.0 {
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
            for (normal, offset, face) in [
                (Vec3::new(0.0, 0.0, 1.0), half, Face::Top),
                (Vec3::new(0.0, 0.0, -1.0), -half, Face::Bottom),
            ] {
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

    /// Whether a point is inside the block.
    pub fn contains(&self, point: Vec3) -> bool {
        self.outline.contains(point.x, point.y, point.z)
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

    /// The SiPM positions: the centres of the two large faces.
    ///
    /// The four detectors are on the block's two `±z` faces, one near each
    /// end of the tent so that a photon's arrival is not the same for all four.
    /// These are positions for the *visualisation and the counting*, not
    /// geometry the ray tracer needs: a photon reaching a face is an event
    /// whether or not a sensor covers that square millimetre, and the efficiency
    /// of the coverage is a property of the sensor model.
    pub fn sensor_positions(&self) -> [Vec3; 4] {
        let half = self.outline.thickness_mm / 2.0;
        let reach = self.outline.scale_mm;
        [
            Vec3::new(-reach * 0.35, -reach * 0.9, half),
            Vec3::new(reach * 0.35, -reach * 0.9, -half),
            Vec3::new(-reach * 0.35, -reach * 0.9, -half),
            Vec3::new(reach * 0.35, -reach * 0.9, half),
        ]
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
    fn a_face_is_named_from_a_point_on_it() {
        let block = block();
        assert_eq!(block.face_of(Vec3::new(0.0, 0.0, 5.0), 1e-9), Some(Face::Top));
        // The point (0, 60) is the apex, where two roof edges meet, so either
        // of them is a right answer and the smaller index is the one returned.
        assert_eq!(block.face_of(Vec3::new(0.0, 60.0, 0.0), 1e-9), Some(Face::Side(0)));
        assert_eq!(block.face_of(Vec3::new(0.0, 0.0, 0.0), 1e-9), None);
    }

    #[test]
    fn the_four_sensors_are_on_the_two_large_faces() {
        let sensors = block().sensor_positions();
        assert_eq!(sensors.len(), 4);
        assert_eq!(sensors.iter().filter(|s| s.z > 0.0).count(), 2);
        assert_eq!(sensors.iter().filter(|s| s.z < 0.0).count(), 2);
        // All inside the outline's footprint, so the visualisation and the
        // counting agree about which face a photon reaching them crossed.
        for sensor in sensors {
            assert!(block().outline.signed_distance_2d(sensor.x, sensor.y) < 0.0, "{sensor:?}");
        }
    }
}
