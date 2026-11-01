//! Triangulation of convex polygon faces with the same diagonals as
//! OpenSCAD.
//!
//! OpenSCAD tessellates faces with a sweep-line tessellator (the GLU /
//! libtess2 algorithm) working in single precision: the face is projected
//! onto the coordinate plane of its dominant normal axis, and a monotone
//! region is cut into triangles walking from its rightmost to its leftmost
//! vertex in sweep order. For a convex face the whole face is one monotone
//! region, so reproducing that walk (with `f32` coordinates, whose ties
//! decide many diagonals) gives the same triangles.

use geometry::glam::{DVec3, Vec2, Vec3};

/// The tessellator's face normal: the largest-area triangle spanned by the
/// two vertices farthest apart along the axis of largest extent and any
/// third vertex (vertices visited with the first one moved last, like the
/// tessellator's internal list; the first of equal values wins).
fn normal(face: &[Vec3]) -> Vec3 {
    let pts: Vec<Vec3> = face.iter().skip(1).chain(face.first()).copied().collect();
    let Some(&first) = pts.first() else {
        return Vec3::Z;
    };
    let (mut min_v, mut max_v) = (first, first);
    let (mut min_i, mut max_i) = ([0usize; 3], [0usize; 3]);
    for (k, p) in pts.iter().enumerate() {
        for i in 0..3 {
            if p[i] < min_v[i] {
                min_v[i] = p[i];
                min_i[i] = k;
            }
            if p[i] > max_v[i] {
                max_v[i] = p[i];
                max_i[i] = k;
            }
        }
    }
    let mut i = 0;
    if max_v.y - min_v.y > max_v.x - min_v.x {
        i = 1;
    }
    if max_v.z - min_v.z > max_v[i] - min_v[i] {
        i = 2;
    }
    if min_v[i] >= max_v[i] {
        return Vec3::Z;
    }
    let (v1, v2) = (pts[min_i[i]], pts[max_i[i]]);
    let d1 = v1 - v2;
    let mut best = 0.0f32;
    let mut norm = Vec3::ZERO;
    for &v in &pts {
        let d2 = v - v2;
        let t = Vec3::new(
            d1.y * d2.z - d1.z * d2.y,
            d1.z * d2.x - d1.x * d2.z,
            d1.x * d2.y - d1.y * d2.x,
        );
        let len2 = t.x * t.x + t.y * t.y + t.z * t.z;
        if len2 > best {
            best = len2;
            norm = t;
        }
    }
    if best <= 0.0 {
        let mut n = Vec3::ZERO;
        n[long_axis(d1)] = 1.0;
        return n;
    }
    norm
}

fn long_axis(v: Vec3) -> usize {
    let mut i = 0;
    if v.y.abs() > v.x.abs() {
        i = 1;
    }
    if v.z.abs() > v[i].abs() {
        i = 2;
    }
    i
}

/// Projects a face like the tessellator: drop the dominant normal axis `i`,
/// `s` = coordinate `i + 1`, `t` = ± coordinate `i + 2`; then flip `t` if
/// that made the face clockwise.
fn project_face(face: &[DVec3]) -> Vec<Vec2> {
    let pts: Vec<Vec3> = face.iter().map(|p| p.as_vec3()).collect();
    let n = normal(&pts);
    let i = long_axis(n);
    let sign = if n[i] > 0.0 { 1.0 } else { -1.0 };
    let mut out: Vec<Vec2> = pts
        .iter()
        .map(|p| Vec2::new(p[(i + 1) % 3], sign * p[(i + 2) % 3]))
        .collect();
    let area: f32 = (0..out.len())
        .map(|k| {
            let (a, b) = (out[k], out[(k + 1) % out.len()]);
            (a.x - b.x) * (a.y + b.y)
        })
        .sum();
    // Twice the signed area, positive when counter-clockwise.
    if area < 0.0 {
        for p in &mut out {
            p.y = -p.y;
        }
    }
    out
}

fn vert_leq(u: Vec2, v: Vec2) -> bool {
    u.x < v.x || (u.x == v.x && u.y <= v.y)
}

fn edge_sign(u: Vec2, v: Vec2, w: Vec2) -> f32 {
    let gap_l = v.x - u.x;
    let gap_r = w.x - v.x;
    if gap_l + gap_r > 0.0 {
        (v.y - w.y) * gap_l + (v.y - u.y) * gap_r
    } else {
        0.0
    }
}

/// Triangles of a convex face (indices into `face`) as OpenSCAD makes
/// them, or `None` when the projected face is not convex.
pub fn convex_face(face: &[DVec3]) -> Option<Vec<[usize; 3]>> {
    let pts = project_face(face);
    let n = pts.len();
    let mut any = false;
    for k in 0..n {
        let (a, b, c) = (pts[k], pts[(k + 1) % n], pts[(k + 2) % n]);
        let turn = (b - a).perp_dot(c - a);
        if turn < 0.0 {
            return None;
        }
        any |= turn > 0.0;
    }
    if !any {
        return None;
    }
    Some(monotone(&pts))
}

/// Triangles (indices into `pts`) of a counter-clockwise monotone polygon,
/// with the tessellator's monotone-region walk.
fn monotone(pts: &[Vec2]) -> Vec<[usize; 3]> {
    let n = pts.len();
    let mut tris = Vec::with_capacity(n.saturating_sub(2));
    if n < 3 {
        return tris;
    }
    // The remaining polygon as a doubly linked list; an edge is named by
    // its origin vertex `v` (from `v` to `next[v]`).
    let mut next: Vec<usize> = (0..n).map(|i| (i + 1) % n).collect();
    let mut prev: Vec<usize> = (0..n).map(|i| (i + n - 1) % n).collect();
    // The edge whose origin is the rightmost vertex.
    let mut up = 0;
    for _ in 0..n {
        if !vert_leq(pts[next[up]], pts[up]) {
            break;
        }
        up = prev[up];
    }
    for _ in 0..n {
        if !vert_leq(pts[up], pts[next[up]]) {
            break;
        }
        up = next[up];
    }
    let mut lo = prev[up];
    let mut remaining = n;
    while next[up] != lo && remaining > 3 {
        if vert_leq(pts[next[up]], pts[lo]) {
            // up's destination is on the left: triangles from lo's origin.
            while next[lo] != up
                && remaining > 3
                && (vert_leq(pts[next[next[lo]]], pts[next[lo]])
                    || edge_sign(pts[lo], pts[next[lo]], pts[next[next[lo]]]) <= 0.0)
            {
                let (a, b) = (next[lo], next[next[lo]]);
                tris.push([lo, a, b]);
                next[lo] = b;
                prev[b] = lo;
                remaining -= 1;
            }
            lo = prev[lo];
        } else {
            // lo's origin is on the left: triangles from up's destination.
            while next[lo] != up
                && remaining > 3
                && (vert_leq(pts[prev[up]], pts[up])
                    || edge_sign(pts[next[up]], pts[up], pts[prev[up]]) >= 0.0)
            {
                let (p, d) = (prev[up], next[up]);
                tris.push([p, up, d]);
                next[p] = d;
                prev[d] = p;
                up = p;
                remaining -= 1;
            }
            up = next[up];
        }
    }
    // A fan from the leftmost vertex closes the region.
    while next[next[lo]] != up && remaining > 3 {
        let (a, b) = (next[lo], next[next[lo]]);
        tris.push([lo, a, b]);
        next[lo] = b;
        prev[b] = lo;
        remaining -= 1;
    }
    let (a, b) = (lo, next[lo]);
    tris.push([a, b, next[b]]);
    tris
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut t: Vec<[usize; 3]>) -> Vec<[usize; 3]> {
        for tri in &mut t {
            let m = (0..3).min_by_key(|&i| tri[i]).unwrap_or(0);
            tri.rotate_left(m);
        }
        t.sort_unstable();
        t
    }

    #[test]
    fn quads_like_openscad() {
        // Faces of cube([1, 2, 3]) with OpenSCAD's diagonals.
        let top = [
            DVec3::new(0.0, 0.0, 3.0),
            DVec3::new(1.0, 0.0, 3.0),
            DVec3::new(1.0, 2.0, 3.0),
            DVec3::new(0.0, 2.0, 3.0),
        ];
        assert_eq!(
            sorted(convex_face(&top).unwrap()),
            vec![[0, 1, 3], [1, 2, 3]]
        );
        let front = [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(1.0, 0.0, 0.0),
            DVec3::new(1.0, 0.0, 3.0),
            DVec3::new(0.0, 0.0, 3.0),
        ];
        assert_eq!(
            sorted(convex_face(&front).unwrap()),
            vec![[0, 1, 2], [0, 2, 3]]
        );
    }

    #[test]
    fn convex_polygon_is_covered() {
        let face: Vec<DVec3> = (0..12)
            .map(|i| {
                let a = f64::from(i) * std::f64::consts::TAU / 12.0;
                DVec3::new(a.cos(), a.sin(), 1.0)
            })
            .collect();
        let tris = convex_face(&face).unwrap();
        assert_eq!(tris.len(), 10);
        let area: f64 = tris
            .iter()
            .map(|t| (face[t[1]] - face[t[0]]).cross(face[t[2]] - face[t[0]]).z / 2.0)
            .sum();
        assert!((area - 3.0).abs() < 1e-12);
        // A concave face is left to the general triangulator.
        let l = [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(2.0, 0.0, 0.0),
            DVec3::new(2.0, 1.0, 0.0),
            DVec3::new(1.0, 1.0, 0.0),
            DVec3::new(1.0, 2.0, 0.0),
            DVec3::new(0.0, 2.0, 0.0),
        ];
        assert!(convex_face(&l).is_none());
    }
}
