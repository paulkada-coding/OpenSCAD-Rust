//! Raw declarations of the exception-safe shim (`shim/shim.cpp`).
//!
//! Only `mshim_*` functions are declared: they are all `noexcept` on the C++
//! side, so no unwinding can cross into Rust. Object-returning functions
//! return null if a C++ exception was caught (message in
//! [`mshim_last_error`]); query functions return 0 in that case.

use std::ffi::{c_char, c_int};

/// Opaque `ManifoldManifold` (a `manifold::Manifold`).
#[repr(C)]
pub struct ManifoldManifold {
    _private: [u8; 0],
}

/// Opaque `ManifoldManifoldVec` (a `std::vector<manifold::Manifold>`).
#[repr(C)]
pub struct ManifoldManifoldVec {
    _private: [u8; 0],
}

/// Opaque `ManifoldCrossSection` (a `manifold::CrossSection`).
#[repr(C)]
pub struct ManifoldCrossSection {
    _private: [u8; 0],
}

/// Opaque `ManifoldMeshGL64` (a `manifold::MeshGL64`).
#[repr(C)]
pub struct ManifoldMeshGL64 {
    _private: [u8; 0],
}

/// Opaque `ManifoldPolygons` (a `manifold::Polygons`).
#[repr(C)]
pub struct ManifoldPolygons {
    _private: [u8; 0],
}

/// Opaque `ManifoldTriangulation` (a `std::vector<manifold::ivec3>`).
#[repr(C)]
pub struct ManifoldTriangulation {
    _private: [u8; 0],
}

/// Opaque `MshimPaths64` (a `Clipper2Lib::Paths64`).
#[repr(C)]
pub struct MshimPaths64 {
    _private: [u8; 0],
}

/// `ManifoldVec2` from `manifold/types.h`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

/// `ManifoldVec3` from `manifold/types.h`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

// `ManifoldOpType` values.
pub const OP_ADD: c_int = 0;
pub const OP_SUBTRACT: c_int = 1;
pub const OP_INTERSECT: c_int = 2;

// `ManifoldFillRule` values.
pub const FILL_EVEN_ODD: c_int = 0;
pub const FILL_NON_ZERO: c_int = 1;
pub const FILL_POSITIVE: c_int = 2;
pub const FILL_NEGATIVE: c_int = 3;

// `ManifoldJoinType` values.
pub const JOIN_SQUARE: c_int = 0;
pub const JOIN_ROUND: c_int = 1;
pub const JOIN_MITER: c_int = 2;
pub const JOIN_BEVEL: c_int = 3;

unsafe extern "C" {
    pub fn mshim_last_error() -> *const c_char;

    // Manifold: lifetime and construction
    pub fn mshim_delete_manifold(m: *mut ManifoldManifold);
    pub fn mshim_manifold_empty() -> *mut ManifoldManifold;
    pub fn mshim_manifold_copy(m: *mut ManifoldManifold) -> *mut ManifoldManifold;
    pub fn mshim_manifold_of_mesh(
        verts: *const f64,
        n_verts: usize,
        tris: *const u64,
        n_tris: usize,
        merge: c_int,
    ) -> *mut ManifoldManifold;
    pub fn mshim_manifold_cube(x: f64, y: f64, z: f64, center: c_int) -> *mut ManifoldManifold;
    pub fn mshim_manifold_sphere(radius: f64, segments: c_int) -> *mut ManifoldManifold;
    pub fn mshim_manifold_cylinder(
        height: f64,
        radius_low: f64,
        radius_high: f64,
        segments: c_int,
        center: c_int,
    ) -> *mut ManifoldManifold;

    // Manifold: booleans, hulls, minkowski
    pub fn mshim_manifold_boolean(
        a: *mut ManifoldManifold,
        b: *mut ManifoldManifold,
        op: c_int,
    ) -> *mut ManifoldManifold;
    pub fn mshim_manifold_batch_boolean(
        ms: *const *mut ManifoldManifold,
        n: usize,
        op: c_int,
    ) -> *mut ManifoldManifold;
    pub fn mshim_manifold_hull(m: *mut ManifoldManifold) -> *mut ManifoldManifold;
    pub fn mshim_manifold_batch_hull(
        ms: *const *mut ManifoldManifold,
        n: usize,
    ) -> *mut ManifoldManifold;
    pub fn mshim_manifold_hull_pts(pts: *const Vec3, n: usize) -> *mut ManifoldManifold;
    pub fn mshim_manifold_minkowski_sum(
        a: *mut ManifoldManifold,
        b: *mut ManifoldManifold,
    ) -> *mut ManifoldManifold;
    pub fn mshim_manifold_minkowski_difference(
        a: *mut ManifoldManifold,
        b: *mut ManifoldManifold,
    ) -> *mut ManifoldManifold;

    // Manifold: transforms
    pub fn mshim_manifold_transform(
        m: *mut ManifoldManifold,
        columns: *const f64,
    ) -> *mut ManifoldManifold;
    pub fn mshim_manifold_translate(
        m: *mut ManifoldManifold,
        x: f64,
        y: f64,
        z: f64,
    ) -> *mut ManifoldManifold;
    pub fn mshim_manifold_scale(
        m: *mut ManifoldManifold,
        x: f64,
        y: f64,
        z: f64,
    ) -> *mut ManifoldManifold;
    pub fn mshim_manifold_rotate(
        m: *mut ManifoldManifold,
        x: f64,
        y: f64,
        z: f64,
    ) -> *mut ManifoldManifold;
    pub fn mshim_manifold_mirror(
        m: *mut ManifoldManifold,
        nx: f64,
        ny: f64,
        nz: f64,
    ) -> *mut ManifoldManifold;

    // Manifold: splitting and decomposition
    pub fn mshim_manifold_split_by_plane(
        m: *mut ManifoldManifold,
        nx: f64,
        ny: f64,
        nz: f64,
        offset: f64,
        first: *mut *mut ManifoldManifold,
        second: *mut *mut ManifoldManifold,
    ) -> c_int;
    pub fn mshim_manifold_split(
        a: *mut ManifoldManifold,
        b: *mut ManifoldManifold,
        first: *mut *mut ManifoldManifold,
        second: *mut *mut ManifoldManifold,
    ) -> c_int;
    pub fn mshim_manifold_trim_by_plane(
        m: *mut ManifoldManifold,
        nx: f64,
        ny: f64,
        nz: f64,
        offset: f64,
    ) -> *mut ManifoldManifold;
    pub fn mshim_manifold_decompose(m: *mut ManifoldManifold) -> *mut ManifoldManifoldVec;
    pub fn mshim_manifold_vec_length(v: *mut ManifoldManifoldVec) -> usize;
    pub fn mshim_manifold_vec_get(v: *mut ManifoldManifoldVec, i: usize) -> *mut ManifoldManifold;
    pub fn mshim_delete_manifold_vec(v: *mut ManifoldManifoldVec);

    // Manifold: queries
    pub fn mshim_manifold_status(m: *mut ManifoldManifold, out: *mut c_int) -> c_int;
    pub fn mshim_manifold_is_empty(m: *mut ManifoldManifold, out: *mut c_int) -> c_int;
    pub fn mshim_manifold_num_vert(m: *mut ManifoldManifold, out: *mut usize) -> c_int;
    pub fn mshim_manifold_num_edge(m: *mut ManifoldManifold, out: *mut usize) -> c_int;
    pub fn mshim_manifold_num_tri(m: *mut ManifoldManifold, out: *mut usize) -> c_int;
    pub fn mshim_manifold_genus(m: *mut ManifoldManifold, out: *mut c_int) -> c_int;
    pub fn mshim_manifold_volume(m: *mut ManifoldManifold, out: *mut f64) -> c_int;
    pub fn mshim_manifold_surface_area(m: *mut ManifoldManifold, out: *mut f64) -> c_int;
    pub fn mshim_manifold_epsilon(m: *mut ManifoldManifold, out: *mut f64) -> c_int;
    pub fn mshim_manifold_bounds(m: *mut ManifoldManifold, out: *mut f64) -> c_int;

    // Manifold: mesh output
    pub fn mshim_manifold_get_mesh(m: *mut ManifoldManifold) -> *mut ManifoldMeshGL64;
    pub fn mshim_mesh_num_prop(mesh: *mut ManifoldMeshGL64) -> usize;
    pub fn mshim_mesh_vert_properties_len(mesh: *mut ManifoldMeshGL64) -> usize;
    pub fn mshim_mesh_tri_len(mesh: *mut ManifoldMeshGL64) -> usize;
    pub fn mshim_mesh_copy_out(
        mesh: *mut ManifoldMeshGL64,
        props: *mut f64,
        tris: *mut u64,
    ) -> c_int;
    pub fn mshim_delete_mesh(mesh: *mut ManifoldMeshGL64);
    pub fn mshim_mesh_num_run(mesh: *mut ManifoldMeshGL64) -> usize;
    pub fn mshim_mesh_copy_runs(
        mesh: *mut ManifoldMeshGL64,
        run_index: *mut u64,
        run_original_id: *mut u32,
    ) -> c_int;

    // Manifold: mesh relations
    pub fn mshim_manifold_as_original(m: *mut ManifoldManifold) -> *mut ManifoldManifold;
    pub fn mshim_manifold_original_id(m: *mut ManifoldManifold, out: *mut c_int) -> c_int;

    // Triangulation
    pub fn mshim_triangulate(
        pts: *const Vec2,
        lens: *const usize,
        n: usize,
        epsilon: f64,
    ) -> *mut ManifoldTriangulation;
    pub fn mshim_triangulation_num_tri(t: *mut ManifoldTriangulation) -> usize;
    pub fn mshim_triangulation_copy_out(t: *mut ManifoldTriangulation, out: *mut c_int) -> c_int;
    pub fn mshim_delete_triangulation(t: *mut ManifoldTriangulation);

    // Clipper2 on integer coordinates
    pub fn mshim_clip_boolean(
        subj_pts: *const i64,
        subj_lens: *const usize,
        n_subj: usize,
        clip_pts: *const i64,
        clip_lens: *const usize,
        n_clip: usize,
        op: c_int,
        fill: c_int,
    ) -> *mut MshimPaths64;
    pub fn mshim_clip_offset(
        pts: *const i64,
        lens: *const usize,
        n: usize,
        delta: f64,
        join: c_int,
        miter_limit: f64,
        arc_tolerance: f64,
    ) -> *mut MshimPaths64;
    pub fn mshim_clip_minkowski(
        pattern: *const i64,
        n_pattern: usize,
        path: *const i64,
        n_path: usize,
    ) -> *mut MshimPaths64;
    pub fn mshim_paths64_len(ps: *mut MshimPaths64) -> usize;
    pub fn mshim_paths64_contour_len(ps: *mut MshimPaths64, i: usize) -> usize;
    pub fn mshim_paths64_copy_out(ps: *mut MshimPaths64, out: *mut i64);
    pub fn mshim_delete_paths64(ps: *mut MshimPaths64);

    // Manifold <-> CrossSection
    pub fn mshim_manifold_slice(m: *mut ManifoldManifold, height: f64)
    -> *mut ManifoldCrossSection;
    pub fn mshim_manifold_project(m: *mut ManifoldManifold) -> *mut ManifoldCrossSection;
    pub fn mshim_manifold_extrude(
        cs: *mut ManifoldCrossSection,
        height: f64,
        divisions: c_int,
        twist_degrees: f64,
        scale_x: f64,
        scale_y: f64,
    ) -> *mut ManifoldManifold;
    pub fn mshim_manifold_revolve(
        cs: *mut ManifoldCrossSection,
        segments: c_int,
        revolve_degrees: f64,
    ) -> *mut ManifoldManifold;

    // CrossSection: lifetime and construction
    pub fn mshim_delete_cross(cs: *mut ManifoldCrossSection);
    pub fn mshim_cross_empty() -> *mut ManifoldCrossSection;
    pub fn mshim_cross_copy(cs: *mut ManifoldCrossSection) -> *mut ManifoldCrossSection;
    pub fn mshim_cross_of_polygons(
        pts: *const Vec2,
        lens: *const usize,
        n: usize,
        fill_rule: c_int,
    ) -> *mut ManifoldCrossSection;
    pub fn mshim_cross_square(x: f64, y: f64, center: c_int) -> *mut ManifoldCrossSection;
    pub fn mshim_cross_circle(radius: f64, segments: c_int) -> *mut ManifoldCrossSection;

    // CrossSection: booleans and hulls
    pub fn mshim_cross_boolean(
        a: *mut ManifoldCrossSection,
        b: *mut ManifoldCrossSection,
        op: c_int,
    ) -> *mut ManifoldCrossSection;
    pub fn mshim_cross_batch_boolean(
        css: *const *mut ManifoldCrossSection,
        n: usize,
        op: c_int,
    ) -> *mut ManifoldCrossSection;
    pub fn mshim_cross_hull(cs: *mut ManifoldCrossSection) -> *mut ManifoldCrossSection;
    pub fn mshim_cross_batch_hull(
        css: *const *mut ManifoldCrossSection,
        n: usize,
    ) -> *mut ManifoldCrossSection;
    pub fn mshim_cross_hull_pts(pts: *const Vec2, n: usize) -> *mut ManifoldCrossSection;

    // CrossSection: transforms and offsets
    pub fn mshim_cross_transform(
        cs: *mut ManifoldCrossSection,
        columns: *const f64,
    ) -> *mut ManifoldCrossSection;
    pub fn mshim_cross_translate(
        cs: *mut ManifoldCrossSection,
        x: f64,
        y: f64,
    ) -> *mut ManifoldCrossSection;
    pub fn mshim_cross_scale(
        cs: *mut ManifoldCrossSection,
        x: f64,
        y: f64,
    ) -> *mut ManifoldCrossSection;
    pub fn mshim_cross_rotate(
        cs: *mut ManifoldCrossSection,
        degrees: f64,
    ) -> *mut ManifoldCrossSection;
    pub fn mshim_cross_mirror(
        cs: *mut ManifoldCrossSection,
        ax: f64,
        ay: f64,
    ) -> *mut ManifoldCrossSection;
    pub fn mshim_cross_offset(
        cs: *mut ManifoldCrossSection,
        delta: f64,
        join_type: c_int,
        miter_limit: f64,
        segments: c_int,
    ) -> *mut ManifoldCrossSection;
    pub fn mshim_cross_simplify(
        cs: *mut ManifoldCrossSection,
        epsilon: f64,
    ) -> *mut ManifoldCrossSection;

    // CrossSection: queries
    pub fn mshim_cross_area(cs: *mut ManifoldCrossSection, out: *mut f64) -> c_int;
    pub fn mshim_cross_num_vert(cs: *mut ManifoldCrossSection, out: *mut usize) -> c_int;
    pub fn mshim_cross_num_contour(cs: *mut ManifoldCrossSection, out: *mut usize) -> c_int;
    pub fn mshim_cross_is_empty(cs: *mut ManifoldCrossSection, out: *mut c_int) -> c_int;
    pub fn mshim_cross_bounds(cs: *mut ManifoldCrossSection, out: *mut f64) -> c_int;

    // CrossSection: polygon output
    pub fn mshim_cross_to_polygons(cs: *mut ManifoldCrossSection) -> *mut ManifoldPolygons;
    pub fn mshim_polygons_len(ps: *mut ManifoldPolygons) -> usize;
    pub fn mshim_polygons_contour_len(ps: *mut ManifoldPolygons, i: usize) -> usize;
    pub fn mshim_polygons_copy_out(ps: *mut ManifoldPolygons, out: *mut Vec2);
    pub fn mshim_delete_polygons(ps: *mut ManifoldPolygons);
}
