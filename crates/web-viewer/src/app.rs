//! The page's egui app: an optional menu bar, the 3D viewport, and STL
//! files opened with a file picker or dropped onto the page.

use eframe::egui;
use eframe::egui_wgpu;
use geometry::Mesh;
use viewer::Viewport;
use viewer::render::{LayerKind, LayerMesh, Projection, ViewPreset};

use crate::{Incoming, SharedInbox};

/// What the status line says about the shown model.
struct ModelInfo {
    name: String,
    triangles: usize,
    size: [f64; 3],
}

pub struct WebApp {
    render_state: egui_wgpu::RenderState,
    viewport: Viewport,
    inbox: SharedInbox,
    toolbar: bool,
    model: Option<ModelInfo>,
    error: Option<String>,
}

impl WebApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        inbox: SharedInbox,
        toolbar: bool,
    ) -> Result<Self, String> {
        let render_state = cc
            .wgpu_render_state
            .clone()
            .ok_or("WebGPU / WebGL 2 is not available in this browser")?;
        let info = render_state.adapter.get_info();
        log::info!("GPU: {} · {:?}", info.name, info.backend);
        inbox.set_context(cc.egui_ctx.clone());
        Ok(Self {
            viewport: Viewport::new(&render_state.device),
            render_state,
            inbox,
            toolbar,
            model: None,
            error: None,
        })
    }

    fn receive(&mut self) {
        for item in self.inbox.take() {
            match item {
                Incoming::Model { name, mesh } => self.show(name, &mesh),
                Incoming::Error(message) => {
                    log::warn!("{message}");
                    self.error = Some(message);
                }
            }
        }
    }

    fn show(&mut self, name: String, mesh: &Mesh) {
        let layers = [LayerMesh {
            kind: LayerKind::Model,
            mesh,
        }];
        self.viewport
            .set_layers(&self.render_state.device, &layers, true);
        self.model = Some(ModelInfo {
            name,
            triangles: mesh.triangle_count(),
            size: mesh.bounds().size().to_array(),
        });
        self.error = None;
    }

    /// Reads STL files dropped onto the page (browsers read files
    /// asynchronously, so they arrive through the inbox).
    fn handle_dropped_files(&self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for file in dropped {
            let inbox = self.inbox.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let name = file.path().display().to_string();
                if !name.to_ascii_lowercase().ends_with(".stl") {
                    inbox.push(Incoming::Error(format!(
                        "{name}: only STL files can be shown"
                    )));
                    return;
                }
                match file.bytes_async().await {
                    Ok(bytes) => inbox.push_stl(name, &bytes),
                    Err(err) => inbox.push(Incoming::Error(format!("{name}: {err}"))),
                }
            });
        }
    }

    fn open_dialog(&self) {
        let inbox = self.inbox.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let picked = rfd::AsyncFileDialog::new()
                .add_filter("STL", &["stl"])
                .pick_file()
                .await;
            if let Some(file) = picked {
                let bytes = file.read().await;
                inbox.push_stl(file.file_name(), &bytes);
            }
        });
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            if ui.button("Open STL…").clicked() {
                self.open_dialog();
            }
            ui.menu_button("View", |ui| {
                if ui.button("Fit to view").clicked() {
                    self.viewport.fit();
                }
                ui.separator();
                for preset in ViewPreset::ALL {
                    if ui.button(preset.label()).clicked() {
                        self.viewport.set_view(preset);
                    }
                }
                ui.separator();
                let mut projection = self.viewport.projection();
                ui.radio_value(&mut projection, Projection::Perspective, "Perspective");
                ui.radio_value(&mut projection, Projection::Orthographic, "Orthographic");
                if projection != self.viewport.projection() {
                    self.viewport.set_projection(projection);
                }
                ui.separator();
                let mut style = self.viewport.style();
                ui.checkbox(&mut style.show_grid, "Grid");
                ui.checkbox(&mut style.show_axes, "Axes");
                ui.checkbox(&mut style.show_edges, "Edges");
                if style != self.viewport.style() {
                    self.viewport.set_style(style);
                }
            });
            ui.separator();
            if let Some(error) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            } else if let Some(model) = &self.model {
                let [x, y, z] = model.size;
                ui.label(format!(
                    "{} · {} triangles · {x:.2} × {y:.2} × {z:.2} mm",
                    model.name, model.triangles
                ));
            } else {
                ui.weak("Drop an STL file here or use Open STL…");
            }
        });
    }
}

impl eframe::App for WebApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.receive();
        self.handle_dropped_files(&ctx);

        if self.toolbar {
            egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        }
        let render_state = self.render_state.clone();
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                // Picking needs the code behind the model: nothing to pick in
                // an STL yet.
                let _ = self.viewport.ui(ui, &render_state);
            });
    }
}
