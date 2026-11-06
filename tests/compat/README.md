# OpenSCAD Compatibility Corpus

Small, deterministic test cases that compare our engine (`scadr`) against
**OpenSCAD Nightly 2026.10.22** (the test oracle). Each case is run with
`<engine> -o <out>.csg <case>.scad` and two outputs are compared:

1. stderr messages (`ECHO:`, `WARNING:`, `ERROR:`, `TRACE:`, `DEPRECATED:`, …);
2. the exported CSG file (geometry tree in textual form).

Observed behavior is documented in detail in
[`docs/openscad-behavior.md`](../../docs/openscad-behavior.md).

Rendered **geometry** comparison (STL export, volume/area/bbox/vertex metrics,
`EXACT` / `EQUIVALENT` / `FAIL` outcomes) has a separate 100-case corpus in
[`geometry/`](geometry/README.md), executed via `cargo run -p compat-runner -- --geometry`.

## Layout

```
tests/compat/
  geometry/                        geometry corpus (see geometry/README.md)
  cases/<topic>_<name>.scad        one case per file (only top-level .scad files are cases)
  cases/lib/                       helper libraries for include/use (not cases)
  cases/data/                      data files for import()/surface() (not cases)
  expected/<name>.log              normalized stderr produced by OpenSCAD
  expected/<name>.csg              normalized CSG export produced by OpenSCAD
                                   (ABSENT if OpenSCAD writes no file, e.g. parse errors)
```

Topic prefixes (236 cases):

| Prefix | Cases | Coverage |
|---|---|---|
| `echo_` | 18 | number, string, vector, range, function, named-argument formatting, expression printing |
| `str_` | 8 | `str`, `chr`, `ord`, string indexing, escapes |
| `num_` | 9 | arithmetic, modulo, division by zero, `round`, `min`/`max`, large integers |
| `trig_` | 4 | exact sin/cos/tan values, signed zeros, inverse functions |
| `op_` | 11 | vector/matrix operations, comparisons, truthiness, bitwise, undefined operations |
| `index_` / `range_` | 4 / 10 | indexing, `.x/.y/.z`, decimal-step ranges, reversed ranges, limits, warnings, comparisons |
| `lc_` / `let_` / `expr_` | 6 / 2 / 1 | list comprehensions, `let`, `echo`/`assert` as expressions |
| `scope_` / `special_` | 10 / 10 | reassignment, evaluation order, `$` variables, `$children`, `parent_module` |
| `include_` / `use_` | 5 / 2 | `include` and `use` semantics, file names in messages |
| `func_` / `module_` | 14 / 9 | functions, function literals, arguments, recursion, modules, `children()` |
| `builtin_` | 18 | built-in functions (`len`, `concat`, `lookup`, `search`, `rands`, `is_*`, objects…) |
| `err_` / `lex_` | 20 / 5 | `assert`, parse errors, TRACE frames, comments, identifiers, deprecated modules, UTF-8 byte order mark, line breaks in strings |
| `csg_` | 58 | CSG export of every built-in module, transforms, modifiers, number formatting |
| `model_` | 12 | multi-feature real-world models (parametric box, gear, bracket, vase, `porta_carte`, `separatore`…) |

### `compat-args` directive

A case may request extra command-line arguments (passed both to OpenSCAD and to our engine)
with a comment in one of the first 5 lines:

```openscad
// compat-args: --enable=object-function
```

## Normalization rules

Applied identically to OpenSCAD's output (when generating expected files) and to our engine's
output (implementation: `tools/compat-runner/src/normalize.rs`).

**Log (stderr):**

1. UTF-8 decoding (lossy); `\r\n` → `\n` (OpenSCAD on Windows writes CRLF);
2. the absolute path of the `cases` directory is stripped (with trailing `\` or `/`), so
   `Can't parse file 'C:\...\cases\x.scad'!` becomes `Can't parse file 'x.scad'!`;
   paths in `in file x.scad, line N` are already relative to the main file's directory;
3. "noise" lines are discarded (after trimming leading spaces, starting with):
   `Fontconfig `, `Geometries in cache:`, `Geometry cache size in bytes:`, `CGAL Polyhedrons in cache:`,
   `CGAL cache size in bytes:`, `Total rendering time:`, `Top level object is`, `Convex:`, `Facets:`,
   `Vertices:`, `Volumes:`, `Halfedges:`, `Edges:`, `Halffacets:`, `Contours:`, `Rendering Polygon Mesh`,
   `Simple:`, `Bounding box:`, `Current top level object is`, `Parsing design`, `Compiling design`,
   `Rendering finished`, `Normalized tree`, `Compile and preview finished`
   (with `.csg` export OpenSCAD does not print render statistics; in practice only `Fontconfig error` lines appear);
4. only message lines are kept: lines starting with `ECHO:`, `WARNING:`, `ERROR:`, `TRACE:`,
   `DEPRECATED:`, `FONT-WARNING:`, `EXPORT-WARNING:`, `EXPORT-ERROR:`, `UI-WARNING:`,
   `Can't parse file`, **plus their continuation lines** (e.g. `echo("a\nb")` produces two
   lines; the invalid-color warning has a second line `For a list of valid color names…`).
   Lines before the first message are discarded;
5. in `TRACE:` lines, the frame count in `*** Excluding 15373 frames ***` becomes `N`
   (it depends on stack size);
6. trailing blank lines are stripped; the file ends with `\n` (empty if there are no messages).

Trailing spaces on lines are preserved: `echo()` outputs `ECHO: ` with a trailing space.

**CSG:**

1. UTF-8 (lossy), `\r\n` → `\n`; absolute path of the `cases` directory stripped;
2. `timestamp = <integer>` (in `import()`/`surface()`, the file modification timestamp) → `timestamp = 0`;
3. trailing blank lines are stripped (OpenSCAD always ends with an extra blank line) and the file
   ends with `\n`; an empty tree produces an empty file.

If OpenSCAD writes no CSG file (parse error: exit code 1), `expected/<name>.csg` does not exist
and the case fails if our engine writes one. The exit code itself is not compared here.

## Running

```powershell
$env:CARGO_TARGET_DIR = 'C:\Software\3D Openscad\target\agent-compat'   # optional
cargo run -p compat-runner                                  # all cases with target\debug\scadr.exe
cargo run -p compat-runner -- --filter csg_ --verbose       # only cases containing "csg_", with diffs
cargo run -p compat-runner -- --engine path\to\scadr.exe    # custom engine path
cargo run -p compat-runner -- --help
```

Options: `--engine <path>`, `--openscad <path>`, `--update-expected`, `--filter <substr>` (repeatable),
`-v/--verbose` (unified diffs for failures), `-j/--jobs <N>`, `--timeout <secs>` (default 30),
`--cases <dir>`, `--expected <dir>`, `--out-dir <dir>` (default `target\compat-out`: raw output
and normalized engine logs are saved there for debugging).

Per-case outcome: `PASS`, `FAIL` (log and/or CSG differ), `ERROR` (cannot launch engine),
`TIMEOUT`, `SKIP` (engine missing or expected file absent). Exit code 0 only if every case passes.
If `scadr` has not been built yet, all cases report `SKIP` with a clear message (exit code 1).

Runner sanity check: passing OpenSCAD itself as the engine must give 100%:

```powershell
cargo run -p compat-runner -- --engine 'C:\Program Files\OpenSCAD (Nightly)\openscad.exe'
```

## Regenerating expected files

```powershell
cargo run -p compat-runner -- --update-expected
# or with a different binary:
cargo run -p compat-runner -- --update-expected --openscad 'C:\path\to\openscad.exe'
```

This re-runs OpenSCAD on every case, applies normalization, and rewrites only changed files
(reporting `new` / `updated` / `unchanged`). Re-running without changes must report
`0 changed`. To add a case: create `cases/<topic>_<name>.scad`, then run
`--update-expected --filter <name>` and inspect the expected files manually before committing.

Case guidelines: no external libraries, no unseeded `rands()`, no wall-clock or stack-size
dependencies (e.g. non-tail recursion limits), keep files small and fast.
