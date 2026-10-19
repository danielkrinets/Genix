#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! Genix — параметрический 3D-CAD.

mod app;
mod camera;
mod commands;
mod examples;
mod files;
mod edges;
mod desktop;
mod gpu;
mod home;
mod backgrounds;
mod mascot;
mod updates;
mod icons;
mod trace_ui;
mod i18n;
mod i18n_en;
mod i18n_he;
mod render;
mod settings;
mod sketcher;
mod theme;
mod thumb;
mod toolbar;
mod viewcube;
mod widgets;

use eframe::egui;

fn main() -> eframe::Result {
    // файл можно передать аргументом (двойной щелчок по документу)
    let open = std::env::args().nth(1).filter(|a| !a.starts_with('-')).map(std::path::PathBuf::from);
    desktop::install();
    eframe::run_native(
        "Genix",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1360.0, 860.0]).with_min_inner_size([900.0, 560.0]).with_title("Genix").with_icon(std::sync::Arc::new(window_icon())),
            window_builder: Some(Box::new(|attributes| { desktop::install(); attributes })),
            depth_buffer: 32,
            multisampling: app::MSAA as u16,
            renderer: eframe::Renderer::Wgpu,
            ..Default::default()
        },
        Box::new(|cc| {
            // Winit installs its default macOS menu while creating the window,
            // so apply Genix's document menu once the native window exists.
            desktop::ensure_menu();
            desktop::attach(&cc.egui_ctx);
            Ok(Box::new(app::App::new(cc, open)))
        }),
    )
}

/// Значок окна (Windows/Linux; на macOS значок берётся из .app).
fn window_icon() -> egui::IconData {
    match image::load_from_memory(include_bytes!("../assets/logo.png")) {
        Ok(img) => {
            let rgba = img.to_rgba8();
            egui::IconData { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() }
        }
        Err(_) => egui::IconData::default(),
    }
}
