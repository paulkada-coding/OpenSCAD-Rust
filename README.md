# OpenSCAD-Rust

A desktop app and an **OpenSCAD engine written from scratch in Rust**: it opens `.scad` files,
evaluates them with a native interpreter, displays the model on the GPU and exports STL.
OpenSCAD does not need to be installed: the official binary is only used as a reference in the
compatibility tests.

![The porta_carte.scad model rendered by the app](docs/images/porta_carte.png)

> **Status: early development (0.1).** The language, 3D rendering and the Customizer work and
> are verified automatically against OpenSCAD. Final name, editor and AI assistant are still to
> come (see [Roadmap](#roadmap)).

## Why

- **Native interpreter**, not a wrapper around OpenSCAD: every triangle knows which line of code
  it comes from, code can be edited without losing its formatting (the basis for direct
  modelling and an AI assistant), and everything ships as a single executable.
- **OpenSCAD compatible**: messages, numbers and geometry are compared automatically with
  OpenSCAD Nightly on hundreds of cases.
- **Fast**: booleans with the [Manifold](https://github.com/elalish/manifold) kernel, rendering
  with `wgpu` (Vulkan/DirectX 12), background evaluation with cancellation.

## What works

**Language.** Expressions, functions and modules, `let`, list comprehensions, deep recursion,
`include`/`use`, `children()`, special variables `$fn`/`$fa`/`$fs` and dynamic `$` variables,
almost all builtin functions, `echo`/`assert`, `text()` with bundled fonts,
`fontmetrics()`/`textmetrics()`, `object()`, libraries from `OPENSCADPATH` and
`Documents\OpenSCAD\libraries`.

**Geometry.** 2D and 3D primitives tessellated exactly like OpenSCAD, transformations, `union` /
`difference` / `intersection`, `hull`, `minkowski`, `offset`, `linear_extrude`,
`rotate_extrude`, `projection`, `text`, `import` of STL/OFF, `surface` from `.dat` files, the
`!`, `#`, `%`, `*` modifiers.

**App.**
- Opens `.scad` and `.stl` files from the menu, by drag and drop or from the command line.
- Reloads automatically when the file or its dependencies change on disk: edit the code in your
  favourite editor and see the result right away.
- **Customizer** ("Parameters" panel): the parameters annotated in the file (`// [min:step:max]`,
  dropdowns, `/* [Group] */` tabs, captions) become sliders, spin boxes, dropdowns, checkboxes,
  text and vector fields; the model updates live while you drag. Presets are saved in the same
  `<model>.json` file as OpenSCAD's, and "Apply to file" writes the values into the `.scad`
  keeping comments and formatting.
- **Click on the model → code**: clicking a part of the model shows the code that made it, with
  the whole chain of calls (top-level statement › module calls › primitive) and the code excerpt;
  the selected code's geometry is highlighted in blue (every instance of it). Choose another link
  of the chain to select a whole module call; Esc or a click beside the model clears it.
- Console with OpenSCAD's messages (`ECHO`, `WARNING`, `ERROR`) and their location in the code.
- Preset views, perspective/orthographic, grid, axes, edges; `#` shown in transparent red and
  `%` in transparent grey.
- Binary or ASCII STL export.
- Persistent settings, including a constant frame rate that prevents flicker on G-Sync/VRR
  monitors (OLED panels especially).

**Command line.** `scadr` accepts the same main options as `openscad`.

**Web.** The 3D viewer also runs in a web page (WebAssembly, WebGPU or WebGL 2), with a
JavaScript API to show STL files or meshes: see [crates/web-viewer](crates/web-viewer/README.md).
The interpreter already builds for WebAssembly; meshing `.scad` files in the browser needs the
Manifold kernel ported first ([docs/web.md](docs/web.md)).

### Measured compatibility

| Suite | Cases | Result |
|---|---|---|
| Text (messages + CSG tree, byte for byte) | 218 | **218/218** identical |
| Customizer (`.param` export, presets, messages) | 17 | **17/17** identical |
| Geometry (volume, area, bounding box, mesh vertices) | 100 | 39 identical, 60 equivalent, 1 known difference |

The known difference (`linear_extrude` with a non-uniform `scale`) is a 0.01% difference in area.

## Requirements

Windows 10/11 only for now (the code is meant to be portable to macOS and Linux, not yet tried).

- [Rust](https://rustup.rs) via `rustup`: the exact version (1.99) is installed automatically
  thanks to `rust-toolchain.toml`.
- [Visual Studio Build Tools 2022](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
  with the **"Desktop development with C++"** workload (needed to compile Manifold).
- [CMake](https://cmake.org) 3.18 or later on the `PATH`.
- A GPU with Vulkan or DirectX 12.
- Optional, only to regenerate the tests' expected results:
  [OpenSCAD Nightly](https://openscad.org/downloads.html#snapshots).

## Build and run

```powershell
git clone https://github.com/capriatiivan-prog/OpenSCAD-Rust.git
cd OpenSCAD-Rust
cargo run -p app                          # start the app
cargo run -p app -- examples\supporto.stl # open a file directly
cargo build --release -p app -p scadr     # optimized executables in target\release
```

The first build takes a few minutes (Manifold is C++); later builds are fast.

### App shortcuts

| Key | Action |
|---|---|
| `Ctrl+O` | Open file |
| `Ctrl+E` | Export STL |
| `F5` | Reload |
| `F` | Frame the model |
| Left / right (or middle) mouse / wheel | Rotate / pan / zoom |

## Command line (`scadr`)

```powershell
scadr -o model.stl model.scad                    # ASCII STL (like OpenSCAD)
scadr -o model.stl --export-format binstl model.scad
scadr -o model.off model.scad
scadr -o model.csg model.scad                    # CSG tree
scadr -o model.echo model.scad                   # messages only
scadr -D "width=40" -D "label=\"hello\"" -o out.stl model.scad
scadr -o model.param model.scad                  # Customizer parameters as JSON
scadr -p model.json -P "big box" -o out.stl model.scad   # render with a preset
scadr -L C:\my\libraries -o out.stl model.scad
```

Messages, file formats and exit codes follow OpenSCAD's.

## Tests

```powershell
cargo test --workspace                            # unit and integration tests
cargo build -p scadr; cargo run -p compat-runner  # text suite against OpenSCAD
cargo run -p compat-runner -- --params            # Customizer suite against OpenSCAD
cargo run -p compat-runner -- --geometry -v       # geometry suite against OpenSCAD
cargo clippy --workspace --all-targets
cargo fmt --all --check
```

The compatibility suites compare our output with OpenSCAD's output stored in `tests/compat`
(details in [tests/compat/README.md](tests/compat/README.md)); OpenSCAD is only needed to add
new cases. GitHub Actions CI runs all of these checks.

## Layout

```
crates/
  scad-syntax/     lexer, parser with a lossless syntax tree (rowan), AST, code edits
  scad-customizer/ Customizer parameters, .param export, presets
  scad-eval/       interpreter and builtin functions
  scad-font/       fonts, shaping and outlines for text()
  geometry/        meshes, CSG tree with references to the source code
  manifold/        Manifold CSG kernel (C++, vendored) with safe Rust bindings
  mesher/          CSG tree → meshes, OpenSCAD-identical tessellation
  mesh-io/         STL/OFF reading and writing
  render/          wgpu rendering (also windowless, to PNG)
  viewer/          interactive 3D viewport widget, shared by desktop and web
  web-viewer/      the viewer in a web page (WebAssembly) with a JavaScript API
  scadr/           command-line executable
  app/             egui user interface
tools/
  compat-runner/   automatic comparison with OpenSCAD
tests/compat/      test cases and OpenSCAD's expected results
examples/          example models
docs/              technical notes and work plan
```

Developer documentation:
- [CLAUDE.md](CLAUDE.md): decisions, architecture, status and commands.
- [docs/next-steps.md](docs/next-steps.md): detailed next steps and project rules.
- [docs/openscad-behavior.md](docs/openscad-behavior.md): OpenSCAD's behavior observed from the
  outside, used as the specification.
- [docs/web.md](docs/web.md): WebAssembly status, rules and plan.

## Roadmap

1. [x] Foundations: window, GPU viewer, STL
2. [x] Complete OpenSCAD language
3. [x] 3D rendering and STL export
4. [x] Customizer: parameters from comments, sliders, presets
5. [ ] SVG/DXF/3MF/OBJ import, 3MF/OBJ export, remaining geometry cases
6. [ ] Compatibility with real-world libraries (BOSL2, MCAD)
7. [ ] Integrated AI assistant (Claude, Gemini)
8. [ ] Direct modelling

## Contributing

The engine is developed **clean-room**: it is based on OpenSCAD's documentation and on the
observed behavior of its executable, without copying its source code. Please read the rules in
[docs/next-steps.md](docs/next-steps.md) before contributing.

## License

Released under the [GNU GPL v3.0 or later](LICENSE).

Third-party components included in the repository:

| Component | Path | License |
|---|---|---|
| [Manifold](https://github.com/elalish/manifold) 3.5.4 | `crates/manifold/vendor/manifold` | Apache-2.0 |
| [Clipper2](https://github.com/AngusJohnson/Clipper2) | `crates/manifold/vendor/clipper2` | BSL-1.0 |
| [Liberation Fonts](https://github.com/liberationfonts/liberation-fonts) 2.00.1 | `crates/scad-font/fonts` | SIL OFL 1.1 |

Rust dependencies are listed in `Cargo.lock`, each under its own license.

This project is not affiliated with the OpenSCAD project. OpenSCAD is only used as a reference
for compatibility testing.
