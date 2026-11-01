//! Orbit camera with Z up, matching OpenSCAD's view conventions.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};

// wgpu uses the DirectX clip-space convention (y up, depth 0..1).
use glam::camera::rh::{proj::directx as proj, view};
use glam::{Mat4, Vec2, Vec3};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Projection {
    #[default]
    Perspective,
    Orthographic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewPreset {
    Iso,
    Top,
    Bottom,
    Front,
    Back,
    Left,
    Right,
}

impl ViewPreset {
    pub const ALL: [Self; 7] = [
        Self::Iso,
        Self::Top,
        Self::Bottom,
        Self::Front,
        Self::Back,
        Self::Left,
        Self::Right,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Iso => "Isometric",
            Self::Top => "Top",
            Self::Bottom => "Bottom",
            Self::Front => "Front",
            Self::Back => "Back",
            Self::Left => "Left",
            Self::Right => "Right",
        }
    }

    /// (yaw, pitch) in radians. Yaw is measured from +X around +Z, pitch is
    /// the elevation of the eye above the XY plane.
    fn angles(self) -> (f32, f32) {
        match self {
            // OpenSCAD's default: looking from front-right, slightly above.
            Self::Iso => (-FRAC_PI_4 - 0.35, 0.6),
            Self::Top => (-FRAC_PI_2, FRAC_PI_2),
            Self::Bottom => (-FRAC_PI_2, -FRAC_PI_2),
            Self::Front => (-FRAC_PI_2, 0.0),
            Self::Back => (FRAC_PI_2, 0.0),
            Self::Left => (std::f32::consts::PI, 0.0),
            Self::Right => (0.0, 0.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrbitCamera {
    pub target: Vec3,
    pub distance: f32,
    pub yaw: f32,
    pub pitch: f32,
    /// Vertical field of view in radians (also sizes the orthographic view).
    pub fov_y: f32,
    pub projection: Projection,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        let (yaw, pitch) = ViewPreset::Iso.angles();
        Self {
            target: Vec3::ZERO,
            distance: 100.0,
            yaw,
            pitch,
            fov_y: 30f32.to_radians(),
            projection: Projection::Perspective,
        }
    }
}

impl OrbitCamera {
    /// Unit vector from the target towards the eye.
    fn backward(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        Vec3::new(cp * cy, cp * sy, sp)
    }

    /// Camera up vector; well defined even when looking straight down.
    pub fn up(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        Vec3::new(-sp * cy, -sp * sy, cp)
    }

    pub fn forward(&self) -> Vec3 {
        -self.backward()
    }

    pub fn right(&self) -> Vec3 {
        self.forward().cross(self.up()).normalize()
    }

    pub fn eye(&self) -> Vec3 {
        self.target + self.backward() * self.distance
    }

    pub fn view(&self) -> Mat4 {
        view::look_at_mat4(self.eye(), self.target, self.up())
    }

    /// Projection for the given aspect ratio; `scene_radius` bounds the
    /// visible content so the depth range can be kept tight.
    pub fn projection_matrix(&self, aspect: f32, scene_radius: f32) -> Mat4 {
        let reach = self.distance + scene_radius * 2.0;
        match self.projection {
            Projection::Perspective => {
                let near = (self.distance * 0.01).max(reach * 1e-5);
                proj::perspective(self.fov_y, aspect, near, reach * 2.0)
            }
            Projection::Orthographic => {
                let half_h = self.view_height() * 0.5;
                let half_w = half_h * aspect;
                proj::orthographic(-half_w, half_w, -half_h, half_h, -reach, reach * 2.0)
            }
        }
    }

    /// The ray through a point of the view, for picking: `ndc` is in
    /// normalized device coordinates (x right, y up, both in -1..=1).
    /// Returns `(origin, direction)`; everything visible at that point lies
    /// on the ray at a positive parameter. Uses the same frustum as
    /// [`Self::projection_matrix`].
    pub fn ray(&self, ndc: Vec2, aspect: f32, scene_radius: f32) -> (Vec3, Vec3) {
        let (forward, right, up) = (self.forward(), self.right(), self.up());
        match self.projection {
            Projection::Perspective => {
                let half_h = (self.fov_y * 0.5).tan();
                let dir = forward + right * (ndc.x * half_h * aspect) + up * (ndc.y * half_h);
                (self.eye(), dir.normalize())
            }
            Projection::Orthographic => {
                let half_h = self.view_height() * 0.5;
                let offset = right * (ndc.x * half_h * aspect) + up * (ndc.y * half_h);
                // The orthographic near plane is behind the eye.
                let reach = self.distance + scene_radius * 2.0;
                (self.eye() + offset - forward * reach, forward)
            }
        }
    }

    /// Height of the view at the target distance, in world units.
    pub fn view_height(&self) -> f32 {
        2.0 * self.distance * (self.fov_y * 0.5).tan()
    }

    /// Rotates around the target; deltas are in radians.
    pub fn orbit(&mut self, d_yaw: f32, d_pitch: f32) {
        self.yaw = (self.yaw + d_yaw).rem_euclid(std::f32::consts::TAU);
        self.pitch = (self.pitch + d_pitch).clamp(-FRAC_PI_2, FRAC_PI_2);
    }

    /// Moves the target in the view plane; deltas are in pixels.
    pub fn pan(&mut self, dx: f32, dy: f32, viewport_height_px: f32) {
        let per_pixel = self.view_height() / viewport_height_px.max(1.0);
        self.target += (-self.right() * dx + self.up() * dy) * per_pixel;
    }

    /// `factor` < 1 zooms in.
    pub fn zoom(&mut self, factor: f32) {
        self.distance = (self.distance * factor).clamp(1e-3, 1e7);
    }

    pub fn set_view(&mut self, preset: ViewPreset) {
        (self.yaw, self.pitch) = preset.angles();
    }

    /// Centers the camera on a box so that it fills the view.
    pub fn fit(&mut self, min: Vec3, max: Vec3) {
        let radius = ((max - min).length() * 0.5).max(1e-3);
        self.target = (min + max) * 0.5;
        self.distance = radius / (self.fov_y * 0.5).sin() * 1.05;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basis_is_orthonormal_in_every_preset() {
        let mut cam = OrbitCamera::default();
        for preset in ViewPreset::ALL {
            cam.set_view(preset);
            let (f, u, r) = (cam.forward(), cam.up(), cam.right());
            assert!(f.dot(u).abs() < 1e-5, "{preset:?}");
            assert!((r.length() - 1.0).abs() < 1e-5, "{preset:?}");
            assert!(cam.view().is_finite(), "{preset:?}");
        }
    }

    #[test]
    fn front_view_looks_along_positive_y() {
        let mut cam = OrbitCamera::default();
        cam.set_view(ViewPreset::Front);
        assert!(cam.forward().abs_diff_eq(Vec3::Y, 1e-6));
        assert!(cam.right().abs_diff_eq(Vec3::X, 1e-6));
    }

    #[test]
    fn ray_matches_projection() {
        let mut cam = OrbitCamera {
            target: Vec3::new(3.0, -2.0, 1.0),
            distance: 40.0,
            ..Default::default()
        };
        let (aspect, radius) = (1.6, 25.0);
        for projection in [Projection::Perspective, Projection::Orthographic] {
            cam.projection = projection;
            let view_proj = cam.projection_matrix(aspect, radius) * cam.view();
            for ndc in [Vec2::ZERO, Vec2::new(0.5, -0.25), Vec2::new(-1.0, 1.0)] {
                let (origin, dir) = cam.ray(ndc, aspect, radius);
                for t in [1.0, 30.0] {
                    let clip = view_proj * (origin + dir * t).extend(1.0);
                    let projected = clip.truncate().truncate() / clip.w;
                    assert!(
                        projected.abs_diff_eq(ndc, 1e-4),
                        "{projection:?} {ndc} -> {projected}"
                    );
                }
            }
        }
        // The orthographic ray starts in front of everything visible.
        cam.projection = Projection::Orthographic;
        let (origin, _) = cam.ray(Vec2::ZERO, aspect, radius);
        let clip = cam.projection_matrix(aspect, radius) * cam.view() * origin.extend(1.0);
        assert!(clip.z / clip.w <= 1e-4);
    }

    #[test]
    fn fit_contains_box() {
        let mut cam = OrbitCamera::default();
        cam.fit(Vec3::splat(-10.0), Vec3::splat(10.0));
        assert_eq!(cam.target, Vec3::ZERO);
        assert!(cam.distance > 3f32.sqrt() * 10.0);
    }
}
