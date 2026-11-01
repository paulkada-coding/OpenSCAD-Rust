//! Safe wrapper around the [Manifold](https://github.com/elalish/manifold)
//! CSG kernel (v3.5.4, Apache-2.0): 3D booleans on [`Manifold`] solids and
//! 2D booleans/offsets on [`CrossSection`] (Clipper2 based).
//!
//! The C++ library is vendored and statically linked (see `build.rs`). All
//! values are `f64`; meshes convert to and from [`geometry::Mesh`].
//!
//! Error model:
//! - Invalid input meshes are reported by [`Manifold::from_mesh`] as an
//!   [`Error`]. Booleans and transforms are lazy (like Manifold itself):
//!   they never fail, an invalid operand propagates its status, and
//!   [`Manifold::status`] reports it.
//! - A C++ exception inside the kernel (out of memory, or Clipper2's range
//!   error for 2D coordinates beyond about ±4.6e10) never unwinds through
//!   FFI: the shim catches it and the wrapper turns it into a Rust panic
//!   carrying the C++ message, which callers may `catch_unwind`.
//!
//! Thread safety: both [`Manifold`] and [`CrossSection`] are `Send + Sync`.
//! Their C++ counterparts are immutable values sharing internal nodes; the
//! lazily evaluated state is guarded by mutexes inside Manifold (since
//! v3.x), so concurrent `&self` calls on the same object are safe. Manifold
//! is built single-threaded (`MANIFOLD_PAR=OFF`): parallelism is up to the
//! caller (e.g. evaluating independent CSG subtrees on separate threads).
//!
//! [`clipper`] exposes Clipper2 on integer coordinates, for callers that
//! need their own snapping grid; [`triangulate`] is Manifold's polygon
//! triangulator.

pub mod clipper;
mod cross_section;
mod ffi;
mod solid;

use std::ffi::CStr;
use std::ptr::NonNull;

pub use cross_section::{CrossSection, FillRule, JoinType, Rect};
pub use geometry::{Aabb, Mesh, glam};
pub use solid::{Manifold, MeshRun, OpType};

use glam::DVec2;

/// Manifold's own status codes (`manifold::Manifold::Error`), minus
/// `NoError`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("la mesh contiene coordinate non finite (NaN o infinito)")]
    NonFiniteVertex,
    #[error("la mesh non è un solido chiuso e ben formato (non manifold)")]
    NotManifold,
    #[error("un triangolo fa riferimento a un vertice inesistente")]
    VertexIndexOutOfBounds,
    #[error("proprietà dei vertici di lunghezza errata")]
    PropertiesWrongLength,
    #[error("mancano le coordinate dei vertici")]
    MissingPositionProperties,
    #[error("vettori di fusione dei vertici di lunghezza diversa")]
    MergeVectorsDifferentLengths,
    #[error("indice di fusione dei vertici fuori intervallo")]
    MergeIndexOutOfBounds,
    #[error("matrice di trasformazione di lunghezza errata")]
    TransformWrongLength,
    #[error("indice dei gruppi di triangoli di lunghezza errata")]
    RunIndexWrongLength,
    #[error("identificativi delle facce di lunghezza errata")]
    FaceIdWrongLength,
    #[error("costruzione geometrica non valida")]
    InvalidConstruction,
    #[error("risultato troppo grande")]
    ResultTooLarge,
    #[error("tangenti non valide")]
    InvalidTangents,
    #[error("operazione annullata")]
    Cancelled,
    #[error("errore sconosciuto di Manifold (codice {0})")]
    Unknown(i32),
}

impl Error {
    /// Maps a `ManifoldError` code; `None` for `MANIFOLD_NO_ERROR`.
    fn from_code(code: i32) -> Option<Self> {
        Some(match code {
            0 => return None,
            1 => Self::NonFiniteVertex,
            2 => Self::NotManifold,
            3 => Self::VertexIndexOutOfBounds,
            4 => Self::PropertiesWrongLength,
            5 => Self::MissingPositionProperties,
            6 => Self::MergeVectorsDifferentLengths,
            7 => Self::MergeIndexOutOfBounds,
            8 => Self::TransformWrongLength,
            9 => Self::RunIndexWrongLength,
            10 => Self::FaceIdWrongLength,
            11 => Self::InvalidConstruction,
            12 => Self::ResultTooLarge,
            13 => Self::InvalidTangents,
            14 => Self::Cancelled,
            other => Self::Unknown(other),
        })
    }
}

/// Extrudes `cross_section` along +Z from `z = 0` to `z = height`.
///
/// - `n_divisions`: extra copies of the section inserted along Z (Manifold's
///   `nDivisions`); OpenSCAD's `slices = n_divisions + 1`.
/// - `twist_degrees`: rotation of the top relative to the bottom, applied
///   counter-clockwise like Manifold (OpenSCAD's `twist` is clockwise, so
///   pass `-twist`). Twisted side quads are split along one diagonal, so
///   the volume converges to the exact one only as O(1/n_divisions).
/// - `scale_top`: XY scale of the top section; zero in both axes makes a
///   cone; negative components are clamped to zero.
///
/// An empty section or a non-positive height gives an empty solid (Manifold
/// itself would return an invalid one).
pub fn extrude(
    cross_section: &CrossSection,
    height: f64,
    n_divisions: u32,
    twist_degrees: f64,
    scale_top: DVec2,
) -> Manifold {
    // Also rejects a NaN height.
    let positive = height.partial_cmp(&0.0) == Some(std::cmp::Ordering::Greater);
    if !positive || cross_section.is_empty() {
        return Manifold::empty();
    }
    solid::extrude(cross_section, height, n_divisions, twist_degrees, scale_top)
}

/// Revolves `cross_section` around the Y axis, which becomes the Z axis of
/// the result (like OpenSCAD's `rotate_extrude`). Parts with `x < 0` are
/// discarded.
///
/// - `circular_segments`: number of segments of the sweep, whatever its
///   angle (Manifold does not scale it for partial turns, unlike OpenSCAD's
///   `$fn`); values below 3 use Manifold's default quality.
/// - `revolve_degrees`: sweep angle starting from +X, counter-clockwise;
///   clamped to 360. A negative angle is not supported by Manifold: revolve
///   by `-angle` and mirror the result across the XZ plane instead.
///
/// An empty section (or one entirely at `x <= 0`) gives an empty solid.
pub fn revolve(
    cross_section: &CrossSection,
    circular_segments: u32,
    revolve_degrees: f64,
) -> Manifold {
    if cross_section.is_empty() || cross_section.bounds().max.x <= 0.0 {
        return Manifold::empty();
    }
    solid::revolve(cross_section, circular_segments, revolve_degrees)
}

/// Triangulates a polygon with holes (Manifold's robust ear clipping):
/// outlines counter-clockwise, holes clockwise. Returned indices refer to
/// the points of all `contours` concatenated in order; triangles are
/// counter-clockwise. `epsilon` is the tolerance for slightly invalid
/// input (negative: derived from the bounds).
pub fn triangulate<P: AsRef<[DVec2]>>(contours: &[P], epsilon: f64) -> Vec<[usize; 3]> {
    let lens: Vec<usize> = contours.iter().map(|c| c.as_ref().len()).collect();
    let pts: Vec<ffi::Vec2> = contours
        .iter()
        .flat_map(|c| c.as_ref().iter())
        .map(|p| ffi::Vec2 { x: p.x, y: p.y })
        .collect();
    // SAFETY: `pts` holds `sum(lens)` points and `lens` `lens.len()`
    // counts; the shim copies both.
    let raw = unsafe { ffi::mshim_triangulate(pts.as_ptr(), lens.as_ptr(), lens.len(), epsilon) };
    let Some(raw) = NonNull::new(raw) else {
        cxx_panic("triangulate");
    };
    let guard = TriangulationGuard(raw);
    // SAFETY: `guard.0` is a live triangulation owned by the guard.
    let num_tri = unsafe { ffi::mshim_triangulation_num_tri(guard.0.as_ptr()) };
    let mut out = vec![0 as std::ffi::c_int; num_tri * 3];
    // SAFETY: `out` has room for the 3 * num_tri indices reported.
    let ok = unsafe { ffi::mshim_triangulation_copy_out(guard.0.as_ptr(), out.as_mut_ptr()) };
    if ok == 0 {
        cxx_panic("triangulate");
    }
    out.as_chunks::<3>()
        .0
        .iter()
        .map(|t| t.map(|i| usize::try_from(i).unwrap_or(0)))
        .collect()
}

/// Frees a triangulation returned by the shim.
struct TriangulationGuard(NonNull<ffi::ManifoldTriangulation>);

impl Drop for TriangulationGuard {
    fn drop(&mut self) {
        // SAFETY: the guard owns the object and it is never used again.
        unsafe { ffi::mshim_delete_triangulation(self.0.as_ptr()) }
    }
}

/// Message of the last C++ exception caught by the shim on this thread.
fn last_error() -> String {
    // SAFETY: the shim returns a pointer to a NUL-terminated thread-local
    // string that stays valid until the next shim call on this thread; it
    // is copied out immediately.
    unsafe { CStr::from_ptr(ffi::mshim_last_error()) }
        .to_string_lossy()
        .into_owned()
}

/// Turns a C++ exception reported by the shim into a Rust panic.
#[track_caller]
fn cxx_panic(operation: &str) -> ! {
    panic!("Manifold: C++ exception in {operation}: {}", last_error())
}

/// Converts a segment count to the C `int` Manifold expects.
fn c_segments(n: u32) -> std::ffi::c_int {
    std::ffi::c_int::try_from(n).unwrap_or(std::ffi::c_int::MAX)
}
