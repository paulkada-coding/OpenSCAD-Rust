// Exception-safe C ABI over Manifold's C bindings (manifoldc).
//
// manifoldc is a thin `extern "C"` layer over the C++ API without any
// try/catch, so a C++ exception (std::bad_alloc, a Clipper2 range error on
// huge 2D coordinates, ...) would unwind straight into Rust, which is
// undefined behaviour. Every function here is `noexcept`: it catches all
// exceptions, records the message (see `mshim_last_error`) and returns a
// null pointer / zero. The Rust side (src/ffi.rs) declares exactly these
// functions and nothing from manifoldc directly.
//
// Objects are heap-allocated with manifoldc's `manifold_alloc_*` (global
// operator new) and freed with `mshim_delete_*` (manifoldc's
// `manifold_delete_*`: destructor + operator delete).

#include <cstddef>
#include <cstdint>
#include <exception>
#include <new>
#include <string>
#include <vector>

#include "manifold/manifoldc.h"

namespace {

thread_local std::string g_last_error;

void SetError(const char* what) noexcept {
  try {
    g_last_error = what != nullptr ? what : "";
  } catch (...) {
  }
}

// Runs `make(mem)` on fresh storage from `alloc`. If `make` throws, the
// storage is released and the exception is rethrown.
template <typename Alloc, typename Make>
auto NewOrThrow(Alloc alloc, Make make) -> decltype(alloc()) {
  void* mem = alloc();
  try {
    return make(mem);
  } catch (...) {
    ::operator delete(mem);
    throw;
  }
}

// Like NewOrThrow, but returns nullptr (recording the message) on failure.
template <typename Alloc, typename Make>
auto New(Alloc alloc, Make make) noexcept -> decltype(alloc()) {
  try {
    return NewOrThrow(alloc, make);
  } catch (const std::exception& e) {
    SetError(e.what());
  } catch (...) {
    SetError("unknown C++ exception");
  }
  return nullptr;
}

// Runs `f`, returning 1 on success and 0 (recording the message) if it threw.
template <typename F>
int Try(F f) noexcept {
  try {
    f();
    return 1;
  } catch (const std::exception& e) {
    SetError(e.what());
  } catch (...) {
    SetError("unknown C++ exception");
  }
  return 0;
}

// Owning handle for temporaries created inside a shim call.
template <typename T, void (*Delete)(T*)>
struct Owned {
  T* p = nullptr;
  explicit Owned(T* ptr) : p(ptr) {}
  Owned(Owned&& other) noexcept : p(other.p) { other.p = nullptr; }
  Owned(const Owned&) = delete;
  Owned& operator=(const Owned&) = delete;
  ~Owned() {
    if (p != nullptr) Delete(p);
  }
};

using OwnedMesh = Owned<ManifoldMeshGL64, manifold_delete_meshgl64>;
using OwnedManifoldVec = Owned<ManifoldManifoldVec, manifold_delete_manifold_vec>;
using OwnedCrossVec =
    Owned<ManifoldCrossSectionVec, manifold_delete_cross_section_vec>;
using OwnedSimplePolygon =
    Owned<ManifoldSimplePolygon, manifold_delete_simple_polygon>;
using OwnedPolygons = Owned<ManifoldPolygons, manifold_delete_polygons>;
using OwnedBox = Owned<ManifoldBox, manifold_delete_box>;
using OwnedRect = Owned<ManifoldRect, manifold_delete_rect>;

ManifoldManifold* NewManifold(
    ManifoldManifold* (*make)(void*, ManifoldManifold*), ManifoldManifold* m) {
  return New(manifold_alloc_manifold, [&](void* mem) { return make(mem, m); });
}

ManifoldCrossSection* NewCross(
    ManifoldCrossSection* (*make)(void*, ManifoldCrossSection*),
    ManifoldCrossSection* cs) {
  return New(manifold_alloc_cross_section,
             [&](void* mem) { return make(mem, cs); });
}

// Copies `ms[0..n]` into a temporary manifoldc vector (cheap: shared nodes).
OwnedManifoldVec ManifoldVec(ManifoldManifold* const* ms, size_t n) {
  OwnedManifoldVec vec(NewOrThrow(manifold_alloc_manifold_vec, [](void* mem) {
    return manifold_manifold_empty_vec(mem);
  }));
  manifold_manifold_vec_reserve(vec.p, n);
  for (size_t i = 0; i < n; ++i) manifold_manifold_vec_push_back(vec.p, ms[i]);
  return vec;
}

OwnedCrossVec CrossVec(ManifoldCrossSection* const* css, size_t n) {
  OwnedCrossVec vec(NewOrThrow(manifold_alloc_cross_section_vec, [](void* mem) {
    return manifold_cross_section_empty_vec(mem);
  }));
  manifold_cross_section_vec_reserve(vec.p, n);
  for (size_t i = 0; i < n; ++i) {
    manifold_cross_section_vec_push_back(vec.p, css[i]);
  }
  return vec;
}

OwnedSimplePolygon SimplePolygon(const ManifoldVec2* pts, size_t n) {
  return OwnedSimplePolygon(
      NewOrThrow(manifold_alloc_simple_polygon, [&](void* mem) {
        return manifold_simple_polygon(mem, const_cast<ManifoldVec2*>(pts), n);
      }));
}

OwnedPolygons CrossToPolygons(ManifoldCrossSection* cs) {
  return OwnedPolygons(NewOrThrow(manifold_alloc_polygons, [&](void* mem) {
    return manifold_cross_section_to_polygons(mem, cs);
  }));
}

}  // namespace

extern "C" {

const char* mshim_last_error(void) noexcept { return g_last_error.c_str(); }

// ---------------------------------------------------------------------------
// Manifold: lifetime and construction

void mshim_delete_manifold(ManifoldManifold* m) noexcept {
  manifold_delete_manifold(m);
}

ManifoldManifold* mshim_manifold_empty(void) noexcept {
  return New(manifold_alloc_manifold,
             [](void* mem) { return manifold_empty(mem); });
}

ManifoldManifold* mshim_manifold_copy(ManifoldManifold* m) noexcept {
  return NewManifold(manifold_copy, m);
}

// `verts` holds n_verts xyz triples, `tris` n_tris index triples. With
// `merge` set, coincident vertices on open edges are welded first
// (MeshGL64::Merge).
ManifoldManifold* mshim_manifold_of_mesh(const double* verts, size_t n_verts,
                                         const uint64_t* tris, size_t n_tris,
                                         int merge) noexcept {
  return New(manifold_alloc_manifold, [&](void* mem) {
    OwnedMesh mesh(NewOrThrow(manifold_alloc_meshgl64, [&](void* mm) {
      return manifold_meshgl64(mm, const_cast<double*>(verts), n_verts, 3,
                               const_cast<uint64_t*>(tris), n_tris);
    }));
    if (merge == 0) return manifold_of_meshgl64(mem, mesh.p);
    OwnedMesh merged(NewOrThrow(manifold_alloc_meshgl64, [&](void* mm) {
      return manifold_meshgl64_merge(mm, mesh.p);
    }));
    return manifold_of_meshgl64(mem, merged.p);
  });
}

ManifoldManifold* mshim_manifold_cube(double x, double y, double z,
                                      int center) noexcept {
  return New(manifold_alloc_manifold, [&](void* mem) {
    return manifold_cube(mem, x, y, z, center);
  });
}

ManifoldManifold* mshim_manifold_sphere(double radius, int segments) noexcept {
  return New(manifold_alloc_manifold, [&](void* mem) {
    return manifold_sphere(mem, radius, segments);
  });
}

ManifoldManifold* mshim_manifold_cylinder(double height, double radius_low,
                                          double radius_high, int segments,
                                          int center) noexcept {
  return New(manifold_alloc_manifold, [&](void* mem) {
    return manifold_cylinder(mem, height, radius_low, radius_high, segments,
                             center);
  });
}

// ---------------------------------------------------------------------------
// Manifold: booleans, hulls, minkowski

ManifoldManifold* mshim_manifold_boolean(ManifoldManifold* a,
                                         ManifoldManifold* b,
                                         int op) noexcept {
  return New(manifold_alloc_manifold, [&](void* mem) {
    return manifold_boolean(mem, a, b, static_cast<ManifoldOpType>(op));
  });
}

ManifoldManifold* mshim_manifold_batch_boolean(ManifoldManifold* const* ms,
                                               size_t n, int op) noexcept {
  return New(manifold_alloc_manifold, [&](void* mem) {
    OwnedManifoldVec vec = ManifoldVec(ms, n);
    return manifold_batch_boolean(mem, vec.p, static_cast<ManifoldOpType>(op));
  });
}

ManifoldManifold* mshim_manifold_hull(ManifoldManifold* m) noexcept {
  return NewManifold(manifold_hull, m);
}

ManifoldManifold* mshim_manifold_batch_hull(ManifoldManifold* const* ms,
                                            size_t n) noexcept {
  return New(manifold_alloc_manifold, [&](void* mem) {
    OwnedManifoldVec vec = ManifoldVec(ms, n);
    return manifold_batch_hull(mem, vec.p);
  });
}

ManifoldManifold* mshim_manifold_hull_pts(const ManifoldVec3* pts,
                                          size_t n) noexcept {
  return New(manifold_alloc_manifold, [&](void* mem) {
    return manifold_hull_pts(mem, const_cast<ManifoldVec3*>(pts), n);
  });
}

ManifoldManifold* mshim_manifold_minkowski_sum(ManifoldManifold* a,
                                               ManifoldManifold* b) noexcept {
  return New(manifold_alloc_manifold,
             [&](void* mem) { return manifold_minkowski_sum(mem, a, b); });
}

ManifoldManifold* mshim_manifold_minkowski_difference(
    ManifoldManifold* a, ManifoldManifold* b) noexcept {
  return New(manifold_alloc_manifold, [&](void* mem) {
    return manifold_minkowski_difference(mem, a, b);
  });
}

// ---------------------------------------------------------------------------
// Manifold: transforms

// `c` is a column-major 3x4 matrix (12 doubles: x axis, y axis, z axis,
// translation).
ManifoldManifold* mshim_manifold_transform(ManifoldManifold* m,
                                           const double* c) noexcept {
  return New(manifold_alloc_manifold, [&](void* mem) {
    return manifold_transform(mem, m, c[0], c[1], c[2], c[3], c[4], c[5], c[6],
                              c[7], c[8], c[9], c[10], c[11]);
  });
}

ManifoldManifold* mshim_manifold_translate(ManifoldManifold* m, double x,
                                           double y, double z) noexcept {
  return New(manifold_alloc_manifold,
             [&](void* mem) { return manifold_translate(mem, m, x, y, z); });
}

ManifoldManifold* mshim_manifold_scale(ManifoldManifold* m, double x, double y,
                                       double z) noexcept {
  return New(manifold_alloc_manifold,
             [&](void* mem) { return manifold_scale(mem, m, x, y, z); });
}

ManifoldManifold* mshim_manifold_rotate(ManifoldManifold* m, double x, double y,
                                        double z) noexcept {
  return New(manifold_alloc_manifold,
             [&](void* mem) { return manifold_rotate(mem, m, x, y, z); });
}

ManifoldManifold* mshim_manifold_mirror(ManifoldManifold* m, double nx,
                                        double ny, double nz) noexcept {
  return New(manifold_alloc_manifold,
             [&](void* mem) { return manifold_mirror(mem, m, nx, ny, nz); });
}

// ---------------------------------------------------------------------------
// Manifold: splitting and decomposition

// Writes two new manifolds into `first` / `second`; returns 0 on failure
// (nothing is allocated then).
int mshim_manifold_split_by_plane(ManifoldManifold* m, double nx, double ny,
                                  double nz, double offset,
                                  ManifoldManifold** first,
                                  ManifoldManifold** second) noexcept {
  void* mem1 = nullptr;
  void* mem2 = nullptr;
  int ok = Try([&] {
    mem1 = manifold_alloc_manifold();
    mem2 = manifold_alloc_manifold();
    ManifoldManifoldPair pair =
        manifold_split_by_plane(mem1, mem2, m, nx, ny, nz, offset);
    *first = pair.first;
    *second = pair.second;
  });
  if (ok == 0) {
    ::operator delete(mem1);
    ::operator delete(mem2);
  }
  return ok;
}

int mshim_manifold_split(ManifoldManifold* a, ManifoldManifold* b,
                         ManifoldManifold** first,
                         ManifoldManifold** second) noexcept {
  void* mem1 = nullptr;
  void* mem2 = nullptr;
  int ok = Try([&] {
    mem1 = manifold_alloc_manifold();
    mem2 = manifold_alloc_manifold();
    ManifoldManifoldPair pair = manifold_split(mem1, mem2, a, b);
    *first = pair.first;
    *second = pair.second;
  });
  if (ok == 0) {
    ::operator delete(mem1);
    ::operator delete(mem2);
  }
  return ok;
}

ManifoldManifold* mshim_manifold_trim_by_plane(ManifoldManifold* m, double nx,
                                               double ny, double nz,
                                               double offset) noexcept {
  return New(manifold_alloc_manifold, [&](void* mem) {
    return manifold_trim_by_plane(mem, m, nx, ny, nz, offset);
  });
}

ManifoldManifoldVec* mshim_manifold_decompose(ManifoldManifold* m) noexcept {
  return New(manifold_alloc_manifold_vec,
             [&](void* mem) { return manifold_decompose(mem, m); });
}

size_t mshim_manifold_vec_length(ManifoldManifoldVec* v) noexcept {
  return manifold_manifold_vec_length(v);
}

ManifoldManifold* mshim_manifold_vec_get(ManifoldManifoldVec* v,
                                         size_t i) noexcept {
  return New(manifold_alloc_manifold,
             [&](void* mem) { return manifold_manifold_vec_get(mem, v, i); });
}

void mshim_delete_manifold_vec(ManifoldManifoldVec* v) noexcept {
  manifold_delete_manifold_vec(v);
}

// ---------------------------------------------------------------------------
// Manifold: queries (these force evaluation of pending booleans)

int mshim_manifold_status(ManifoldManifold* m, int* out) noexcept {
  return Try([&] { *out = static_cast<int>(manifold_status(m)); });
}

int mshim_manifold_is_empty(ManifoldManifold* m, int* out) noexcept {
  return Try([&] { *out = manifold_is_empty(m); });
}

int mshim_manifold_num_vert(ManifoldManifold* m, size_t* out) noexcept {
  return Try([&] { *out = manifold_num_vert(m); });
}

int mshim_manifold_num_edge(ManifoldManifold* m, size_t* out) noexcept {
  return Try([&] { *out = manifold_num_edge(m); });
}

int mshim_manifold_num_tri(ManifoldManifold* m, size_t* out) noexcept {
  return Try([&] { *out = manifold_num_tri(m); });
}

int mshim_manifold_genus(ManifoldManifold* m, int* out) noexcept {
  return Try([&] { *out = manifold_genus(m); });
}

int mshim_manifold_volume(ManifoldManifold* m, double* out) noexcept {
  return Try([&] { *out = manifold_volume(m); });
}

int mshim_manifold_surface_area(ManifoldManifold* m, double* out) noexcept {
  return Try([&] { *out = manifold_surface_area(m); });
}

int mshim_manifold_epsilon(ManifoldManifold* m, double* out) noexcept {
  return Try([&] { *out = manifold_epsilon(m); });
}

// Writes min xyz then max xyz into `out[6]`.
int mshim_manifold_bounds(ManifoldManifold* m, double* out) noexcept {
  return Try([&] {
    OwnedBox box(NewOrThrow(manifold_alloc_box, [&](void* mem) {
      return manifold_bounding_box(mem, m);
    }));
    ManifoldVec3 lo = manifold_box_min(box.p);
    ManifoldVec3 hi = manifold_box_max(box.p);
    out[0] = lo.x;
    out[1] = lo.y;
    out[2] = lo.z;
    out[3] = hi.x;
    out[4] = hi.y;
    out[5] = hi.z;
  });
}

// ---------------------------------------------------------------------------
// Manifold: mesh output

ManifoldMeshGL64* mshim_manifold_get_mesh(ManifoldManifold* m) noexcept {
  return New(manifold_alloc_meshgl64,
             [&](void* mem) { return manifold_get_meshgl64(mem, m); });
}

size_t mshim_mesh_num_prop(ManifoldMeshGL64* mesh) noexcept {
  return manifold_meshgl64_num_prop(mesh);
}

size_t mshim_mesh_vert_properties_len(ManifoldMeshGL64* mesh) noexcept {
  return manifold_meshgl64_vert_properties_length(mesh);
}

size_t mshim_mesh_tri_len(ManifoldMeshGL64* mesh) noexcept {
  return manifold_meshgl64_tri_length(mesh);
}

// Copies vertProperties into `props` and triVerts into `tris`, which must
// hold the lengths reported above.
int mshim_mesh_copy_out(ManifoldMeshGL64* mesh, double* props,
                        uint64_t* tris) noexcept {
  return Try([&] {
    manifold_meshgl64_vert_properties(props, mesh);
    manifold_meshgl64_tri_verts(tris, mesh);
  });
}

void mshim_delete_mesh(ManifoldMeshGL64* mesh) noexcept {
  manifold_delete_meshgl64(mesh);
}

// ---------------------------------------------------------------------------
// Manifold <-> CrossSection

ManifoldCrossSection* mshim_manifold_slice(ManifoldManifold* m,
                                           double height) noexcept {
  return New(manifold_alloc_cross_section, [&](void* mem) {
    OwnedPolygons polys(NewOrThrow(manifold_alloc_polygons, [&](void* pm) {
      return manifold_slice(pm, m, height);
    }));
    return manifold_cross_section_of_polygons(mem, polys.p,
                                              MANIFOLD_FILL_RULE_POSITIVE);
  });
}

ManifoldCrossSection* mshim_manifold_project(ManifoldManifold* m) noexcept {
  return New(manifold_alloc_cross_section, [&](void* mem) {
    OwnedPolygons polys(NewOrThrow(manifold_alloc_polygons, [&](void* pm) {
      return manifold_project(pm, m);
    }));
    return manifold_cross_section_of_polygons(mem, polys.p,
                                              MANIFOLD_FILL_RULE_POSITIVE);
  });
}

ManifoldManifold* mshim_manifold_extrude(ManifoldCrossSection* cs,
                                         double height, int divisions,
                                         double twist_degrees, double scale_x,
                                         double scale_y) noexcept {
  return New(manifold_alloc_manifold, [&](void* mem) {
    OwnedPolygons polys = CrossToPolygons(cs);
    return manifold_extrude(mem, polys.p, height, divisions, twist_degrees,
                            scale_x, scale_y);
  });
}

ManifoldManifold* mshim_manifold_revolve(ManifoldCrossSection* cs,
                                         int segments,
                                         double revolve_degrees) noexcept {
  return New(manifold_alloc_manifold, [&](void* mem) {
    OwnedPolygons polys = CrossToPolygons(cs);
    return manifold_revolve(mem, polys.p, segments, revolve_degrees);
  });
}

// ---------------------------------------------------------------------------
// CrossSection: lifetime and construction

void mshim_delete_cross(ManifoldCrossSection* cs) noexcept {
  manifold_delete_cross_section(cs);
}

ManifoldCrossSection* mshim_cross_empty(void) noexcept {
  return New(manifold_alloc_cross_section,
             [](void* mem) { return manifold_cross_section_empty(mem); });
}

ManifoldCrossSection* mshim_cross_copy(ManifoldCrossSection* cs) noexcept {
  return NewCross(manifold_cross_section_copy, cs);
}

// `pts` holds all contours back to back; `lens[i]` is the point count of
// contour i.
ManifoldCrossSection* mshim_cross_of_polygons(const ManifoldVec2* pts,
                                              const size_t* lens, size_t n,
                                              int fill_rule) noexcept {
  return New(manifold_alloc_cross_section, [&](void* mem) {
    std::vector<OwnedSimplePolygon> contours;
    std::vector<ManifoldSimplePolygon*> raw;
    contours.reserve(n);
    raw.reserve(n);
    size_t start = 0;
    for (size_t i = 0; i < n; ++i) {
      contours.push_back(SimplePolygon(pts + start, lens[i]));
      raw.push_back(contours.back().p);
      start += lens[i];
    }
    OwnedPolygons polys(NewOrThrow(manifold_alloc_polygons, [&](void* pm) {
      return manifold_polygons(pm, raw.data(), n);
    }));
    return manifold_cross_section_of_polygons(
        mem, polys.p, static_cast<ManifoldFillRule>(fill_rule));
  });
}

ManifoldCrossSection* mshim_cross_square(double x, double y,
                                         int center) noexcept {
  return New(manifold_alloc_cross_section, [&](void* mem) {
    return manifold_cross_section_square(mem, x, y, center);
  });
}

ManifoldCrossSection* mshim_cross_circle(double radius, int segments) noexcept {
  return New(manifold_alloc_cross_section, [&](void* mem) {
    return manifold_cross_section_circle(mem, radius, segments);
  });
}

// ---------------------------------------------------------------------------
// CrossSection: booleans and hulls

ManifoldCrossSection* mshim_cross_boolean(ManifoldCrossSection* a,
                                          ManifoldCrossSection* b,
                                          int op) noexcept {
  return New(manifold_alloc_cross_section, [&](void* mem) {
    return manifold_cross_section_boolean(mem, a, b,
                                          static_cast<ManifoldOpType>(op));
  });
}

ManifoldCrossSection* mshim_cross_batch_boolean(
    ManifoldCrossSection* const* css, size_t n, int op) noexcept {
  return New(manifold_alloc_cross_section, [&](void* mem) {
    OwnedCrossVec vec = CrossVec(css, n);
    return manifold_cross_section_batch_boolean(
        mem, vec.p, static_cast<ManifoldOpType>(op));
  });
}

ManifoldCrossSection* mshim_cross_hull(ManifoldCrossSection* cs) noexcept {
  return NewCross(manifold_cross_section_hull, cs);
}

ManifoldCrossSection* mshim_cross_batch_hull(ManifoldCrossSection* const* css,
                                             size_t n) noexcept {
  return New(manifold_alloc_cross_section, [&](void* mem) {
    OwnedCrossVec vec = CrossVec(css, n);
    return manifold_cross_section_batch_hull(mem, vec.p);
  });
}

ManifoldCrossSection* mshim_cross_hull_pts(const ManifoldVec2* pts,
                                           size_t n) noexcept {
  return New(manifold_alloc_cross_section, [&](void* mem) {
    OwnedSimplePolygon poly = SimplePolygon(pts, n);
    return manifold_cross_section_hull_simple_polygon(mem, poly.p);
  });
}

// ---------------------------------------------------------------------------
// CrossSection: transforms and offsets

// `c` is a column-major 2x3 matrix (6 doubles: x axis, y axis, translation).
ManifoldCrossSection* mshim_cross_transform(ManifoldCrossSection* cs,
                                            const double* c) noexcept {
  return New(manifold_alloc_cross_section, [&](void* mem) {
    return manifold_cross_section_transform(mem, cs, c[0], c[1], c[2], c[3],
                                            c[4], c[5]);
  });
}

ManifoldCrossSection* mshim_cross_translate(ManifoldCrossSection* cs, double x,
                                            double y) noexcept {
  return New(manifold_alloc_cross_section, [&](void* mem) {
    return manifold_cross_section_translate(mem, cs, x, y);
  });
}

ManifoldCrossSection* mshim_cross_scale(ManifoldCrossSection* cs, double x,
                                        double y) noexcept {
  return New(manifold_alloc_cross_section, [&](void* mem) {
    return manifold_cross_section_scale(mem, cs, x, y);
  });
}

ManifoldCrossSection* mshim_cross_rotate(ManifoldCrossSection* cs,
                                         double degrees) noexcept {
  return New(manifold_alloc_cross_section, [&](void* mem) {
    return manifold_cross_section_rotate(mem, cs, degrees);
  });
}

ManifoldCrossSection* mshim_cross_mirror(ManifoldCrossSection* cs, double ax,
                                         double ay) noexcept {
  return New(manifold_alloc_cross_section, [&](void* mem) {
    return manifold_cross_section_mirror(mem, cs, ax, ay);
  });
}

ManifoldCrossSection* mshim_cross_offset(ManifoldCrossSection* cs,
                                         double delta, int join_type,
                                         double miter_limit,
                                         int segments) noexcept {
  return New(manifold_alloc_cross_section, [&](void* mem) {
    return manifold_cross_section_offset(
        mem, cs, delta, static_cast<ManifoldJoinType>(join_type), miter_limit,
        segments);
  });
}

ManifoldCrossSection* mshim_cross_simplify(ManifoldCrossSection* cs,
                                           double epsilon) noexcept {
  return New(manifold_alloc_cross_section, [&](void* mem) {
    return manifold_cross_section_simplify(mem, cs, epsilon);
  });
}

// ---------------------------------------------------------------------------
// CrossSection: queries

int mshim_cross_area(ManifoldCrossSection* cs, double* out) noexcept {
  return Try([&] { *out = manifold_cross_section_area(cs); });
}

int mshim_cross_num_vert(ManifoldCrossSection* cs, size_t* out) noexcept {
  return Try([&] { *out = manifold_cross_section_num_vert(cs); });
}

int mshim_cross_num_contour(ManifoldCrossSection* cs, size_t* out) noexcept {
  return Try([&] { *out = manifold_cross_section_num_contour(cs); });
}

int mshim_cross_is_empty(ManifoldCrossSection* cs, int* out) noexcept {
  return Try([&] { *out = manifold_cross_section_is_empty(cs); });
}

// Writes min xy then max xy into `out[4]`.
int mshim_cross_bounds(ManifoldCrossSection* cs, double* out) noexcept {
  return Try([&] {
    OwnedRect rect(NewOrThrow(manifold_alloc_rect, [&](void* mem) {
      return manifold_cross_section_bounds(mem, cs);
    }));
    ManifoldVec2 lo = manifold_rect_min(rect.p);
    ManifoldVec2 hi = manifold_rect_max(rect.p);
    out[0] = lo.x;
    out[1] = lo.y;
    out[2] = hi.x;
    out[3] = hi.y;
  });
}

// ---------------------------------------------------------------------------
// CrossSection: polygon output

ManifoldPolygons* mshim_cross_to_polygons(ManifoldCrossSection* cs) noexcept {
  return New(manifold_alloc_polygons, [&](void* mem) {
    return manifold_cross_section_to_polygons(mem, cs);
  });
}

size_t mshim_polygons_len(ManifoldPolygons* ps) noexcept {
  return manifold_polygons_length(ps);
}

size_t mshim_polygons_contour_len(ManifoldPolygons* ps, size_t i) noexcept {
  return manifold_polygons_simple_length(ps, i);
}

// Copies all points, contour after contour, into `out`, which must hold the
// sum of the contour lengths.
void mshim_polygons_copy_out(ManifoldPolygons* ps, ManifoldVec2* out) noexcept {
  size_t k = 0;
  size_t n = manifold_polygons_length(ps);
  for (size_t i = 0; i < n; ++i) {
    size_t len = manifold_polygons_simple_length(ps, i);
    for (size_t j = 0; j < len; ++j) {
      out[k++] = manifold_polygons_get_point(ps, i, j);
    }
  }
}

void mshim_delete_polygons(ManifoldPolygons* ps) noexcept {
  manifold_delete_polygons(ps);
}

}  // extern "C"
