//! Primitives with OpenSCAD's exact tessellation.

use geometry::csg::Segments;
use geometry::glam::{DVec2, DVec3};
use manifold::FillRule;

use crate::fragments::{cos_degrees, fragments, sin_degrees};
use crate::planar::Planar;
use crate::polyset::PolySet;

fn positive(v: f64) -> bool {
    v > 0.0 && v.is_finite()
}

pub fn cube(size: DVec3, center: bool) -> Option<PolySet> {
    if !(positive(size.x) && positive(size.y) && positive(size.z)) {
        return None;
    }
    let (lo, hi) = if center {
        (-size / 2.0, size / 2.0)
    } else {
        (DVec3::ZERO, size)
    };
    let vertices = (0..8)
        .map(|i| {
            DVec3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            )
        })
        .collect();
    // Same faces (and starting vertices) as OpenSCAD's OFF output.
    let faces = vec![
        vec![4, 5, 7, 6],
        vec![2, 3, 1, 0],
        vec![0, 1, 5, 4],
        vec![1, 3, 7, 5],
        vec![3, 2, 6, 7],
        vec![2, 0, 4, 6],
    ];
    Some(PolySet { vertices, faces })
}

/// Points of a regular polygon inscribed in a circle, starting on +X.
pub fn circle_points(r: f64, n: u32) -> Vec<DVec2> {
    (0..n)
        .map(|i| {
            let phi = 360.0 * f64::from(i) / f64::from(n);
            DVec2::new(r * cos_degrees(phi), r * sin_degrees(phi))
        })
        .collect()
}

pub fn sphere(r: f64, octa: bool, seg: &Segments) -> Option<PolySet> {
    if !positive(r) {
        return None;
    }
    let n = fragments(r, seg);
    Some(if octa {
        octa_sphere(r, n)
    } else {
        orig_sphere(r, n)
    })
}

/// Rings of latitude between the poles (style "orig").
fn orig_sphere(r: f64, n: u32) -> PolySet {
    let rings = n.div_ceil(2);
    let mut vertices = Vec::with_capacity((rings * n) as usize);
    for i in 0..rings {
        let phi = 180.0 * (f64::from(i) + 0.5) / f64::from(rings);
        let ring_r = r * sin_degrees(phi);
        let z = r * cos_degrees(phi);
        vertices.extend(circle_points(ring_r, n).into_iter().map(|p| p.extend(z)));
    }
    let idx = |ring: u32, j: u32| ring * n + j % n;
    let mut faces = Vec::with_capacity((rings + 1) as usize * n as usize);
    faces.push((0..n).map(|j| idx(0, j)).collect());
    for ring in 0..rings - 1 {
        for j in 0..n {
            faces.push(vec![
                idx(ring, j + 1),
                idx(ring, j),
                idx(ring + 1, j),
                idx(ring + 1, j + 1),
            ]);
        }
    }
    faces.push((0..n).rev().map(|j| idx(rings - 1, j)).collect());
    PolySet { vertices, faces }
}

/// Octahedron subdivision (style "octa"): each octant face gets a
/// triangular grid of `q = ceil(n / 4)` steps. Points on the octahedron
/// edges split the 90° arcs evenly, exactly like OpenSCAD; interior points
/// average the spherical interpolations along the three grid directions,
/// which is close to (not bit-identical with) OpenSCAD for `q > 2`.
fn octa_sphere(r: f64, n: u32) -> PolySet {
    let q = n.div_ceil(4).max(1);
    let qf = f64::from(q);
    // Point on the arc from axis `a` to axis `b` at grid index `t`.
    let arc = |a: usize, b: usize, t: u32| {
        let deg = 90.0 * f64::from(t) / qf;
        let mut v = DVec3::ZERO;
        v[a] = cos_degrees(deg);
        v[b] = sin_degrees(deg);
        v
    };
    let slerp = |u: DVec3, v: DVec3, t: f64| {
        let w = u.dot(v).clamp(-1.0, 1.0).acos();
        if w.abs() < 1e-15 {
            return u;
        }
        (u * ((1.0 - t) * w).sin() + v * (t * w).sin()) / w.sin()
    };
    let point = |i: u32, j: u32, k: u32| -> DVec3 {
        // Corners and edges: exact arcs.
        match (i, j, k) {
            (_, _, 0) => return arc(0, 1, j),
            (_, 0, _) => return arc(0, 2, k),
            (0, _, _) => return arc(1, 2, k),
            _ => {}
        }
        let rows = [
            slerp(arc(0, 2, k), arc(1, 2, k), f64::from(j) / f64::from(i + j)),
            slerp(arc(1, 0, i), arc(2, 0, i), f64::from(k) / f64::from(j + k)),
            slerp(arc(0, 1, j), arc(2, 1, j), f64::from(k) / f64::from(i + k)),
        ];
        ((rows[0] + rows[1] + rows[2]) / 3.0).normalize()
    };
    let mut points = Vec::new();
    for i in 0..=q {
        for j in 0..=q - i {
            let p = point(i, j, q - i - j);
            for signs in 0..8 {
                let s = DVec3::new(
                    if signs & 1 == 0 { 1.0 } else { -1.0 },
                    if signs & 2 == 0 { 1.0 } else { -1.0 },
                    if signs & 4 == 0 { 1.0 } else { -1.0 },
                );
                points.push(r * (p * s) + DVec3::ZERO);
            }
        }
    }
    points.sort_by(|a, b| {
        a.x.total_cmp(&b.x)
            .then(a.y.total_cmp(&b.y))
            .then(a.z.total_cmp(&b.z))
    });
    points.dedup();
    // The sphere is convex: its faces are the hull of its points.
    let mesh = manifold::Manifold::hull_of_points(&points).to_mesh();
    PolySet {
        faces: mesh.triangles.iter().map(|t| t.to_vec()).collect(),
        vertices: mesh.vertices,
    }
}

pub fn cylinder(h: f64, r1: f64, r2: f64, center: bool, seg: &Segments) -> Option<PolySet> {
    let valid_r = |r: f64| r >= 0.0 && r.is_finite();
    if !positive(h) || !valid_r(r1) || !valid_r(r2) || (r1 <= 0.0 && r2 <= 0.0) {
        return None;
    }
    let n = fragments(r1.max(r2), seg);
    let (z1, z2) = if center {
        (-h / 2.0, h / 2.0)
    } else {
        (0.0, h)
    };
    let mut vertices = Vec::new();
    let mut ring = |r: f64, z: f64| -> Option<u32> {
        let start = vertices.len() as u32;
        if r > 0.0 {
            vertices.extend(circle_points(r, n).into_iter().map(|p| p.extend(z)));
            Some(start)
        } else {
            vertices.push(DVec3::new(0.0, 0.0, z));
            None
        }
    };
    let bottom = ring(r1, z1);
    let top = ring(r2, z2);
    let mut faces = Vec::new();
    match (bottom, top) {
        (Some(b), Some(t)) => {
            faces.push((0..n).rev().map(|j| b + j).collect());
            faces.push((0..n).map(|j| t + j).collect());
            for j in 0..n {
                let k = (j + 1) % n;
                faces.push(vec![b + j, b + k, t + k, t + j]);
            }
        }
        (Some(b), None) => {
            let apex = n;
            faces.push((0..n).rev().map(|j| b + j).collect());
            for j in 0..n {
                faces.push(vec![b + j, b + (j + 1) % n, apex]);
            }
        }
        (None, Some(t)) => {
            let apex = 0;
            faces.push((0..n).map(|j| t + j).collect());
            for j in 0..n {
                faces.push(vec![t + (j + 1) % n, t + j, apex]);
            }
        }
        (None, None) => return None,
    }
    Some(PolySet { vertices, faces })
}

/// Result of checking polyhedron input.
pub struct PolyhedronResult {
    pub polyset: PolySet,
    pub warnings: Vec<String>,
}

/// OpenSCAD's faces are clockwise seen from outside: they are reversed.
/// Faces referring to missing points are dropped with OpenSCAD's warning.
pub fn polyhedron(points: &[DVec3], faces: &[Vec<usize>]) -> PolyhedronResult {
    let mut warnings = Vec::new();
    let mut out_faces = Vec::with_capacity(faces.len());
    'faces: for (fi, face) in faces.iter().enumerate() {
        for (vi, &p) in face.iter().enumerate() {
            if p >= points.len() {
                warnings.push(format!(
                    "Point index {p} is out of bounds (from faces[{fi}][{vi}])"
                ));
                continue 'faces;
            }
        }
        // Repeated consecutive points add nothing.
        let mut f: Vec<u32> = Vec::with_capacity(face.len());
        for &p in face.iter().rev() {
            if f.last() != Some(&(p as u32)) {
                f.push(p as u32);
            }
        }
        while f.len() > 1 && f.first() == f.last() {
            f.pop();
        }
        if f.len() >= 3 {
            out_faces.push(f);
        }
    }
    PolyhedronResult {
        polyset: PolySet {
            vertices: points.to_vec(),
            faces: out_faces,
        },
        warnings,
    }
}

pub fn square(size: DVec2, center: bool) -> Option<Planar> {
    if !(positive(size.x) && positive(size.y)) {
        return None;
    }
    let (lo, hi) = if center {
        (-size / 2.0, size / 2.0)
    } else {
        (DVec2::ZERO, size)
    };
    Some(Planar::new(vec![vec![
        lo,
        DVec2::new(hi.x, lo.y),
        hi,
        DVec2::new(lo.x, hi.y),
    ]]))
}

pub fn circle(r: f64, seg: &Segments) -> Option<Planar> {
    if !positive(r) {
        return None;
    }
    Some(Planar::new(vec![circle_points(r, fragments(r, seg))]))
}

/// `polygon()`: paths (or all points in order) combined with the even-odd
/// rule and snapped, like OpenSCAD.
pub fn polygon(points: &[DVec2], paths: Option<&[Vec<usize>]>) -> Planar {
    let contours: Vec<Vec<DVec2>> = match paths {
        None => vec![points.to_vec()],
        Some(paths) => paths
            .iter()
            .map(|path| {
                path.iter()
                    .filter_map(|&i| points.get(i).copied())
                    .collect()
            })
            .collect(),
    };
    let contours: Vec<Vec<DVec2>> = contours
        .into_iter()
        .filter(|c| c.len() >= 3 && c.iter().all(|p| p.is_finite()))
        .collect();
    Planar::from_contours(&contours, FillRule::EvenOdd)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fn_(n: f64) -> Segments {
        Segments {
            fn_: n,
            ..Segments::default()
        }
    }

    fn volume(ps: &PolySet) -> f64 {
        ps.triangulate().volume()
    }

    #[test]
    fn cube_volume_and_bounds() {
        let c = cube(DVec3::new(1.0, 2.0, 3.0), true).unwrap();
        assert!((volume(&c) - 6.0).abs() < 1e-12);
        let b = c.triangulate().bounds();
        assert_eq!(b.min, DVec3::new(-0.5, -1.0, -1.5));
        assert!(cube(DVec3::new(1.0, 0.0, 1.0), false).is_none());
        assert!(cube(DVec3::splat(-1.0), false).is_none());
    }

    #[test]
    fn sphere_matches_openscad_layout() {
        // $fn = 8: 4 rings of 8 points; first point of ring 0 at
        // (sin 22.5, 0, cos 22.5).
        let s = sphere(1.0, false, &fn_(8.0)).unwrap();
        assert_eq!(s.vertices.len(), 32);
        assert_eq!(
            s.vertices[0],
            DVec3::new(0.3826834323650898, 0.0, 0.9238795325112867)
        );
        assert!(volume(&s) > 0.0);
        let mesh = s.to_manifold().unwrap();
        assert_eq!(mesh.status(), Ok(()));
        // Odd fragment counts round the ring count up.
        assert_eq!(sphere(1.0, false, &fn_(5.0)).unwrap().vertices.len(), 15);
    }

    #[test]
    fn octa_sphere_counts() {
        for (n, verts) in [(8.0, 18), (5.0, 18), (12.0, 38), (16.0, 66)] {
            let s = sphere(1.0, true, &fn_(n)).unwrap();
            assert_eq!(s.vertices.len(), verts, "$fn = {n}");
            let m = s.to_manifold().unwrap();
            assert_eq!(m.status(), Ok(()));
            assert!(m.volume() > 0.0);
        }
    }

    #[test]
    fn cylinders_and_cones() {
        let c = cylinder(2.0, 1.0, 1.0, false, &fn_(4.0)).unwrap();
        // Square prism inscribed in the unit circle: area 2.
        assert!((volume(&c) - 4.0).abs() < 1e-12);
        let cone = cylinder(3.0, 1.0, 0.0, true, &fn_(4.0)).unwrap();
        assert_eq!(cone.vertices.len(), 5);
        assert!((volume(&cone) - 2.0).abs() < 1e-12);
        assert_eq!(cone.to_manifold().unwrap().status(), Ok(()));
        let inverted = cylinder(3.0, 0.0, 1.0, false, &fn_(4.0)).unwrap();
        assert!((volume(&inverted) - 2.0).abs() < 1e-12);
        assert!(cylinder(1.0, 0.0, 0.0, false, &fn_(4.0)).is_none());
        assert!(cylinder(0.0, 1.0, 1.0, false, &fn_(4.0)).is_none());
    }

    #[test]
    fn polyhedron_faces_are_reversed_and_checked() {
        let points = [DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::Z];
        let faces = vec![
            vec![0, 1, 2],
            vec![0, 3, 1],
            vec![0, 2, 3],
            vec![1, 3, 2],
            vec![0, 1, 7],
        ];
        let r = polyhedron(&points, &faces);
        assert_eq!(
            r.warnings,
            ["Point index 7 is out of bounds (from faces[4][2])"]
        );
        assert_eq!(r.polyset.faces.len(), 4);
        assert!((volume(&r.polyset) - 1.0 / 6.0).abs() < 1e-12);
    }

    #[test]
    fn planar_primitives() {
        let s = square(DVec2::new(2.0, 3.0), true).unwrap();
        assert_eq!(s.area(), 6.0);
        let c = circle(1.0, &fn_(4.0)).unwrap();
        assert!((c.area() - 2.0).abs() < 1e-12);
        let p = polygon(
            &[
                DVec2::ZERO,
                DVec2::new(4.0, 0.0),
                DVec2::new(4.0, 4.0),
                DVec2::new(0.0, 4.0),
                DVec2::new(1.0, 1.0),
                DVec2::new(3.0, 1.0),
                DVec2::new(3.0, 3.0),
                DVec2::new(1.0, 3.0),
            ],
            Some(&[vec![0, 1, 2, 3], vec![4, 5, 6, 7]]),
        );
        assert_eq!(p.area(), 12.0);
    }
}
