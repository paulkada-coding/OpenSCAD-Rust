//! Polygon meshes with exact OpenSCAD vertex positions, kept outside the
//! kernel until an operation needs a [`Manifold`] (a lone primitive is
//! output as is, like OpenSCAD does).

use geometry::Mesh;
use geometry::glam::{DMat4, DVec2, DVec3};
use manifold::Manifold;

/// Indexed polygon mesh. Faces are counter-clockwise seen from outside.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PolySet {
    pub vertices: Vec<DVec3>,
    pub faces: Vec<Vec<u32>>,
}

impl PolySet {
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    /// Applies an affine transform to every vertex, flipping the faces when
    /// the transform mirrors.
    pub fn transform(&mut self, m: &DMat4) {
        for v in &mut self.vertices {
            *v = transform_point(m, *v);
        }
        if m.determinant() < 0.0 {
            for f in &mut self.faces {
                f.reverse();
            }
        }
    }

    /// Triangle mesh: triangles and convex quads are split directly,
    /// other faces are triangulated in the plane of their normal.
    pub fn triangulate(&self) -> Mesh {
        let mut triangles = Vec::with_capacity(self.faces.len() * 2);
        for face in &self.faces {
            triangulate_face(&self.vertices, face, &mut triangles);
        }
        Mesh {
            vertices: self.vertices.clone(),
            triangles,
        }
    }

    /// Converts to a kernel solid, welding duplicated vertices when needed.
    pub fn to_manifold(&self) -> Result<Manifold, manifold::Error> {
        let mesh = self.triangulate();
        match Manifold::from_mesh(&mesh) {
            Ok(m) => Ok(m),
            Err(manifold::Error::NotManifold) => Manifold::from_mesh_merged(&mesh),
            Err(e) => Err(e),
        }
    }
}

/// `m * (p, 1)`, summed like Eigen's matrix-vector product
/// (`((m00 x + m01 y) + m02 z) + m03`).
pub fn transform_point(m: &DMat4, p: DVec3) -> DVec3 {
    let c = &m.to_cols_array_2d();
    DVec3::new(
        c[0][0] * p.x + c[1][0] * p.y + c[2][0] * p.z + c[3][0],
        c[0][1] * p.x + c[1][1] * p.y + c[2][1] * p.z + c[3][1],
        c[0][2] * p.x + c[1][2] * p.y + c[2][2] * p.z + c[3][2],
    )
}

/// Newell normal of a polygon (not normalized; zero when degenerate).
pub fn newell_normal(vertices: &[DVec3], face: &[u32]) -> DVec3 {
    let mut n = DVec3::ZERO;
    for i in 0..face.len() {
        let a = vertices[face[i] as usize];
        let b = vertices[face[(i + 1) % face.len()] as usize];
        n.x += (a.y - b.y) * (a.z + b.z);
        n.y += (a.z - b.z) * (a.x + b.x);
        n.z += (a.x - b.x) * (a.y + b.y);
    }
    n
}

/// Projects onto the coordinate plane most perpendicular to `normal`,
/// keeping the polygon's orientation (counter-clockwise stays so).
pub fn project_to_plane(normal: DVec3, p: DVec3) -> DVec2 {
    let a = normal.abs();
    if a.z >= a.x && a.z >= a.y {
        if normal.z >= 0.0 {
            DVec2::new(p.x, p.y)
        } else {
            DVec2::new(p.y, p.x)
        }
    } else if a.x >= a.y {
        if normal.x >= 0.0 {
            DVec2::new(p.y, p.z)
        } else {
            DVec2::new(p.z, p.y)
        }
    } else if normal.y >= 0.0 {
        DVec2::new(p.z, p.x)
    } else {
        DVec2::new(p.x, p.z)
    }
}

fn triangulate_face(vertices: &[DVec3], face: &[u32], out: &mut Vec<[u32; 3]>) {
    match face.len() {
        0..=2 => {}
        3 => out.push([face[0], face[1], face[2]]),
        _ => {
            let normal = newell_normal(vertices, face);
            if normal == DVec3::ZERO {
                return;
            }
            let pts: Vec<DVec2> = face
                .iter()
                .map(|&i| project_to_plane(normal, vertices[i as usize]))
                .collect();
            if face.len() == 4 {
                // Split a convex quad along the shorter diagonal.
                let cross = |a: DVec2, b: DVec2, c: DVec2| (b - a).perp_dot(c - a);
                let convex =
                    (0..4).all(|i| cross(pts[i], pts[(i + 1) % 4], pts[(i + 2) % 4]) > 0.0);
                if convex {
                    let ac = (pts[2] - pts[0]).length_squared();
                    let bd = (pts[3] - pts[1]).length_squared();
                    if bd < ac {
                        out.push([face[0], face[1], face[3]]);
                        out.push([face[1], face[2], face[3]]);
                    } else {
                        out.push([face[0], face[1], face[2]]);
                        out.push([face[0], face[2], face[3]]);
                    }
                    return;
                }
            }
            if is_convex(&pts) {
                for i in 1..face.len() - 1 {
                    out.push([face[0], face[i], face[i + 1]]);
                }
                return;
            }
            for t in manifold::triangulate(&[pts], -1.0) {
                out.push(t.map(|i| face[i]));
            }
        }
    }
}

/// Strictly convex, counter-clockwise polygon.
fn is_convex(pts: &[DVec2]) -> bool {
    let n = pts.len();
    (0..n).all(|i| {
        let (a, b, c) = (pts[i], pts[(i + 1) % n], pts[(i + 2) % n]);
        (b - a).perp_dot(c - a) > 0.0
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concave_face_is_triangulated() {
        // An L-shaped hexagon in the XY plane.
        let vertices = vec![
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(2.0, 0.0, 0.0),
            DVec3::new(2.0, 1.0, 0.0),
            DVec3::new(1.0, 1.0, 0.0),
            DVec3::new(1.0, 2.0, 0.0),
            DVec3::new(0.0, 2.0, 0.0),
        ];
        let ps = PolySet {
            vertices,
            faces: vec![vec![0, 1, 2, 3, 4, 5]],
        };
        let mesh = ps.triangulate();
        assert_eq!(mesh.triangles.len(), 4);
        let area: f64 = mesh
            .triangles()
            .map(|[a, b, c]| (b - a).cross(c - a).z / 2.0)
            .sum();
        assert!((area - 3.0).abs() < 1e-12);
    }

    #[test]
    fn mirror_flips_faces() {
        let mut ps = PolySet {
            vertices: vec![DVec3::ZERO, DVec3::X, DVec3::Y],
            faces: vec![vec![0, 1, 2]],
        };
        ps.transform(&DMat4::from_scale(DVec3::new(-1.0, 1.0, 1.0)));
        assert_eq!(ps.faces[0], vec![2, 1, 0]);
        assert_eq!(ps.vertices[1], DVec3::new(-1.0, 0.0, 0.0));
    }
}
