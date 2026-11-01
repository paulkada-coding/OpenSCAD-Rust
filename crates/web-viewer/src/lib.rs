//! The 3D viewer in a web page: the same [`viewer::Viewport`] as the desktop
//! app, running on WebAssembly with WebGPU (or WebGL 2 where WebGPU is not
//! available).
//!
//! A page embeds it through the JavaScript class [`WebViewer`]:
//!
//! ```js
//! import init, { WebViewer } from "./pkg/web_viewer.js";
//! await init();
//! const viewer = new WebViewer();
//! await viewer.start(document.getElementById("canvas"), true);
//! viewer.load_stl("part.stl", new Uint8Array(await (await fetch("part.stl")).arrayBuffer()));
//! // or any mesh: flat xyz coordinates and vertex indices, 3 per triangle
//! viewer.set_mesh("triangle", [0, 0, 0, 10, 0, 0, 0, 10, 0], [0, 1, 2]);
//! ```
//!
//! Build: see `crates/web-viewer/README.md`.
//!
//! Not here yet: evaluating `.scad` files in the browser. The interpreter
//! already builds for WebAssembly; the mesher waits for the Manifold kernel
//! (C++) to be ported (see `docs/web.md`).

#![cfg(target_arch = "wasm32")]

mod app;

use std::cell::RefCell;
use std::rc::Rc;

use eframe::egui;
use geometry::Mesh;
use wasm_bindgen::prelude::*;

/// A model on its way to the viewer, from JavaScript or a dropped file.
enum Incoming {
    Model { name: String, mesh: Mesh },
    Error(String),
}

/// Models waiting to be shown. JavaScript calls and file reads (which
/// finish asynchronously) put them here; the app takes them on its next
/// frame.
#[derive(Default)]
struct Inbox {
    items: Vec<Incoming>,
    /// Set once the app runs, to wake it up when something arrives.
    ctx: Option<egui::Context>,
}

#[derive(Clone, Default)]
struct SharedInbox(Rc<RefCell<Inbox>>);

impl SharedInbox {
    fn push(&self, item: Incoming) {
        let mut inbox = self.0.borrow_mut();
        inbox.items.push(item);
        if let Some(ctx) = &inbox.ctx {
            ctx.request_repaint();
        }
    }

    fn push_stl(&self, name: String, bytes: &[u8]) {
        self.push(match mesh_io::read_stl(bytes) {
            Ok(mesh) => Incoming::Model { name, mesh },
            Err(err) => Incoming::Error(format!("{name}: {err}")),
        });
    }

    fn take(&self) -> Vec<Incoming> {
        std::mem::take(&mut self.0.borrow_mut().items)
    }

    fn set_context(&self, ctx: egui::Context) {
        self.0.borrow_mut().ctx = Some(ctx);
    }
}

/// The viewer, as seen from JavaScript.
#[wasm_bindgen]
pub struct WebViewer {
    runner: eframe::WebRunner,
    inbox: SharedInbox,
}

impl Default for WebViewer {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl WebViewer {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        // Fails harmlessly when a second viewer is created.
        let _ = eframe::WebLogger::init(log::LevelFilter::Info);
        Self {
            runner: eframe::WebRunner::new(),
            inbox: SharedInbox::default(),
        }
    }

    /// Starts drawing into `canvas`. With `toolbar`, a menu bar offers
    /// opening STL files and the view options; without it, the page drives
    /// the viewer through this API only.
    pub async fn start(
        &self,
        canvas: web_sys::HtmlCanvasElement,
        toolbar: bool,
    ) -> Result<(), JsValue> {
        let inbox = self.inbox.clone();
        self.runner
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(move |cc| Ok(Box::new(app::WebApp::new(cc, inbox, toolbar)?))),
            )
            .await
    }

    /// Shows an STL file (binary or ASCII) given as bytes. Parse errors are
    /// returned to JavaScript.
    pub fn load_stl(&self, name: &str, bytes: &[u8]) -> Result<(), JsError> {
        let mesh = mesh_io::read_stl(bytes).map_err(|err| JsError::new(&err.to_string()))?;
        self.inbox.push(Incoming::Model {
            name: name.to_owned(),
            mesh,
        });
        Ok(())
    }

    /// Shows a triangle mesh: `vertices` holds x, y, z of each vertex,
    /// `triangles` three vertex indices per triangle, counter-clockwise seen
    /// from outside (as in STL files).
    pub fn set_mesh(&self, name: &str, vertices: &[f64], triangles: &[u32]) -> Result<(), JsError> {
        let mesh = Mesh::from_flat(vertices, triangles).map_err(|err| JsError::new(&err))?;
        self.inbox.push(Incoming::Model {
            name: name.to_owned(),
            mesh,
        });
        Ok(())
    }

    /// Shows the sample model bundled with the viewer.
    pub fn load_demo(&self) {
        self.inbox.push_stl(
            "supporto.stl".into(),
            include_bytes!("../../../examples/supporto.stl"),
        );
    }

    /// Stops the viewer and releases the canvas.
    pub fn destroy(&self) {
        self.runner.destroy();
    }

    /// Whether the viewer stopped because of an internal error.
    pub fn has_panicked(&self) -> bool {
        self.runner.has_panicked()
    }
}
