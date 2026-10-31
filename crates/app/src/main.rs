// No console window for release builds started from Explorer.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod viewport;

use std::path::PathBuf;

use anyhow::{Context as _, anyhow, bail};
use eframe::{egui, egui_wgpu};
use render::{HeadlessGpu, OrbitCamera, Scene};

const APP_TITLE: &str = "OpenSCAD Rust (prototipo)";
const USAGE: &str =
    "uso: app [file.stl] | app --render-png <input.stl> <output.png> [LARGHEZZAxALTEZZA]";

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(
        // wgpu_hal at warn level reports harmless Vulkan loader registry noise.
        env_logger::Env::default().default_filter_or("info,wgpu_core=warn,wgpu_hal=error,naga=warn"),
    )
    .init();

    let mut args = std::env::args_os().skip(1);
    match args.next() {
        Some(flag) if flag == "--render-png" => {
            let input = PathBuf::from(args.next().context(USAGE)?);
            let output = PathBuf::from(args.next().context(USAGE)?);
            let size = match args.next() {
                Some(size) => parse_size(&size.to_string_lossy())?,
                None => (1280, 800),
            };
            render_png(&input, &output, size)
        }
        Some(flag) if flag == "--help" || flag == "-h" => {
            println!("{USAGE}");
            Ok(())
        }
        first => run_gui(first.map(PathBuf::from)),
    }
}

fn run_gui(initial: Option<PathBuf>) -> anyhow::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_TITLE)
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([640.0, 400.0])
            .with_drag_and_drop(true),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: egui_wgpu::WgpuConfiguration {
            wgpu_setup: egui_wgpu::WgpuSetupCreateNew {
                // Prefer the discrete GPU on hybrid systems.
                power_preference: egui_wgpu::wgpu::PowerPreference::HighPerformance,
                ..egui_wgpu::WgpuSetupCreateNew::without_display_handle()
            }
            .into(),
            ..Default::default()
        },
        ..Default::default()
    };
    eframe::run_native(
        APP_TITLE,
        options,
        Box::new(move |cc| Ok(Box::new(app::ViewerApp::new(cc, initial)?))),
    )
    .map_err(|err| anyhow!("{err}"))
}

/// Renders an STL to a PNG without opening a window.
fn render_png(
    input: &PathBuf,
    output: &PathBuf,
    (width, height): (u32, u32),
) -> anyhow::Result<()> {
    let mesh =
        mesh_io::read_stl_file(input).with_context(|| format!("lettura di {}", input.display()))?;
    let gpu = HeadlessGpu::new()?;
    log::info!(
        "GPU: {} · {:?}",
        gpu.adapter_info.name,
        gpu.adapter_info.backend
    );

    let mut scene = Scene::new(&gpu.device);
    scene.set_mesh(&gpu.device, &mesh);
    let mut camera = OrbitCamera::default();
    if let Some((min, max)) = scene.bounds() {
        camera.fit(min, max);
    }
    render::render_image(&gpu, &mut scene, &camera, width, height)?
        .save_png(output)
        .with_context(|| format!("scrittura di {}", output.display()))?;
    log::info!("Salvato {}", output.display());
    Ok(())
}

fn parse_size(text: &str) -> anyhow::Result<(u32, u32)> {
    let Some((w, h)) = text.split_once(['x', 'X']) else {
        bail!("dimensione non valida {text:?}, atteso LARGHEZZAxALTEZZA");
    };
    let (w, h): (u32, u32) = (w.trim().parse()?, h.trim().parse()?);
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        bail!("dimensione fuori intervallo: {w}x{h}");
    }
    Ok((w, h))
}
