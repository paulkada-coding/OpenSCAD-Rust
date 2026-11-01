# Milestone 2 Review — Branch `milestone-2-complete`

Handoff and traceability document prepared for **Claude**'s review prior to merging into `main`.

---

## 1. Executive Summary

- **Working branch**: `milestone-2-complete` (branched from `main` at `34dc2e0`; `main` was left untouched).
- **Compatibility with OpenSCAD Nightly 2026.10.22** (`tools/compat-runner` on `tests/compat`):
  - Initial state on `main`: **181 / 215 cases passed (84.2%)** — 34 failing cases.
  - Final state on `milestone-2-complete`: **215 / 215 cases passed (100.0%)** — 0 failed, 0 errors, 0 timeouts.
- **Code quality and constraints**:
  - `cargo test --workspace`: all unit and integration tests passing.
  - `cargo clippy --workspace --all-targets`: **0 warnings** (also fixed the 3 pre-existing `clippy` warnings in `crates/scad-eval/src/value.rs`).
  - **Zero `unsafe`** added (`unsafe_code = "deny"` respected across all language/interpreter/geometry crates).
  - **Clean-room**: no OpenSCAD C++ (GPL) source files were read or copied; all behavior was derived solely from `docs/openscad-behavior.md`, expected outputs in `tests/compat/expected/`, and black-box execution of `openscad.com` on small `.scad` test snippets.
  - Formatting (`rustfmt --edition 2024`) applied only to modified files, avoiding diff noise on untouched files.

---

## 2. Quick Review Commands

```powershell
# Atomic commits relative to main
git log --oneline --reverse main..milestone-2-complete

# Per-commit file stats
git log --stat --reverse main..milestone-2-complete

# Full diff relative to main
git diff main..milestone-2-complete

# Full verification (clippy, unit tests, compatibility suite)
cargo clippy --workspace --all-targets
cargo test --workspace
cargo build -p scadr
cargo run -p compat-runner
```

---

## 3. Commit-by-Commit Breakdown

### Commit 1 — `eb6f001`
**`feat(scad-syntax,scad-eval): add lexer/parser diagnostics and escape rules`**
*(+8 compat cases: 181/215 → 189/215)*

- **Modified files**:
  - `crates/scad-syntax/src/{syntax_kind.rs, lexer.rs, lower.rs, parser.rs, lib.rs, tests.rs}`
  - `crates/scad-eval/src/{message.rs, eval.rs}`
- **Implemented**:
  1. **Hexadecimal literals and digit-leading identifiers**:
     - Restricted hex regex to `0x[0-9a-fA-F]+` (lowercase `0x`); `0XFF` now lexes as `IDENT` (matching OpenSCAD), emitting `DEPRECATED` followed by `WARNING: Ignoring unknown variable "0XFF"`.
     - Added `LexWarning` in `scad-syntax` and `Level::Deprecated` (`DEPRECATED: ...`) in `scad-eval`.
     - Every `IDENT` starting with an ASCII digit emits `DEPRECATED: Variable names starting with digits ("...") will be removed in future releases.`.
  2. **Integers not exactly representable in IEEE-754 `f64`**:
     - For decimal integer literals exceeding 53-bit precision (`(n as f64) as u128 != n` or `u128` overflow), emits `WARNING: Integer "..." cannot be represented precisely`.
  3. **Float literal overflow (`1e400`) and lossless CST**:
     - Introduced `SyntaxKind::OVERFLOW_NUMBER` classified as trivia (`is_trivia() == true`).
     - This preserves every source character inside the `rowan` CST (keeping the lossless property intact for refactoring and AI) while the parser transparently skips out-of-range numbers just like Flex/Bison in OpenSCAD (`echo(1e400);` → `ECHO: `, `1e400 ? 1 : 2` → syntax error).
  4. **String escape sequences (`lower.rs::unescape`)**:
     - Valid escapes: `\"`, `\\`, `\n`, `\t`, `\r`, `\x01`..`\x7f`, `\uXXXX` (4 hex), `\UXXXXXXXX` (6 hex).
     - Null codepoint (`\x00`, `\u0000`, `\U000000`), UTF-16 surrogates (`0xD800..=0xDFFF`), or values `> 0x10FFFF` convert to space `' '` without warning.
     - `\x` with fewer than 2 hex digits or value `>= 0x80` (`\x80`..`\xff`) is treated as an undefined escape: strips `\` and keeps the rest literal, emitting `WARNING: Undefined escape sequence` (capped at 5 warnings per string literal, matching OpenSCAD).
  5. **Unterminated strings and block comments**:
     - Added `UNTERMINATED_STRING` and `UNTERMINATED_COMMENT`; the reported line number follows OpenSCAD's Flex buffer convention (`newline_count + 3`), emitting `ERROR: Parser error: Unterminated string/comment ...` followed by `ERROR: Parser error: syntax error ...`.
- **Resolved cases**: `echo_numbers_edge`, `err_parse_number_overflow`, `err_parse_unterminated_comment`, `err_parse_unterminated_string`, `lex_identifiers`, `num_integers_large`, `str_bad_escapes`, `str_escapes`.

---

### Commit 2 — `7c1fc28`
**`fix(scad-eval): warn on missing include files and unsupported child modules`**
*(+3 compat cases: 189/215 → 192/215)*

- **Modified files**:
  - `crates/scad-eval/src/{eval.rs, builtins/modules.rs}`
- **Implemented**:
  1. **Non-fatal `include <missing.scad>`**:
     - Replaced fatal error with `WARNING: Can't find include file '{path}'.` (pointing to the `include` statement) while continuing evaluation of the calling file, consistent with `use <missing.scad>`.
  2. **Leaf modules that do not accept children**:
     - Added `builtins::modules::accepts_children(name)` (`false` for `cube`, `sphere`, `cylinder`, `polyhedron`, `square`, `circle`, `polygon`, `text`, `import`, `surface`).
     - When invoked with a non-empty child block (`cube() echo("x");` or `cube() sphere();`), emits `WARNING: module {name}() does not support child modules` and skips evaluating the child body (while still validating the parent module's arguments).
- **Resolved cases**: `include_missing`, `module_with_child_not_supported`, `err_parse_missing_semicolon`.

---

### Commit 3 — `09e34a3`
**`fix(scad-eval,scadr): validate builtin module parameters and match matrix signed zeros`**
*(+12 compat cases: 192/215 → 204/215)*

- **Modified files**:
  - `crates/scad-eval/src/{message.rs, eval.rs, builtins/modules.rs}`
  - `crates/scadr/src/main.rs`
- **Implemented**:
  1. **Strict boolean conversion in built-in module parameters (`Params::bool`)**:
     - Replaced `Value::is_truthy()` with `Value::as_bool().unwrap_or(default)`: in OpenSCAD `cube(center = "yes")` or `cube(center = 1)` falls back to default `false` rather than coercing by truthiness.
  2. **Built-in module parameter conversion warnings**:
     - `square`: `Unable to convert square(size=..., ...) parameter to a number or a vec2 of numbers`.
     - `polygon` / `polyhedron`: `Unable to convert points = ... to a vector of coordinates`.
     - `translate`: `Unable to convert translate(...) parameter to a vec3 or vec2 of numbers`.
     - `scale`: `Unable to convert scale(...) parameter to a number or a vec3 or vec2 of numbers`.
     - `mirror`: `Unable to convert mirror(...) parameter to a vec3 or vec2 of numbers`.
     - `rotate`: `Problem converting rotate(a=...) parameter` (distinguishing positional `rotate(x)` from named `rotate(a=x)`).
     - `color`: `Unable to parse color "..."` followed by continuation line `For a list of valid color names, see the <a href="open-window://colorlist"><b>Color List</b></a> window.` (via `Message::continuation` with no `WARNING:` prefix or `in file ...` suffix), plus `[0.0, 1.0]` range check (`color() expects numbers between 0.0 and 1.0. Value of {:.1} is out of range`).
  3. **Exact `-0.0` signs in 4×4 matrices (`DMat4`)**:
     - `scale_matrix`: built with component-wise row multiplication matching `Eigen::Scaling` so `scale([-0, 1, 1])` produces `[-0, -0, -0]` in column 0.
     - `mirror`: avoided intermediate `sqrt()` normalization (`1.0 - 2.0 * a * b / len_sq`, with `len_sq = x*x + y*y + z*z`), eliminating `2.22045e-16` residuals on `mirror([1, 1, 0])` and preserving off-diagonal `-0` on `mirror([1, 0, 0])`.
     - Single-axis `rotation`: `+ 0.0` normalization on off-diagonal terms to prevent spurious `-0` on `rotate(-90, [0, 0, 1])`.
     - `euler_rotation_matrix([x, y, z])`: direct closed-form expression matching OpenSCAD's Eigen composition (preserves `[-1, 0, 0, 0], [-0, -1, 0, 0], [-0, 0, 1, 0]` on `rotate([0, 0, 180])`).
- **Resolved cases**: `csg_2d_invalid`, `csg_color_hex`, `csg_color_invalid`, `csg_cube_invalid`, `csg_mirror`, `csg_polygon`, `csg_polyhedron`, `csg_rotate_axis`, `csg_rotate_invalid`, `csg_rotate_vector`, `csg_scale`, `csg_translate_invalid`.

---

### Commit 4 — `0585ed1`
**`feat(geometry,scad-eval): add structured CSG nodes and export for text, import, and surface`**
*(+5 compat cases: 204/215 → 209/215)*

- **Modified files**:
  - `crates/geometry/src/{csg.rs, csg_text.rs}`
  - `crates/scad-eval/src/{eval.rs, builtins/modules.rs}`
- **Implemented**:
  1. **Typed CSG tree variants (`NodeKind::{Text, Import, Surface}`)**:
     - Designed for **Milestones 3 and 5**: instead of formatting opaque strings into `NodeKind::Other`, resolved parameters (including `Tessellation`, `dpi`, `origin`, `timestamp`, `script`, `direction`) are stored as typed fields in `geometry::csg::NodeKind`, ready for direct consumption by the 2D/3D mesher.
  2. **ISO 15924 Unicode script detection and automatic RTL direction in `text()`**:
     - When `script` is omitted: scans `text` characters to find the first non-Common/Inherited script (`Latn`, `Cyrl`, `Arab`, `Hani`, `Grek`, `Hebr`, `Deva`, `Kana`, `Hang`, etc.); when `text` contains only digits, spaces, or is empty, `script` is omitted from `.csg` export matching OpenSCAD.
     - When `script` is explicitly provided (e.g. `"latin"`, `"arabic"`): normalized to 4-char TitleCase (`"Lati"`, `"Arab"`), matching `hb_script_from_string`.
     - When `direction` is omitted and the resolved script is RTL (`Arab`, `Hebr`, `Syrc`, `Thaa`), `direction` defaults to `"rtl"`, otherwise `"ltr"`.
     - Validates `text` type: `WARNING: text(..., text=...) Invalid type: expected string, found ...`.
  3. **`import()` and `surface()`**:
     - Resolves `timestamp` (Unix mtime in seconds of the path resolved relative to the `.scad` source directory, `0` if missing), emits `dpi` only for `.svg` (default `72`), and matches OpenSCAD's `.csg` formatting.
- **Resolved cases**: `csg_import_surface`, `csg_text`, `csg_text_invalid`, `csg_text_scripts`, `model_text_label`.

---

### Commit 5 — `334c215`
**`feat(scad-eval,scadr): add mt19937 rands, search checks, object(), and vector-swizzle`**
*(+5 compat cases: 209/215 → 214/215)*

- **Modified files**:
  - `crates/scad-eval/src/{value.rs, eval.rs, builtins/functions.rs}`
  - `crates/scadr/src/main.rs`
- **Implemented**:
  1. **`rands()` with 32-bit Mersenne Twister `Mt19937`**:
     - Replaced `splitmix64` stub with exact `std::mt19937` + `std::generate_canonical<double, 53>` (two `u32` draws per `double`: `(r0 + r1 * 2^32) / 2^64`).
     - Added arity check (`expected 3 or 4`), automatic swap when `min > max`, and `i64 as u32` seed truncation.
  2. **`search()`**:
     - Arity check (`expected between 2 and 4`) and warning `Invalid entry in search vector at index ..., required number of values in the entry: ...`.
  3. **`--enable` CLI flag and experimental features (`object-function`, `vector-swizzle`, `import-function`)**:
     - Added `--enable <feature>` / `--enable=<feature>` (including `all`) in `scadr` wired to `EvalOptions`.
     - Calling an experimental built-in (`object`, `has_key`, `import`) without `--enable` emits `WARNING: Experimental builtin function '...' is not enabled` before `WARNING: Ignoring unknown function '...'`.
     - Added `Value::Object(Rc<Vec<(String, Value)>>)` preserving insertion order, `{ a = 1; b = "x"; }` formatting, `obj.key` and `obj["key"]` access, key iteration in `for (k = obj)`, `len(obj)`, `is_object(v)`, `has_key(obj, k)`, and construction/copy/override via `object(...)` (named arguments and `[key, value]` pairs, including key removal with `undef`).
     - Added vector swizzling (`.xyzw` / `.rgba`, 1 to 4 components; 1 component returns scalar, 2..4 return list) when `vector_swizzle` is enabled.
  4. **Clippy cleanup**:
     - Fixed the 3 pre-existing `clippy` warnings in `crates/scad-eval/src/value.rs`.
- **Resolved cases**: `builtin_object_disabled`, `builtin_object_enabled`, `builtin_rands_seeded`, `builtin_search_bad`, `builtin_swizzle_enabled`.

---

### Commit 6 — `6c3e058`
**`feat(scad-eval): implement fontmetrics() and textmetrics() with skrifa and rustybuzz`**
*(+1 compat case: 214/215 → **215/215 — 100.0%**)*

- **Added / modified files**:
  - `crates/scad-eval/Cargo.toml`, `Cargo.lock` (`fontdb` 0.24, `rustybuzz` 0.20, `skrifa` 0.49)
  - `crates/scad-eval/fonts/Liberation-2.00.1/{LICENSE, AUTHORS, ttf/LiberationSans-*.ttf}`
  - `crates/scad-eval/src/builtins/{fonts.rs, functions.rs, mod.rs}`
- **Implemented**:
  1. **Bundled Liberation Sans 2.00.1 fonts (SIL Open Font License 1.1)**:
     - Bundled all 4 styles (`Regular`, `Bold`, `Italic`, `BoldItalic`) alongside `LICENSE` and `AUTHORS` required by SIL OFL 1.1.
     - Shared `fontdb` database (`LazyLock`) loading bundled Liberation Sans first (working anywhere regardless of working directory) followed by system fonts as fallback (e.g. `"Arial"`).
     - Fontconfig-style font specification parser (`"Liberation Sans:style=Bold"`, `"Family1,Family2:Bold Italic"`, weights, styles, and widths).
  2. **FreeType 26.6 + HarfBuzz scaling math**:
     - Reproduced FreeType `FT_Set_Char_Size(face, 100_000, 100_000, 100, 100)` scaling (`dim_26_6 = 138_889`, `ppem = 2170`, `FT_MulFix` with 16.16 signed-magnitude rounding) multiplied by `size / 100_000.0`.
     - TrueType hinting via `skrifa::outline::HintingInstance` at `Size::new(2170.0)` and grid-fitted control box (`FT_GLYPH_BBOX_GRIDFIT`: `floor` on minima, `ceil` on maxima in integer pixels).
     - Full `rustybuzz` shaping (`ltr`, `rtl`, `ttb`, `btt` directions, language, ISO 15924 script, `spacing`, `halign`, `valign`) including vertical metric synthesis from `OS/2` (`sTypoAscender - sTypoDescender`) when a font lacks a `vmtx` table (like Liberation Sans).
     - Type validation for all `fontmetrics()` and `textmetrics()` arguments with matching warnings.
- **Resolved case**: `builtin_textmetrics` (+ extra unit tests in `builtins::fonts::tests` for `"ab"`, `"gp"`, spaces, Bold `size=20`, and vertical `ttb` layout).

---

## 4. Review Notes for Claude

1. **`crates/geometry/src/csg.rs` (`NodeKind::{Text, Import, Surface}`)**:
   - Check that the new `NodeKind` variants fit **Milestone 5** cleanly (where `Text` uses `fontdb` + `skrifa` + `rustybuzz` to extract 2D glyph outlines and pass them to Clipper2 / `CrossSection`). Font discovery and shaping were subsequently extracted into `crates/scad-font` on `milestone-2-improvements`.
2. **`crates/scad-syntax/src/syntax_kind.rs` (`OVERFLOW_NUMBER` as trivia)**:
   - Ensure future CST formatting/refactoring tools do not assume trivia tokens are exclusively whitespace and comments: `OVERFLOW_NUMBER` only occurs on pathological IEEE-754 out-of-range literals (`> 1.797e308`), keeping original source text intact in the CST.
3. **`crates/scad-eval/src/value.rs` (`Value::Object`)**:
   - Objects use `Rc<Vec<(String, Value)>>` to preserve key insertion order (matching OpenSCAD's output) with minimal overhead on typical field counts. If objects with thousands of keys ever matter, `IndexMap` can be introduced alongside it.
