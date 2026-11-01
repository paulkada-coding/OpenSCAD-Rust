//! Rendering without a window: used for PNG snapshots, tests and, later,
//! for giving the AI assistant a picture of the model.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use crate::camera::OrbitCamera;
use crate::scene::Scene;

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("no compatible GPU adapter: {0}")]
    Adapter(#[from] wgpu::RequestAdapterError),
    #[error("failed to initialize GPU device: {0}")]
    Device(#[from] wgpu::RequestDeviceError),
    #[error("GPU poll error: {0}")]
    Poll(#[from] wgpu::PollError),
    #[error("failed to read image back from GPU: {0}")]
    Readback(String),
    #[error("PNG encoding error: {0}")]
    Png(#[from] png::EncodingError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct HeadlessGpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_info: wgpu::AdapterInfo,
}

impl HeadlessGpu {
    /// Picks the high-performance GPU (the discrete card on hybrid systems).
    pub fn new() -> Result<Self, RenderError> {
        pollster::block_on(async {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::PRIMARY,
                ..wgpu::InstanceDescriptor::new_without_display_handle()
            });
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    ..Default::default()
                })
                .await?;
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("headless"),
                    ..Default::default()
                })
                .await?;
            Ok(Self {
                device,
                queue,
                adapter_info: adapter.get_info(),
            })
        })
    }
}

/// An RGBA8 image with sRGB-encoded pixels, rows top to bottom.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Image {
    pub fn save_png(&self, path: impl AsRef<Path>) -> Result<(), RenderError> {
        let file = BufWriter::new(File::create(path)?);
        let mut encoder = png::Encoder::new(file, self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
        encoder.write_header()?.write_image_data(&self.pixels)?;
        Ok(())
    }
}

/// Renders the scene and copies the result back to the CPU.
pub fn render_image(
    gpu: &HeadlessGpu,
    scene: &mut Scene,
    camera: &OrbitCamera,
    width: u32,
    height: u32,
) -> Result<Image, RenderError> {
    scene.render(&gpu.device, &gpu.queue, camera, [width, height]);
    let texture = scene.output_texture().expect("render creates the output");

    // Buffer rows must be aligned to 256 bytes.
    let unpadded = width * 4;
    let padded =
        unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(padded) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("readback"),
        });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(height),
            },
        },
        texture.size(),
    );
    gpu.queue.submit([encoder.finish()]);

    let (sender, receiver) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
    gpu.device.poll(wgpu::PollType::wait_indefinitely())?;
    receiver
        .recv()
        .map_err(|e| RenderError::Readback(e.to_string()))?
        .map_err(|e| RenderError::Readback(e.to_string()))?;

    let mut pixels = Vec::with_capacity((unpadded * height) as usize);
    {
        let mapped = buffer
            .slice(..)
            .get_mapped_range()
            .map_err(|e| RenderError::Readback(e.to_string()))?;
        for row in mapped.chunks_exact(padded as usize) {
            pixels.extend_from_slice(&row[..unpadded as usize]);
        }
    }
    buffer.unmap();

    Ok(Image {
        width,
        height,
        pixels,
    })
}
