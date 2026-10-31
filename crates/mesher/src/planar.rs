//! 2D regions. Like OpenSCAD, every 2D boolean (including the clean-up of
//! `polygon()` input) runs Clipper on integer coordinates with a fixed grid
//! of 2^-27 units (observed: `polygon([[0.3, 0], ...])` comes out as
//! 0.29999999701976776 = round(0.3 * 2^27) / 2^27), while `circle()` and
//! `square()` keep their exact coordinates until an operation needs Clipper.

use geometry::Mesh;
use geometry::glam::{DMat4, DVec2, DVec3};
use manifold::FillRule;
use manifold::clipper::{self, ClipOp, Join, Path64};

/// Clipper scale: coordinates are snapped to multiples of 1 / GRID.
pub const GRID: f64 = 134_217_728.0;

/// A 2D region: outlines counter-clockwise, holes clockwise, no overlaps.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Planar {
    pub contours: Vec<Vec<DVec2>>,
}

impl Planar {
    pub fn new(contours: Vec<Vec<DVec2>>) -> Self {
        Self { contours }
    }

    pub fn is_empty(&self) -> bool {
        self.contours.is_empty()
    }

    /// Region enclosed by arbitrary contours under `fill` (snapped).
    pub fn from_contours(contours: &[Vec<DVec2>], fill: FillRule) -> Self {
        let paths: Vec<Path64> = contours.iter().map(|c| to_path(c)).collect();
        from_paths(&clipper::boolean(&paths, &[], ClipOp::Union, fill))
    }

    pub fn to_paths(&self) -> Vec<Path64> {
        self.contours.iter().map(|c| to_path(c)).collect()
    }

    /// Applies the XY part of an affine 3D matrix.
    pub fn transform(&mut self, m: &DMat4) {
        let c = m.to_cols_array_2d();
        for contour in &mut self.contours {
            for p in contour.iter_mut() {
                *p = DVec2::new(
                    c[0][0] * p.x + c[1][0] * p.y + c[3][0],
                    c[0][1] * p.x + c[1][1] * p.y + c[3][1],
                );
            }
        }
        let det = c[0][0] * c[1][1] - c[1][0] * c[0][1];
        if det < 0.0 {
            for contour in &mut self.contours {
                contour.reverse();
            }
        }
        if det == 0.0 || !det.is_finite() {
            // Collapsed to a line or a point: no area left.
            self.contours.clear();
        }
    }

    pub fn bounds(&self) -> (DVec2, DVec2) {
        let mut min = DVec2::splat(f64::INFINITY);
        let mut max = DVec2::splat(f64::NEG_INFINITY);
        for p in self.contours.iter().flatten() {
            min = min.min(*p);
            max = max.max(*p);
        }
        (min, max)
    }

    pub fn area(&self) -> f64 {
        self.contours.iter().map(|c| signed_area(c)).sum()
    }

    /// Triangles (counter-clockwise) over the concatenated contour points.
    pub fn triangulate(&self) -> Vec<[usize; 3]> {
        if self.contours.is_empty() {
            return Vec::new();
        }
        manifold::triangulate(&self.contours, -1.0)
    }

    /// Display mesh at z = 0, facing +Z.
    pub fn fill_mesh(&self) -> Mesh {
        let vertices: Vec<DVec3> = self
            .contours
            .iter()
            .flatten()
            .map(|p| p.extend(0.0))
            .collect();
        let triangles = self
            .triangulate()
            .into_iter()
            .map(|t| t.map(|i| i as u32))
            .collect();
        Mesh {
            vertices,
            triangles,
        }
    }

    /// Outer outlines only (OpenSCAD `fill()`).
    pub fn without_holes(&self) -> Self {
        let outer: Vec<Vec<DVec2>> = self
            .contours
            .iter()
            .filter(|c| signed_area(c) > 0.0)
            .cloned()
            .collect();
        Self::from_contours(&outer, FillRule::NonZero)
    }
}

pub fn signed_area(c: &[DVec2]) -> f64 {
    let n = c.len();
    (0..n).map(|i| c[i].perp_dot(c[(i + 1) % n])).sum::<f64>() / 2.0
}

pub fn snap(v: f64) -> i64 {
    (v * GRID).round() as i64
}

pub fn to_path(c: &[DVec2]) -> Path64 {
    c.iter().map(|p| [snap(p.x), snap(p.y)]).collect()
}

pub fn from_paths(paths: &[Path64]) -> Planar {
    Planar {
        contours: paths
            .iter()
            .filter(|p| p.len() >= 3)
            .map(|p| {
                p.iter()
                    .map(|q| DVec2::new(q[0] as f64 / GRID, q[1] as f64 / GRID))
                    .collect()
            })
            .collect(),
    }
}

/// Union of regions.
pub fn union(items: &[Planar]) -> Planar {
    let paths: Vec<Path64> = items.iter().flat_map(Planar::to_paths).collect();
    from_paths(&clipper::boolean(
        &paths,
        &[],
        ClipOp::Union,
        FillRule::NonZero,
    ))
}

/// First region minus all the others.
pub fn difference(first: &Planar, rest: &[Planar]) -> Planar {
    let clip: Vec<Path64> = rest.iter().flat_map(Planar::to_paths).collect();
    from_paths(&clipper::boolean(
        &first.to_paths(),
        &clip,
        ClipOp::Difference,
        FillRule::NonZero,
    ))
}

/// Intersection of all regions.
pub fn intersection(items: &[Planar]) -> Planar {
    let Some((first, rest)) = items.split_first() else {
        return Planar::default();
    };
    let mut acc = first.to_paths();
    for item in rest {
        acc = clipper::boolean(
            &acc,
            &item.to_paths(),
            ClipOp::Intersection,
            FillRule::NonZero,
        );
        if acc.is_empty() {
            break;
        }
    }
    from_paths(&acc)
}

/// Offsets a region (in grid units internally).
pub fn offset(region: &Planar, delta: f64, join: Join, arc_tolerance: f64) -> Planar {
    from_paths(&clipper::offset(
        &region.to_paths(),
        delta * GRID,
        join,
        // Large enough that `offset(delta)` keeps sharp corners.
        1e9,
        arc_tolerance * GRID,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<DVec2> {
        vec![
            DVec2::new(x0, y0),
            DVec2::new(x1, y0),
            DVec2::new(x1, y1),
            DVec2::new(x0, y1),
        ]
    }

    #[test]
    fn snapping_matches_openscad() {
        let p = Planar::from_contours(
            &[vec![
                DVec2::ZERO,
                DVec2::new(1.1, 0.0),
                DVec2::new(0.3, 0.7),
            ]],
            FillRule::EvenOdd,
        );
        let pts: Vec<DVec2> = p.contours.concat();
        assert!(pts.contains(&DVec2::new(1.1000000014901161, 0.0)));
        assert!(pts.contains(&DVec2::new(0.29999999701976776, 0.7000000029802322)));
    }

    #[test]
    fn booleans() {
        let a = Planar::new(vec![square(0.0, 0.0, 2.0, 2.0)]);
        let b = Planar::new(vec![square(1.0, 1.0, 3.0, 3.0)]);
        assert_eq!(union(&[a.clone(), b.clone()]).area(), 7.0);
        assert_eq!(difference(&a, std::slice::from_ref(&b)).area(), 3.0);
        assert_eq!(intersection(&[a.clone(), b]).area(), 1.0);
        let grown = offset(&a, 1.0, Join::Miter, 0.0);
        assert_eq!(grown.area(), 16.0);
        let frame = Planar::from_contours(
            &[square(0.0, 0.0, 4.0, 4.0), square(1.0, 1.0, 3.0, 3.0)],
            FillRule::EvenOdd,
        );
        assert_eq!(frame.area(), 12.0);
        assert_eq!(frame.without_holes().area(), 16.0);
        assert_eq!(frame.triangulate().len(), 8);
    }
}
