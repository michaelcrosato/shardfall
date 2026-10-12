//! A piece's own frame (plan §8.5, step 3): the yaw-only box that holds it tightest (3° steps,
//! then a 1° refinement), or the principal-axes box when that is at least 15% smaller. Angles
//! are snapped (§8.5 step 4): whole degrees, and 0/90/180 within 2°. Plus the Euler angles the
//! format writes (YXZ, degrees).

use glam::{DMat3, DQuat, DVec3, EulerRot};

/// A box in the world: its axes (columns, a proper rotation), centre and half size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub rot: DMat3,
    pub centre: DVec3,
    pub half: DVec3,
}

impl Frame {
    /// A world point in the frame.
    pub fn local(&self, p: DVec3) -> DVec3 {
        self.rot.transpose() * (p - self.centre)
    }

    pub fn volume(&self) -> f64 {
        8.0 * self.half.x * self.half.y * self.half.z
    }
}

/// The smallest half size a fitted part gets (0.5 mm: the format's 0.1 cm).
pub const MIN_HALF: f64 = 0.0005;

/// The box round `points` in the axes of `rot`.
pub fn fit_box(points: &[DVec3], rot: DMat3) -> Frame {
    let t = rot.transpose();
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for p in points {
        let q = t * *p;
        lo = lo.min(q);
        hi = hi.max(q);
    }
    let centre = rot * ((lo + hi) * 0.5);
    Frame { rot, centre, half: ((hi - lo) * 0.5).max(DVec3::splat(MIN_HALF)) }
}

/// A degree angle in whole degrees, snapped to a quarter turn within 2°, in (-180, 180].
pub fn snap(deg: f64) -> f64 {
    let mut d = deg.round();
    let quarter = (d / 90.0).round() * 90.0;
    if (deg - quarter).abs() <= 2.0 {
        d = quarter;
    }
    while d <= -180.0 {
        d += 360.0;
    }
    while d > 180.0 {
        d -= 360.0;
    }
    if d == -0.0 { 0.0 } else { d }
}

/// The yaw, pitch and roll (degrees) of a rotation, as format 2 applies them (Y, then X, then Z).
pub fn euler(rot: DMat3) -> [f64; 3] {
    let (y, x, z) = DQuat::from_mat3(&rot).normalize().to_euler(EulerRot::YXZ);
    [y.to_degrees(), x.to_degrees(), z.to_degrees()]
}

/// The rotation of yaw, pitch and roll in degrees.
pub fn from_euler(e: [f64; 3]) -> DMat3 {
    DMat3::from_quat(DQuat::from_euler(EulerRot::YXZ, e[0].to_radians(), e[1].to_radians(), e[2].to_radians()))
}

/// A rotation with its Euler angles snapped.
pub fn snapped(rot: DMat3) -> ([f64; 3], DMat3) {
    let e = euler(rot).map(snap);
    (e, from_euler(e))
}

/// The eigenvectors of a symmetric 3×3 matrix (columns), largest eigenvalue first (Jacobi).
#[allow(clippy::needless_range_loop)] // rotations index rows and columns by name
fn eigenvectors(m: DMat3) -> DMat3 {
    let mut a = m.to_cols_array_2d();
    let mut v = DMat3::IDENTITY.to_cols_array_2d();
    for _ in 0..32 {
        let (mut p, mut q, mut big) = (0, 1, 0.0);
        for i in 0..3 {
            for j in i + 1..3 {
                if a[i][j].abs() > big {
                    (p, q, big) = (i, j, a[i][j].abs());
                }
            }
        }
        if big < 1e-15 {
            break;
        }
        let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
        let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
        let t = if theta == 0.0 { 1.0 } else { t };
        let c = 1.0 / (t * t + 1.0).sqrt();
        let s = t * c;
        for k in 0..3 {
            let (akp, akq) = (a[k][p], a[k][q]);
            a[k][p] = c * akp - s * akq;
            a[k][q] = s * akp + c * akq;
        }
        for k in 0..3 {
            let (apk, aqk) = (a[p][k], a[q][k]);
            a[p][k] = c * apk - s * aqk;
            a[q][k] = s * apk + c * aqk;
        }
        for row in &mut v {
            let (vp, vq) = (row[p], row[q]);
            row[p] = c * vp - s * vq;
            row[q] = s * vp + c * vq;
        }
    }
    // `v` holds the vectors in its columns as [column][row] after the transposed updates.
    let vecs = DMat3::from_cols_array_2d(&v).transpose();
    let mut order = [0, 1, 2];
    order.sort_by(|&i, &j| a[j][j].total_cmp(&a[i][i]));
    let mut cols = order.map(|i| vecs.col(i).normalize());
    if cols[0].cross(cols[1]).dot(cols[2]) < 0.0 {
        cols[2] = -cols[2];
    }
    DMat3::from_cols(cols[0], cols[1], cols[2])
}

/// The piece's frame from its corners and surface samples.
pub fn frame(points: &[DVec3], samples: &[DVec3]) -> Frame {
    let yaw_box = |deg: f64| fit_box(points, DMat3::from_rotation_y(deg.to_radians()));
    let area = |f: &Frame| f.half.x * f.half.z;
    let mut best = (0.0, yaw_box(0.0));
    for step in 1..30 {
        let deg = step as f64 * 3.0;
        let f = yaw_box(deg);
        if area(&f) < area(&best.1) * (1.0 - 1e-9) {
            best = (deg, f);
        }
    }
    for d in [-2.0, -1.0, 1.0, 2.0] {
        let f = yaw_box(best.0 + d);
        if area(&f) < area(&best.1) * (1.0 - 1e-9) {
            best = (best.0 + d, f);
        }
    }
    let yaw = snap(best.0);
    let yaw_frame = yaw_box(yaw);
    // The principal axes of the surface.
    if samples.len() >= 4 {
        let mean = samples.iter().copied().sum::<DVec3>() / samples.len() as f64;
        let mut cov = DMat3::ZERO;
        for p in samples {
            let d = *p - mean;
            cov += DMat3::from_cols(d * d.x, d * d.y, d * d.z);
        }
        let axes = eigenvectors(cov);
        let (_, rot) = snapped(axes);
        let pca = fit_box(points, rot);
        if pca.volume() <= 0.85 * yaw_frame.volume() {
            return pca;
        }
    }
    yaw_frame
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapping_and_euler_round_trip() {
        assert_eq!(snap(88.4), 90.0);
        assert_eq!(snap(87.4), 87.0);
        assert_eq!(snap(-179.0), 180.0);
        assert_eq!(snap(181.5), 180.0);
        let r = from_euler([30.0, -20.0, 10.0]);
        let e = euler(r);
        assert!((e[0] - 30.0).abs() < 1e-9 && (e[1] + 20.0).abs() < 1e-9 && (e[2] - 10.0).abs() < 1e-9, "{e:?}");
    }

    #[test]
    fn a_turned_box_gets_its_own_axes() {
        let rot = DMat3::from_rotation_y(30f64.to_radians());
        let corners: Vec<DVec3> = (0..8)
            .map(|i| {
                rot * DVec3::new(
                    if i & 1 == 0 { -0.5 } else { 0.5 },
                    if i & 2 == 0 { -0.1 } else { 0.1 },
                    if i & 4 == 0 { -0.2 } else { 0.2 },
                )
            })
            .collect();
        let f = frame(&corners, &[]);
        assert!((f.volume() - 0.08).abs() < 1e-6, "{f:?}");
        // A tilted stick: the principal axes find it.
        let tilt = DMat3::from_rotation_z(35f64.to_radians());
        let stick: Vec<DVec3> = (0..8)
            .map(|i| {
                tilt * DVec3::new(
                    if i & 1 == 0 { -0.02 } else { 0.02 },
                    if i & 2 == 0 { -0.5 } else { 0.5 },
                    if i & 4 == 0 { -0.02 } else { 0.02 },
                )
            })
            .collect();
        let samples: Vec<DVec3> = (0..200).map(|k| tilt * DVec3::new(0.0, k as f64 / 199.0 - 0.5, 0.0)).collect();
        let f = frame(&stick, &samples);
        assert!(f.volume() < 0.002, "{f:?}");
    }
}
