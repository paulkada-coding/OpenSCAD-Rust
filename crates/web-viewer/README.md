# web-viewer

The 3D viewer of the desktop app in a web page: the same `viewer::Viewport` widget and
`render::Scene`, compiled to WebAssembly. It uses WebGPU and falls back to WebGL 2 in
browsers without WebGPU.

Today it shows meshes (STL files or arrays handed over by JavaScript). Evaluating `.scad`
files in the browser comes later: see [`docs/web.md`](../../docs/web.md).

## Build and try

Once (installs the WebAssembly target and the `wasm-bindgen` tool; its version must match the
`wasm-bindgen` crate in `Cargo.lock`):

```bash
rustup target add wasm32-unknown-unknown
```

```bash
cargo install wasm-bindgen-cli --version 0.2.130 --locked
```

Then, from the repository root (same commands in PowerShell):

```bash
cargo build --release --target wasm32-unknown-unknown -p web-viewer
```

```bash
wasm-bindgen --target web --out-dir crates/web-viewer/www/pkg target/wasm32-unknown-unknown/release/web_viewer.wasm
```

Serve `crates/web-viewer/www` with any static web server (browsers do not load WebAssembly
from `file://`) and open it, for example:

```bash
python -m http.server 8080 --directory crates/web-viewer/www
```

and open <http://localhost:8080>. The page shows the bundled sample model; drop an STL file
on it or use "Open STL…".

## Embedding in a web app

`www/index.html` is a complete example. The JavaScript API (`pkg/web_viewer.d.ts` lists it
with types):

```js
import init, { WebViewer } from "./pkg/web_viewer.js";

await init();
const viewer = new WebViewer();
// The viewer fills the canvas: size it with CSS. `false` hides the built-in menu bar.
await viewer.start(document.getElementById("canvas"), false);

// An STL file (binary or ASCII) as bytes; throws on malformed files.
const bytes = new Uint8Array(await (await fetch("part.stl")).arrayBuffer());
viewer.load_stl("part.stl", bytes);

// Any triangle mesh: x, y, z per vertex, then 3 vertex indices per triangle,
// counter-clockwise seen from outside (the STL convention).
viewer.set_mesh("tetrahedron",
  [0, 0, 0, 30, 0, 0, 0, 30, 0, 0, 0, 30],
  [0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3]);

viewer.load_demo();   // the bundled sample model
viewer.destroy();     // stop and release the canvas
```

Mouse: left button orbits, right/middle button or Shift pans, wheel zooms, double-click fits
the model.
