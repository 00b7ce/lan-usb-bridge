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

pub fn show(ui: &mut egui::Ui, state: &mut AppState) -> ViewAction {
    let mut action = ViewAction::None;
    let snapshot = state.snapshot.clone();
    let other_owner = snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.session.as_ref())
        .is_some_and(|session| {
            state
                .config
                .as_ref()
                .is_none_or(|config| session.client_id != config.client_id)
        });
    let attach_targets: Vec<UsbDevice> = snapshot
        .as_ref()
        .map(|snapshot| {
            snapshot
                .devices
                .iter()
                .filter(|device| state.selected_devices.contains(&device.bus_id))
                .filter(|device| {
                    snapshot
                        .session
                        .as_ref()
                        .is_none_or(|session| !session.devices.contains(&device.bus_id))
                })
                .filter(|device| device.selectable)
                .filter(|device| evaluate(device).level != PolicyLevel::Prohibited)
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let detach_targets: Vec<String> = snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.session.as_ref())
        .map(|session| {
            session
                .devices
                .iter()
                .filter(|bus_id| state.selected_devices.contains(*bus_id))
                .cloned()
                .collect()
        })
        .unwrap_or_default();

    egui::Frame::central_panel(ui.style()).show(ui, |ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
        ui.horizontal(|ui| {
            ui.heading(RichText::new("LAN USB Bridge").size(21.0));
            ui.separator();
            let connected = snapshot.is_some();
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
        ui.add_space(4.0);
        ui.separator();

        ui.horizontal(|ui| {
            ui.label(format!("選択: {}台", state.selected_devices.len()));
            if ui
                .add_enabled(
                    !state.busy && !other_owner && !attach_targets.is_empty(),
                    egui::Button::new(format!("選択を接続 ({})", attach_targets.len())),
                )
                .clicked()
            {
                action = ViewAction::Connect(attach_targets.clone());
                state.selected_devices.clear();
            }
            if ui
                .add_enabled(
                    !state.busy && !other_owner && !detach_targets.is_empty(),
                    egui::Button::new(format!("選択を切断 ({})", detach_targets.len())),
                )
                .clicked()
            {
                action = ViewAction::Disconnect(detach_targets.clone());
                state.selected_devices.clear();
            }
            if ui
                .add_enabled(
                    !state.selected_devices.is_empty(),
                    egui::Button::new("選択解除"),
                )
                .clicked()
            {
                state.selected_devices.clear();
            }
            if other_owner {
                ui.colored_label(theme::GRAY, "他PCのセッション中");
            }
        });
        ui.separator();

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if let Some(snapshot) = &snapshot {
                    let groups = group_devices(&snapshot.devices);
                    if groups.is_empty() {
                        ui.label("USBデバイスが見つかりません");
                    }
                    for group in groups {
                        let session = snapshot.session.as_ref();
                        egui::Frame::group(ui.style())
                            .fill(theme::CARD)
                            .corner_radius(6)
                            .inner_margin(8)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&group.title).strong().size(14.0));
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.small(format!("USBパス: {}", group.id));
                                        },
                                    );
                                });
                                ui.separator();
                                for (index, device) in group.devices.iter().enumerate() {
                                    if index > 0 {
                                        ui.separator();
                                    }
                                    let connected = session.is_some_and(|session| {
                                        session.devices.contains(&device.bus_id)
                                    });
                                    let row_action = device_row(
                                        ui,
                                        device,
                                        connected,
                                        other_owner,
                                        state.busy,
                                        &mut state.selected_devices,
                                    );
                                    if !matches!(row_action, ViewAction::None) {
                                        action = row_action;
                                    }
                                }
                            });
                        ui.add_space(6.0);
                    }
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label("再読込してサーバーへ接続してください");
                    });
                }
            });

        ui.separator();
        let owner = snapshot
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

fn device_row(
    ui: &mut egui::Ui,
    device: &UsbDevice,
    connected: bool,
    other_owner: bool,
    busy: bool,
    selected_devices: &mut std::collections::BTreeSet<String>,
) -> ViewAction {
    let mut action = ViewAction::None;
    let policy = evaluate(device);
    let (color, icon) = match policy.level {
        PolicyLevel::Allowed => (theme::GREEN, "✓"),
        PolicyLevel::Warning => (theme::YELLOW, "⚠"),
        PolicyLevel::Prohibited => (theme::RED, "⊘"),
    };
    ui.horizontal(|ui| {
        let mut selected = selected_devices.contains(&device.bus_id);
        let can_select = !busy
            && !other_owner
            && (connected || (device.selectable && policy.level != PolicyLevel::Prohibited));
        if ui
            .add_enabled_ui(can_select, |ui| ui.checkbox(&mut selected, ""))
            .inner
            .changed()
        {
            if selected {
                selected_devices.insert(device.bus_id.clone());
            } else {
                selected_devices.remove(&device.bus_id);
            }
        }
        ui.colored_label(color, icon);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(device.product.as_deref().unwrap_or("製品名不明")).strong());
                ui.small(format!(
                    "{}  {}:{}",
                    device.bus_id, device.vendor_id, device.product_id
                ));
            });
            if policy.level != PolicyLevel::Allowed {
                ui.small(RichText::new(policy.detail).color(color));
            }
            if let Some(warning) = &device.warning {
                ui.small(warning);
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if connected {
                if ui
                    .add_enabled(!busy && !other_owner, egui::Button::new("切断"))
                    .clicked()
                {
                    action = ViewAction::Disconnect(vec![device.bus_id.clone()]);
                }
            } else if ui
                .add_enabled(
                    !busy && !other_owner && policy.level != PolicyLevel::Prohibited,
                    egui::Button::new(if policy.level == PolicyLevel::Prohibited {
                        "接続禁止"
                    } else {
                        "接続"
                    }),
                )
                .clicked()
            {
                action = ViewAction::Connect(vec![device.clone()]);
            }
            ui.add_space(4.0);
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
    action
}
