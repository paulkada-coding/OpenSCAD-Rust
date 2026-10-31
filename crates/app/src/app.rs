//! Main window: menus, model info panel, status bar and the 3D viewport.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};

use anyhow::Context as _;
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};
use eframe::egui_wgpu;
use geometry::{Aabb, Mesh};
use mesh_io::StlFormat;
use render::{Projection, ViewPreset};

use crate::settings::{self, FrameRate, Settings};
use crate::viewport::Viewport;

const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const EXPORT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::E);
const FIT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F);

struct LoadedModel {
    path: PathBuf,
    mesh: Mesh,
    bounds: Aabb,
    volume: f64,
    area: f64,
}

struct PendingLoad {
    path: PathBuf,
    result: Receiver<Result<Mesh, mesh_io::StlError>>,
}

enum Status {
    Info(String),
    Error(String),
}

pub struct ViewerApp {
    render_state: egui_wgpu::RenderState,
    viewport: Viewport,
    model: Option<LoadedModel>,
    loading: Option<PendingLoad>,
    status: Status,
    gpu_name: String,
    settings: Settings,
    settings_open: bool,
}

impl ViewerApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> anyhow::Result<Self> {
        let render_state = cc
            .wgpu_render_state
            .clone()
            .context("il renderer wgpu non è disponibile")?;
        let info = render_state.adapter.get_info();
        let gpu_name = format!("{} · {:?}", info.name, info.backend);
        log::info!("GPU: {gpu_name}");

        let mut app = Self {
            viewport: Viewport::new(&render_state.device),
            render_state,
            model: None,
            loading: None,
            status: Status::Info("Pronto".into()),
            gpu_name,
            settings: Settings::load(cc.storage),
            settings_open: false,
        };
        if let Some(path) = initial {
            app.start_loading(path, &cc.egui_ctx);
        }
        Ok(app)
    }

    fn start_loading(&mut self, path: PathBuf, ctx: &egui::Context) {
        let (sender, receiver) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        let source = path.clone();
        std::thread::spawn(move || {
            let _ = sender.send(mesh_io::read_stl_file(&source));
            ctx.request_repaint();
        });
        self.status = Status::Info(format!("Caricamento di {}…", file_name(&path)));
        self.loading = Some(PendingLoad {
            path,
            result: receiver,
        });
    }

    fn poll_loading(&mut self) {
        let Some(pending) = &self.loading else {
            return;
        };
        let outcome = match pending.result.try_recv() {
            Ok(outcome) => outcome,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                self.status = Status::Error("caricamento interrotto".into());
                self.loading = None;
                return;
            }
        };
        let path = pending.path.clone();
        self.loading = None;

        match outcome {
            Ok(mesh) => {
                self.viewport.set_mesh(&self.render_state.device, &mesh);
                self.status = Status::Info(format!(
                    "Aperto {} ({} triangoli)",
                    file_name(&path),
                    mesh.triangle_count()
                ));
                self.model = Some(LoadedModel {
                    bounds: mesh.bounds(),
                    volume: mesh.volume(),
                    area: mesh.surface_area(),
                    path,
                    mesh,
                });
            }
            Err(err) => {
                self.status = Status::Error(format!("{}: {err}", file_name(&path)));
            }
        }
    }

    fn open_dialog(&mut self, ctx: &egui::Context) {
        let picked = rfd::FileDialog::new()
            .set_title("Apri modello")
            .add_filter("STL", &["stl"])
            .pick_file();
        if let Some(path) = picked {
            self.start_loading(path, ctx);
        }
    }

    fn export_dialog(&mut self, format: StlFormat) {
        let Some(model) = &self.model else {
            return;
        };
        let default_name = model
            .path
            .file_stem()
            .map(|s| format!("{}.stl", s.to_string_lossy()))
            .unwrap_or_else(|| "modello.stl".into());
        let picked = rfd::FileDialog::new()
            .set_title("Esporta STL")
            .add_filter("STL", &["stl"])
            .set_file_name(default_name)
            .save_file();
        let Some(path) = picked else {
            return;
        };
        self.status = match mesh_io::write_stl_file(&model.mesh, &path, format) {
            Ok(()) => Status::Info(format!("Esportato {}", path.display())),
            Err(err) => Status::Error(format!("Esportazione fallita: {err}")),
        };
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .find(|p| has_extension(p, "stl"))
        });
        if let Some(path) = dropped {
            self.start_loading(path, ctx);
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|i| i.consume_shortcut(&OPEN)) {
            self.open_dialog(ctx);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&EXPORT)) {
            self.export_dialog(StlFormat::Binary);
        }
        let typing = ctx.egui_wants_keyboard_input();
        if !typing && ctx.input_mut(|i| i.consume_shortcut(&FIT)) {
            self.viewport.fit();
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                let shortcut = ui.ctx().format_shortcut(&OPEN);
                if ui
                    .add(egui::Button::new("Apri STL…").shortcut_text(shortcut))
                    .clicked()
                {
                    self.open_dialog(ui.ctx());
                }
                ui.separator();
                let has_model = self.model.is_some();
                let shortcut = ui.ctx().format_shortcut(&EXPORT);
                if ui
                    .add_enabled(
                        has_model,
                        egui::Button::new("Esporta STL (binario)…").shortcut_text(shortcut),
                    )
                    .clicked()
                {
                    self.export_dialog(StlFormat::Binary);
                }
                if ui
                    .add_enabled(has_model, egui::Button::new("Esporta STL (ASCII)…"))
                    .clicked()
                {
                    self.export_dialog(StlFormat::Ascii);
                }
                ui.separator();
                if ui.button("Impostazioni…").clicked() {
                    self.settings_open = true;
                }
                ui.separator();
                if ui.button("Esci").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });

            ui.menu_button("Vista", |ui| {
                let shortcut = ui.ctx().format_shortcut(&FIT);
                if ui
                    .add(egui::Button::new("Adatta alla finestra").shortcut_text(shortcut))
                    .clicked()
                {
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
                ui.radio_value(&mut projection, Projection::Perspective, "Prospettiva");
                ui.radio_value(&mut projection, Projection::Orthographic, "Ortogonale");
                if projection != self.viewport.projection() {
                    self.viewport.set_projection(projection);
                }
                ui.separator();
                let mut style = self.viewport.style();
                ui.checkbox(&mut style.show_grid, "Griglia");
                ui.checkbox(&mut style.show_axes, "Assi");
                ui.checkbox(&mut style.show_edges, "Spigoli");
                if style != self.viewport.style() {
                    self.viewport.set_style(style);
                }
            });
        });
    }

    fn model_panel(&self, ui: &mut egui::Ui) {
        ui.heading("Modello");
        ui.add_space(4.0);
        let Some(model) = &self.model else {
            ui.label("Nessun modello caricato.");
            ui.add_space(8.0);
            ui.weak("Trascina un file STL nella finestra oppure usa File → Apri.");
            return;
        };

        let size = model.bounds.size();
        egui::Grid::new("model_info")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label("File");
                ui.label(file_name(&model.path))
                    .on_hover_text(model.path.display().to_string());
                ui.end_row();
                ui.label("Triangoli");
                ui.label(model.mesh.triangle_count().to_string());
                ui.end_row();
                ui.label("Vertici");
                ui.label(model.mesh.vertices.len().to_string());
                ui.end_row();
                ui.label("Dimensioni");
                ui.label(format!("{:.2} × {:.2} × {:.2} mm", size.x, size.y, size.z));
                ui.end_row();
                ui.label("Min");
                ui.label(format_point(model.bounds.min));
                ui.end_row();
                ui.label("Max");
                ui.label(format_point(model.bounds.max));
                ui.end_row();
                ui.label("Volume");
                ui.label(format!("{:.2} mm³", model.volume));
                ui.end_row();
                ui.label("Superficie");
                ui.label(format!("{:.2} mm²", model.area));
                ui.end_row();
            });

        ui.add_space(12.0);
        ui.separator();
        ui.weak("Sinistro: ruota · Destro/centrale o Maiusc: sposta · Rotella: zoom · Doppio clic o F: adatta");
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if self.loading.is_some() {
                ui.spinner();
            }
            match &self.status {
                Status::Info(text) => {
                    ui.label(text);
                }
                Status::Error(text) => {
                    ui.colored_label(ui.visuals().error_fg_color, text);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak(&self.gpu_name);
            });
        });
    }
}

impl eframe::App for ViewerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_loading();
        self.handle_dropped_files(&ctx);
        self.handle_shortcuts(&ctx);

        // With vsync on, this presents exactly once per monitor refresh.
        if self.settings.frame_rate == FrameRate::Constant {
            ctx.request_repaint();
        }

        let mut open = self.settings_open;
        egui::Window::new("Impostazioni")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(420.0)
            .show(&ctx, |ui| {
                settings::settings_ui(ui, &mut self.settings);
            });
        self.settings_open = open;

        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("model")
            .default_size(260.0)
            .show(ui, |ui| self.model_panel(ui));

        let render_state = self.render_state.clone();
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.viewport.ui(ui, &render_state));
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, settings::STORAGE_KEY, &self.settings);
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

fn has_extension(path: &Path, ext: &str) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

fn format_point(p: geometry::glam::DVec3) -> String {
    format!("{:.2}, {:.2}, {:.2}", p.x, p.y, p.z)
}
