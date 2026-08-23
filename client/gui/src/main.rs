#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_os = "windows"))]
compile_error!("usb-bridge-gui is a Windows-only native application");

mod app;
mod messages;
mod state;
mod theme;
mod views;

use std::{fs, path::PathBuf, sync::Arc};

use eframe::egui;
use usb_bridge_client_core::logging::FileLogger;

fn main() -> eframe::Result {
    let japanese_font = load_windows_japanese_font();
    let logger = FileLogger::new().ok().map(Arc::new);
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_title("LAN USB Bridge")
            .with_inner_size([960.0, 720.0])
            .with_min_inner_size([760.0, 560.0]),
        centered: true,
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(
        "LAN USB Bridge",
        options,
        Box::new(move |creation| {
            Ok(Box::new(app::BridgeApp::new(
                creation,
                japanese_font,
                logger,
            )))
        }),
    )
}

fn load_windows_japanese_font() -> Option<Vec<u8>> {
    let windows = std::env::var_os("WINDIR").map(PathBuf::from)?;
    ["YuGothM.ttc", "meiryo.ttc", "msgothic.ttc"]
        .into_iter()
        .find_map(|name| fs::read(windows.join("Fonts").join(name)).ok())
}
