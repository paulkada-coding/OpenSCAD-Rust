//! GPU rendering of models with wgpu (DirectX 12 / Vulkan / Metal).
//!
//! The scene renders offscreen into a texture: the UI displays it, and the
//! same path produces PNG snapshots without a window.
//!
//! Everything but [`headless`] also builds for WebAssembly (WebGPU, with a
//! WebGL 2 fallback): the scene only needs a `wgpu::Device` and a queue,
//! wherever they come from. Headless rendering blocks on the GPU, which a
//! browser does not allow, so it is native only.

pub mod camera;
#[cfg(not(target_arch = "wasm32"))]
pub mod headless;
pub mod scene;

pub use camera::{OrbitCamera, Projection, ViewPreset};
#[cfg(not(target_arch = "wasm32"))]
pub use headless::{HeadlessGpu, Image, RenderError, render_image};
pub use scene::{LayerKind, LayerMesh, OUTPUT_FORMAT, Scene, SceneStyle};
pub use wgpu;
