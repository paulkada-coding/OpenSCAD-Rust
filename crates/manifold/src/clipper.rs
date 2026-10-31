//! Direct access to Clipper2 on integer coordinates.
//!
//! [`crate::CrossSection`] always works on Clipper2's decimal grid (1e-8).
//! These functions take `i64` points instead, so the caller decides the
//! grid (e.g. a power of two, to reproduce another program's snapping
//! exactly). Coordinates must stay within about ±4.6e18 (Clipper2's range);
//! beyond that Clipper2 throws, which panics.

use std::ffi::c_int;
use std::ptr::NonNull;

use crate::cross_section::FillRule;
use crate::{cxx_panic, ffi};

/// A closed contour of integer points.
pub type Path64 = Vec<[i64; 2]>;

/// Boolean operation kind (Clipper2 `ClipType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClipOp {
    Intersection,
    Union,
    Difference,
    Xor,
}

impl ClipOp {
    fn code(self) -> c_int {
        match self {
            Self::Intersection => 1,
            Self::Union => 2,
            Self::Difference => 3,
            Self::Xor => 4,
        }
    }
}

/// Corner treatment for [`offset`] (Clipper2 `JoinType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Join {
    Square,
    Bevel,
    Round,
    Miter,
}

impl Join {
    fn code(self) -> c_int {
        match self {
            Self::Square => 0,
            Self::Bevel => 1,
            Self::Round => 2,
            Self::Miter => 3,
        }
    }
}

fn fill_code(fill: FillRule) -> c_int {
    match fill {
        FillRule::EvenOdd => 0,
        FillRule::NonZero => 1,
        FillRule::Positive => 2,
        FillRule::Negative => 3,
    }
}

/// Flattens contours for the shim: x, y pairs and per-contour lengths.
fn flatten(paths: &[Path64]) -> (Vec<i64>, Vec<usize>) {
    let lens = paths.iter().map(Vec::len).collect();
    let pts = paths.iter().flatten().flat_map(|p| *p).collect();
    (pts, lens)
}

/// Copies a shim result out and frees it; null means a C++ exception.
#[track_caller]
fn take(raw: *mut ffi::MshimPaths64, operation: &str) -> Vec<Path64> {
    let Some(raw) = NonNull::new(raw) else {
        cxx_panic(operation);
    };
    let guard = PathsGuard(raw);
    let p = guard.0.as_ptr();
    // SAFETY: `p` is a live paths object owned by the guard; indices are
    // in range.
    let lens: Vec<usize> = unsafe {
        (0..ffi::mshim_paths64_len(p))
            .map(|i| ffi::mshim_paths64_contour_len(p, i))
            .collect()
    };
    let mut flat = vec![0i64; 2 * lens.iter().sum::<usize>()];
    // SAFETY: `flat` has room for every coordinate of every contour.
    unsafe { ffi::mshim_paths64_copy_out(p, flat.as_mut_ptr()) };
    let mut points = flat.as_chunks::<2>().0.iter().copied();
    lens.iter()
        .map(|&len| points.by_ref().take(len).collect())
        .collect()
}

/// Boolean of `subject` and `clip` regions, both read with `fill`.
/// Results: outlines counter-clockwise, holes clockwise.
pub fn boolean(subject: &[Path64], clip: &[Path64], op: ClipOp, fill: FillRule) -> Vec<Path64> {
    let (sp, sl) = flatten(subject);
    let (cp, cl) = flatten(clip);
    // SAFETY: each point buffer holds 2 * sum(lens) coordinates matching
    // its length list; the shim copies everything.
    let raw = unsafe {
        ffi::mshim_clip_boolean(
            sp.as_ptr(),
            sl.as_ptr(),
            sl.len(),
            cp.as_ptr(),
            cl.as_ptr(),
            cl.len(),
            op.code(),
            fill_code(fill),
        )
    };
    take(raw, "clipper boolean")
}

/// Grows (`delta > 0`) or shrinks closed polygons. `arc_tolerance` is the
/// maximum distance between a rounded join and its true arc (0: Clipper2's
/// default, `|delta| / 500`).
pub fn offset(
    paths: &[Path64],
    delta: f64,
    join: Join,
    miter_limit: f64,
    arc_tolerance: f64,
) -> Vec<Path64> {
    let (pts, lens) = flatten(paths);
    // SAFETY: `pts` holds 2 * sum(lens) coordinates; the shim copies them.
    let raw = unsafe {
        ffi::mshim_clip_offset(
            pts.as_ptr(),
            lens.as_ptr(),
            lens.len(),
            delta,
            join.code(),
            miter_limit,
            arc_tolerance,
        )
    };
    take(raw, "clipper offset")
}

/// Minkowski sum of `pattern` swept along the boundary of the closed
/// polygon `path` (unioned, NonZero fill). The interior of `path` is not
/// included: union it (or `pattern` placed at any of its points) to get
/// the sum of the two regions.
pub fn minkowski_sum(pattern: &[[i64; 2]], path: &[[i64; 2]]) -> Vec<Path64> {
    let a: Vec<i64> = pattern.iter().flatten().copied().collect();
    let b: Vec<i64> = path.iter().flatten().copied().collect();
    // SAFETY: the buffers hold 2 * len coordinates; the shim copies them.
    let raw =
        unsafe { ffi::mshim_clip_minkowski(a.as_ptr(), pattern.len(), b.as_ptr(), path.len()) };
    take(raw, "clipper minkowski")
}

/// Frees a paths object returned by the shim.
struct PathsGuard(NonNull<ffi::MshimPaths64>);

impl Drop for PathsGuard {
    fn drop(&mut self) {
        // SAFETY: the guard owns the object and it is never used again.
        unsafe { ffi::mshim_delete_paths64(self.0.as_ptr()) }
    }
}
