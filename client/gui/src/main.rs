#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_os = "windows"))]
compile_error!("usb-bridge-gui is a Windows-only native application");

mod app;
mod messages;
mod state;
mod theme;
mod views;

use std::sync::Arc;

use eframe::egui;
use usb_bridge_client_core::logging::FileLogger;

fn main() -> eframe::Result {
    let logger = FileLogger::new().ok().map(Arc::new);
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_title("LAN USB Bridge")
            .with_inner_size([820.0, 600.0])
            .with_min_inner_size([680.0, 480.0]),
        centered: true,
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(
        "LAN USB Bridge",
        options,
        Box::new(move |creation| Ok(Box::new(app::BridgeApp::new(creation, logger)))),
    )
}
