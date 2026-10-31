//! 3D solids: [`Manifold`].

use std::ffi::c_int;
use std::fmt;
use std::ptr::{self, NonNull};

use geometry::glam::{DAffine3, DVec2, DVec3};
use geometry::{Aabb, Mesh};

use crate::cross_section::CrossSection;
use crate::{Error, c_segments, cxx_panic, ffi};

/// Boolean operation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OpType {
    /// Union.
    Add,
    /// Difference (first operand minus the others).
    Subtract,
    /// Intersection.
    Intersect,
}

impl OpType {
    pub(crate) fn code(self) -> c_int {
        match self {
            Self::Add => ffi::OP_ADD,
            Self::Subtract => ffi::OP_SUBTRACT,
            Self::Intersect => ffi::OP_INTERSECT,
        }
    }
}

/// A closed, oriented, watertight solid (`manifold::Manifold`).
///
/// Cheap to clone: copies share the underlying geometry. Booleans and
/// transforms are evaluated lazily, on the first query that needs the
/// result (mesh, volume, status, ...).
pub struct Manifold {
    ptr: NonNull<ffi::ManifoldManifold>,
}

// SAFETY: a `manifold::Manifold` owns its data through `shared_ptr`s and
// has no thread affinity, so it can move between threads.
unsafe impl Send for Manifold {}
// SAFETY: every method takes `&self` and maps to a `const` C++ method.
// Lazy evaluation mutates internal caches, but Manifold guards them with
// mutexes (`pNodeMutex_`, `CsgNode::mutex_`), so concurrent calls on the
// same object are data-race free.
unsafe impl Sync for Manifold {}

impl Manifold {
    /// Takes ownership of an object returned by the shim; a null pointer
    /// means a C++ exception was caught and becomes a panic.
    #[track_caller]
    fn from_raw(ptr: *mut ffi::ManifoldManifold, operation: &str) -> Self {
        match NonNull::new(ptr) {
            Some(ptr) => Self { ptr },
            None => cxx_panic(operation),
        }
    }

    fn raw(&self) -> *mut ffi::ManifoldManifold {
        self.ptr.as_ptr()
    }

    /// Runs a shim query with an out parameter.
    #[track_caller]
    fn query<T: Default>(
        &self,
        operation: &str,
        f: unsafe extern "C" fn(*mut ffi::ManifoldManifold, *mut T) -> c_int,
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

    /// The empty solid.
    pub fn empty() -> Self {
        // SAFETY: no arguments; returns a new object or null.
        Self::from_raw(unsafe { ffi::mshim_manifold_empty() }, "empty")
    }

    /// Builds a solid from an indexed triangle mesh (counter-clockwise
    /// triangles seen from outside). The mesh must already be a closed
    /// 2-manifold: vertices shared by index, every edge used by exactly two
    /// triangles. An empty mesh gives the empty solid.
    pub fn from_mesh(mesh: &Mesh) -> Result<Self, Error> {
        Self::of_mesh(mesh, false)
    }

    /// Like [`Self::from_mesh`], but first welds vertices of open edges that
    /// coincide within Manifold's tolerance (`MeshGL64::Merge`), which fixes
    /// meshes whose vertices were duplicated (e.g. read from STL).
    pub fn from_mesh_merged(mesh: &Mesh) -> Result<Self, Error> {
        Self::of_mesh(mesh, true)
    }

    fn of_mesh(mesh: &Mesh, merge: bool) -> Result<Self, Error> {
        let verts: Vec<f64> = mesh.vertices.iter().flat_map(|v| v.to_array()).collect();
        let tris: Vec<u64> = mesh
            .triangles
            .iter()
            .flat_map(|t| t.map(u64::from))
            .collect();
        // SAFETY: `verts` holds `vertices.len()` xyz triples and `tris`
        // `triangles.len()` index triples; the shim copies both.
        let ptr = unsafe {
            ffi::mshim_manifold_of_mesh(
                verts.as_ptr(),
                mesh.vertices.len(),
                tris.as_ptr(),
                mesh.triangles.len(),
                c_int::from(merge),
            )
        };
        let manifold = Self::from_raw(ptr, "from_mesh");
        manifold.status()?;
        Ok(manifold)
    }

    /// Axis-aligned box from the origin to `size` (or centered on the
    /// origin). Non-positive sizes give an invalid solid.
    pub fn cube(size: DVec3, center: bool) -> Self {
        // SAFETY: plain values; returns a new object or null.
        let ptr = unsafe { ffi::mshim_manifold_cube(size.x, size.y, size.z, c_int::from(center)) };
        Self::from_raw(ptr, "cube")
    }

    /// Geodesic sphere (Manifold's tessellation, not OpenSCAD's). `segments`
    /// below 3 uses Manifold's default quality.
    pub fn sphere(radius: f64, segments: u32) -> Self {
        // SAFETY: plain values; returns a new object or null.
        let ptr = unsafe { ffi::mshim_manifold_sphere(radius, c_segments(segments)) };
        Self::from_raw(ptr, "sphere")
    }

    /// Cylinder or cone along +Z (Manifold's tessellation). `segments`
    /// below 3 uses Manifold's default quality.
    pub fn cylinder(
        height: f64,
        radius_low: f64,
        radius_high: f64,
        segments: u32,
        center: bool,
    ) -> Self {
        // SAFETY: plain values; returns a new object or null.
        let ptr = unsafe {
            ffi::mshim_manifold_cylinder(
                height,
                radius_low,
                radius_high,
                c_segments(segments),
                c_int::from(center),
            )
        };
        Self::from_raw(ptr, "cylinder")
    }

    // ----- Output and queries -----

    /// Triangle mesh of the solid (counter-clockwise triangles seen from
    /// outside). Vertices are shared between triangles.
    pub fn to_mesh(&self) -> Mesh {
        // SAFETY: `self.raw()` is live; returns a new mesh object or null.
        let raw = unsafe { ffi::mshim_manifold_get_mesh(self.raw()) };
        let Some(raw) = NonNull::new(raw) else {
            cxx_panic("to_mesh");
        };
        let mesh = MeshGuard(raw);
        // SAFETY: `mesh.0` is a live MeshGL64 owned by the guard.
        let (num_prop, props_len, tri_len) = unsafe {
            (
                ffi::mshim_mesh_num_prop(mesh.0.as_ptr()),
                ffi::mshim_mesh_vert_properties_len(mesh.0.as_ptr()),
                ffi::mshim_mesh_tri_len(mesh.0.as_ptr()),
            )
        };
        let mut props = vec![0.0f64; props_len];
        let mut tris = vec![0u64; tri_len];
        // SAFETY: the buffers have exactly the lengths reported by the mesh.
        let ok = unsafe {
            ffi::mshim_mesh_copy_out(mesh.0.as_ptr(), props.as_mut_ptr(), tris.as_mut_ptr())
        };
        if ok == 0 {
            cxx_panic("to_mesh");
        }
        // Positions are the first three properties of each vertex; we never
        // add others, but stay correct if Manifold does.
        let stride = num_prop.max(3);
        let vertices = props
            .chunks_exact(stride)
            .map(|p| DVec3::new(p[0], p[1], p[2]))
            .collect::<Vec<_>>();
        let to_u32 = |i: u64| u32::try_from(i).expect("mesh exceeds u32 vertex indices");
        let triangles = tris
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| t.map(to_u32))
            .collect();
        Mesh {
            vertices,
            triangles,
        }
    }

    /// `Ok` if the solid is valid, else the error that made it invalid (an
    /// invalid input propagates through every operation that uses it).
    pub fn status(&self) -> Result<(), Error> {
        let code = self.query("status", ffi::mshim_manifold_status);
        match Error::from_code(code) {
            None => Ok(()),
            Some(err) => Err(err),
        }
    }

    /// True for the empty solid, including invalid ones.
    pub fn is_empty(&self) -> bool {
        self.query("is_empty", ffi::mshim_manifold_is_empty) != 0
    }

    pub fn num_vert(&self) -> usize {
        self.query("num_vert", ffi::mshim_manifold_num_vert)
    }

    pub fn num_edge(&self) -> usize {
        self.query("num_edge", ffi::mshim_manifold_num_edge)
    }

    pub fn num_tri(&self) -> usize {
        self.query("num_tri", ffi::mshim_manifold_num_tri)
    }

    /// Topological genus (number of handles) of a connected solid.
    pub fn genus(&self) -> i32 {
        self.query("genus", ffi::mshim_manifold_genus)
    }

    pub fn volume(&self) -> f64 {
        self.query("volume", ffi::mshim_manifold_volume)
    }

    pub fn surface_area(&self) -> f64 {
        self.query("surface_area", ffi::mshim_manifold_surface_area)
    }

    /// Manifold's geometric tolerance for this solid: features smaller than
    /// this may be merged or removed by booleans. Scales with the size of
    /// the bounding box (about 1e-12 relative).
    pub fn epsilon(&self) -> f64 {
        self.query("epsilon", ffi::mshim_manifold_epsilon)
    }

    /// Bounding box; [`Aabb::EMPTY`] for the empty solid.
    pub fn bounds(&self) -> Aabb {
        let mut b = [0.0f64; 6];
        // SAFETY: `self.raw()` is live and `b` has room for 6 doubles.
        let ok = unsafe { ffi::mshim_manifold_bounds(self.raw(), b.as_mut_ptr()) };
        if ok == 0 {
            cxx_panic("bounds");
        }
        let aabb = Aabb {
            min: DVec3::new(b[0], b[1], b[2]),
            max: DVec3::new(b[3], b[4], b[5]),
        };
        if aabb.is_empty() { Aabb::EMPTY } else { aabb }
    }

    // ----- Booleans -----

    pub fn boolean(&self, other: &Self, op: OpType) -> Self {
        // SAFETY: both objects are live; returns a new object or null.
        let ptr = unsafe { ffi::mshim_manifold_boolean(self.raw(), other.raw(), op.code()) };
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

    /// Applies `op` to all solids at once (`Subtract`: the first minus all
    /// the others). Much faster than chaining binary booleans. An empty
    /// slice gives the empty solid.
    pub fn batch_boolean(manifolds: &[Self], op: OpType) -> Self {
        let raws = raw_list(manifolds);
        // SAFETY: `raws` points to `raws.len()` live objects borrowed for
        // the duration of the call; the shim copies them.
        let ptr =
            unsafe { ffi::mshim_manifold_batch_boolean(raws.as_ptr(), raws.len(), op.code()) };
        Self::from_raw(ptr, "batch_boolean")
    }

    /// Union of all solids (replaces Manifold's deprecated `Compose`).
    pub fn batch_union(manifolds: &[Self]) -> Self {
        Self::batch_boolean(manifolds, OpType::Add)
    }

    /// Splits by `cutter`: returns `(self ∩ cutter, self − cutter)`
    /// computed in one pass.
    pub fn split(&self, cutter: &Self) -> (Self, Self) {
        let mut first = ptr::null_mut();
        let mut second = ptr::null_mut();
        // SAFETY: both objects are live; on success the shim writes two new
        // objects into the out pointers, on failure it writes nothing.
        let ok =
            unsafe { ffi::mshim_manifold_split(self.raw(), cutter.raw(), &mut first, &mut second) };
        if ok == 0 {
            cxx_panic("split");
        }
        (
            Self::from_raw(first, "split"),
            Self::from_raw(second, "split"),
        )
    }

    /// Splits by the plane `dot(normal, p) = offset` (the length of
    /// `normal` does not matter): returns `(part on the normal side, other
    /// part)`.
    pub fn split_by_plane(&self, normal: DVec3, offset: f64) -> (Self, Self) {
        let mut first = ptr::null_mut();
        let mut second = ptr::null_mut();
        // SAFETY: as in `split`.
        let ok = unsafe {
            ffi::mshim_manifold_split_by_plane(
                self.raw(),
                normal.x,
                normal.y,
                normal.z,
                offset,
                &mut first,
                &mut second,
            )
        };
        if ok == 0 {
            cxx_panic("split_by_plane");
        }
        (
            Self::from_raw(first, "split_by_plane"),
            Self::from_raw(second, "split_by_plane"),
        )
    }

    /// Keeps only the part on the `normal` side of the plane
    /// `dot(normal, p) = offset`.
    pub fn trim_by_plane(&self, normal: DVec3, offset: f64) -> Self {
        // SAFETY: `self.raw()` is live; returns a new object or null.
        let ptr = unsafe {
            ffi::mshim_manifold_trim_by_plane(self.raw(), normal.x, normal.y, normal.z, offset)
        };
        Self::from_raw(ptr, "trim_by_plane")
    }

    /// Connected components as separate solids.
    pub fn decompose(&self) -> Vec<Self> {
        // SAFETY: `self.raw()` is live; returns a new vector object or null.
        let vec = unsafe { ffi::mshim_manifold_decompose(self.raw()) };
        let Some(vec) = NonNull::new(vec) else {
            cxx_panic("decompose");
        };
        let vec = VecGuard(vec);
        // SAFETY: `vec.0` is a live vector owned by the guard.
        let len = unsafe { ffi::mshim_manifold_vec_length(vec.0.as_ptr()) };
        (0..len)
            .map(|i| {
                // SAFETY: `i < len`; returns a new object (copy) or null.
                let ptr = unsafe { ffi::mshim_manifold_vec_get(vec.0.as_ptr(), i) };
                Self::from_raw(ptr, "decompose")
            })
            .collect()
    }

    // ----- Transforms -----

    /// Applies an affine transform (a mirroring transform also flips the
    /// triangle winding, so the result stays valid).
    pub fn transform(&self, m: &DAffine3) -> Self {
        let c = [
            m.matrix3.x_axis.x,
            m.matrix3.x_axis.y,
            m.matrix3.x_axis.z,
            m.matrix3.y_axis.x,
            m.matrix3.y_axis.y,
            m.matrix3.y_axis.z,
            m.matrix3.z_axis.x,
            m.matrix3.z_axis.y,
            m.matrix3.z_axis.z,
            m.translation.x,
            m.translation.y,
            m.translation.z,
        ];
        // SAFETY: `self.raw()` is live and `c` holds the 12 doubles read.
        let ptr = unsafe { ffi::mshim_manifold_transform(self.raw(), c.as_ptr()) };
        Self::from_raw(ptr, "transform")
    }

    pub fn translate(&self, v: DVec3) -> Self {
        // SAFETY: `self.raw()` is live; returns a new object or null.
        let ptr = unsafe { ffi::mshim_manifold_translate(self.raw(), v.x, v.y, v.z) };
        Self::from_raw(ptr, "translate")
    }

    pub fn scale(&self, v: DVec3) -> Self {
        // SAFETY: `self.raw()` is live; returns a new object or null.
        let ptr = unsafe { ffi::mshim_manifold_scale(self.raw(), v.x, v.y, v.z) };
        Self::from_raw(ptr, "scale")
    }

    /// Rotates by `degrees.x` about X, then `degrees.y` about Y, then
    /// `degrees.z` about Z (OpenSCAD's `rotate([x, y, z])` order). Multiples
    /// of 90° are exact.
    pub fn rotate(&self, degrees: DVec3) -> Self {
        // SAFETY: `self.raw()` is live; returns a new object or null.
        let ptr =
            unsafe { ffi::mshim_manifold_rotate(self.raw(), degrees.x, degrees.y, degrees.z) };
        Self::from_raw(ptr, "rotate")
    }

    /// Mirrors across the plane through the origin with the given normal
    /// (any length). A zero normal gives the empty solid.
    pub fn mirror(&self, normal: DVec3) -> Self {
        // SAFETY: `self.raw()` is live; returns a new object or null.
        let ptr = unsafe { ffi::mshim_manifold_mirror(self.raw(), normal.x, normal.y, normal.z) };
        Self::from_raw(ptr, "mirror")
    }

    // ----- Hulls and Minkowski -----

    /// Convex hull of this solid.
    pub fn hull(&self) -> Self {
        // SAFETY: `self.raw()` is live; returns a new object or null.
        Self::from_raw(unsafe { ffi::mshim_manifold_hull(self.raw()) }, "hull")
    }

    /// Convex hull of several solids together.
    pub fn hull_of(manifolds: &[Self]) -> Self {
        let raws = raw_list(manifolds);
        // SAFETY: `raws` points to `raws.len()` live objects.
        let ptr = unsafe { ffi::mshim_manifold_batch_hull(raws.as_ptr(), raws.len()) };
        Self::from_raw(ptr, "hull_of")
    }

    /// Convex hull of a point cloud. Coplanar points give a degenerate
    /// zero-volume solid (valid status, not empty): check [`Self::volume`].
    pub fn hull_of_points(points: &[DVec3]) -> Self {
        let pts: Vec<ffi::Vec3> = points
            .iter()
            .map(|p| ffi::Vec3 {
                x: p.x,
                y: p.y,
                z: p.z,
            })
            .collect();
        // SAFETY: `pts` holds `pts.len()` points; the shim copies them.
        let ptr = unsafe { ffi::mshim_manifold_hull_pts(pts.as_ptr(), pts.len()) };
        Self::from_raw(ptr, "hull_of_points")
    }

    /// Minkowski sum. Fast when both operands are convex; otherwise Manifold
    /// decomposes the non-convex operand and unions many hulls, which can be
    /// slow.
    pub fn minkowski_sum(&self, other: &Self) -> Self {
        // SAFETY: both objects are live; returns a new object or null.
        let ptr = unsafe { ffi::mshim_manifold_minkowski_sum(self.raw(), other.raw()) };
        Self::from_raw(ptr, "minkowski_sum")
    }

    /// Minkowski difference (erosion of `self` by `other`).
    pub fn minkowski_difference(&self, other: &Self) -> Self {
        // SAFETY: both objects are live; returns a new object or null.
        let ptr = unsafe { ffi::mshim_manifold_minkowski_difference(self.raw(), other.raw()) };
        Self::from_raw(ptr, "minkowski_difference")
    }

    // ----- 3D -> 2D -----

    /// Cross-section at height `z`.
    pub fn slice(&self, z: f64) -> CrossSection {
        // SAFETY: `self.raw()` is live; returns a new object or null.
        let ptr = unsafe { ffi::mshim_manifold_slice(self.raw(), z) };
        CrossSection::from_raw(ptr, "slice")
    }

    /// Outline of the projection onto the XY plane.
    pub fn project(&self) -> CrossSection {
        // SAFETY: `self.raw()` is live; returns a new object or null.
        let ptr = unsafe { ffi::mshim_manifold_project(self.raw()) };
        CrossSection::from_raw(ptr, "project")
    }
}

/// See [`crate::extrude`].
pub(crate) fn extrude(
    cs: &CrossSection,
    height: f64,
    n_divisions: u32,
    twist_degrees: f64,
    scale_top: DVec2,
) -> Manifold {
    // SAFETY: `cs.raw()` is live; returns a new object or null.
    let ptr = unsafe {
        ffi::mshim_manifold_extrude(
            cs.raw(),
            height,
            c_segments(n_divisions),
            twist_degrees,
            scale_top.x,
            scale_top.y,
        )
    };
    Manifold::from_raw(ptr, "extrude")
}

/// See [`crate::revolve`].
pub(crate) fn revolve(cs: &CrossSection, circular_segments: u32, revolve_degrees: f64) -> Manifold {
    // SAFETY: `cs.raw()` is live; returns a new object or null.
    let ptr = unsafe {
        ffi::mshim_manifold_revolve(cs.raw(), c_segments(circular_segments), revolve_degrees)
    };
    Manifold::from_raw(ptr, "revolve")
}

fn raw_list(manifolds: &[Manifold]) -> Vec<*mut ffi::ManifoldManifold> {
    manifolds.iter().map(Manifold::raw).collect()
}

impl Clone for Manifold {
    fn clone(&self) -> Self {
        // SAFETY: `self.raw()` is live; returns a new object or null.
        Self::from_raw(unsafe { ffi::mshim_manifold_copy(self.raw()) }, "clone")
    }
}

impl Drop for Manifold {
    fn drop(&mut self) {
        // SAFETY: we own the object and it is never used again.
        unsafe { ffi::mshim_delete_manifold(self.raw()) }
    }
}

impl Default for Manifold {
    fn default() -> Self {
        Self::empty()
    }
}

impl fmt::Debug for Manifold {
    // Does not force lazy evaluation.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Manifold").finish_non_exhaustive()
    }
}

/// Frees a mesh returned by the shim.
struct MeshGuard(NonNull<ffi::ManifoldMeshGL64>);

impl Drop for MeshGuard {
    fn drop(&mut self) {
        // SAFETY: the guard owns the mesh and it is never used again.
        unsafe { ffi::mshim_delete_mesh(self.0.as_ptr()) }
    }
}

/// Frees a manifold vector returned by the shim.
struct VecGuard(NonNull<ffi::ManifoldManifoldVec>);

impl Drop for VecGuard {
    fn drop(&mut self) {
        // SAFETY: the guard owns the vector and it is never used again.
        unsafe { ffi::mshim_delete_manifold_vec(self.0.as_ptr()) }
    }
}
