use std::collections::VecDeque;

use eframe::egui;

pub fn logs(ctx: &egui::Context, open: &mut bool, messages: &VecDeque<String>, file: Option<&str>) {
    egui::Window::new("詳細ログ")
        .open(open)
        .default_size([700.0, 420.0])
        .show(ctx, |ui| {
            if let Some(file) = file {
                ui.small(format!("ログファイル: {file}"));
            }
            ui.separator();
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for message in messages {
                        ui.monospace(message);
                    }
                });
        });
}
