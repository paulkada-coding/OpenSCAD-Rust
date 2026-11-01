# Milestone 4 Review — Branch `m4-customizer`

Handoff and traceability document prepared for **Claude**'s review prior to merging `m4-customizer` into `main`.

---

## 1. Executive Summary

- **Working branch**: `m4-customizer` (branched from `main` at `865b927`; `main` was left untouched and nothing was pushed).
- **Scope delivered**:
  1. Completed the interrupted Italian-to-English repository translation across all documentation, error messages, CLI usage/diagnostic strings, and desktop UI strings (`ddff869`), keeping the user's `.scad` models untouched.
  2. Implemented lossless-CST assignment literal rewriting in `crates/scad-syntax` (`replace_assignment_value` / `replace_assignment_values`, `e5c519f`).
  3. Created the engine-layer `crates/scad-customizer` crate (`9a5ca32`) implementing clean-room OpenSCAD Customizer parameter extraction, range-adjustment warnings, byte-compatible `.param` JSON export, value-to-literal formatting, and `<stem>.json` preset reading/validation/writing, documented in `docs/openscad-behavior.md` §17.
  4. Extended `crates/scadr` with `-o x.param` / `--export-format param` and `-p <file.json> -P <set>`, extended `tools/compat-runner` with `--params`, added a 17-case parameter compatibility corpus under `tests/compat/params/` generated strictly via `--update-expected` against OpenSCAD Nightly 2026.10.22, and added CI coverage (`89a1e30`).
  5. Implemented the desktop Customizer ("Parameters") panel in `crates/app` (`b7dae71`): collapsible `/* [Group] */` tabs, sliders, spinboxes, labeled dropdowns, checkboxes, `maxLength` text boxes, 1..=4D vector controls, `<stem>.json` preset management (`Save` / `Save as…` / `Rename…` / `Delete`), live 100 ms debounced background evaluation, value preservation across disk auto-reloads, per-file `eframe` persistence, **"Apply to file"**, and a user setting in **File → Settings** to position the panel on the right (default recommended) or left.
- **Verification summary**:
  - `cargo fmt --all --check`: **clean** (0 diffs).
  - `cargo clippy --workspace --all-targets -- -D warnings`: **0 warnings**.
  - `cargo test --workspace`: **all unit and integration tests passing**.
  - `cargo run -p compat-runner` (text): **217 / 217 passed (100.0%)**.
  - `cargo run -p compat-runner -- --params`: **17 / 17 passed (100.0%)** (byte-identical `.param` JSON and normalized `.log`).
  - `cargo run -p compat-runner -- --geometry`: **100 cases — 39 exact, 60 equivalent, 1 failed (`lex_scale_vector`)**, zero regressions.
  - **Zero `unsafe`** outside `crates/manifold` (`unsafe_code = "deny"` across workspace lints).
  - **Clean-room**: all OpenSCAD Customizer and preset behaviors were derived exclusively from black-box probes executed with `C:\Program Files\OpenSCAD (Nightly)\openscad.com`.

---

## 2. Quick Review Commands

```powershell
# Atomic commits relative to main
git log --oneline --reverse main..m4-customizer

# Per-commit file stats
git log --stat --reverse main..m4-customizer

# Mandatory verification suite
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p scadr
cargo run -p compat-runner
cargo run -p compat-runner -- --params
cargo run -p compat-runner -- --geometry
```

---

## 3. Commit-by-Commit Breakdown

### Commit 1 — `ddff869`
**`chore: translate documentation, CLI, and app UI to English`**

- Finished translating all remaining Italian documentation (`CLAUDE.md`, `README.md`, `docs/*.md`, `tests/compat/**/README.md`), Rust error strings (`manifold`, `mesh-io`, `mesher`, `render`, `scad-syntax`), `scadr` CLI strings, and `crates/app` UI strings into English.
- Preserved `examples/*.scad` and `tests/compat/**/cases/*.scad` untouched.

### Commit 2 — `e5c519f`
**`feat(scad-syntax): add lossless CST assignment literal rewriting`**

- Added `crates/scad-syntax/src/edit.rs` (`EditError`, `replace_assignment_value`, `replace_assignment_values`):
  - Refuses broken input files (`SourceHasSyntaxErrors`) and validates replacement expressions (`InvalidReplacementLiteral`) before and after splicing so statements or trailing comments can never be injected or corrupted.
  - Targets the last top-level `ASSIGNMENT` matching `name` (consistent with OpenSCAD's last-assignment-wins semantics) while ignoring inner module/function parameters and block-local assignments.
  - Splices strictly the non-trivia token byte range of the value expression, preserving alignment spaces, inline block comments, trailing `// [...]` Customizer annotations, and LF/CRLF line endings.
- Exposed `unescape_string_literal` and `parse_number_literal` for reuse by `scad-customizer`.

### Commit 3 — `9a5ca32`
**`feat(scad-customizer): extract OpenSCAD parameters, format .param JSON, and manage presets`**

- Created `crates/scad-customizer` (no UI dependencies):
  - `extract.rs`: clean-room parameter extractor matching OpenSCAD Nightly:
    - Stops at the first `{` outside `//` and `/* … */` comments (including `{` inside string literals, matching OpenSCAD's scanner).
    - Tracks single-line `/* [Group] */` headers (default `"Parameters"`, case-sensitive `"Hidden"`, unbracketed single-line `/* … */` omitting `"group"`), `line - 1` `//` captions, trailing `[max]`, `[min:max]`, `[min:step:max]`, `[val, …]`, `[val:Label, …]`, bare `// step`, and bare `// maxLength` comments.
    - Emits OpenSCAD-compatible `ParamWarning`s (`below the declared minimum` / `above the declared maximum`) and expands bounds when initial values lie outside declared ranges.
    - Deduplicates reassigned top-level variables (order from first assignment, value/location/annotations from last assignment) and models `-D name=expr` via `extract_parameters_with_defines`.
  - `param_json.rs`: compact single-line `.param` JSON serializer with alphabetically sorted keys, `"parameters"` omitted when empty, no trailing newline, and `-4 <= exponent <= 14` shortest round-trip float formatting.
  - `presets.rs`: `<stem>.json` reader/writer (`fileFormatVersion: "1"`, `parameterSets`, preserving unknown top-level keys, 4-space pretty JSON) and `resolve_preset_overrides` / `coerce_preset_value` (clamping ranged numbers/vectors, filtering invalid enum entries, truncating strings to `maxLength`, enforcing vector arity).
- Documented all observed rules in `docs/openscad-behavior.md` §17.

### Commit 4 — `89a1e30`
**`feat(scadr,compat-runner): support .param export, -p/-P presets, and --params suite`**

- `crates/scadr`: added `ExportFormat::Param` (`-o out.param` / `--export-format param`) and `-p <file.json> -P <set_name>` flags matching OpenSCAD's warning/error ordering, exit codes, and `-D` + `-p/-P` precedence.
- `tools/compat-runner`: added `--params` mode comparing both `expected/<name>.param` (verbatim) and `expected/<name>.log` (normalized).
- `tests/compat/params/`: added 17 `.scad` cases (covering empty files, scalars, groups/Hidden, captions, ranges, out-of-range warnings, numeric/string enums, vectors 1..=4, `{` cut-off, reassignments, float formatting, `-D`, `-p/-P`, `model_customizer_params`, `model_porta_carte`, and `model_separatore`), with `expected/*` generated solely via `cargo run -p compat-runner -- --params --update-expected`.
- Added CLI integration tests in `crates/scadr/tests/cli.rs` and enabled `--params` in `.github/workflows/ci.yml`.

### Commit 5 — `b7dae71`
**`feat(app): add Customizer panel, presets manager, and configurable panel side`**

- `crates/app/src/settings.rs`: added `PanelSide::{Right, Left}` (`Right` default recommended, per user request) and **Layout → Parameters panel position** in `File → Settings`.
- `crates/app/src/worker.rs`: threaded `defines: Vec<(String, String)>` into `Worker::submit` and `EvalOptions`.
- `crates/app/src/customizer.rs` & `crates/app/src/app.rs`:
  - **View → Parameters** toggle; panel rendered on the configured side whenever a `.scad` file is open.
  - Presets bar (`<default>` + sets from `<stem>.json`, **Save**, **Save as…**, **Rename…**, **Delete**, plus automatic refresh when `<stem>.json` changes externally).
  - Collapsible group headers (`/* [Group] */`), sliders/spinboxes with IEEE-754 step-precision rounding (`-0.25 + 11 * 0.05` → `0.3`, never `0.30000000000000004`), labeled enum `ComboBox`es, boolean checkboxes, `maxLength` text fields, and 1..=4D vector drag boxes.
  - Immediate evaluation on discrete controls (checkboxes, dropdowns, presets, resets) and 100 ms live periodic debounce while dragging sliders/spinboxes or typing (`ctx.request_repaint_after` wakes `FrameRate::OnDemand` cleanly).
  - Preserves modified parameter values across disk auto-reloads when name + type remain compatible, and persists only user-modified overrides per file in `eframe` storage so untouched parameters always track external edits to the `.scad` file.
  - **Apply to file** button rewrites modified literals into the `.scad` file via `scad_syntax::replace_assignment_values` and updates watched dependency timestamps without causing a redundant reload.

---

## 4. Performance Measurements & Caching Decision (§2.6)

Measured end-to-end (`eval` + Manifold `mesh`) on the user's real models (RTX 5080 / Windows 11):

| Model | Mode | Debug (`opt-level = 1`, deps `3`) | Release (`lto = "thin"`) |
|---|---|---|---|
| `examples/separatore.scad` | Default (`FINESTRA = "x"`, embossed text) | **39 ms** | **39 ms** |
| `examples/porta_carte.scad` | `PEZZO = "stampa"` (box + lid side by side) | **85 ms** | **87 ms** |
| `examples/porta_carte.scad` | `PEZZO = "taratura"` (6 calibration rings + text) | **315 ms** | **320 ms** |

*(Debug and release times are nearly identical because C++ Manifold is built in `Release` mode in both profiles and workspace dependencies already use `opt-level = 3` in `dev`.)*

**Decision on CSG subtree caching**:
- Evaluation itself takes ~1 ms, and typical interactive configurations (`separatore`, `porta_carte` box/lid/print) complete in **39–85 ms**, well within the 100 ms slider debounce window.
- Even in the heaviest calibration gauge mode (`taratura`, ~315 ms), the worker thread drains superseded slider positions (`try_recv`) and cancels in-flight Manifold work via `RenderOptions::cancel`, keeping the UI at full monitor refresh rate without stalling.
- Introducing a CSG subtree mesh cache in `mesher` was therefore deferred: a single-parameter slider change typically invalidates the root boolean operands anyway, so subtree caching is better designed alongside BOSL2/library profiling in Milestone 6 if multi-part assemblies show clear cache-hit ratios.
