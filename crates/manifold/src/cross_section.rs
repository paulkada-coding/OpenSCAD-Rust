//! 2D regions: [`CrossSection`] (Clipper2 based).

use std::ffi::c_int;
use std::fmt;
use std::ptr::NonNull;

use geometry::glam::{DAffine2, DVec2};

use crate::solid::OpType;
use crate::{c_segments, cxx_panic, ffi};

/// How overlapping or nested contours decide what is inside
/// (Clipper2 fill rules).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FillRule {
    /// Inside where an odd number of contours overlap. OpenSCAD uses this
    /// for `polygon()` with several paths.
    EvenOdd,
    /// Inside where the winding number is non-zero.
    NonZero,
    /// Inside where the winding number is positive (counter-clockwise
    /// outlines, clockwise holes). Manifold's default.
    #[default]
    Positive,
    /// Inside where the winding number is negative.
    Negative,
}

impl FillRule {
    fn code(self) -> c_int {
        match self {
            Self::EvenOdd => ffi::FILL_EVEN_ODD,
            Self::NonZero => ffi::FILL_NON_ZERO,
            Self::Positive => ffi::FILL_POSITIVE,
            Self::Negative => ffi::FILL_NEGATIVE,
        }
    }
}

/// Corner treatment for [`CrossSection::offset`] (Clipper2 join types).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JoinType {
    /// Corners squared off at distance `delta` from the original vertex.
    Square,
    /// Arcs of radius `delta` (OpenSCAD `offset(r)`).
    Round,
    /// Sharp corners, cut back when longer than `miter_limit * delta`
    /// (OpenSCAD `offset(delta)`).
    Miter,
    /// Corners cut straight between the offset edges.
    Bevel,
}

impl JoinType {
    fn code(self) -> c_int {
        match self {
            Self::Square => ffi::JOIN_SQUARE,
            Self::Round => ffi::JOIN_ROUND,
            Self::Miter => ffi::JOIN_MITER,
            Self::Bevel => ffi::JOIN_BEVEL,
        }
    }
}

/// Axis-aligned 2D rectangle. An empty rectangle has `min > max`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub min: DVec2,
    pub max: DVec2,
}

impl Rect {
    pub const EMPTY: Self = Self {
        min: DVec2::splat(f64::INFINITY),
        max: DVec2::splat(f64::NEG_INFINITY),
    };

    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y
    }

    pub fn size(&self) -> DVec2 {
        if self.is_empty() {
            DVec2::ZERO
        } else {
            self.max - self.min
        }
    }
}

/// A 2D region made of non-overlapping polygons with holes
/// (`manifold::CrossSection`). Cheap to clone.
///
/// Coordinates are snapped to a 1e-8 grid by Clipper2, and must stay
/// within about ±4.6e10 (beyond that Clipper2 throws, which panics).
pub struct CrossSection {
    ptr: NonNull<ffi::ManifoldCrossSection>,
}

// SAFETY: a `manifold::CrossSection` owns its data through `shared_ptr`s
// and has no thread affinity.
unsafe impl Send for CrossSection {}
// SAFETY: every method takes `&self` and maps to a `const` C++ method; the
// lazily applied transform is guarded by `pathsMutex_` inside Manifold.
unsafe impl Sync for CrossSection {}

impl CrossSection {
    /// Takes ownership of an object returned by the shim; a null pointer
    /// means a C++ exception was caught and becomes a panic.
    #[track_caller]
    pub(crate) fn from_raw(ptr: *mut ffi::ManifoldCrossSection, operation: &str) -> Self {
        match NonNull::new(ptr) {
            Some(ptr) => Self { ptr },
            None => cxx_panic(operation),
        }
    }

    pub(crate) fn raw(&self) -> *mut ffi::ManifoldCrossSection {
        self.ptr.as_ptr()
    }

    /// Runs a shim query with an out parameter.
    #[track_caller]
    fn query<T: Default>(
        &self,
        operation: &str,
        f: unsafe extern "C" fn(*mut ffi::ManifoldCrossSection, *mut T) -> c_int,
    ) -> T {
        let mut out = T::default();
        // SAFETY: `self.raw()` is a live object and `out` a valid, writable
        // `T`; the shim only reads the object and writes `out`.
        let ok = unsafe { f(self.raw(), &mut out) };
        if ok == 0 {
            cxx_panic(operation);
        }
        out
    }

    // ----- Construction -----

    pub fn empty() -> Self {
        // SAFETY: no arguments; returns a new object or null.
        Self::from_raw(unsafe { ffi::mshim_cross_empty() }, "empty")
    }

    /// Region enclosed by `contours` (closed, any orientation,
    /// self-intersections allowed) according to `fill_rule`.
    pub fn from_polygons<P: AsRef<[DVec2]>>(contours: &[P], fill_rule: FillRule) -> Self {
        let lens: Vec<usize> = contours.iter().map(|c| c.as_ref().len()).collect();
        let pts: Vec<ffi::Vec2> = contours
            .iter()
            .flat_map(|c| c.as_ref().iter())
            .map(|p| ffi::Vec2 { x: p.x, y: p.y })
            .collect();
        // SAFETY: `pts` holds `sum(lens)` points and `lens` `lens.len()`
        // counts; the shim copies both.
        let ptr = unsafe {
            ffi::mshim_cross_of_polygons(pts.as_ptr(), lens.as_ptr(), lens.len(), fill_rule.code())
        };
        Self::from_raw(ptr, "from_polygons")
    }

    /// Rectangle from the origin to `size` (or centered on the origin).
    pub fn square(size: DVec2, center: bool) -> Self {
        // SAFETY: plain values; returns a new object or null.
        let ptr = unsafe { ffi::mshim_cross_square(size.x, size.y, c_int::from(center)) };
        Self::from_raw(ptr, "square")
    }

    /// Regular polygon inscribed in a circle (Manifold's tessellation).
    /// `segments` below 3 uses Manifold's default quality.
    pub fn circle(radius: f64, segments: u32) -> Self {
        // SAFETY: plain values; returns a new object or null.
        let ptr = unsafe { ffi::mshim_cross_circle(radius, c_segments(segments)) };
        Self::from_raw(ptr, "circle")
    }

    // ----- Output and queries -----

    /// Contours of the region: outlines counter-clockwise, holes clockwise.
    pub fn to_polygons(&self) -> Vec<Vec<DVec2>> {
        // SAFETY: `self.raw()` is live; returns a new polygons object or null.
        let raw = unsafe { ffi::mshim_cross_to_polygons(self.raw()) };
        let Some(raw) = NonNull::new(raw) else {
            cxx_panic("to_polygons");
        };
        let polys = PolygonsGuard(raw);
        let p = polys.0.as_ptr();
        // SAFETY: `p` is a live polygons object; indices are in range.
        let lens: Vec<usize> = unsafe {
            (0..ffi::mshim_polygons_len(p))
                .map(|i| ffi::mshim_polygons_contour_len(p, i))
                .collect()
        };
        let mut pts = vec![ffi::Vec2::default(); lens.iter().sum()];
        // SAFETY: `pts` has room for every point of every contour.
        unsafe { ffi::mshim_polygons_copy_out(p, pts.as_mut_ptr()) };
        let mut start = 0;
        lens.iter()
            .map(|&len| {
                let contour = pts[start..start + len]
                    .iter()
                    .map(|v| DVec2::new(v.x, v.y))
                    .collect();
                start += len;
                contour
            })
            .collect()
    }

    pub fn area(&self) -> f64 {
        self.query("area", ffi::mshim_cross_area)
    }

    pub fn num_vert(&self) -> usize {
        self.query("num_vert", ffi::mshim_cross_num_vert)
    }

    pub fn num_contour(&self) -> usize {
        self.query("num_contour", ffi::mshim_cross_num_contour)
    }

    pub fn is_empty(&self) -> bool {
        self.query("is_empty", ffi::mshim_cross_is_empty) != 0
    }

    /// Bounding rectangle; [`Rect::EMPTY`] for the empty region.
    pub fn bounds(&self) -> Rect {
        if self.is_empty() {
            return Rect::EMPTY;
        }
        let mut b = [0.0f64; 4];
        // SAFETY: `self.raw()` is live and `b` has room for 4 doubles.
        let ok = unsafe { ffi::mshim_cross_bounds(self.raw(), b.as_mut_ptr()) };
        if ok == 0 {
            cxx_panic("bounds");
        }
        Rect {
            min: DVec2::new(b[0], b[1]),
            max: DVec2::new(b[2], b[3]),
        }
    }

    // ----- Booleans -----

    pub fn boolean(&self, other: &Self, op: OpType) -> Self {
        // SAFETY: both objects are live; returns a new object or null.
        let ptr = unsafe { ffi::mshim_cross_boolean(self.raw(), other.raw(), op.code()) };
        Self::from_raw(ptr, "boolean")
    }

    pub fn union(&self, other: &Self) -> Self {
        self.boolean(other, OpType::Add)
    }

    pub fn difference(&self, other: &Self) -> Self {
        self.boolean(other, OpType::Subtract)
    }

    pub fn intersection(&self, other: &Self) -> Self {
        self.boolean(other, OpType::Intersect)
    }

    /// Applies `op` to all regions at once (`Subtract`: the first minus all
    /// the others). An empty slice gives the empty region.
    pub fn batch_boolean(sections: &[Self], op: OpType) -> Self {
        let raws = raw_list(sections);
        // SAFETY: `raws` points to `raws.len()` live objects; the shim
        // copies them.
        let ptr = unsafe { ffi::mshim_cross_batch_boolean(raws.as_ptr(), raws.len(), op.code()) };
        Self::from_raw(ptr, "batch_boolean")
    }

    pub fn batch_union(sections: &[Self]) -> Self {
        Self::batch_boolean(sections, OpType::Add)
    }

    // ----- Offset, hull, simplify -----

    /// Grows (`delta > 0`) or shrinks (`delta < 0`) the region.
    /// `miter_limit` only matters for [`JoinType::Miter`] (Manifold's
    /// default is 2); `circular_segments` only for [`JoinType::Round`]
    /// (segments of a full circle of radius `delta`; below 3 uses
    /// Manifold's default quality).
    pub fn offset(
        &self,
        delta: f64,
        join_type: JoinType,
        miter_limit: f64,
        circular_segments: u32,
    ) -> Self {
        // SAFETY: `self.raw()` is live; returns a new object or null.
        let ptr = unsafe {
            ffi::mshim_cross_offset(
                self.raw(),
                delta,
                join_type.code(),
                miter_limit,
                c_segments(circular_segments),
            )
        };
        Self::from_raw(ptr, "offset")
    }

    /// Convex hull of this region.
    pub fn hull(&self) -> Self {
        // SAFETY: `self.raw()` is live; returns a new object or null.
        Self::from_raw(unsafe { ffi::mshim_cross_hull(self.raw()) }, "hull")
    }

    /// Convex hull of several regions together.
    pub fn hull_of(sections: &[Self]) -> Self {
        let raws = raw_list(sections);
        // SAFETY: `raws` points to `raws.len()` live objects.
        let ptr = unsafe { ffi::mshim_cross_batch_hull(raws.as_ptr(), raws.len()) };
        Self::from_raw(ptr, "hull_of")
    }

    /// Convex hull of a point set.
    pub fn hull_of_points(points: &[DVec2]) -> Self {
        let pts: Vec<ffi::Vec2> = points
            .iter()
            .map(|p| ffi::Vec2 { x: p.x, y: p.y })
            .collect();
        // SAFETY: `pts` holds `pts.len()` points; the shim copies them.
        let ptr = unsafe { ffi::mshim_cross_hull_pts(pts.as_ptr(), pts.len()) };
        Self::from_raw(ptr, "hull_of_points")
    }

    /// Removes vertices closer than `epsilon` to the line through their
    /// neighbours.
    pub fn simplify(&self, epsilon: f64) -> Self {
        // SAFETY: `self.raw()` is live; returns a new object or null.
        let ptr = unsafe { ffi::mshim_cross_simplify(self.raw(), epsilon) };
        Self::from_raw(ptr, "simplify")
    }

    // ----- Transforms -----

    /// Applies an affine transform (mirroring transforms keep the region
    /// valid).
    pub fn transform(&self, m: &DAffine2) -> Self {
        let c = [
            m.matrix2.x_axis.x,
            m.matrix2.x_axis.y,
            m.matrix2.y_axis.x,
            m.matrix2.y_axis.y,
            m.translation.x,
            m.translation.y,
        ];
        // SAFETY: `self.raw()` is live and `c` holds the 6 doubles read.
        let ptr = unsafe { ffi::mshim_cross_transform(self.raw(), c.as_ptr()) };
        Self::from_raw(ptr, "transform")
    }

    pub fn translate(&self, v: DVec2) -> Self {
        // SAFETY: `self.raw()` is live; returns a new object or null.
        let ptr = unsafe { ffi::mshim_cross_translate(self.raw(), v.x, v.y) };
        Self::from_raw(ptr, "translate")
    }

    pub fn scale(&self, v: DVec2) -> Self {
        // SAFETY: `self.raw()` is live; returns a new object or null.
        let ptr = unsafe { ffi::mshim_cross_scale(self.raw(), v.x, v.y) };
        Self::from_raw(ptr, "scale")
    }

    /// Counter-clockwise rotation in degrees (multiples of 90° are exact).
    pub fn rotate(&self, degrees: f64) -> Self {
        // SAFETY: `self.raw()` is live; returns a new object or null.
        let ptr = unsafe { ffi::mshim_cross_rotate(self.raw(), degrees) };
        Self::from_raw(ptr, "rotate")
    }

    /// Mirrors across the line through the origin perpendicular to `axis`
    /// (any length). A zero axis gives the empty region.
    pub fn mirror(&self, axis: DVec2) -> Self {
        // SAFETY: `self.raw()` is live; returns a new object or null.
        let ptr = unsafe { ffi::mshim_cross_mirror(self.raw(), axis.x, axis.y) };
        Self::from_raw(ptr, "mirror")
    }
}

fn raw_list(sections: &[CrossSection]) -> Vec<*mut ffi::ManifoldCrossSection> {
    sections.iter().map(CrossSection::raw).collect()
}

impl Clone for CrossSection {
    fn clone(&self) -> Self {
        // SAFETY: `self.raw()` is live; returns a new object or null.
        Self::from_raw(unsafe { ffi::mshim_cross_copy(self.raw()) }, "clone")
    }
}

impl Drop for CrossSection {
    fn drop(&mut self) {
        // SAFETY: we own the object and it is never used again.
        unsafe { ffi::mshim_delete_cross(self.raw()) }
    }
}

impl Default for CrossSection {
    fn default() -> Self {
        Self::empty()
    }
}

impl fmt::Debug for CrossSection {
    // Does not force lazy evaluation.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CrossSection").finish_non_exhaustive()
    }
}

/// Frees a polygons object returned by the shim.
struct PolygonsGuard(NonNull<ffi::ManifoldPolygons>);

impl Drop for PolygonsGuard {
    fn drop(&mut self) {
        // SAFETY: the guard owns the object and it is never used again.
        unsafe { ffi::mshim_delete_polygons(self.0.as_ptr()) }
    }
}
