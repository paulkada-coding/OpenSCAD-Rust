//! GPU rendering of models with wgpu (DirectX 12 / Vulkan / Metal).
//!
//! The scene renders offscreen into a texture: the UI displays it, and the
//! same path produces PNG snapshots without a window.

pub mod camera;
pub mod headless;
pub mod scene;

pub use camera::{OrbitCamera, Projection, ViewPreset};
pub use headless::{HeadlessGpu, Image, RenderError, render_image};
pub use scene::{LayerKind, LayerMesh, OUTPUT_FORMAT, Scene, SceneStyle};
pub use wgpu;
