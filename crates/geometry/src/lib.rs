//! Core geometry types shared by the evaluator, the exporters and the renderer.
//!
//! Coordinates are `f64` end to end (like OpenSCAD and Manifold); conversion to
//! `f32` happens only at the GPU boundary.

use std::collections::HashMap;

pub use glam;
use glam::DVec3;

/// Axis-aligned bounding box. An empty box has `min > max`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: DVec3,
    pub max: DVec3,
}

impl Aabb {
    pub const EMPTY: Self = Self {
        min: DVec3::splat(f64::INFINITY),
        max: DVec3::splat(f64::NEG_INFINITY),
    };

    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y || self.min.z > self.max.z
    }

    pub fn include(&mut self, p: DVec3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }

    pub fn size(&self) -> DVec3 {
        if self.is_empty() {
            DVec3::ZERO
        } else {
            self.max - self.min
        }
    }

    pub fn center(&self) -> DVec3 {
        (self.min + self.max) * 0.5
    }
}

impl Default for Aabb {
    fn default() -> Self {
        Self::EMPTY
    }
}

/// Indexed triangle mesh. Triangles are wound counter-clockwise when seen
/// from outside the solid (the STL / OpenSCAD convention).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub vertices: Vec<DVec3>,
    pub triangles: Vec<[u32; 3]>,
}

impl Mesh {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds an indexed mesh from unconnected triangles, merging vertices
    /// with bit-identical coordinates (as found in STL files).
    pub fn from_triangle_soup(soup: impl IntoIterator<Item = [DVec3; 3]>) -> Self {
        let mut mesh = Self::new();
        let mut index: HashMap<[u64; 3], u32> = HashMap::new();
        for tri in soup {
            let ids = tri.map(|p| {
                // Treat -0.0 and 0.0 as the same point.
                let p = p + DVec3::ZERO;
                let key = [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
                *index.entry(key).or_insert_with(|| {
                    mesh.vertices.push(p);
                    (mesh.vertices.len() - 1) as u32
                })
            });
            // Skip triangles that collapsed to a line or a point.
            if ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2] {
                mesh.triangles.push(ids);
            }
        }
        mesh
    }

    pub fn is_empty(&self) -> bool {
        self.triangles.is_empty()
    }

    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }

    pub fn triangle(&self, i: usize) -> [DVec3; 3] {
        self.triangles[i].map(|v| self.vertices[v as usize])
    }

    pub fn triangles(&self) -> impl ExactSizeIterator<Item = [DVec3; 3]> + '_ {
        self.triangles
            .iter()
            .map(|t| t.map(|v| self.vertices[v as usize]))
    }

    /// Unit normal of triangle `i`, or zero for a degenerate triangle.
    pub fn face_normal(&self, i: usize) -> DVec3 {
        let [a, b, c] = self.triangle(i);
        (b - a).cross(c - a).normalize_or_zero()
    }

    /// Bounds of the vertices actually referenced by triangles.
    pub fn bounds(&self) -> Aabb {
        let mut aabb = Aabb::EMPTY;
        for t in &self.triangles {
            for &v in t {
                aabb.include(self.vertices[v as usize]);
            }
        }
        aabb
    }

    /// Enclosed volume (divergence theorem). Positive for a closed mesh with
    /// outward-facing triangles; meaningless for open meshes.
    pub fn volume(&self) -> f64 {
        self.triangles()
            .map(|[a, b, c]| a.dot(b.cross(c)))
            .sum::<f64>()
            / 6.0
    }

    pub fn surface_area(&self) -> f64 {
        self.triangles()
            .map(|[a, b, c]| (b - a).cross(c - a).length() * 0.5)
            .sum()
    }

    /// Edges worth drawing as outlines: creases sharper than `min_angle_deg`,
    /// plus open (one face) and non-manifold (3+ faces) edges.
    pub fn feature_edges(&self, min_angle_deg: f64) -> Vec<[u32; 2]> {
        let mut faces_by_edge: HashMap<[u32; 2], Vec<usize>> = HashMap::new();
        for (face, tri) in self.triangles.iter().enumerate() {
            for k in 0..3 {
                let (a, b) = (tri[k], tri[(k + 1) % 3]);
                faces_by_edge
                    .entry([a.min(b), a.max(b)])
                    .or_default()
                    .push(face);
            }
        }
        let cos_limit = min_angle_deg.to_radians().cos();
        let mut edges: Vec<[u32; 2]> = faces_by_edge
            .into_iter()
            .filter(|(_, faces)| match faces.as_slice() {
                [f, g] => self.face_normal(*f).dot(self.face_normal(*g)) < cos_limit,
                _ => true,
            })
            .map(|(edge, _)| edge)
            .collect();
        edges.sort_unstable();
        edges
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unit cube from 12 outward-facing triangles, as an STL would store it.
    fn cube_soup() -> Vec<[DVec3; 3]> {
        let p = |x: f64, y: f64, z: f64| DVec3::new(x, y, z);
        let quads = [
            [p(0., 0., 0.), p(0., 1., 0.), p(1., 1., 0.), p(1., 0., 0.)], // bottom (-z)
            [p(0., 0., 1.), p(1., 0., 1.), p(1., 1., 1.), p(0., 1., 1.)], // top (+z)
            [p(0., 0., 0.), p(1., 0., 0.), p(1., 0., 1.), p(0., 0., 1.)], // front (-y)
            [p(0., 1., 0.), p(0., 1., 1.), p(1., 1., 1.), p(1., 1., 0.)], // back (+y)
            [p(0., 0., 0.), p(0., 0., 1.), p(0., 1., 1.), p(0., 1., 0.)], // left (-x)
            [p(1., 0., 0.), p(1., 1., 0.), p(1., 1., 1.), p(1., 0., 1.)], // right (+x)
        ];
        quads
            .iter()
            .flat_map(|[a, b, c, d]| [[*a, *b, *c], [*a, *c, *d]])
            .collect()
    }

    #[test]
    fn soup_is_welded() {
        let mesh = Mesh::from_triangle_soup(cube_soup());
        assert_eq!(mesh.vertices.len(), 8);
        assert_eq!(mesh.triangle_count(), 12);
    }

    #[test]
    fn cube_measures() {
        let mesh = Mesh::from_triangle_soup(cube_soup());
        assert!((mesh.volume() - 1.0).abs() < 1e-12);
        assert!((mesh.surface_area() - 6.0).abs() < 1e-12);
        let b = mesh.bounds();
        assert_eq!(b.min, DVec3::ZERO);
        assert_eq!(b.max, DVec3::ONE);
        assert_eq!(mesh.face_normal(0), DVec3::NEG_Z);
    }

    #[test]
    fn cube_has_twelve_feature_edges() {
        let mesh = Mesh::from_triangle_soup(cube_soup());
        // The face diagonals are flat and must not be reported.
        assert_eq!(mesh.feature_edges(20.0).len(), 12);
    }

    #[test]
    fn open_edges_are_features() {
        let mesh = Mesh::from_triangle_soup([[DVec3::ZERO, DVec3::X, DVec3::Y]]);
        assert_eq!(mesh.feature_edges(20.0).len(), 3);
    }

    #[test]
    fn degenerate_triangles_are_dropped() {
        let a = DVec3::ZERO;
        let b = DVec3::X;
        let mesh = Mesh::from_triangle_soup([[a, b, b], [a, -a, b]]);
        assert!(mesh.is_empty());
    }

    #[test]
    fn empty_bounds() {
        assert!(Mesh::new().bounds().is_empty());
        assert_eq!(Aabb::EMPTY.size(), DVec3::ZERO);
    }
}
