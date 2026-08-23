use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use usb_bridge_client_core::{
    config::{self, Config, Overrides},
    connection,
    logging::FileLogger,
    usbip::WindowsUsbip,
};
use usb_bridge_protocol::UsbDevice;

use crate::{
    messages::WorkerMessage,
    state::{AppState, SettingsDraft},
    theme,
    views::{
        dialogs,
        main_view::{self, ViewAction},
        settings::{self, SettingsAction},
    },
};

pub struct BridgeApp {
    state: AppState,
    sender: Sender<WorkerMessage>,
    receiver: Receiver<WorkerMessage>,
    shutdown: Arc<AtomicBool>,
    log_sender: Option<Sender<String>>,
    log_thread: Option<JoinHandle<()>>,
    log_path: Option<String>,
}

impl BridgeApp {
    pub fn new(
        creation: &eframe::CreationContext<'_>,
        japanese_font: Option<Vec<u8>>,
        logger: Option<Arc<FileLogger>>,
    ) -> Self {
        theme::apply(&creation.egui_ctx);
        if let Some(font) = japanese_font {
            install_japanese_font(&creation.egui_ctx, font);
        }
        let (sender, receiver) = mpsc::channel();
        let (log_sender, log_thread, log_path) = start_logger(logger);
        let mut app = Self {
            state: AppState::default(),
            sender,
            receiver,
            shutdown: Arc::new(AtomicBool::new(false)),
            log_sender,
            log_thread,
            log_path,
        };
        app.spawn_load_config(creation.egui_ctx.clone());
        app
    }

    fn spawn_load_config(&mut self, ctx: egui::Context) {
        let sender = self.sender.clone();
        thread::spawn(move || {
            let result = config::load(Overrides::default()).map_err(|error| error.to_string());
            let _ = sender.send(WorkerMessage::ConfigLoaded(result));
            ctx.request_repaint();
        });
    }

    fn spawn_refresh(&mut self, ctx: egui::Context) {
        let Some(config) = self.state.config.clone() else {
            return;
        };
        self.state.busy = true;
        self.state.progress = "サーバーを再読込しています".into();
        self.state.last_poll = Instant::now();
        let sender = self.sender.clone();
        thread::spawn(move || {
            let result = connection::refresh(&config).map_err(|error| error.to_string());
            let _ = sender.send(WorkerMessage::Refreshed(result));
            ctx.request_repaint();
        });
    }

    fn spawn_connect(&mut self, ctx: egui::Context, devices: Vec<UsbDevice>) {
        let Some(config) = self.state.config.clone() else {
            return;
        };
        self.state.busy = true;
        self.state.last_error = None;
        let sender = self.sender.clone();
        let shutdown = self.shutdown.clone();
        thread::spawn(move || {
            let usbip = WindowsUsbip::new(config.usbip_path.clone(), false);
            let progress_sender = sender.clone();
            let result =
                connection::connect_group(&config, &usbip, &devices, &shutdown, |message| {
                    let _ = progress_sender.send(WorkerMessage::Progress(message));
                    ctx.request_repaint();
                })
                .map(|_| "グループを接続しました".to_owned())
                .map_err(|error| error.to_string());
            let _ = sender.send(WorkerMessage::OperationFinished(result));
            ctx.request_repaint();
        });
    }

    fn spawn_disconnect(&mut self, ctx: egui::Context, devices: Vec<String>) {
        let Some(config) = self.state.config.clone() else {
            return;
        };
        self.state.busy = true;
        self.state.last_error = None;
        let sender = self.sender.clone();
        thread::spawn(move || {
            let usbip = WindowsUsbip::new(config.usbip_path.clone(), false);
            let progress_sender = sender.clone();
            let result = connection::disconnect_group(&config, &usbip, &devices, |message| {
                let _ = progress_sender.send(WorkerMessage::Progress(message));
                ctx.request_repaint();
            })
            .map(|()| "グループを切断しました".to_owned())
            .map_err(|error| error.to_string());
            let _ = sender.send(WorkerMessage::OperationFinished(result));
            ctx.request_repaint();
        });
    }

    fn spawn_save(&mut self, ctx: egui::Context, new_config: Config) {
        self.state.busy = true;
        self.state.progress = "設定を保存しています".into();
        let sender = self.sender.clone();
        thread::spawn(move || {
            let result = config::save(&new_config)
                .map(|()| new_config)
                .map_err(|error| error.to_string());
            let _ = sender.send(WorkerMessage::ConfigSaved(result));
            ctx.request_repaint();
        });
    }

    fn handle_messages(&mut self, ctx: &egui::Context) {
        let mut refresh_after = false;
        while let Ok(message) = self.receiver.try_recv() {
            match message {
                WorkerMessage::ConfigLoaded(result) => match result {
                    Ok(config) => {
                        self.state.settings = SettingsDraft::from(&config);
                        self.state.config = Some(config);
                        self.state.busy = false;
                        self.log("設定を読み込みました".into());
                        refresh_after = true;
                    }
                    Err(error) => self.fail(error),
                },
                WorkerMessage::Refreshed(result) => match result {
                    Ok(snapshot) => {
                        self.state.snapshot = Some(snapshot);
                        self.state.busy = false;
                        self.state.progress = "最新の状態です".into();
                        self.state.last_error = None;
                        self.log("サーバー状態を更新しました".into());
                    }
                    Err(error) => {
                        self.state.snapshot = None;
                        self.fail(error);
                    }
                },
                WorkerMessage::Progress(progress) => {
                    self.state.progress = progress.clone();
                    self.log(progress);
                }
                WorkerMessage::OperationFinished(result) => {
                    self.state.busy = false;
                    match result {
                        Ok(message) => {
                            self.state.progress = message.clone();
                            self.log(message);
                        }
                        Err(error) => self.fail(error),
                    }
                    refresh_after = true;
                }
                WorkerMessage::ConfigSaved(result) => match result {
                    Ok(config) => {
                        self.state.settings = SettingsDraft::from(&config);
                        self.state.config = Some(config);
                        self.state.show_settings = false;
                        self.state.busy = false;
                        self.log("設定を保存しました".into());
                        refresh_after = true;
                    }
                    Err(error) => self.fail(error),
                },
            }
        }
        if refresh_after && !self.state.busy {
            self.spawn_refresh(ctx.clone());
        }
    }

    fn fail(&mut self, error: String) {
        self.state.busy = false;
        self.state.progress = "処理に失敗しました".into();
        self.state.last_error = Some(error.clone());
        self.log(format!("ERROR: {error}"));
    }

    fn log(&mut self, message: String) {
        self.state.push_log(message.clone());
        if let Some(sender) = &self.log_sender {
            let _ = sender.send(message);
        }
    }

    fn maybe_poll(&mut self, ctx: &egui::Context) {
        if self.state.busy || self.state.config.is_none() {
            return;
        }
        let visible = ctx.input(|input| input.viewport().visible().unwrap_or(true));
        let interval = if visible {
            Duration::from_secs(5)
        } else {
            Duration::from_secs(20)
        };
        if self.state.last_poll.elapsed() >= interval {
            self.spawn_refresh(ctx.clone());
        }
        ctx.request_repaint_after(interval);
    }
}

impl eframe::App for BridgeApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_messages(ctx);
        self.maybe_poll(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        match main_view::show(ui, &self.state) {
            ViewAction::None => {}
            ViewAction::Refresh => self.spawn_refresh(ui.ctx().clone()),
            ViewAction::OpenSettings => self.state.show_settings = true,
            ViewAction::OpenLogs => self.state.show_logs = true,
            ViewAction::Connect(devices) => self.spawn_connect(ui.ctx().clone(), devices),
            ViewAction::Disconnect(devices) => self.spawn_disconnect(ui.ctx().clone(), devices),
        }
        if self.state.show_settings {
            match settings::show(
                ui.ctx(),
                &mut self.state.show_settings,
                &mut self.state.settings,
            ) {
                SettingsAction::None => {}
                SettingsAction::Cancel => self.state.show_settings = false,
                SettingsAction::Save(config) => self.spawn_save(ui.ctx().clone(), config),
            }
        }
        if self.state.show_logs {
            dialogs::logs(
                ui.ctx(),
                &mut self.state.show_logs,
                &self.state.logs,
                self.log_path.as_deref(),
            );
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.shutdown.store(true, Ordering::Release);
        self.log_sender.take();
        if let Some(thread) = self.log_thread.take() {
            let _ = thread.join();
        }
    }
}

fn install_japanese_font(ctx: &egui::Context, bytes: Vec<u8>) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "windows-japanese".into(),
        Arc::new(FontData::from_owned(bytes)),
    );
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "windows-japanese".into());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .push("windows-japanese".into());
    ctx.set_fonts(fonts);
}

fn start_logger(
    logger: Option<Arc<FileLogger>>,
) -> (
    Option<Sender<String>>,
    Option<JoinHandle<()>>,
    Option<String>,
) {
    let Some(logger) = logger else {
        return (None, None, None);
    };
    let path = logger.path().display().to_string();
    let (sender, receiver) = mpsc::channel::<String>();
    let thread = thread::spawn(move || {
        while let Ok(message) = receiver.recv() {
            let _ = logger.append("INFO", &message);
        }
    });
    (Some(sender), Some(thread), Some(path))
}
