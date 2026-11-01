//! `linear_extrude` and `rotate_extrude` as exact polygon meshes.

use std::collections::HashMap;

use geometry::csg::Segments;
use geometry::glam::{DVec2, DVec3};

use crate::fragments::{clamp_count, cos_degrees, fragments, sin_degrees};
use crate::planar::Planar;
use crate::polyset::PolySet;

pub struct LinearParams {
    pub height: f64,
    pub direction: Option<DVec3>,
    pub center: bool,
    pub twist: f64,
    pub slices: Option<f64>,
    pub segments: Option<f64>,
    pub scale: DVec2,
    pub tessellation: Segments,
}

/// Builds a mesh while merging vertices with identical coordinates (rings
/// collapsed by a zero scale, points on the rotation axis).
struct Builder {
    ps: PolySet,
    index: HashMap<[u64; 3], u32>,
}

impl Builder {
    fn new() -> Self {
        Self {
            ps: PolySet::default(),
            index: HashMap::new(),
        }
    }

    fn vertex(&mut self, p: DVec3) -> u32 {
        let p = p + DVec3::ZERO;
        let key = [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
        *self.index.entry(key).or_insert_with(|| {
            self.ps.vertices.push(p);
            (self.ps.vertices.len() - 1) as u32
        })
    }

    /// Adds a face, dropping repeated vertices; degenerate faces vanish.
    fn face(&mut self, f: &[u32]) {
        let mut out: Vec<u32> = Vec::with_capacity(f.len());
        for &v in f {
            if out.last() != Some(&v) {
                out.push(v);
            }
        }
        while out.len() > 1 && out.first() == out.last() {
            out.pop();
        }
        if out.len() >= 3 {
            self.ps.faces.push(out);
        }
    }

    /// Flips every face if the mesh came out inside-out.
    fn finish(mut self) -> PolySet {
        if self.ps.triangulate().volume() < 0.0 {
            for f in &mut self.ps.faces {
                f.reverse();
            }
        }
        self.ps
    }
}

/// Number of slices OpenSCAD uses when `slices` is not given.
fn default_slices(p: &LinearParams, max_r: f64) -> u32 {
    let twist = p.twist.abs();
    let mut slices = 1;
    if twist > 0.0 {
        let seg = &p.tessellation;
        let n = if seg.fn_ > 0.0 {
            (seg.fn_.max(3.0) * twist / 360.0).ceil()
        } else {
            (twist / seg.fa).ceil()
        };
        slices = clamp_count(n).max(1);
    }
    let _ = max_r;
    slices
}

/// Which diagonal splits the side quads: from the lower vertex of an edge
/// to the upper vertex of the next one, or the other way.
fn twist_split_low(twist: f64) -> bool {
    twist >= 0.0
}

pub fn linear_extrude(region: &Planar, p: &LinearParams) -> Option<PolySet> {
    if !(p.height > 0.0 && p.height.is_finite()) || region.is_empty() {
        return None;
    }
    let max_r = region
        .contours
        .iter()
        .flatten()
        .map(|q| q.length())
        .fold(0.0, f64::max);
    let slices = match p.slices {
        Some(s) => clamp_count(s).max(1),
        None => default_slices(p, max_r),
    };
    let scale = DVec2::new(p.scale.x.max(0.0), p.scale.y.max(0.0));
    // The extrusion runs `height` along the (unit) direction.
    let dir = match p.direction {
        // Already unit length (normalizing again would move it by an ulp).
        Some(v) if v.z > 0.0 && v.is_finite() => v,
        _ => DVec3::Z,
    };
    let base = if p.center {
        -dir * (p.height / 2.0)
    } else {
        DVec3::ZERO
    };

    let mut b = Builder::new();
    // Vertex ids per slice, contour, point.
    let mut rings: Vec<Vec<Vec<u32>>> = Vec::with_capacity(slices as usize + 1);
    for i in 0..=slices {
        let (fi, fs) = (f64::from(i), f64::from(slices));
        let angle = -p.twist * fi / fs;
        let s = DVec2::ONE + (scale - DVec2::ONE) * fi / fs;
        let (c, sn) = (cos_degrees(angle), sin_degrees(angle));
        let along = p.height * f64::from(i) / f64::from(slices);
        let shift = if dir == DVec3::Z {
            DVec3::new(0.0, 0.0, along + base.z)
        } else {
            dir * along + base
        };
        let slice = region
            .contours
            .iter()
            .map(|contour| {
                contour
                    .iter()
                    .map(|q| {
                        let q = q * s;
                        let r = DVec2::new(c * q.x - sn * q.y, sn * q.x + c * q.y);
                        b.vertex(DVec3::new(r.x + shift.x, r.y + shift.y, shift.z))
                    })
                    .collect::<Vec<u32>>()
            })
            .collect();
        rings.push(slice);
    }
    // Sides.
    for i in 0..slices as usize {
        for (lo, hi) in rings[i].iter().zip(&rings[i + 1]) {
            let n = lo.len();
            for k in 0..n {
                let k1 = (k + 1) % n;
                // Twisted quads are not planar: split them like OpenSCAD.
                if twist_split_low(p.twist) {
                    b.face(&[lo[k], lo[k1], hi[k1]]);
                    b.face(&[lo[k], hi[k1], hi[k]]);
                } else {
                    b.face(&[lo[k], lo[k1], hi[k]]);
                    b.face(&[lo[k1], hi[k1], hi[k]]);
                }
            }
        }
    }
    // Caps.
    let tris = region.triangulate();
    for (ring, flip) in [(&rings[0], true), (&rings[slices as usize], false)] {
        let ids: Vec<u32> = ring.iter().flatten().copied().collect();
        for t in &tris {
            let f = [ids[t[0]], ids[t[1]], ids[t[2]]];
            if flip {
                b.face(&[f[0], f[2], f[1]]);
            } else {
                b.face(&f);
            }
        }
    }
    Some(b.finish())
}

pub struct RotateParams {
    pub angle: f64,
    pub start: f64,
    pub tessellation: Segments,
}

/// Errors reported by OpenSCAD for `rotate_extrude`.
pub enum RotateError {
    CrossesAxis { min_x: f64, max_x: f64 },
}

impl RotateError {
    pub fn message(&self) -> String {
        match self {
            Self::CrossesAxis { min_x, max_x } => format!(
                "Children of rotate_extrude() may not lie across the Y axis (Range of X coords for all children [{min_x:.2} : {max_x:.2}])"
            ),
        }
    }
}

pub fn rotate_extrude(region: &Planar, p: &RotateParams) -> Result<Option<PolySet>, RotateError> {
    if region.is_empty() || p.angle == 0.0 || !p.angle.is_finite() {
        return Ok(None);
    }
    let (min, max) = region.bounds();
    if min.x < 0.0 && max.x > 0.0 {
        return Err(RotateError::CrossesAxis {
            min_x: min.x,
            max_x: max.x,
        });
    }
    // A profile entirely at x <= 0 is mirrored onto the positive side.
    let mut region = region.clone();
    if max.x <= 0.0 {
        for c in &mut region.contours {
            for q in c.iter_mut() {
                q.x = -q.x;
            }
            c.reverse();
        }
    }
    let max_x = region.bounds().1.x;
    let angle = p.angle.clamp(-360.0, 360.0);
    let full = angle.abs() >= 360.0;
    let n = fragments(max_x, &p.tessellation);
    let steps = clamp_count((f64::from(n) * angle.abs() / 360.0).ceil()).max(1);
    let start = if full { p.start } else { p.start };

    let mut b = Builder::new();
    let ring_count = if full { steps } else { steps + 1 };
    let mut rings: Vec<Vec<Vec<u32>>> = Vec::with_capacity(ring_count as usize);
    for j in 0..ring_count {
        let a = start + angle * f64::from(j) / f64::from(steps);
        let (c, s) = (cos_degrees(a), sin_degrees(a));
        rings.push(
            region
                .contours
                .iter()
                .map(|contour| {
                    contour
                        .iter()
                        .map(|q| b.vertex(DVec3::new(q.x * c, q.x * s, q.y)))
                        .collect()
                })
                .collect(),
        );
    }
    for j in 0..steps as usize {
        let next = (j + 1) % ring_count as usize;
        for (cur, nxt) in rings[j].iter().zip(&rings[next]) {
            let len = cur.len();
            for k in 0..len {
                let k1 = (k + 1) % len;
                b.face(&[cur[k], nxt[k], nxt[k1], cur[k1]]);
            }
        }
    }
    if !full {
        let tris = region.triangulate();
        let first: Vec<u32> = rings[0].iter().flatten().copied().collect();
        let last: Vec<u32> = rings[steps as usize].iter().flatten().copied().collect();
        for t in &tris {
            b.face(&[first[t[0]], first[t[1]], first[t[2]]]);
            b.face(&[last[t[0]], last[t[2]], last[t[1]]]);
        }
    }
    Ok(Some(b.finish()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Planar {
        Planar::new(vec![vec![
            DVec2::new(x0, y0),
            DVec2::new(x1, y0),
            DVec2::new(x1, y1),
            DVec2::new(x0, y1),
        ]])
    }

    fn fn_(n: f64) -> Segments {
        Segments {
            fn_: n,
            ..Segments::default()
        }
    }

    fn linear(height: f64) -> LinearParams {
        LinearParams {
            height,
            direction: None,
            center: false,
            twist: 0.0,
            slices: None,
            segments: None,
            scale: DVec2::ONE,
            tessellation: Segments::default(),
        }
    }

    #[test]
    fn prism_and_pyramid() {
        let sq = square(0.0, 0.0, 2.0, 3.0);
        let prism = linear_extrude(&sq, &linear(4.0)).unwrap();
        assert_eq!(prism.vertices.len(), 8);
        let m = prism.to_manifold().unwrap();
        assert!((m.volume() - 24.0).abs() < 1e-9);
        let pyramid = linear_extrude(
            &sq,
            &LinearParams {
                scale: DVec2::ZERO,
                center: true,
                ..linear(3.0)
            },
        )
        .unwrap();
        assert_eq!(pyramid.vertices.len(), 5);
        let m = pyramid.to_manifold().unwrap();
        assert!((m.volume() - 6.0).abs() < 1e-9);
        assert_eq!(m.bounds().min.z, -1.5);
    }

    #[test]
    fn twisted_slices() {
        let sq = square(-1.0, -1.0, 1.0, 1.0);
        let twisted = linear_extrude(
            &sq,
            &LinearParams {
                twist: 90.0,
                ..linear(1.0)
            },
        )
        .unwrap();
        let m = twisted.to_manifold().unwrap();
        assert_eq!(m.status(), Ok(()));
        // Clockwise: the corner (1, -1) ends at (-1, -1).
        assert!(twisted.vertices.contains(&DVec3::new(-1.0, -1.0, 1.0)));
    }

    #[test]
    fn revolutions() {
        let sq = square(5.0, 0.0, 6.0, 1.0);
        let ring = rotate_extrude(
            &sq,
            &RotateParams {
                angle: 360.0,
                start: 180.0,
                tessellation: fn_(8.0),
            },
        )
        .ok()
        .flatten()
        .unwrap();
        assert_eq!(ring.vertices.len(), 32);
        let m = ring.to_manifold().unwrap();
        assert_eq!(m.genus(), 1);
        let quarter = rotate_extrude(
            &sq,
            &RotateParams {
                angle: 90.0,
                start: 0.0,
                tessellation: fn_(8.0),
            },
        )
        .ok()
        .flatten()
        .unwrap();
        assert_eq!(quarter.vertices.len(), 12);
        assert!(quarter.to_manifold().unwrap().volume() > 0.0);
        // Touching the axis: points there are shared.
        let disc = rotate_extrude(
            &square(0.0, 0.0, 1.0, 1.0),
            &RotateParams {
                angle: 360.0,
                start: 180.0,
                tessellation: fn_(8.0),
            },
        )
        .ok()
        .flatten()
        .unwrap();
        assert_eq!(disc.vertices.len(), 18);
        assert_eq!(disc.to_manifold().unwrap().status(), Ok(()));
        let across = rotate_extrude(
            &square(-1.0, 0.0, 1.0, 1.0),
            &RotateParams {
                angle: 360.0,
                start: 0.0,
                tessellation: fn_(8.0),
            },
        );
        assert_eq!(
            across.err().map(|e| e.message()).unwrap(),
            "Children of rotate_extrude() may not lie across the Y axis (Range of X coords for all children [-1.00 : 1.00])"
        );
    }
}
