//! AeroFlow — interaktywny tunel aerodynamiczny 2D.
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod analysis;
mod app;
mod physics;
mod prefs;
mod render;
mod scene;
mod sim;
mod ui;
mod update;

use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    env_logger::init();
    // aeroflow [plik.aeroflow] [--preset ID] [--field NAZWA]
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let launch = app::Launch {
        open: args.iter().find(|a| !a.starts_with("--") && std::path::Path::new(a).exists()).map(PathBuf::from),
        preset: flag("--preset"),
        field: flag("--field"),
    };
    let open = launch;
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).unwrap_or_default();
    let options = |renderer| eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(format!("AeroFlow {}", update::VERSION))
            .with_app_id("aeroflow")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 600.0])
            .with_icon(icon.clone()),
        vsync: true,
        renderer,
        ..Default::default()
    };
    // OpenGL działa prawie wszędzie; gdy sterownik go nie udostępnia (maszyny wirtualne, pulpit
    // zdalny), próbujemy wgpu (DirectX 12 / Vulkan / Metal).
    let first = open.clone();
    match eframe::run_native("AeroFlow", options(eframe::Renderer::Glow), Box::new(move |cc| Ok(Box::new(app::App::new(cc, first))))) {
        Ok(()) => Ok(()),
        Err(e) => {
            log::warn!("OpenGL niedostępny ({e}), próba z wgpu");
            eframe::run_native("AeroFlow", options(eframe::Renderer::Wgpu), Box::new(move |cc| Ok(Box::new(app::App::new(cc, open)))))
        }
    }
}
