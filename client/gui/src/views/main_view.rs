use eframe::egui::{self, Color32, RichText};
use usb_bridge_client_core::{
    device_policy::{PolicyLevel, evaluate},
    grouping::group_devices,
};
use usb_bridge_protocol::UsbDevice;

use crate::{state::AppState, theme};

pub enum ViewAction {
    None,
    Refresh,
    OpenSettings,
    OpenLogs,
    Connect(Vec<UsbDevice>),
    Disconnect(Vec<String>),
}

pub fn show(ui: &mut egui::Ui, state: &AppState) -> ViewAction {
    let mut action = ViewAction::None;
    egui::Frame::central_panel(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.heading(RichText::new("LAN USB Bridge").size(24.0));
            ui.separator();
            let connected = state.snapshot.is_some();
            ui.colored_label(
                if connected { theme::GREEN } else { theme::RED },
                if connected {
                    "● サーバー接続済み"
                } else {
                    "● 未接続"
                },
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("⚙ 設定").clicked() {
                    action = ViewAction::OpenSettings;
                }
                if ui
                    .add_enabled(!state.busy, egui::Button::new("↻ 再読込"))
                    .clicked()
                {
                    action = ViewAction::Refresh;
                }
            });
        });
        ui.add_space(12.0);
        ui.separator();

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if let Some(snapshot) = &state.snapshot {
                    let groups = group_devices(&snapshot.devices);
                    if groups.is_empty() {
                        ui.label("USBデバイスが見つかりません");
                    }
                    for group in groups {
                        let session = snapshot.session.as_ref();
                        let other_owner = session.is_some_and(|session| {
                            state
                                .config
                                .as_ref()
                                .is_none_or(|config| session.client_id != config.client_id)
                        });
                        let group_ids: Vec<String> = group
                            .devices
                            .iter()
                            .map(|device| device.bus_id.clone())
                            .collect();
                        let connected = session.is_some_and(|session| {
                            group_ids.iter().any(|id| session.devices.contains(id))
                        });
                        let prohibited = group
                            .devices
                            .iter()
                            .any(|device| evaluate(device).level == PolicyLevel::Prohibited);
                        egui::Frame::group(ui.style())
                            .fill(theme::CARD)
                            .corner_radius(8)
                            .inner_margin(14)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(RichText::new(&group.title).strong().size(17.0));
                                        ui.small(format!("グループ: {}", group.id));
                                    });
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if connected {
                                                if ui
                                                    .add_enabled(
                                                        !state.busy && !other_owner,
                                                        egui::Button::new("切断"),
                                                    )
                                                    .clicked()
                                                {
                                                    action =
                                                        ViewAction::Disconnect(group_ids.clone());
                                                }
                                            } else if ui
                                                .add_enabled(
                                                    !state.busy
                                                        && !other_owner
                                                        && !prohibited
                                                        && session.is_none(),
                                                    egui::Button::new(if prohibited {
                                                        "接続禁止"
                                                    } else {
                                                        "グループ接続"
                                                    }),
                                                )
                                                .clicked()
                                            {
                                                action = ViewAction::Connect(group.devices.clone());
                                            }
                                        },
                                    );
                                });
                                ui.separator();
                                for device in &group.devices {
                                    device_row(ui, device, connected, other_owner);
                                }
                            });
                        ui.add_space(10.0);
                    }
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label("再読込してサーバーへ接続してください");
                    });
                }
            });

        ui.separator();
        let owner = state
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.session.as_ref())
            .map(|session| session.client_id.as_str())
            .unwrap_or("なし");
        ui.horizontal(|ui| {
            ui.label(format!("セッション所有者: {owner}"));
            ui.separator();
            if state.busy {
                ui.spinner();
            }
            ui.label(&state.progress);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("詳細ログ").clicked() {
                    action = ViewAction::OpenLogs;
                }
            });
        });
        if let Some(error) = &state.last_error {
            ui.colored_label(theme::RED, format!("✕ 最後のエラー: {error}"));
        }
    });
    action
}

fn device_row(ui: &mut egui::Ui, device: &UsbDevice, connected: bool, other_owner: bool) {
    let policy = evaluate(device);
    let (color, icon) = match policy.level {
        PolicyLevel::Allowed => (theme::GREEN, "✓"),
        PolicyLevel::Warning => (theme::YELLOW, "⚠"),
        PolicyLevel::Prohibited => (theme::RED, "⊘"),
    };
    ui.horizontal(|ui| {
        ui.colored_label(color, format!("{icon} {}", policy.label));
        ui.vertical(|ui| {
            ui.label(RichText::new(device.product.as_deref().unwrap_or("製品名不明")).strong());
            ui.small(format!(
                "BUS ID: {}    VID:PID {}:{}",
                device.bus_id, device.vendor_id, device.product_id
            ));
            if policy.level != PolicyLevel::Allowed {
                ui.colored_label(color, policy.detail);
            }
            if let Some(warning) = &device.warning {
                ui.small(warning);
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (status_color, status) = if other_owner {
                (theme::GRAY, "他PCが使用中")
            } else if policy.level == PolicyLevel::Prohibited {
                (theme::RED, "接続禁止")
            } else if connected {
                (theme::BLUE, "接続中")
            } else if policy.level == PolicyLevel::Warning {
                (theme::YELLOW, "要注意")
            } else {
                (Color32::LIGHT_GREEN, "利用可能")
            };
            ui.colored_label(status_color, status);
        });
    });
}
