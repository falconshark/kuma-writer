#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod config;
mod docs;
mod fonts;
mod journal;
mod lock;
mod scene;
mod stats;
mod theme;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let cfg = config::Config::load();
    // 小黑屋未結束時一律以全螢幕啟動
    let fullscreen = cfg.start_fullscreen || lock::LockSession::load().is_some();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Kuma Write")
            .with_app_id("kuma-write")
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([480.0, 360.0])
            .with_fullscreen(fullscreen),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        "Kuma Write",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, cfg)))),
    )
}
