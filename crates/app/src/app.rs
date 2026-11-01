//! Main window: menus, model info panel, console, status bar and the 3D
//! viewport. `.scad` files are evaluated in the background and reloaded
//! automatically when they (or their includes) change on disk.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant, SystemTime};

use anyhow::Context as _;
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};
use eframe::egui_wgpu;
use geometry::{Aabb, Mesh};
use mesh_io::StlFormat;
use mesher::{RenderOutput, Shape};
use render::{LayerKind, LayerMesh, Projection, ViewPreset};

use crate::customizer::{self, CustomizerEvent, CustomizerState, PersistedCustomizer};
use crate::selection::{self, Provenance, Selection, SelectionEvent};
use crate::settings::{self, FrameRate, PanelSide, Settings};
use crate::worker::{ConsoleLine, JobResult, LineKind, Worker, modified_time};
use viewer::{PickRay, Viewport};

const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const EXPORT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::E);
const RELOAD: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F5);
const FIT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F);
const CLEAR_SELECTION: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::Escape);

/// How often open files are checked for changes on disk.
const WATCH_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DocumentKind {
    Scad,
    Stl,
}

/// Measurements of the displayed geometry.
struct Stats {
    bounds: Aabb,
    triangles: usize,
    vertices: usize,
    /// Volume and area are meaningful for solids only.
    volume: Option<f64>,
    area: f64,
}

impl Stats {
    fn of(mesh: &Mesh, solid: bool) -> Self {
        Self {
            bounds: mesh.bounds(),
            triangles: mesh.triangle_count(),
            vertices: mesh.vertices.len(),
            volume: solid.then(|| mesh.volume()),
            area: mesh.surface_area(),
        }
    }
}

struct Document {
    path: PathBuf,
    kind: DocumentKind,
    /// The exportable solid, when the result is 3D.
    solid: Option<Mesh>,
    /// Which code produced each triangle of `solid` (`.scad` only).
    provenance: Option<Provenance>,
    /// The picked code, highlighted in the viewport.
    selection: Option<Selection>,
    is_planar: bool,
    stats: Option<Stats>,
    /// Files to watch, with the modification time last seen.
    dependencies: Vec<(PathBuf, Option<SystemTime>)>,
    /// Generation of the evaluation in progress, if any.
    pending: Option<u64>,
    /// The camera is framed once, on the first result, and then left alone
    /// so reloads while editing do not move the view.
    framed: bool,
    timings: Option<(Duration, Duration)>,
}

impl Document {
    fn new(path: PathBuf, kind: DocumentKind) -> Self {
        Self {
            dependencies: vec![(path.clone(), modified_time(&path))],
            path,
            kind,
            solid: None,
            provenance: None,
            selection: None,
            is_planar: false,
            stats: None,
            pending: None,
            framed: false,
            timings: None,
        }
    }
}

struct PendingStl {
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
    worker: Worker,
    document: Option<Document>,
    customizer: Option<CustomizerState>,
    persisted_customizer: PersistedCustomizer,
    show_customizer: bool,
    loading_stl: Option<PendingStl>,
    console: Vec<ConsoleLine>,
    show_console: bool,
    last_watch: Instant,
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
            .context("wgpu render state is not available")?;
        let info = render_state.adapter.get_info();
        let gpu_name = format!("{} · {:?}", info.name, info.backend);
        log::info!("GPU: {gpu_name}");

        let mut app = Self {
            viewport: Viewport::new(&render_state.device),
            render_state,
            worker: Worker::spawn(cc.egui_ctx.clone()),
            document: None,
            customizer: None,
            persisted_customizer: PersistedCustomizer::load(cc.storage),
            show_customizer: true,
            loading_stl: None,
            console: Vec::new(),
            show_console: true,
            last_watch: Instant::now(),
            status: Status::Info("Ready".into()),
            gpu_name,
            settings: Settings::load(cc.storage),
            settings_open: false,
        };
        if let Some(path) = initial {
            app.open(path, &cc.egui_ctx);
        }
        Ok(app)
    }

    // ------------------------------------------------------------ opening

    fn open(&mut self, path: PathBuf, ctx: &egui::Context) {
        if let Some(prev) = &self.customizer {
            prev.persist_into(&mut self.persisted_customizer);
        }
        if has_extension(&path, "scad") {
            self.console.clear();
            self.document = Some(Document::new(path.clone(), DocumentKind::Scad));
            self.customizer = Some(CustomizerState::load_for_file(
                &path,
                &self.persisted_customizer,
            ));
            self.viewport.clear(&self.render_state.device);
            self.evaluate(&path);
        } else {
            self.start_stl_loading(path, ctx);
        }
    }

    fn evaluate(&mut self, path: &Path) {
        let defines = self
            .customizer
            .as_ref()
            .map_or_else(Vec::new, CustomizerState::active_defines);
        let generation = self.worker.submit(path.to_path_buf(), defines);
        if let Some(doc) = &mut self.document {
            doc.pending = Some(generation);
        }
        self.status = Status::Info(format!("Evaluating {}…", file_name(path)));
    }

    fn reload(&mut self, ctx: &egui::Context) {
        let Some(doc) = &self.document else {
            return;
        };
        let path = doc.path.clone();
        match doc.kind {
            DocumentKind::Scad => {
                if let Some(cust) = &mut self.customizer {
                    cust.reload_from_disk();
                }
                self.evaluate(&path);
            }
            DocumentKind::Stl => self.start_stl_loading(path, ctx),
        }
    }

    fn start_stl_loading(&mut self, path: PathBuf, ctx: &egui::Context) {
        let (sender, receiver) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        let source = path.clone();
        std::thread::spawn(move || {
            let _ = sender.send(mesh_io::read_stl_file(&source));
            ctx.request_repaint();
        });
        self.status = Status::Info(format!("Loading {}…", file_name(&path)));
        self.loading_stl = Some(PendingStl {
            path,
            result: receiver,
        });
    }

    fn poll_stl_loading(&mut self) {
        let Some(pending) = &self.loading_stl else {
            return;
        };
        let outcome = match pending.result.try_recv() {
            Ok(outcome) => outcome,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                self.status = Status::Error("loading interrupted".into());
                self.loading_stl = None;
                return;
            }
        };
        let path = pending.path.clone();
        self.loading_stl = None;

        match outcome {
            Ok(mesh) => {
                let reloading = self
                    .document
                    .as_ref()
                    .is_some_and(|d| d.path == path && d.kind == DocumentKind::Stl);
                if !reloading {
                    if let Some(prev) = &self.customizer {
                        prev.persist_into(&mut self.persisted_customizer);
                    }
                    self.customizer = None;
                    self.console.clear();
                    self.document = Some(Document::new(path.clone(), DocumentKind::Stl));
                }
                self.status = Status::Info(format!(
                    "Opened {} ({} triangles)",
                    file_name(&path),
                    mesh.triangle_count()
                ));
                self.show_geometry(&[LayerMesh {
                    kind: LayerKind::Model,
                    mesh: &mesh,
                }]);
                if let Some(doc) = &mut self.document {
                    doc.dependencies = vec![(path.clone(), modified_time(&path))];
                    doc.stats = Some(Stats::of(&mesh, true));
                    doc.is_planar = false;
                    doc.solid = Some(mesh);
                }
            }
            Err(err) => {
                self.status = Status::Error(format!("{}: {err}", file_name(&path)));
            }
        }
    }

    // ------------------------------------------------------ evaluation

    fn poll_worker(&mut self) {
        while let Ok(result) = self.worker.results.try_recv() {
            let current = self
                .document
                .as_ref()
                .is_some_and(|d| d.pending == Some(result.generation) && d.path == result.path);
            if current {
                self.apply_result(result);
            }
        }
    }

    fn apply_result(&mut self, result: JobResult) {
        self.console = result.console;
        let name = file_name(&result.path);
        let Some(doc) = &mut self.document else {
            return;
        };
        doc.pending = None;
        doc.dependencies = result.dependencies;

        let Some(output) = result.output else {
            // Keep showing the last good geometry while the file is fixed.
            self.status = Status::Error(format!("{name}: error, see console"));
            self.show_console = true;
            return;
        };
        doc.timings = Some((result.eval_time, result.mesh_time));
        let has_errors = self.console.iter().any(|l| l.kind == LineKind::Error);
        let timing = format!(
            "eval {} ms · mesh {} ms",
            result.eval_time.as_millis(),
            result.mesh_time.as_millis()
        );
        self.status = if has_errors {
            Status::Error(format!("{name}: error, see console ({timing})"))
        } else {
            Status::Info(format!("{name} · {timing}"))
        };

        let (model, solid) = match &output.shape {
            Shape::Solid(mesh) => (Some(mesh), true),
            Shape::Planar { fill, .. } => (Some(fill), false),
            Shape::Empty => (None, false),
        };
        doc.stats = model.filter(|m| !m.is_empty()).map(|m| Stats::of(m, solid));
        doc.is_planar = matches!(output.shape, Shape::Planar { .. });
        doc.solid = match &output.shape {
            Shape::Solid(mesh) if !mesh.is_empty() => Some(mesh.clone()),
            _ => None,
        };
        doc.provenance = result.provenance.filter(|_| doc.solid.is_some());
        // Keep the selection while the code it points to is unchanged
        // (e.g. a new Customizer value).
        doc.selection = doc
            .selection
            .take()
            .zip(doc.provenance.as_ref())
            .and_then(|(old, provenance)| provenance.carry_over(&old));
        let frame = !doc.framed;
        doc.framed = true;

        let layers = layers_of(&output);
        self.show_geometry(&layers);
        self.update_highlight();
        if frame {
            self.viewport.fit();
        }
    }

    fn show_geometry(&mut self, layers: &[LayerMesh<'_>]) {
        let fit = self
            .document
            .as_ref()
            .is_some_and(|d| d.kind == DocumentKind::Stl);
        self.viewport
            .set_layers(&self.render_state.device, layers, fit);
    }

    // ---------------------------------------------------------- selection

    /// Selects the code that produced the clicked triangle; a click beside
    /// the model clears the selection.
    fn pick(&mut self, ray: PickRay) {
        let Some(doc) = &mut self.document else {
            return;
        };
        let hit = doc
            .solid
            .as_ref()
            .and_then(|mesh| mesh.raycast(ray.origin, ray.direction));
        let Some(hit) = hit else {
            if doc.selection.take().is_some() {
                self.update_highlight();
            }
            return;
        };
        let selection = match (&doc.provenance, doc.kind) {
            (Some(provenance), _) => provenance.select(hit.triangle),
            (None, DocumentKind::Stl) => {
                self.status = Status::Info("STL files do not record the code of the model".into());
                return;
            }
            (None, DocumentKind::Scad) => None,
        };
        let Some(selection) = selection else {
            self.status = Status::Info("No source information for this part of the model".into());
            return;
        };
        let link = selection.current();
        self.status = Status::Info(format!(
            "{} — {}",
            link.location.summary(),
            link.location.describe()
        ));
        doc.selection = Some(selection);
        self.update_highlight();
    }

    fn handle_selection_event(&mut self, event: SelectionEvent) {
        let Some(doc) = &mut self.document else {
            return;
        };
        match event {
            SelectionEvent::None => return,
            SelectionEvent::Level(level) => {
                if let Some(selection) = &mut doc.selection {
                    selection.level = level;
                }
            }
            SelectionEvent::Clear => doc.selection = None,
        }
        self.update_highlight();
    }

    /// Sends the triangles of the selected code to the viewport.
    fn update_highlight(&mut self) {
        let device = &self.render_state.device;
        let Some(doc) = &mut self.document else {
            self.viewport.set_selection(device, None);
            return;
        };
        let highlight = match (&mut doc.selection, &doc.provenance, &doc.solid) {
            (Some(selection), Some(provenance), Some(model)) => {
                let mesh = provenance.highlight(model, selection.current().source);
                selection.triangles = mesh.triangle_count();
                Some(mesh)
            }
            _ => None,
        };
        self.viewport.set_selection(device, highlight.as_ref());
    }

    /// Re-evaluates when a watched file changed on disk.
    fn watch_files(&mut self, ctx: &egui::Context) {
        if self.last_watch.elapsed() < WATCH_INTERVAL {
            return;
        }
        self.last_watch = Instant::now();
        let Some(doc) = &self.document else {
            return;
        };
        if doc.pending.is_some() || self.loading_stl.is_some() {
            return;
        }
        let changed = doc
            .dependencies
            .iter()
            .any(|(path, seen)| modified_time(path) != *seen);
        if changed {
            self.reload(ctx);
        } else if let Some(cust) = &mut self.customizer {
            cust.refresh_presets_if_changed();
        }
    }

    // ------------------------------------------------------------- export

    fn open_dialog(&mut self, ctx: &egui::Context) {
        let picked = rfd::FileDialog::new()
            .set_title("Open model")
            .add_filter("Models (OpenSCAD, STL)", &["scad", "stl"])
            .add_filter("OpenSCAD", &["scad"])
            .add_filter("STL", &["stl"])
            .pick_file();
        if let Some(path) = picked {
            self.open(path, ctx);
        }
    }

    fn export_dialog(&mut self, format: StlFormat) {
        let Some(doc) = &self.document else {
            return;
        };
        let Some(solid) = &doc.solid else {
            self.status = Status::Error(if doc.is_planar {
                "Result is 2D: STL requires a 3D object".into()
            } else {
                "Nothing to export".into()
            });
            return;
        };
        let default_name = doc
            .path
            .file_stem()
            .map(|s| format!("{}.stl", s.to_string_lossy()))
            .unwrap_or_else(|| "model.stl".into());
        let picked = rfd::FileDialog::new()
            .set_title("Export STL")
            .add_filter("STL", &["stl"])
            .set_file_name(default_name)
            .save_file();
        let Some(path) = picked else {
            return;
        };
        self.status = match mesh_io::write_stl_file(solid, &path, format) {
            Ok(()) => Status::Info(format!("Exported {}", path.display())),
            Err(err) => Status::Error(format!("Export failed: {err}")),
        };
    }

    // ----------------------------------------------------------------- input

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .find(|p| has_extension(p, "stl") || has_extension(p, "scad"))
        });
        if let Some(path) = dropped {
            self.open(path, ctx);
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|i| i.consume_shortcut(&OPEN)) {
            self.open_dialog(ctx);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&EXPORT)) {
            self.export_dialog(StlFormat::Binary);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&RELOAD)) {
            self.reload(ctx);
        }
        let typing = ctx.egui_wants_keyboard_input();
        if !typing && ctx.input_mut(|i| i.consume_shortcut(&FIT)) {
            self.viewport.fit();
        }
        let has_selection = self
            .document
            .as_ref()
            .is_some_and(|d| d.selection.is_some());
        if !typing && has_selection && ctx.input_mut(|i| i.consume_shortcut(&CLEAR_SELECTION)) {
            self.handle_selection_event(SelectionEvent::Clear);
        }
    }

    // -------------------------------------------------------------------- UI

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                let shortcut = ui.ctx().format_shortcut(&OPEN);
                if ui
                    .add(egui::Button::new("Open…").shortcut_text(shortcut))
                    .clicked()
                {
                    self.open_dialog(ui.ctx());
                }
                let shortcut = ui.ctx().format_shortcut(&RELOAD);
                if ui
                    .add_enabled(
                        self.document.is_some(),
                        egui::Button::new("Reload").shortcut_text(shortcut),
                    )
                    .clicked()
                {
                    self.reload(ui.ctx());
                }
                ui.separator();
                let has_solid = self.document.as_ref().is_some_and(|d| d.solid.is_some());
                let shortcut = ui.ctx().format_shortcut(&EXPORT);
                if ui
                    .add_enabled(
                        has_solid,
                        egui::Button::new("Export STL (binary)…").shortcut_text(shortcut),
                    )
                    .clicked()
                {
                    self.export_dialog(StlFormat::Binary);
                }
                if ui
                    .add_enabled(has_solid, egui::Button::new("Export STL (ASCII)…"))
                    .clicked()
                {
                    self.export_dialog(StlFormat::Ascii);
                }
                ui.separator();
                if ui.button("Settings…").clicked() {
                    self.settings_open = true;
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });

            ui.menu_button("View", |ui| {
                let shortcut = ui.ctx().format_shortcut(&FIT);
                if ui
                    .add(egui::Button::new("Fit to view").shortcut_text(shortcut))
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
                ui.separator();
                ui.checkbox(&mut self.show_customizer, "Parameters");
                ui.checkbox(&mut self.show_console, "Console");
            });
        });
    }

    fn customizer_panel(&mut self, ui: &mut egui::Ui) {
        let Some(cust) = &mut self.customizer else {
            return;
        };
        let event = cust.ui(ui);
        self.handle_customizer_event(event);
    }

    fn handle_customizer_event(&mut self, event: CustomizerEvent) {
        match event {
            CustomizerEvent::None => {}
            CustomizerEvent::Evaluate => {
                if let Some(path) = self.document.as_ref().map(|d| d.path.clone()) {
                    self.evaluate(&path);
                }
            }
            CustomizerEvent::AppliedToFile { count, new_mtime } => {
                if let Some(doc) = &mut self.document {
                    for (path, seen) in &mut doc.dependencies {
                        if *path == doc.path {
                            *seen = new_mtime;
                        }
                    }
                    // A change still inside the debounce window was not
                    // rendered yet: render the file as it is now on disk.
                    let path = doc.path.clone();
                    self.evaluate(&path);
                    self.status = Status::Info(format!(
                        "Applied {count} parameter(s) to {}",
                        file_name(&path)
                    ));
                }
            }
            CustomizerEvent::Status { is_error, message } => {
                self.status = if is_error {
                    Status::Error(message)
                } else {
                    Status::Info(message)
                };
            }
        }
    }

    fn model_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Model");
        ui.add_space(4.0);
        let Some(doc) = &self.document else {
            ui.label("No model loaded.");
            ui.add_space(8.0);
            ui.weak("Drop a .scad or .stl file into the window or use File → Open.");
            return;
        };

        egui::Grid::new("model_info")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label("File");
                ui.label(file_name(&doc.path))
                    .on_hover_text(doc.path.display().to_string());
                ui.end_row();
                if let Some(stats) = &doc.stats {
                    let size = stats.bounds.size();
                    ui.label("Type");
                    ui.label(if doc.is_planar { "2D" } else { "3D" });
                    ui.end_row();
                    ui.label("Triangles");
                    ui.label(stats.triangles.to_string());
                    ui.end_row();
                    ui.label("Vertices");
                    ui.label(stats.vertices.to_string());
                    ui.end_row();
                    ui.label("Size");
                    ui.label(format!("{:.2} × {:.2} × {:.2} mm", size.x, size.y, size.z));
                    ui.end_row();
                    ui.label("Min");
                    ui.label(format_point(stats.bounds.min));
                    ui.end_row();
                    ui.label("Max");
                    ui.label(format_point(stats.bounds.max));
                    ui.end_row();
                    if let Some(volume) = stats.volume {
                        ui.label("Volume");
                        ui.label(format!("{volume:.2} mm³"));
                        ui.end_row();
                    }
                    ui.label("Surface");
                    ui.label(format!("{:.2} mm²", stats.area));
                    ui.end_row();
                }
                if let Some((eval, mesh)) = doc.timings {
                    ui.label("Evaluation");
                    ui.label(format!("{} ms", eval.as_millis()));
                    ui.end_row();
                    ui.label("Geometry");
                    ui.label(format!("{} ms", mesh.as_millis()));
                    ui.end_row();
                }
            });

        let mut event = SelectionEvent::None;
        if doc.kind == DocumentKind::Scad && doc.solid.is_some() {
            ui.add_space(12.0);
            ui.separator();
            event = selection::selection_ui(ui, doc.selection.as_ref());
        }

        ui.add_space(12.0);
        ui.separator();
        if doc.kind == DocumentKind::Scad {
            ui.weak(
                "Reloads automatically when you save the file (including from another editor).",
            );
            ui.add_space(4.0);
        }
        ui.weak(
            "Left: orbit · Right/middle or Shift: pan · Wheel: zoom · Double-click or F: fit · \
             Click: select",
        );
        self.handle_selection_event(event);
    }

    fn console_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("Console");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Clear").clicked() {
                    self.console.clear();
                }
            });
        });
        ui.separator();
        let visuals = ui.visuals().clone();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if self.console.is_empty() {
                    ui.weak("No messages.");
                }
                for line in &self.console {
                    let color = match line.kind {
                        LineKind::Echo => visuals.text_color(),
                        LineKind::Warning => visuals.warn_fg_color,
                        LineKind::Error => visuals.error_fg_color,
                        LineKind::Trace => visuals.weak_text_color(),
                    };
                    ui.add(
                        egui::Label::new(egui::RichText::new(&line.text).monospace().color(color))
                            .wrap(),
                    );
                }
            });
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let busy = self.loading_stl.is_some()
                || self.document.as_ref().is_some_and(|d| d.pending.is_some());
            if busy {
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

/// Meshes to display for a render: the model, then `#` and `%` subtrees.
fn layers_of(output: &RenderOutput) -> Vec<LayerMesh<'_>> {
    fn mesh_of(shape: &Shape) -> Option<&Mesh> {
        match shape {
            Shape::Solid(mesh) => Some(mesh),
            Shape::Planar { fill, .. } => Some(fill),
            Shape::Empty => None,
        }
    }
    let mut layers = Vec::new();
    if let Some(mesh) = mesh_of(&output.shape) {
        layers.push(LayerMesh {
            kind: LayerKind::Model,
            mesh,
        });
    }
    for (shapes, kind) in [
        (&output.highlighted, LayerKind::Highlight),
        (&output.background, LayerKind::Background),
    ] {
        layers.extend(
            shapes
                .iter()
                .filter_map(mesh_of)
                .map(|mesh| LayerMesh { kind, mesh }),
        );
    }
    layers
}

impl eframe::App for ViewerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_stl_loading();
        self.poll_worker();
        self.watch_files(&ctx);
        self.handle_dropped_files(&ctx);
        self.handle_shortcuts(&ctx);

        if self
            .customizer
            .as_mut()
            .is_some_and(CustomizerState::take_due_evaluation)
            && let Some(path) = self.document.as_ref().map(|d| d.path.clone())
        {
            self.evaluate(&path);
        }

        // With vsync on, this presents exactly once per monitor refresh.
        if self.settings.frame_rate == FrameRate::Constant {
            ctx.request_repaint();
        } else {
            let mut next_wake = self.document.is_some().then_some(WATCH_INTERVAL);
            if let Some(delay) = self
                .customizer
                .as_ref()
                .and_then(CustomizerState::pending_repaint_delay)
            {
                next_wake = Some(next_wake.map_or(delay, |w| w.min(delay)));
            }
            if let Some(delay) = next_wake {
                ctx.request_repaint_after(delay);
            }
        }

        let mut open = self.settings_open;
        egui::Window::new("Settings")
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
        if self.show_console {
            egui::Panel::bottom("console")
                .resizable(true)
                .default_size(170.0)
                .min_size(60.0)
                .show(ui, |ui| self.console_panel(ui));
        }
        egui::Panel::left("model")
            .default_size(260.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.model_panel(ui));
            });
        if self.show_customizer && self.customizer.is_some() {
            let panel = match self.settings.customizer_side {
                PanelSide::Right => egui::Panel::right("parameters"),
                PanelSide::Left => egui::Panel::left("parameters"),
            };
            panel
                .resizable(true)
                .default_size(320.0)
                .min_size(240.0)
                .show(ui, |ui| self.customizer_panel(ui));
        }

        let render_state = self.render_state.clone();
        let pick = egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.viewport.ui(ui, &render_state))
            .inner;
        if let Some(ray) = pick {
            self.pick(ray);
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if let Some(cust) = &self.customizer {
            cust.persist_into(&mut self.persisted_customizer);
        }
        eframe::set_value(storage, settings::STORAGE_KEY, &self.settings);
        eframe::set_value(storage, customizer::STORAGE_KEY, &self.persisted_customizer);
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
