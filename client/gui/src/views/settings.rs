use std::path::PathBuf;

use eframe::egui;
use usb_bridge_client_core::config::{self, Config};

use crate::state::SettingsDraft;

pub enum SettingsAction {
    None,
    Save(Config),
    Cancel,
}

pub fn show(ctx: &egui::Context, open: &mut bool, draft: &mut SettingsDraft) -> SettingsAction {
    let mut action = SettingsAction::None;
    let mut window_open = *open;
    egui::Window::new("設定")
        .open(&mut window_open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label("サーバーURL");
            ui.text_edit_singleline(&mut draft.server_url);
            ui.label("client_id");
            ui.text_edit_singleline(&mut draft.client_id);
            ui.label("usbip.exeのパス");
            ui.text_edit_singleline(&mut draft.usbip_path);
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("保存").clicked() {
                    match config::from_values(
                        &draft.server_url,
                        &draft.client_id,
                        PathBuf::from(&draft.usbip_path),
                    ) {
                        Ok(config) => action = SettingsAction::Save(config),
                        Err(error) => {
                            ui.colored_label(egui::Color32::RED, error.to_string());
                        }
                    }
                }
                if ui.button("キャンセル").clicked() {
                    action = SettingsAction::Cancel;
                }
            });
        });
    if !window_open {
        action = SettingsAction::Cancel;
    }
    action
}
