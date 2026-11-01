# Next steps — brief for whoever continues the work

Self-contained document: the reader needs no context beyond the repository.
Updated on 2026-11-01, after milestone 3 (commit `8cde69a` on `main`).

---

## 0. Before you start

### Required reading
1. `CLAUDE.md`: project goal, decisions, stack, architecture, status, commands, environment.
2. `docs/openscad-behavior.md`: observed behavior of OpenSCAD (messages, numbers, CSG,
   export). It is the project's "specification".
3. `tests/compat/README.md` and `tests/compat/geometry/README.md`: how the test benches work.
4. The code of the crate you are working on (each crate has a description at the top of `lib.rs`).

### Project rules (non-negotiable)
- **Clean-room.** Never read or copy the OpenSCAD C++ source (GPL). Behavior is derived only by
  running OpenSCAD Nightly (`C:\Program Files\OpenSCAD (Nightly)\openscad.exe`,
  with `--backend=manifold`) and from the public manual. The derived rules must be documented in
  comments and, if general, in `docs/openscad-behavior.md`.
- **No `unsafe`** outside `crates/manifold` (the workspace has `unsafe_code = "deny"`).
- **Never edit by hand** the expected files in `tests/compat/expected` and `tests/compat/geometry/expected`.
  They are regenerated only from OpenSCAD with `--update-expected`, and only for new cases.
- **The engine does not depend on the UI**: `scad-*`, `geometry`, `mesher`, `mesh-io` must not
  depend on `app`, `render` or `egui`.
- **Language**: all repository content (code, comments, identifiers, documentation, UI strings,
  commit messages) is in English; only the conversation with the user is in Italian. Messages that
  imitate OpenSCAD keep its exact English.
- **Never lose user data.** Write user files (`.scad`, `<stem>.json`, exports) atomically
  (temporary file + rename, `scad_customizer::write_file_atomic`), never overwrite a file that could
  not be parsed, never rewrite a value that changed on disk since it was read, and ask before
  replacing or deleting something the user named.
- **Style**: imitate the surrounding code (comment density, names, idioms). `cargo fmt --all`.

### Mandatory checks before every commit
```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets          # must report 0 warnings
cargo test --workspace
cargo build -p scadr
cargo run -p compat-runner                      # text suite: must stay at 218/218
cargo run -p compat-runner -- --geometry        # geometry suite: today 39 exact, 60 equivalent, 1 failed
```
The geometry suite must not get worse: the only case failing today is `lex_scale_vector`
(see §3.4). An EXACT case that becomes EQUIVALENT is a regression that must be explained.

### Practical notes on the environment (Windows 11, PowerShell 5.1)
- Shells opened before the tools were installed do not see the PATH: start commands
  with `$env:Path = [Environment]::GetEnvironmentVariable('Path','Machine') + ';' + [Environment]::GetEnvironmentVariable('Path','User');`
- **Python is not installed**: use the editing tools to modify files, not Python scripts.
- **Multi-line commit messages**: PowerShell 5.1 mangles `-m` with quotes; write the
  message to a file and use `git commit -F file`. End every message with
  `Co-Authored-By: <model> <address>` as in the existing commits.
- PowerShell 5.1 `Set-Content`/`Out-File` add a UTF-8 BOM: for files read by other
  programs (RON, JSON) use `[IO.File]::WriteAllText`.
- Temporary files go in `out\` (ignored by git).
- The GPU is an RTX 5080 (plus an integrated AMD one); the app uses Vulkan on the NVIDIA.

### Delivery workflow (it has worked well so far)
- Work on a dedicated **branch** (e.g. `m5-model-to-code`), with small, atomic commits and
  conventional messages (`feat(crate): …`, `fix(crate): …`).
- At the end, write `docs/<branch>-review.md` with: summary, verification results,
  commit-by-commit detail (what and why), points of attention for the review.
- Do not push and do not merge into `main`: Claude does the review and the merge.
- Update the "Status" section of `CLAUDE.md` with a dated entry.

---

## 1. Current status in brief

| Area | Status |
|---|---|
| Language (parser + interpreter) | Complete, 218/218 cases identical to OpenSCAD |
| Geometry (`crates/mesher`) | 99/100 cases equivalent to OpenSCAD, 39 vertex for vertex |
| `scadr` CLI | `-o x.csg/.echo/.stl/.off/.param`, `-p`/`-P` presets, `-D`, `-L`, `--enable`, `--export-format`; OpenSCAD messages and exit codes |
| App | Opens `.scad`/`.stl`, background computation, console, automatic reload, transparent `#`/`%`, STL export, persistent settings (constant frame rate for OLED/G-Sync), Customizer panel with presets and "Apply to file" |
| Customizer | 17/17 `.param` cases identical to OpenSCAD; `scad-customizer` crate |
| Missing | Integrated editor, model→code click, SVG/DXF/3MF/OBJ import, 3MF export, verified BOSL2 compatibility, AI |

To be confirmed by the user (cannot be verified automatically on this machine, simulated clicks
do not reach the window): mouse interaction in the viewport, export from the menu, absence of
flicker on their OLED monitor with the constant frame rate.

---

## 2. Milestone 4 — Customizer (done, merged into `main` on 2026-11-01)

Kept as a record of the specification; see `docs/m4-customizer-review.md` for what was built.
The next work is the backlog in §3, starting from §3.1.

Goal for the user: open a `.scad`, see a panel with the project's parameters
(sliders, checkboxes, drop-down menus), move them and see the model update immediately; save and
recall combinations of values (presets) compatible with OpenSCAD.

The real test files are `examples/porta_carte.scad` and `examples/separatore.scad`: they use tabs
(`/* [Pezzo] */`, `/* [Hidden] */`), drop-down menus (`// [stampa, scatola, …]`), ranges
(`// [30:1:270]`, `// [-0.25:0.05:0.6]`), booleans and strings, and descriptive comments on the
previous line. They must work perfectly.

### 2.1 Parameter extraction (engine, no UI)
- New module (in `scad-syntax` or a new crate `scad-customizer`) that, from the **lossless CST**,
  extracts the parameters of the main file: name, default value, type, constraints, description,
  tab (group), line/span of the assignment.
- Rules to be derived **from the oracle**: OpenSCAD exports parameters with
  `openscad -o out.param file.scad` (`param` format, JSON). Use it to establish precisely:
  - which assignments are parameters (top-level only, literal value only, up to where in the
    file, effect of `/* [Hidden] */` and of tabs);
  - syntax of the constraints in the trailing comment: `// [min:max]`, `// [min:step:max]`, `// [a, b, c]`,
    `// [10:Piccolo, 20:Grande]` (value:label), maximum string length, step only;
  - description from the comment on the previous line;
  - types: number, boolean, string, vector of numbers (up to 4 elements).
- **Compatibility tests**: add a corpus (e.g. `tests/compat/params/`) with `.scad` cases
  and the expected `.param` generated by OpenSCAD, and a `--params` mode in `tools/compat-runner`
  that compares the JSON we produce (add the `-o x.param` export to `scadr`).
  Include the user's two files.

### 2.2 Applying the values
- Use `EvalOptions::defines` (`crates/scad-eval/src/eval.rs`): it already has the semantics of
  OpenSCAD's `-D` (it replaces the expression at the original position, without a reassignment
  warning; invalid `-D`s produce a warning). A function is needed that formats a Customizer value
  as an OpenSCAD literal (escaped strings, numbers with enough digits not to lose
  precision, vectors, booleans).

### 2.3 OpenSCAD-compatible presets
- OpenSCAD JSON format (`<file_name>.json` next to the `.scad`):
  `{"parameterSets": {"name": {"param": "value"}}, "fileFormatVersion": "1"}` (verify with
  the oracle: `openscad -p preset.json -P name -o x.csg file.scad`).
- Read, select, save, rename and delete sets, preserving existing sets and keys.
- `scadr` must support `-p file.json -P name` like OpenSCAD, with comparison tests.

### 2.4 User interface (`app` crate)
- "Parameters" panel (on the right, or in place of the Model panel when the file has parameters):
  collapsible groups per tab, one control per type:
  - number with a range → slider + numeric field (respecting step and decimals);
  - number without a range → numeric field with dragging;
  - list of values → drop-down menu (with labels if present);
  - boolean → checkbox; string → text field (with maximum length); vector → 2–4 fields.
- Description as a tooltip or as text below the control; button to reset to the default (per
  parameter and global); indication of modified parameters.
- Live update: recompute with *debounce* (~100–150 ms) while the slider moves; the
  worker (`crates/app/src/worker.rs`) already cancels the previous computation. Keep the
  camera still (already the case) and show the computation time.
- Presets: drop-down menu of the sets + Save / Save as / Delete.
- When the file on disk changes (automatic reload), keep the modified values if the
  parameter still exists with the same type.
- Persistence: remember the last used values per file (in eframe storage, like the
  settings in `crates/app/src/settings.rs`).

### 2.5 Writing values back to the file (optional but important for the future)
- "Apply to file" command: rewrites only the assignment's literal in the source using the
  rowan CST (comments and formatting intact). It is the same operation that the AI assistant
  and direct modeling will need: implement it as a reusable function in `scad-syntax`
  (e.g. `replace_assignment_value(text, name, new_literal) -> Result<String, _>`) with tests.

### 2.6 Performance
- Evaluation ~1 ms and geometry 40–80 ms on the user's models (debug build): enough for the
  live slider. Measure in release too. If a real model exceeds ~200 ms, consider a
  cache of results per CSG subtree (node hash → mesh) in the mesher; document the
  choice before implementing it.

### 2.7 M4 acceptance criteria
- Parameter corpus: all cases identical to OpenSCAD; presets: `-p/-P` identical to OpenSCAD.
- In the user's files all parameters appear with the right tabs; changing `PEZZO`,
  `N_FILE`, `LUNGHEZZA`, `SERRAGGIO`, `TESTO`, `FINESTRA` updates the model and it matches
  the OpenSCAD export with the same `-D`s (verifiable with `compat-runner --geometry` and the
  `// compat-args:` directive).
- No regressions in the checks of §0.

---

## 3. Next work (in order of value for the user)

### 3.1 Click on the model → code
- The mesher already provides `RenderOutput::triangle_sources` + `sources` (provenance of every
  triangle, including through booleans) and `Node::source`/`root_call`; the interpreter has
  `Evaluator::resolve_source`.
- *Picking* is needed: an ID buffer in the renderer (`crates/render/src/scene.rs`, a second color
  target with the triangle index) or a CPU raycast. On click, show file, line and
  snippet; highlight in yellow all the triangles from the same source.

### 3.2 Integrated editor
- Editor panel with syntax highlighting based on our lexer (`scad-syntax`),
  line numbers, underlined parse errors, Ctrl+S that saves and triggers a recompute.
  Connect it to §3.1 (click on the model → cursor on the line). The engine must be able to evaluate the
  text of the unsaved buffer (today the worker reads from disk).

### 3.3 3MF (and OBJ) export
- 3MF is the format preferred by modern slicers (units in mm, multiple objects, colors). Writing
  in `crates/mesh-io`, menu in the app, `-o x.3mf` in `scadr` with OpenSCAD's messages.

### 3.4 Known geometry gaps
- `linear_extrude` with non-uniform scale and no twist: OpenSCAD subdivides the sides with a rule
  not yet found (case `lex_scale_vector`, area +0.01%).
- Sphere `style="octa"` with `$fn > 12`: inner points off by ~0.05.
- `linear_extrude(segments=…)` only partially handled; `import(center/scale)` ignored for 3D files.
- SVG / DXF import (crates `usvg`, `dxf`), 3MF / OBJ; `surface()` from PNG.
- Every fix must come with cases in `tests/compat/geometry` (generated by OpenSCAD).

### 3.5 Milestone 6 — compatibility with real libraries
- Install BOSL2 and MCAD in a library folder, write a corpus of examples that
  use them, compare text and geometry with OpenSCAD. Measure interpreter performance
  on BOSL2 (much heavier than the user's files); if needed, profile before optimizing.

### 3.6 Distribution
- Release build with icon, `windows_subsystem` (already set in release), installer (e.g.
  `cargo-packager`), final product name (the user's decision).

### 3.7 Milestones 7–8 — AI and direct modeling
- Postponed. The foundations are ready: lossless CST for surgical code edits, headless
  PNG rendering (`render::render_image`) as the model's "eyes", triangle→source
  provenance. They require product decisions from the user before starting.
