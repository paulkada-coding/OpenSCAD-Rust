//! GPU scene: owns pipelines, buffers and offscreen targets, and renders the
//! model with grid and axes into a texture that the UI (or a PNG export)
//! can consume.

use bytemuck::{Pod, Zeroable};
use geometry::Mesh;
use glam::{Mat4, Vec2, Vec3, Vec4};
use wgpu::util::DeviceExt;

use crate::camera::{OrbitCamera, Projection};

/// Storage format of the output texture. Its bytes are sRGB-encoded, which is
/// what egui and PNG files expect.
pub const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Format used while rendering, so the hardware does the sRGB encoding.
const RENDER_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SAMPLE_COUNT: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneStyle {
    /// Colors are sRGB, as usually written by hand.
    pub model_color: [f32; 3],
    pub back_face_color: [f32; 3],
    pub background_top: [f32; 3],
    pub background_bottom: [f32; 3],
    pub show_grid: bool,
    pub show_axes: bool,
    pub show_edges: bool,
}

impl Default for SceneStyle {
    fn default() -> Self {
        Self {
            model_color: [0.976, 0.843, 0.173], // OpenSCAD yellow #F9D72C
            back_face_color: [0.33, 0.73, 0.40],
            background_top: [0.20, 0.22, 0.26],
            background_bottom: [0.07, 0.08, 0.09],
            show_grid: true,
            show_axes: true,
            show_edges: true,
        }
    }
}

/// Creases sharper than this are outlined.
const FEATURE_EDGE_ANGLE_DEG: f64 = 25.0;
const EDGE_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 0.55];
/// sRGB color of picked triangles: a saturated blue.
const SELECTION_COLOR: [f32; 3] = [0.20, 0.55, 1.0];

/// What a mesh represents, which decides how it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerKind {
    /// The model: opaque, with outlined edges.
    Model,
    /// `#` subtrees: translucent red, like OpenSCAD's preview.
    Highlight,
    /// `%` subtrees: translucent grey, not part of the result.
    Background,
    /// Picked triangles, drawn over the model in a color that stands out
    /// from the yellow model, the green back faces and the red `#`.
    Selection,
}

impl LayerKind {
    /// sRGB color and opacity of transparent layers.
    fn translucent_color(self) -> Option<([f32; 3], f32)> {
        match self {
            Self::Model => None,
            Self::Selection => Some((SELECTION_COLOR, 1.0)),
            // OpenSCAD's highlight color, RGB (255, 81, 81).
            Self::Highlight => Some(([1.0, 81.0 / 255.0, 81.0 / 255.0], 0.5)),
            Self::Background => Some(([0.71, 0.71, 0.71], 0.3)),
        }
    }
}

/// A mesh to show, with its role.
pub struct LayerMesh<'a> {
    pub kind: LayerKind,
    pub mesh: &'a Mesh,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LayerUniforms {
    front: [f32; 4],
    back: [f32; 4],
}

struct Layer {
    kind: LayerKind,
    triangles: VertexBuffer,
    edges: Option<VertexBuffer>,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    forward: [f32; 4],
    key_dir: [f32; 4],
    fill_dir: [f32; 4],
    model_color: [f32; 4],
    back_color: [f32; 4],
    bg_top: [f32; 4],
    bg_bottom: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MeshVertex {
    position: [f32; 3],
    normal: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LineVertex {
    position: [f32; 3],
    color: [f32; 4],
}

struct VertexBuffer {
    buffer: wgpu::Buffer,
    count: u32,
}

struct Targets {
    size: [u32; 2],
    msaa_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    output: wgpu::Texture,
    /// sRGB view of `output`, used as the MSAA resolve target.
    resolve_view: wgpu::TextureView,
    /// Plain view of `output` for sampling as raw sRGB bytes.
    output_view: wgpu::TextureView,
}

pub struct Scene {
    background_pipeline: wgpu::RenderPipeline,
    mesh_pipeline: wgpu::RenderPipeline,
    translucent_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    edge_pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    layer_bind_group_layout: wgpu::BindGroupLayout,
    layers: Vec<Layer>,
    /// Picked triangles, drawn over the model.
    selection: Option<Layer>,
    grid: Option<VertexBuffer>,
    axes: Option<VertexBuffer>,
    bounds: Option<(Vec3, Vec3)>,
    targets: Option<Targets>,
    style: SceneStyle,
}

impl Scene {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("shaders/scene.wgsl"));

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let layer_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("layer uniforms"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let mesh_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene mesh"),
            bind_group_layouts: &[Some(&bind_group_layout), Some(&layer_bind_group_layout)],
            immediate_size: 0,
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene uniforms"),
            size: size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene uniforms"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let pipeline = |label: &str,
                        layout: &wgpu::PipelineLayout,
                        vs: &str,
                        fs: &str,
                        buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
                        topology: wgpu::PrimitiveTopology,
                        depth_write: bool,
                        depth_compare: wgpu::CompareFunction,
                        blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState {
                    topology,
                    // Back faces are shaded differently, not culled.
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(depth_compare),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: SAMPLE_COUNT,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: RENDER_FORMAT,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };

        let background_pipeline = pipeline(
            "background",
            &layout,
            "vs_background",
            "fs_background",
            &[],
            wgpu::PrimitiveTopology::TriangleList,
            false,
            wgpu::CompareFunction::Always,
            None,
        );
        let mesh_buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: size_of::<MeshVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3],
        })];
        let mesh_pipeline = pipeline(
            "mesh",
            &mesh_layout,
            "vs_mesh",
            "fs_mesh",
            &mesh_buffers,
            wgpu::PrimitiveTopology::TriangleList,
            true,
            wgpu::CompareFunction::Less,
            None,
        );
        // Translucent layers test against the model but do not hide each
        // other; LessEqual lets them show on faces they share with it.
        let translucent_pipeline = pipeline(
            "translucent mesh",
            &mesh_layout,
            "vs_mesh",
            "fs_mesh",
            &mesh_buffers,
            wgpu::PrimitiveTopology::TriangleList,
            false,
            wgpu::CompareFunction::LessEqual,
            Some(wgpu::BlendState::ALPHA_BLENDING),
        );
        let line_layout = [Some(wgpu::VertexBufferLayout {
            array_stride: size_of::<LineVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4],
        })];
        let line_pipeline = pipeline(
            "lines",
            &layout,
            "vs_line",
            "fs_line",
            &line_layout,
            wgpu::PrimitiveTopology::LineList,
            false,
            wgpu::CompareFunction::LessEqual,
            Some(wgpu::BlendState::ALPHA_BLENDING),
        );
        let edge_pipeline = pipeline(
            "edges",
            &layout,
            "vs_edge",
            "fs_line",
            &line_layout,
            wgpu::PrimitiveTopology::LineList,
            false,
            wgpu::CompareFunction::LessEqual,
            Some(wgpu::BlendState::ALPHA_BLENDING),
        );

        let mut scene = Self {
            background_pipeline,
            mesh_pipeline,
            translucent_pipeline,
            line_pipeline,
            edge_pipeline,
            uniform_buffer,
            bind_group,
            layer_bind_group_layout,
            layers: Vec::new(),
            selection: None,
            grid: None,
            axes: None,
            bounds: None,
            targets: None,
            style: SceneStyle::default(),
        };
        scene.rebuild_overlays(device);
        scene
    }

    pub fn style(&self) -> &SceneStyle {
        &self.style
    }

    pub fn set_style(&mut self, style: SceneStyle) {
        self.style = style;
    }

    /// Bounds of the current model, if any.
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        self.bounds
    }

    /// Shows a single opaque model.
    pub fn set_mesh(&mut self, device: &wgpu::Device, mesh: &Mesh) {
        self.set_layers(
            device,
            &[LayerMesh {
                kind: LayerKind::Model,
                mesh,
            }],
        );
    }

    /// Replaces everything shown, clearing the selection. Empty meshes are
    /// skipped.
    pub fn set_layers(&mut self, device: &wgpu::Device, layers: &[LayerMesh<'_>]) {
        self.layers.clear();
        self.selection = None;
        let mut bounds = geometry::Aabb::EMPTY;
        for layer in layers {
            let mesh_bounds = layer.mesh.bounds();
            if mesh_bounds.is_empty() {
                continue;
            }
            bounds.include(mesh_bounds.min);
            bounds.include(mesh_bounds.max);
            self.layers.push(self.upload_layer(device, layer));
        }
        self.bounds = (!bounds.is_empty()).then(|| (bounds.min.as_vec3(), bounds.max.as_vec3()));
        self.rebuild_overlays(device);
    }

    pub fn clear_mesh(&mut self, device: &wgpu::Device) {
        self.set_layers(device, &[]);
    }

    /// Highlights some triangles of the model (normally a subset of its
    /// triangles, so they cover it exactly), or clears the highlight.
    pub fn set_selection(&mut self, device: &wgpu::Device, mesh: Option<&Mesh>) {
        self.selection = mesh.filter(|m| !m.is_empty()).map(|mesh| {
            self.upload_layer(
                device,
                &LayerMesh {
                    kind: LayerKind::Selection,
                    mesh,
                },
            )
        });
    }

    /// The ray under a point of the image, for picking: `pos` and `size`
    /// in the same units (pixels or points), `pos` from the top-left
    /// corner. Returns `(origin, direction)` in world coordinates.
    pub fn pick_ray(&self, camera: &OrbitCamera, size: [f32; 2], pos: [f32; 2]) -> (Vec3, Vec3) {
        let (w, h) = (size[0].max(1.0), size[1].max(1.0));
        let ndc = Vec2::new(pos[0] / w * 2.0 - 1.0, 1.0 - pos[1] / h * 2.0);
        camera.ray(ndc, w / h, self.scene_radius())
    }

    fn upload_layer(&self, device: &wgpu::Device, layer: &LayerMesh<'_>) -> Layer {
        let mesh = layer.mesh;
        // Flat shading: every triangle gets its own vertices and face normal.
        let mut vertices = Vec::with_capacity(mesh.triangle_count() * 3);
        for (i, tri) in mesh.triangles().enumerate() {
            let normal = mesh.face_normal(i).as_vec3().to_array();
            for p in tri {
                vertices.push(MeshVertex {
                    position: p.as_vec3().to_array(),
                    normal,
                });
            }
        }

        // Only the model gets outlines; translucent layers stay light.
        let edges = (layer.kind == LayerKind::Model)
            .then(|| {
                let lines: Vec<LineVertex> = mesh
                    .feature_edges(FEATURE_EDGE_ANGLE_DEG)
                    .iter()
                    .flatten()
                    .map(|&v| LineVertex {
                        position: mesh.vertices[v as usize].as_vec3().to_array(),
                        color: EDGE_COLOR,
                    })
                    .collect();
                (!lines.is_empty()).then(|| create_vertex_buffer(device, "edges", &lines))
            })
            .flatten();

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("layer uniforms"),
            size: size_of::<LayerUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("layer uniforms"),
            layout: &self.layer_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });
        Layer {
            kind: layer.kind,
            triangles: create_vertex_buffer(device, "mesh", &vertices),
            edges,
            uniform_buffer,
            bind_group,
        }
    }

    fn layer_uniforms(&self, kind: LayerKind) -> LayerUniforms {
        match kind.translucent_color() {
            None => LayerUniforms {
                front: linear(self.style.model_color),
                back: linear(self.style.back_face_color),
            },
            Some((rgb, alpha)) => {
                let mut front = linear(rgb);
                front[3] = alpha;
                // Inner faces of translucent shapes are slightly darker.
                let back = [front[0] * 0.7, front[1] * 0.7, front[2] * 0.7, alpha];
                LayerUniforms { front, back }
            }
        }
    }

    fn rebuild_overlays(&mut self, device: &wgpu::Device) {
        let extent = grid_extent(self.bounds);
        let (grid, axes) = build_overlays(extent);
        self.grid = Some(create_vertex_buffer(device, "grid", &grid));
        self.axes = Some(create_vertex_buffer(device, "axes", &axes));
    }

    /// Renders into the offscreen output texture, resizing it if needed.
    /// Returns `true` when the output texture was (re)created, in which case
    /// any external reference to [`Self::output_view`] must be refreshed.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera: &OrbitCamera,
        size: [u32; 2],
    ) -> bool {
        let size = [size[0].max(1), size[1].max(1)];
        let recreated = self.targets.as_ref().is_none_or(|t| t.size != size);
        if recreated {
            self.targets = Some(create_targets(device, size));
        }
        let targets = self.targets.as_ref().expect("targets were just created");

        let aspect = size[0] as f32 / size[1] as f32;
        let scene_radius = self.scene_radius();
        let view_proj = camera.projection_matrix(aspect, scene_radius) * camera.view();
        queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::bytes_of(&self.uniforms(camera, view_proj)),
        );
        for layer in self.layers.iter().chain(&self.selection) {
            queue.write_buffer(
                &layer.uniform_buffer,
                0,
                bytemuck::bytes_of(&self.layer_uniforms(layer.kind)),
            );
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("scene"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.msaa_view,
                    depth_slice: None,
                    resolve_target: Some(&targets.resolve_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.bind_group, &[]);

            pass.set_pipeline(&self.background_pipeline);
            pass.draw(0..3, 0..1);

            let (opaque, translucent): (Vec<&Layer>, Vec<&Layer>) =
                self.layers.iter().partition(|l| l.kind == LayerKind::Model);

            pass.set_pipeline(&self.mesh_pipeline);
            for layer in &opaque {
                pass.set_bind_group(1, &layer.bind_group, &[]);
                pass.set_vertex_buffer(0, layer.triangles.buffer.slice(..));
                pass.draw(0..layer.triangles.count, 0..1);
            }
            // Over the model's own faces (equal depth passes), under the
            // edges so the shape stays readable.
            if let Some(layer) = &self.selection {
                pass.set_pipeline(&self.translucent_pipeline);
                pass.set_bind_group(1, &layer.bind_group, &[]);
                pass.set_vertex_buffer(0, layer.triangles.buffer.slice(..));
                pass.draw(0..layer.triangles.count, 0..1);
            }
            if self.style.show_edges {
                pass.set_pipeline(&self.edge_pipeline);
                for edges in opaque.iter().filter_map(|l| l.edges.as_ref()) {
                    pass.set_vertex_buffer(0, edges.buffer.slice(..));
                    pass.draw(0..edges.count, 0..1);
                }
            }

            pass.set_pipeline(&self.line_pipeline);
            let overlays = [
                (self.style.show_grid, &self.grid),
                (self.style.show_axes, &self.axes),
            ];
            for (visible, lines) in overlays {
                if let (true, Some(lines)) = (visible, lines) {
                    pass.set_vertex_buffer(0, lines.buffer.slice(..));
                    pass.draw(0..lines.count, 0..1);
                }
            }

            // Last, so the grid and the model show through them.
            pass.set_pipeline(&self.translucent_pipeline);
            for layer in &translucent {
                pass.set_bind_group(1, &layer.bind_group, &[]);
                pass.set_vertex_buffer(0, layer.triangles.buffer.slice(..));
                pass.draw(0..layer.triangles.count, 0..1);
            }
        }
        queue.submit([encoder.finish()]);
        recreated
    }

    /// View of the last rendered image (format [`OUTPUT_FORMAT`]).
    pub fn output_view(&self) -> Option<&wgpu::TextureView> {
        self.targets.as_ref().map(|t| &t.output_view)
    }

    pub fn output_texture(&self) -> Option<&wgpu::Texture> {
        self.targets.as_ref().map(|t| &t.output)
    }

    fn scene_radius(&self) -> f32 {
        let extent = grid_extent(self.bounds);
        let model = self
            .bounds
            .map_or(0.0, |(min, max)| min.abs().max(max.abs()).length());
        extent.max(model) * 1.5
    }

    fn uniforms(&self, camera: &OrbitCamera, view_proj: Mat4) -> Uniforms {
        let (forward, up, right) = (camera.forward(), camera.up(), camera.right());
        // Lights move with the camera so the model is always well lit.
        let key = (-forward * 0.6 + up - right * 0.55).normalize();
        let fill = (-forward + right * 0.6 - up * 0.2).normalize();
        let ortho = if camera.projection == Projection::Orthographic {
            1.0
        } else {
            0.0
        };
        let style = &self.style;
        Uniforms {
            view_proj: view_proj.to_cols_array_2d(),
            eye: camera.eye().extend(1.0).to_array(),
            forward: forward.extend(ortho).to_array(),
            key_dir: key.extend(0.0).to_array(),
            fill_dir: fill.extend(0.0).to_array(),
            model_color: linear(style.model_color),
            back_color: linear(style.back_face_color),
            bg_top: linear(style.background_top),
            bg_bottom: linear(style.background_bottom),
        }
    }
}

fn linear(srgb: [f32; 3]) -> [f32; 4] {
    let channel = |c: f32| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    [channel(srgb[0]), channel(srgb[1]), channel(srgb[2]), 1.0]
}

fn create_vertex_buffer<T: Pod>(device: &wgpu::Device, label: &str, data: &[T]) -> VertexBuffer {
    VertexBuffer {
        buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytemuck::cast_slice(data),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        count: data.len() as u32,
    }
}

fn create_targets(device: &wgpu::Device, size: [u32; 2]) -> Targets {
    let extent = wgpu::Extent3d {
        width: size[0],
        height: size[1],
        depth_or_array_layers: 1,
    };
    let texture = |label: &str, format, samples, usage, view_formats: &[wgpu::TextureFormat]| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: extent,
            mip_level_count: 1,
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats,
        })
    };

    let msaa = texture(
        "scene msaa",
        RENDER_FORMAT,
        SAMPLE_COUNT,
        wgpu::TextureUsages::RENDER_ATTACHMENT,
        &[],
    );
    let depth = texture(
        "scene depth",
        DEPTH_FORMAT,
        SAMPLE_COUNT,
        wgpu::TextureUsages::RENDER_ATTACHMENT,
        &[],
    );
    let output = texture(
        "scene output",
        OUTPUT_FORMAT,
        1,
        wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        &[RENDER_FORMAT],
    );
    let resolve_view = output.create_view(&wgpu::TextureViewDescriptor {
        format: Some(RENDER_FORMAT),
        ..Default::default()
    });
    Targets {
        size,
        msaa_view: msaa.create_view(&Default::default()),
        depth_view: depth.create_view(&Default::default()),
        output_view: output.create_view(&Default::default()),
        resolve_view,
        output,
    }
}

/// Half-size of the square grid, large enough to frame the model.
fn grid_extent(bounds: Option<(Vec3, Vec3)>) -> f32 {
    let reach = bounds.map_or(50.0, |(min, max)| {
        min.x
            .abs()
            .max(max.x.abs())
            .max(min.y.abs())
            .max(max.y.abs())
            .max((max - min).max_element())
    });
    let step = major_step(reach);
    ((reach * 1.2) / step).ceil().max(1.0) * step
}

/// A "nice" major grid spacing (1, 2 or 5 × 10ⁿ) giving ~5 lines per side.
fn major_step(extent: f32) -> f32 {
    let raw = (extent / 5.0).max(1e-6);
    let magnitude = 10f32.powf(raw.log10().floor());
    let fraction = raw / magnitude;
    let nice = if fraction < 1.5 {
        1.0
    } else if fraction < 3.5 {
        2.0
    } else if fraction < 7.5 {
        5.0
    } else {
        10.0
    };
    nice * magnitude
}

fn build_overlays(extent: f32) -> (Vec<LineVertex>, Vec<LineVertex>) {
    let major = major_step(extent);
    let minor = major / 5.0;
    let minor_color = [0.55, 0.58, 0.62, 0.10];
    let major_color = [0.65, 0.68, 0.72, 0.28];

    let mut grid = Vec::new();
    let line = |a: Vec3, b: Vec3, color: [f32; 4], out: &mut Vec<LineVertex>| {
        out.push(LineVertex {
            position: a.to_array(),
            color,
        });
        out.push(LineVertex {
            position: b.to_array(),
            color,
        });
    };

    let steps = (extent / minor).round() as i32;
    for i in -steps..=steps {
        if i == 0 {
            continue; // the axes are drawn there
        }
        let t = i as f32 * minor;
        let color = if i % 5 == 0 { major_color } else { minor_color };
        line(
            Vec3::new(t, -extent, 0.0),
            Vec3::new(t, extent, 0.0),
            color,
            &mut grid,
        );
        line(
            Vec3::new(-extent, t, 0.0),
            Vec3::new(extent, t, 0.0),
            color,
            &mut grid,
        );
    }

    let mut axes = Vec::new();
    let axis_colors = [
        (Vec3::X, Vec4::new(0.90, 0.20, 0.20, 1.0)),
        (Vec3::Y, Vec4::new(0.25, 0.80, 0.25, 1.0)),
        (Vec3::Z, Vec4::new(0.25, 0.45, 0.95, 1.0)),
    ];
    for (dir, color) in axis_colors {
        let faded = (color.truncate() * 0.6).extend(0.45).to_array();
        line(Vec3::ZERO, dir * extent, color.to_array(), &mut axes);
        line(Vec3::ZERO, -dir * extent, faded, &mut axes);
    }
    (grid, axes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn major_steps_are_nice() {
        assert_eq!(major_step(50.0), 10.0);
        assert_eq!(major_step(12.0), 2.0);
        assert_eq!(major_step(240.0), 50.0);
    }

    #[test]
    fn grid_covers_model() {
        let extent = grid_extent(Some((
            Vec3::new(-3.0, 0.0, 0.0),
            Vec3::new(37.0, 20.0, 5.0),
        )));
        assert!(extent >= 40.0);
    }

    #[test]
    fn srgb_to_linear() {
        assert_eq!(linear([0.0, 1.0, 0.0])[..3], [0.0, 1.0, 0.0]);
        assert!((linear([0.5, 0.5, 0.5])[0] - 0.214).abs() < 1e-3);
    }
}
