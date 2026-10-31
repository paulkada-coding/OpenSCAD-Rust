//! 3D viewport widget: shows the GPU-rendered scene and turns mouse input
//! into camera movements.

use eframe::egui::{self, PointerButton, Rect, Sense, pos2};
use eframe::egui_wgpu::{self, wgpu};
use geometry::Mesh;
use render::{OrbitCamera, Projection, Scene, SceneStyle, ViewPreset};

const ORBIT_SPEED: f32 = 0.008; // radians per point
const ZOOM_SPEED: f32 = 0.0015; // per scrolled point

pub struct Viewport {
    scene: Scene,
    camera: OrbitCamera,
    texture_id: Option<egui::TextureId>,
    rendered_size: [u32; 2],
    dirty: bool,
}

impl Viewport {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            scene: Scene::new(device),
            camera: OrbitCamera::default(),
            texture_id: None,
            rendered_size: [0, 0],
            dirty: true,
        }
    }

    pub fn set_mesh(&mut self, device: &wgpu::Device, mesh: &Mesh) {
        self.scene.set_mesh(device, mesh);
        self.fit();
    }

    pub fn fit(&mut self) {
        if let Some((min, max)) = self.scene.bounds() {
            self.camera.fit(min, max);
        } else {
            self.camera = OrbitCamera::default();
        }
        self.dirty = true;
    }

    pub fn set_view(&mut self, preset: ViewPreset) {
        self.camera.set_view(preset);
        self.dirty = true;
    }

    pub fn projection(&self) -> Projection {
        self.camera.projection
    }

    pub fn set_projection(&mut self, projection: Projection) {
        self.camera.projection = projection;
        self.dirty = true;
    }

    pub fn style(&self) -> SceneStyle {
        *self.scene.style()
    }

    pub fn set_style(&mut self, style: SceneStyle) {
        self.scene.set_style(style);
        self.dirty = true;
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, render_state: &egui_wgpu::RenderState) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.handle_input(ui, &response, rect);

        let ppp = ui.ctx().pixels_per_point();
        let size = [
            (rect.width() * ppp).round() as u32,
            (rect.height() * ppp).round() as u32,
        ];
        if size[0] == 0 || size[1] == 0 {
            return;
        }

        // The scene is re-rendered only when something changed; otherwise
        // egui keeps showing the previous image.
        if self.dirty || size != self.rendered_size {
            let recreated = self.scene.render(
                &render_state.device,
                &render_state.queue,
                &self.camera,
                size,
            );
            if recreated || self.texture_id.is_none() {
                self.register_texture(render_state);
            }
            self.rendered_size = size;
            self.dirty = false;
        }

        if let Some(id) = self.texture_id {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            ui.painter().image(id, rect, uv, egui::Color32::WHITE);
        }
    }

    fn register_texture(&mut self, render_state: &egui_wgpu::RenderState) {
        let Some(view) = self.scene.output_view() else {
            return;
        };
        let mut renderer = render_state.renderer.write();
        match self.texture_id {
            Some(id) => renderer.update_egui_texture_from_wgpu_texture(
                &render_state.device,
                view,
                wgpu::FilterMode::Linear,
                id,
            ),
            None => {
                self.texture_id = Some(renderer.register_native_texture(
                    &render_state.device,
                    view,
                    wgpu::FilterMode::Linear,
                ));
            }
        }
    }

    fn handle_input(&mut self, ui: &egui::Ui, response: &egui::Response, rect: Rect) {
        let delta = response.drag_delta();
        let shift = ui.input(|i| i.modifiers.shift);

        let panning = response.dragged_by(PointerButton::Secondary)
            || response.dragged_by(PointerButton::Middle)
            || (shift && response.dragged_by(PointerButton::Primary));
        if panning {
            self.camera.pan(delta.x, delta.y, rect.height());
            self.dirty = true;
        } else if response.dragged_by(PointerButton::Primary) {
            self.camera
                .orbit(-delta.x * ORBIT_SPEED, delta.y * ORBIT_SPEED);
            self.dirty = true;
        }

        if response.hovered() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            if scroll != 0.0 || pinch != 1.0 {
                self.camera.zoom((-scroll * ZOOM_SPEED).exp() / pinch);
                self.dirty = true;
            }
        }

        if response.double_clicked() {
            self.fit();
        }
    }
}
