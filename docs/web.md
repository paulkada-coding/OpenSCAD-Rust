# WebAssembly: status and plan

Goal: the 3D viewer, and later the whole engine (`.scad` → mesh), usable in a web app.
Updated on 2026-11-01.

## What runs in the browser today

| Crate | wasm32 | Notes |
|---|---|---|
| `scad-syntax`, `scad-eval`, `scad-customizer`, `scad-font` | ✅ builds and runs | Checked in Node: `porta_carte.scad` gives the same `ECHO`s as on desktop; `textmetrics()` works with the bundled font. There is no file system: `include`/`use` of other files reports "Can't find include file". |
| `geometry`, `mesh-io` | ✅ | STL parsing from bytes (`mesh_io::read_stl`), `Mesh::from_flat` for meshes coming from JavaScript. |
| `render` | ✅ (without `headless`) | WebGPU, or WebGL 2. The final sRGB encoding is a separate pass (`shaders/encode.wgsl`) because WebGL 2 cannot reinterpret texture formats; the desktop output stayed identical pixel for pixel. `headless` (PNG snapshots) blocks on the GPU and is native only. |
| `viewer` | ✅ | The egui viewport widget, shared by the desktop app and the web viewer. |
| `web-viewer` | ✅ | The page and its JavaScript API: see `crates/web-viewer/README.md`. Verified in Chromium with WebGL 2 (SwiftShader) and WebGPU. |
| `manifold`, `mesher` | ❌ | The Manifold kernel is C++: see below. Its build script stops with a clear message on wasm32. |
| `app`, `scadr`, `compat-runner` | ❌ (by design) | Desktop programs (threads, files, file watching, native dialogs). |

CI (`.github/workflows/ci.yml`, job `web`) runs clippy for wasm32 on the crates above and
builds the web viewer, so they stay WebAssembly-compatible.

## Rules that keep it that way

- Engine and rendering crates must not use threads, blocking waits, `std::time::Instant` /
  `SystemTime::now()` (they panic in the browser), or assume a file system is present: file
  access must fail gracefully (an error or a missing file, never a panic).
- Platform-specific code goes behind `#[cfg(target_arch = "wasm32")]` /
  `#[cfg(not(target_arch = "wasm32"))]`, with native-only dependencies under
  `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]` (see `crates/render/Cargo.toml`).
- New rendering features must work with WebGL 2 limits (no texture view reinterpretation, no
  storage buffers in vertex shaders, 8 color attachments at most…), or degrade gracefully.

## Next steps towards `.scad` files in the browser

1. **Manifold for WebAssembly** (the main step). Recommended: compile the vendored C++ with
   clang for `wasm32-unknown-unknown` against the `wasi-sdk` sysroot (libc, libc++ built
   without exceptions), linked into the same module as the Rust code. Manifold already runs
   single-threaded (`MANIFOLD_PAR=OFF`) and our shim already avoids exceptions; the few WASI
   imports libc needs (memory, clock) are provided by small stubs. Alternatives considered:
   Rust's `wasm32-unknown-emscripten` target (C++ is easy, but wgpu/winit/egui support it
   poorly) or Manifold's official JavaScript build as a separate module (loses our shim, the
   original-ID provenance and the exact tessellation rules, and doubles the data copies).
2. **Sources without a file system**: a `SourceProvider` trait in `scad-eval` (disk on
   desktop, an in-memory map of files on the web) for the main file, `include`/`use` and
   `import()`/`surface()`.
3. **Background evaluation in a Web Worker** (the desktop app uses a thread): the worker
   runs interpreter + mesher and sends back the mesh and its provenance.
4. **Customizer and click → code on the web**: the code is already platform-independent
   (`scad-customizer`, `app/src/selection.rs`); move the panels from `app` into a shared UI
   crate, like `viewer`.
5. **Snapshots for the AI on the web**: an asynchronous version of `render::render_image`.
