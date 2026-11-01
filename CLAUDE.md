# Project: native OpenSCAD engine and app in Rust (OpenSCAD-Rust, final name TBD)

Desktop app to open, parametrize, display (GPU) and export to STL/3MF OpenSCAD projects,
built on an **OpenSCAD interpreter written from scratch in Rust** (it does not use the OpenSCAD
binary at runtime). Prepared for direct 3D modelling and an integrated AI assistant
(Claude / Gemini).

Repository: https://github.com/capriatiivan-prog/OpenSCAD-Rust (branch `main`).

> This file is maintained by Claude: update it at every decision or milestone
> ("Decisions", "Status" and "Roadmap" sections).

## User and way of working

- The user speaks Italian: **talk to the user in Italian**.
- **Everything in the repository is in English**: code, identifiers, comments, documentation,
  UI strings, commit messages.
- The user does not know Rust: Claude owns all the source code. Explain choices in plain terms,
  ask for confirmation only on product decisions.
- Use parallel agents when there are independent parts (separate crates, research, tests).
- At every milestone the user tries the app and provides real `.scad` files as test cases.

## Decisions

| Date | Decision | Reason |
|---|---|---|
| 2026-10-31 | Option B: native OpenSCAD interpreter in Rust | Geometry→code source mapping, incremental rendering, lossless code editing (modelling and AI), a single executable |
| 2026-10-31 | Windows only for now; code portable to macOS/Linux | Keep Windows-specific APIs in isolated modules |
| 2026-10-31 | Visual Studio Build Tools **2022** (not 2026) | Best compatibility with the Rust/CMake ecosystem; 2026 brings no benefit for us |
| 2026-10-31 | Official OpenSCAD (Nightly) used **only as a test oracle** | Automatic comparison of results to measure compatibility |
| 2026-10-31 | **Constant** frame rate by default (Settings → Display), "on demand" optional | With G-Sync/VRR, irregular frames change the monitor's refresh rate and the user's OLED panels flicker |
| 2026-11-01 | License **GPL-3.0-or-later** (user's choice); README, GitHub Actions CI (Windows) | Publishing on GitHub; bundled parts (Manifold Apache-2.0, Clipper2 BSL-1.0, OFL fonts) are compatible |
| 2026-11-01 | All repository content in English, including the app UI | Public repository; an Italian UI translation can come later through i18n |
| 2026-11-01 | Model → code picking with a **CPU raycast** (not a GPU ID buffer); selection drawn in **blue** | Raycast: simple, testable without a GPU, reusable by direct modelling and the AI, < 1 ms on the user's models. Blue: the model is already OpenSCAD yellow, back faces green, `#` red |

## Stack

| Area | Technology |
|---|---|
| Lexer / parser | `logos` + hand-written recursive parser + `rowan` (lossless CST, error recovery) |
| Interpreter | tree-walking over a typed AST (bytecode later), `stacker` for deep recursion |
| 3D CSG kernel | Manifold (C++, Apache-2.0) via Rust bindings |
| 2D | Manifold CrossSection (Clipper2) |
| Fonts (`text()`) | `skrifa` + `rustybuzz` + `fontdb`, bundled Liberation Sans (crate `scad-font`) |
| Rendering | `wgpu` (DX12/Vulkan/Metal) + WGSL shaders; `HighPerformance` adapter (PC with RTX 5080 + AMD iGPU) |
| UI | `egui` / `eframe` + `egui_dock` |
| Export | STL (binary/ASCII), 3MF, OBJ; SVG/DXF for 2D |
| AI | `reqwest` + `tokio`, `LlmProvider` trait (Claude, Gemini), tool use + viewport screenshots |
| Tests | `insta`, `cargo-fuzz`, `compat-runner` against OpenSCAD |

License constraint: do not copy OpenSCAD source code (GPL). Clean-room implementation based on
documentation and observed behavior.

## Architecture (Cargo workspace)

```
crates/
  scad-syntax/     logos lexer, recursive parser, lossless rowan CST, AST, lexical diagnostics, surgical CST edits
  scad-customizer/ OpenSCAD Customizer parameter extraction, `.param` JSON export, value-to-literal formatting, `<stem>.json` presets
  scad-font/       font discovery (`fontdb` + Liberation Sans SIL OFL), `rustybuzz` shaping, `skrifa` metrics and 2D outlines
  scad-eval/       interpreter, lexical scopes + dynamic `$` variables, cross-run `SourceCache`, `Value::Object`, builtins
  scadr/           OpenSCAD-compatible CLI (`-o x.csg|x.echo|x.param|x.stl|x.off`, `-p`, `-P`, `-L`, `-D`, `--enable=...`, exit codes and diagnostics)
  manifold/        vendored Manifold v3.5.4 + C++ no-exception shim; triangulation, original IDs, integer Clipper2
  geometry/        indexed f64 mesh, Aabb, volume/area, sharp edges, CSG tree (`Node` with `source` + `root_call`) and `.csg` export
  mesher/          CSG tree → meshes with Manifold: OpenSCAD-identical tessellation, 2D on a 2^-27 grid, extrusions, text, import, triangle→source provenance with the chain of calls
  mesh-io/         STL (binary/ASCII, OpenSCAD format) and OFF, reading and writing; later 3MF/OBJ, SVG/DXF import
  render/          offscreen wgpu Scene (MSAA 4x, sRGB), Z-up OrbitCamera, picking rays, selection highlight, headless rendering → PNG
  ai/              (to do) Claude/Gemini providers, tools
  app/             egui UI: menu, model panel, click on the model → code (selection), Customizer ("Parameters") panel + presets, viewport, status bar, persistent settings
tools/
  compat-runner/   automatic parallel comparison with OpenSCAD Nightly's expected outputs (text, `--params`, `--geometry`)
examples/          sample .scad files (+ STL generated with OpenSCAD)
tests/compat/      text suite (218 `.csg`/`.log` cases) + `params/` (17 `.param`/`.log` cases) + `geometry/` (100 cases with OpenSCAD mesh metrics)
```

Rendering notes:
- The scene is drawn into an offscreen texture (`Rgba8Unorm`, written through an
  `Rgba8UnormSrgb` view), then shown by egui as a native texture. The same path produces
  headless PNGs, which will also be the AI's "eyes".
- Redraw only when something changes (camera, size, style, mesh), unless the constant frame
  rate setting is on.
- Flat shading (per-face normal), back faces in green (like OpenSCAD) to reveal defects, sharp
  edges > 25° drawn as lines.
- glam 0.34: projections live in `glam::camera::rh::{view, proj::directx}` (no longer on `Mat4`).
- egui 0.37: `App::ui(&mut self, ui, frame)`, panels with `egui::Panel::{top,left,…}`.

Principles:
- The engine (`scad-*`, `geometry`, `mesher`, `mesh-io`) does not depend on the UI.
- Every geometric node carries the span of the source code that produced it.
- Automatic code edits (modelling, AI) go through the lossless CST.

## Roadmap

1. [x] **Foundations** — toolchain, workspace, window with a wgpu viewer that loads/rotates an STL
2. [x] **Language** — parser, expressions, functions, modules, `echo`, tests vs OpenSCAD (**215/215 — 100%**)
3. [x] **First render** — 3D primitives, transformations, Manifold booleans, STL export (**99/100 geometry cases**)
4. [x] **Customizer** — parameters from `// [min:step:max]` comments, `.param` export (**17/17 — 100%**), sliders, `<stem>.json` presets (`-p`/`-P`), live debounced rendering, CST "Apply to file" (merged 2026-11-01)
5. [ ] **2D and advanced** — done in M3: extrude, offset, hull, minkowski, text, STL/OFF import, `.dat` surface. Missing: SVG/DXF/3MF/OBJ import, PNG surface, `linear_extrude` subdivision rule with non-uniform scale, "octa" sphere with `$fn > 12`
6. [ ] **Compatibility** — MCAD, BOSL2, corpus of real models
7. [ ] **AI** — Claude/Gemini panel with tool use
8. [ ] **Direct modelling**

## Status

- 2026-10-31: prerequisites installed and verified.
- 2026-10-31: **milestone 1 done.** egui app with wgpu viewport (RTX 5080 via Vulkan), STL
  opening (menu, drag & drop, command-line argument), binary/ASCII STL export, preset views,
  perspective/orthographic, grid/axes/edges, model info. Verified: headless rendering and GUI
  screenshot. To try by hand: mouse interaction.
- 2026-10-31: **milestone 2 done on branch `milestone-2-complete` (pending review).**
  Active crates: `scad-syntax` (logos lexer, lossless rowan parser, AST, lexical diagnostics),
  `scad-eval` (complete interpreter: lexical scopes + dynamic `$` variables, tail calls, list
  comprehensions, include/use, children, builtins, `Mt19937`, `Value::Object`, `.xyzw`/`.rgba`
  swizzles, `fontmetrics()`/`textmetrics()` with `skrifa` + `rustybuzz` + `fontdb` and the
  Liberation Sans SIL OFL font), `scadr` (CLI `scadr -o x.csg f.scad`, `--enable=...`, `-D`),
  `manifold` (Manifold v3.5.4 + C++ shim, 24 tests; integrated in M3), `tools/compat-runner` +
  `tests/compat`. Compatibility: **215/215 cases (100.0%)**, zero `clippy` warnings, zero
  `unsafe` in the language/interpreter crates. Detailed review report in
  `docs/milestone2-review.md`.
- 2026-10-31: **architectural improvements on branch `milestone-2-improvements`** (from
  `milestone-2-complete`): extracted the shared `scad-font` crate (with flattened 2D outline
  extraction ready for M5), bidirectional source mapping on `geometry::csg::Node` (`source` +
  `root_call` + `find_at_offset` + `Evaluator::resolve_source`), `SourceCache` reusable across
  runs with automatic invalidation on disk changes, standard library paths (`OPENSCADPATH` +
  `Documents/OpenSCAD/libraries`), `-L`, `-D var=expr` flags and `.echo` export in `scadr`,
  property/fuzz tests on live typing and uniform `cargo fmt --all` across the workspace. Review
  guide in `docs/milestone2-improvements-review.md`.
- 2026-10-31: **Claude's review done, both branches merged into `main`** (fast-forward).
  Independently verified: 215/215 cases, clean tests/clippy/fmt, no new `unsafe`, `tests/compat`
  and the runner's normalization untouched (genuine result). Fix applied: `-D` overrides are
  parsed once at startup as virtual sources (`-D name`), an invalid `-D` produces
  `WARNING: Ignoring invalid definition` instead of being silently ignored, and messages raised
  inside a `-D` point to the `-D` and not to the main file. Next step: milestone 3.
- 2026-10-31: user testing. Their `examples/porta_carte.scad` and `examples/separatore.scad`
  (a real project: offset, hull, linear_extrude, text+resize) produce output identical to
  OpenSCAD; added to the corpus as `model_porta_carte` / `model_separatore` (**217/217**).
  Reported flicker on an OLED monitor with G-Sync: added a **File → Settings** window with a
  constant (default) / on-demand "Frame rate", saved with eframe persistence in
  `%APPDATA%\openscad-rust\data\app.ron` (together with window and panels). Verified: when idle
  "constant" redraws continuously, "on demand" uses 0 CPU; loading and saving work. The flicker
  on the user's monitor is still to be confirmed by them.
- 2026-11-01: **milestone 3 done** (three parallel tracks, branches `m3-mesher` and `m3-export`
  in worktrees `C:\Software\wt-*`, merged into `main`).
  - `mesher`: vertex-for-vertex OpenSCAD-identical tessellation (primitives, 2D, extrusions,
    text, surface; face triangulation like OpenSCAD's tessellator), batched Manifold booleans,
    `!`/`#`/`%`, triangle→node provenance, cancellation, diagnostics with locations. Rules
    derived from the oracle are documented in the comments.
  - `scadr -o x.stl|x.off` with OpenSCAD's messages and exit codes; `stl` = ASCII like OpenSCAD.
  - Geometry harness `compat-runner --geometry`: **100 cases, 39 identical, 60 equivalent,
    1 failed** (`lex_scale_vector`: area +0.01%, subdivision rule with non-uniform scale not
    found).
  - App: opens `.scad` (also drag & drop), evaluation in a worker thread with cancellation,
    colored console, automatic reload when the file or its dependencies change, F5, framing only
    on first load, last valid geometry kept on error, `#` in transparent red and `%` in
    transparent grey, 2D results shown but not exportable.
  - Verified in the app with `porta_carte` (also in assembly mode with `%`) and `separatore`:
    correct geometry, 5636 triangles like OpenSCAD, 40–80 ms.
- 2026-11-01: cleanup. Removed the `C:\Software\wt-*` worktrees, the merged branches
  (`m3-*`, `milestone-2-*`), the agents' build folders and the contents of `out/`.
  **Detailed next steps in `docs/next-steps.md`** (self-contained brief for other agents,
  priority: milestone 4 Customizer).
- 2026-11-01: **published on GitHub** with README, GPL-3.0-or-later license, CI (fmt, clippy,
  tests, compatibility suites on `windows-latest`). All repository content translated to
  English, app UI included.
- 2026-11-01: **milestone 4 (Customizer) done on branch `m4-customizer`** (by Argon Gemini).
  Finished the repository-wide English translation (`ddff869`), added surgical rowan CST assignment
  literal rewriting (`scad_syntax::replace_assignment_value` / `replace_assignment_values`),
  created `scad-customizer` (OpenSCAD-compatible parameter extraction, range-adjustment warnings,
  `.param` JSON export, and `<stem>.json` presets), added `-o x.param` and `-p`/`-P` to `scadr`,
  added `compat-runner --params` + `tests/compat/params/` (**17/17 — 100%**), and built the
  desktop Customizer panel in `app` (tabs, sliders, spinboxes, dropdowns, checkboxes, text,
  1..=4D vectors, presets Save/Save as/Rename/Delete, 100 ms live debounce, reload preservation,
  per-file `eframe` persistence, "Apply to file", and configurable Right/Left panel side in
  Settings). Detailed review report in `docs/m4-customizer-review.md`.
- 2026-11-01: **Claude's review of `m4-customizer` done, merged into `main`.** Independently
  verified: fmt/clippy/tests clean, 217/217 text, 17/17 params (the expected `.param`/`.log`
  files were regenerated from OpenSCAD and are identical: genuine), 99/100 geometry, `-p`/`-P`
  STL identical to OpenSCAD's, panel checked in the app with `porta_carte`. Fixes applied (none
  of them lost data in tests, all could have for the user):
  - presets: an unreadable `<stem>.json` is reported and never overwritten (it used to be
    replaced by an empty file on "Save as"); numeric `fileFormatVersion` accepted; rename and
    "Save as" never replace another preset; delete asks for confirmation;
  - user files written atomically (`scad_customizer::write_file_atomic`);
  - parameters and modified values kept while the file has syntax errors;
  - "Apply to file" refuses to replace a value that changed on disk since it was read, and the
    model is re-rendered after applying;
  - number controls: no rounding of unannotated numbers (`wall = 1.2` stayed `1`), slider
    precision also from min/max/default, non-finite input ignored;
  - parser: a leading UTF-8 byte order mark is accepted like OpenSCAD (new case
    `lex_utf8_bom`, **218/218**).

- 2026-11-01: **click on the model → code (next-steps §3.1) on branch `claude/vibrant-rubin-sc5ljg`**
  (developed in a Linux cloud session).
  - `mesher`: `RenderOutput::source_paths`, the chain of nested instantiations behind every
    traced source (top-level statement › module calls › transformations › primitive); the same
    code reached through different calls gets separate entries.
  - `geometry`: `Mesh::raycast` (two-sided Möller–Trumbore, nearest hit) and `Aabb::hit_by_ray`.
  - `render`: `OrbitCamera::ray` (perspective and orthographic, checked against the projection
    matrix), `Scene::pick_ray`, `Scene::set_selection` (blue layer drawn over the model's own
    faces, under the edges); GPU test `crates/render/tests/selection.rs` (skipped without an
    adapter).
  - `app`: new `selection` module. A click selects the primitive under the cursor; the model
    panel shows the chain of calls (click a link to select a whole module call), file, lines,
    code excerpt and highlighted triangle count; every triangle produced through the selected
    code is highlighted. Esc or a click beside the model clears it. The selection survives
    re-renders of unchanged code (e.g. Customizer changes). STL files: a message explains that
    they carry no code.
  - Verified: fmt/clippy/tests clean, 218/218 text, 17/17 params, geometry unchanged (39/60/1);
    the app was driven under Xvfb with Mesa lavapipe on `porta_carte.scad` (selection, chain
    levels, Customizer change keeping the selection, Esc, click beside the model). To be tried by
    the user on Windows.

## Notes for the next milestones

- `SourceCache` uses `Rc<RefCell>` and is not `Send`: in the app evaluation runs in a dedicated
  worker thread that owns the cache (do not share it across threads).
- `SourceCache` covers `include`/`use`; the main file is always re-parsed (in the app it will
  come from the editor buffer, not from disk).
- `text()` is already meshed by the mesher through `scad-font` (vertices identical to OpenSCAD).
- `RenderOutput::triangle_sources` + `sources` + `source_paths` give the provenance of every
  triangle; the app turns them into `selection::Provenance` in the worker (where the source texts
  live). Planar (2D) results are not traced yet, so they cannot be picked.
- The workspace also builds and tests on Linux (cloud sessions): `apt-get install
  mesa-vulkan-drivers` gives a software Vulkan adapter (lavapipe) for the GPU tests, and the app
  runs under `Xvfb` (needs `libxkbcommon-x11-0`; `xdotool` drives it).
- The mesher is not incremental: every reload recomputes everything (40–80 ms on the user's
  models). M4 measured 39–85 ms per change (315 ms for the `taratura` mode): a per-subtree cache is not needed yet.
- `-D` overrides are the basis for Customizer values (M4): same semantics as OpenSCAD (an
  assignment appended at the end of the file → replaces the expression at its original
  position).
- The user's models (`examples/*.scad` and their copies in `tests/compat`) keep their Italian
  names and comments: they are the user's files and test data, not documentation.

## Commands

```powershell
cargo run -p app                                   # start the app
cargo run -p app -- examples\supporto.stl          # open a file
cargo run -p app -- --render-png in.stl out\x.png 1280x800   # snapshot without a window
cargo test --workspace
cargo build -p scadr; cargo run -p compat-runner                 # text suite vs OpenSCAD
cargo run -p compat-runner -- --params                           # Customizer .param suite vs OpenSCAD
cargo run -p compat-runner -- --geometry -v                      # geometry suite vs OpenSCAD
.\target\debug\scadr.exe -o out\x.stl in.scad          # STL export (ASCII like OpenSCAD; binstl for binary)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
& 'C:\Program Files\OpenSCAD (Nightly)\openscad.exe' --backend=manifold -o out.stl in.scad  # oracle
```

The `out/` folder is ignored by git: use it for temporary output.

## Development environment

- Windows 11 Pro, PowerShell 5.1 shell.
- GPU: NVIDIA GeForce RTX 5080 + integrated AMD Radeon.
- Rust 1.99.1 stable, `x86_64-pc-windows-msvc` toolchain (clippy, rustfmt included).
- VS Build Tools 2022 in `C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools`.
- Git 2.56, CMake 4.4.4 (winget package, on the user PATH). No `gh` CLI; GitHub Desktop is
  installed.
- LLVM in `C:\Program Files\LLVM` (not on the PATH); user `LIBCLANG_PATH` =
  `C:\Program Files\LLVM\bin` (currently unused: no crate uses bindgen).
- OpenSCAD Nightly 2026.10.22 (test oracle):
  `C:\Program Files\OpenSCAD (Nightly)\openscad.exe` (not on the PATH).
  An old OpenSCAD 2021.01 also exists in `C:\Program Files\OpenSCAD` (preinstalled, do not use).
- Shells opened before an installation do not see the new PATH: reopen them or reload
  `$env:Path` from the registry.
