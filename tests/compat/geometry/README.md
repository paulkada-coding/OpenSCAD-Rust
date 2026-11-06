# Geometry Compatibility Corpus (3D Render)

Test cases comparing the **geometry** produced by our engine (`scadr` + `mesher`) against
**OpenSCAD Nightly 2026.10.22** (Manifold backend). Each case is run with:

```
<engine> -o <name>.stl --export-format asciistl [compat-args] <name>.scad
```

Mesh metrics are computed from the ASCII STL and compared against those recorded from OpenSCAD in
`expected/<name>.json`. ASCII STL is used because OpenSCAD writes it at full double precision
(shortest round-trip numbers); OpenSCAD's OFF format only writes 6 significant digits and binary
STL uses 32-bit `float`. Observed export behavior is documented in
[`docs/openscad-behavior.md`](../../../docs/openscad-behavior.md) §16.

Expected files are checked into the repository: for normal runs **OpenSCAD is not required**.

## Layout

```
tests/compat/geometry/
  cases/<topic>_<name>.scad       one case per file (only top-level .scad files)
  cases/data/                     octahedron.stl (for import), heights.dat (for surface)
  expected/<name>.json            metrics, log, and exit code recorded from OpenSCAD
```

The `// compat-args:` directive (within the first 5 lines) works just like in the text corpus:
real-world models use it to vary parameters via `-D`, for example
`// compat-args: -D FINESTRA="ruota" -D TESTO="BLU" -D N_POS=2 -D TAB_POS=1`
(arguments are split on whitespace and passed directly without a shell: no spaces inside values).

## Metrics (`expected/<name>.json`)

```json
{
  "format": 1,
  "exit_code": 0,
  "top_level": null,
  "log": "WARNING: ...\n",
  "geometry": {
    "triangles": 12, "vertices": 8,
    "volume": 1000, "area": 600,
    "bbox_min": [0, 0, 0], "bbox_max": [10, 10, 10],
    "manifold": true, "open_edges": 0, "nonmanifold_edges": 0, "degenerate_triangles": 0,
    "components": 1, "euler": 2, "genus": 0,
    "vertex_hash": "fnv1a64:…", "triangle_hash": "fnv1a64:…",
    "vertex_list": [[0, 0, 0], …]
  }
}
```

| Field | Meaning |
|---|---|
| `exit_code` | engine exit code (OpenSCAD: 0, or 1 when no file is written) |
| `top_level` | `Current top level object is empty.` / `… is not a 3D object.` line if printed, otherwise `null` |
| `log` | normalized stderr as in the text corpus (messages only: `ECHO:`, `WARNING:`, `ERROR:`…; render stats discarded) |
| `geometry` | `null` if no output file was written |
| `triangles` | STL facets (including degenerate ones) |
| `vertices` | distinct vertices after bit-exact coordinate welding |
| `volume`, `area` | signed volume (divergence theorem relative to bounding-box center) and surface area |
| `bbox_min`, `bbox_max` | axis-aligned bounding box |
| `manifold` | every edge is shared by exactly two triangles with opposite winding |
| `open_edges`, `nonmanifold_edges`, `degenerate_triangles` | edges with a single triangle, edges with > 2 triangles (or two with matching winding), triangles with repeated vertices |
| `components`, `euler`, `genus` | connected components, χ = V − E + F, genus using Manifold's convention printed by OpenSCAD: `1 − χ/2` (two disjoint cubes → −1) |
| `vertex_hash` | 64-bit FNV-1a of the sorted list of distinct vertices rounded to 1e-6 |
| `triangle_hash` | 64-bit FNV-1a of triangles (vertices rounded to 1e-6, winding-preserving canonical rotation, sorted list) |
| `vertex_list` | sorted rounded vertices, included only when at most 64 |

All metrics are independent of triangle order in the file (triangles are placed in canonical order
before summation, making volume and area bit-reproducible).

## Outcomes

| Outcome | Meaning |
|---|---|
| `EXACT` | same vertices and triangles (at 1e-6), same log, same exit code; for empty-geometry cases: same `top_level` outcome |
| `EQUIVALENT` | volume, area, and bounding box within tolerances, same topology (`manifold`, `components`, `genus`), same log and exit code, but vertices or triangulation differ |
| `FAIL` | metric outside tolerance, missing or unexpected geometry, differing log or exit code |
| `ERROR` / `TIMEOUT` / `SKIP` | engine failed to start or unreadable STL / timed out / missing expected file |

Default tolerances: **1e-6 relative** for volume and area, **1e-6 absolute** for bounding box
(`--tol-volume`, `--tol-area`, `--tol-bbox`). Triangle count, vertex count, and open edges are
informational (they do not decide the outcome on their own). Exit code 0 if every case is `EXACT`
or `EQUIVALENT` (`--strict`: `EXACT` only).

## Running

```powershell
cargo build -p scadr
cargo run -p compat-runner -- --geometry                       # all cases with target\debug\scadr.exe
cargo run -p compat-runner -- --geometry --filter bool_ -v     # subset, with metric tables
cargo run -p compat-runner -- --geometry --tol-volume 1e-9 --strict
cargo run -p compat-runner -- --geometry --ignore-log          # ignore stderr messages
```

Output (one line per case followed by a summary; with `-v`, non-`EXACT` cases include a
metric/expected/actual/delta/tolerance table and log diff). Illustrative example:

```
CASE                 RESULT        TIME  DETAIL
prim_cube_basic      EXACT        0.05s  12 tri, 8 vert
prim_sphere_default  EQUIVALENT   0.06s  vertices differ; tri 250 vs 252, vert 127 vs 128; vol rel 2.1e-9, area rel 1.0e-9
bool_cross_holes     FAIL         0.02s  exit code 1 (expected 0); no geometry written (Current top level object is empty.)

Total: 100 cases, 1 exact, 1 equivalent, 98 failed, 0 errors, 0 timeouts, 0 skipped (1.0% exact, 2.0% exact or equivalent)
```

Raw STLs and computed metrics are written to `target\compat-out\geometry\engine\<name>.stl|json`
(OpenSCAD's outputs go to `…\geometry\openscad\` when running `--update-expected`).

Sanity check: running with OpenSCAD itself as the engine must produce `EXACT` on all cases:

```powershell
cargo run -p compat-runner -- --geometry --engine 'C:\Program Files\OpenSCAD (Nightly)\openscad.exe'
```

## Regenerating expected files

```powershell
cargo run -p compat-runner -- --geometry --update-expected                 # all cases
cargo run -p compat-runner -- --geometry --update-expected --filter rex_   # subset
```

Reports `new` / `updated` / `unchanged`; re-running without changes must report `0 changed`.
To add a case: create `cases/<topic>_<name>.scad`, regenerate with `--filter <name>`, inspect the
JSON, and verify that the case remains `EXACT` against OpenSCAD across multiple runs (see below).

Case guidelines: fast (< 1 s in OpenSCAD; currently the slowest, `model_porta_carte_taratura`,
takes 0.5 s), deterministic, no external libraries. **Watch out for OpenSCAD non-determinism**:
`minkowski()` with a non-convex operand and a curved second operand (sphere, cylinder) produces
different triangulations and collinear vertices across runs (same volume to 1e-16) because convex
pieces are unioned in parallel. `mink_nonconvex` therefore uses a cube as its second operand, which
is deterministic (15/15 runs).

## Cases (101)

| Prefix | Cases | Coverage |
|---|---|---|
| `prim_` | 24 | `cube` (scalar, vector, centered, tiny, zero dimension → empty), `sphere` (default, odd `$fn` and < 3, `$fa`/`$fs`, tiny and large radius, fine `$fs`), `cylinder` (default, `$fn` 6 and 500, `$fa` 1 on r = 500, tiny, cone, centered inverted cone, frustum with diameters and `$fs`), `polyhedron` (tetrahedron, quad faces, square-base pyramid, reversed-winding faces, points repeated at the same position) |
| `twod_` | 6 | extruded 2D primitives: square + circle, circle with `$fa`/`$fs`, polygon with hole (`paths`), concave star, overlapping paths (overlap becomes a hole), disjoint islands |
| `xf_` | 8 | `translate`/`rotate` on three axes, axis-angle rotation, non-uniform `scale` + `mirror`, negative scale, `multmatrix` (shear, 3×4 matrix), `resize`, `resize` with `auto`, `color` |
| `bool_` | 11 | union, difference, intersection, coplanar faces (difference and union), edge-touching cubes, three orthogonal holes, empty result, implicit union in `for`, `intersection_for`, nested booleans |
| `hull_` / `mink_` | 2 / 3 | 3D and 2D `hull`; convex, non-convex, and 2D `minkowski` |
| `lex_` | 7 | `linear_extrude`: `center`, `twist` + `slices`, negative `twist` with computed slices, vector `scale`, `scale = 0`, combined options, direction `v` |
| `rex_` | 6 | `rotate_extrude`: torus, default fragments, `angle` 90 and −135, `start`, axis-touching profile |
| `off_` / `proj_` / `fill_` / `text_` | 4 / 2 / 1 / 2 | positive and negative `offset` `r`, `delta`, `chamfer`; full and `cut` `projection`; `fill`; simple and centered `text` with `$fn` |
| `import_` / `surface_` | 2 / 2 | ASCII STL `import` (including scaled inside a difference); `.dat` `surface`, including centered |
| `mod_` | 4 | modifiers `#` (included), `%` (excluded), `!` (root subtree only), `*` (disabled) |
| `cli_` | 5 | empty top level, 2D object exported to STL (no file, exit 1), 2D mixed with 3D (render warnings), evaluation error (partial geometry, exit 0), `$preview` false during render |
| `model_` | 12 | copies of `examples/porta_carte.scad` (print layout, vertical 2-row box, lid, calibration gauge) and `examples/separatore.scad` (default, wheel window with text, vertical without foot), `-D` variants; copies from the text corpus of `model_bracket`, `model_vase`, `model_gear_polygon`, `model_parametric_box`, `model_spiral_tower` |
