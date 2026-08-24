use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use eframe::egui;
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
    operation_thread: Option<JoinHandle<()>>,
    log_sender: Option<Sender<String>>,
    log_thread: Option<JoinHandle<()>>,
    log_path: Option<String>,
}

impl BridgeApp {
    pub fn new(creation: &eframe::CreationContext<'_>, logger: Option<Arc<FileLogger>>) -> Self {
        theme::apply(&creation.egui_ctx);
        let (sender, receiver) = mpsc::channel();
        let (log_sender, log_thread, log_path) = start_logger(logger);
        let mut app = Self {
            state: AppState::default(),
            sender,
            receiver,
            shutdown: Arc::new(AtomicBool::new(false)),
            operation_thread: None,
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

    fn spawn_refresh(&mut self, ctx: egui::Context, show_feedback: bool) {
        if self.state.refresh_in_flight {
            return;
        }
        let Some(config) = self.state.config.clone() else {
            return;
        };
        self.state.refresh_in_flight = true;
        if show_feedback {
            self.state.progress = "Refreshing server state".into();
        }
        self.state.last_poll = Instant::now();
        let sender = self.sender.clone();
        thread::spawn(move || {
            let result = connection::refresh(&config).map_err(|error| error.to_string());
            let _ = sender.send(WorkerMessage::Refreshed {
                result,
                show_feedback,
            });
            ctx.request_repaint();
        });
    }

    fn spawn_connect(&mut self, ctx: egui::Context, devices: Vec<UsbDevice>) {
        let Some(config) = self.state.config.clone() else {
            return;
        };
        self.state.busy = true;
        self.state.usb_transition_in_progress = true;
        self.state.last_error = None;
        let sender = self.sender.clone();
        let shutdown = self.shutdown.clone();
        let target_count = devices.len();
        self.operation_thread = Some(thread::spawn(move || {
            let usbip = WindowsUsbip::new(config.usbip_path.clone(), false);
            let progress_sender = sender.clone();
            let result =
                connection::connect_group(&config, &usbip, &devices, &shutdown, |message| {
                    let _ = progress_sender.send(WorkerMessage::Progress(message));
                    ctx.request_repaint();
                })
                .map(|_| {
                    if target_count == 1 {
                        "Device attached".to_owned()
                    } else {
                        "Devices attached".to_owned()
                    }
                })
                .map_err(|error| error.to_string());
            let _ = sender.send(WorkerMessage::OperationFinished(result));
            ctx.request_repaint();
        }));
    }

    fn spawn_disconnect(&mut self, ctx: egui::Context, devices: Vec<String>) {
        let Some(config) = self.state.config.clone() else {
            return;
        };
        self.state.busy = true;
        self.state.usb_transition_in_progress = true;
        self.state.last_error = None;
        let sender = self.sender.clone();
        let target_count = devices.len();
        self.operation_thread = Some(thread::spawn(move || {
            let usbip = WindowsUsbip::new(config.usbip_path.clone(), false);
            let progress_sender = sender.clone();
            let result = connection::disconnect_group(&config, &usbip, &devices, |message| {
                let _ = progress_sender.send(WorkerMessage::Progress(message));
                ctx.request_repaint();
            })
            .map(|()| {
                if target_count == 1 {
                    "Device detached".to_owned()
                } else {
                    "Devices detached".to_owned()
                }
            })
            .map_err(|error| error.to_string());
            let _ = sender.send(WorkerMessage::OperationFinished(result));
            ctx.request_repaint();
        }));
    }

    fn spawn_save(&mut self, ctx: egui::Context, new_config: Config) {
        self.state.busy = true;
        self.state.progress = "Saving settings".into();
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
                        self.log("Settings loaded".into());
                        refresh_after = true;
                    }
                    Err(error) => self.fail(error),
                },
                WorkerMessage::Refreshed {
                    result,
                    show_feedback,
                } => match result {
                    Ok(snapshot) => {
                        self.state.selected_devices.retain(|bus_id| {
                            snapshot
                                .devices
                                .iter()
                                .any(|device| &device.bus_id == bus_id)
                        });
                        self.state.snapshot = Some(snapshot);
                        self.state.refresh_in_flight = false;
                        self.state.last_error = None;
                        if show_feedback {
                            self.state.progress = "Up to date".into();
                            self.log("Server state refreshed".into());
                        }
                    }
                    Err(error) => {
                        self.state.refresh_in_flight = false;
                        self.state.snapshot = None;
                        self.state.progress = "Failed to refresh server state".into();
                        self.state.last_error = Some(error.clone());
                        self.log(format!("ERROR: {error}"));
                    }
                },
                WorkerMessage::Progress(progress) => {
                    self.state.progress = progress.clone();
                    self.log(progress);
                }
                WorkerMessage::OperationFinished(result) => {
                    self.join_operation_thread();
                    self.state.busy = false;
                    self.state.usb_transition_in_progress = false;
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
                        self.log("Settings saved".into());
                        refresh_after = true;
                    }
                    Err(error) => self.fail(error),
                },
                WorkerMessage::HeartbeatFinished(result) => {
                    self.state.heartbeat_in_flight = false;
                    if let Err(error) = result {
                        self.log(format!("ERROR: heartbeat failed: {error}"));
                    }
                }
            }
        }
        if refresh_after && !self.state.busy {
            self.spawn_refresh(ctx.clone(), true);
        }
    }

    fn fail(&mut self, error: String) {
        self.state.busy = false;
        self.state.progress = "Operation failed".into();
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
        if self.state.busy || self.state.refresh_in_flight || self.state.config.is_none() {
            return;
        }
        let visible = ctx.input(|input| input.viewport().visible().unwrap_or(true));
        let interval = if visible {
            Duration::from_secs(5)
        } else {
            Duration::from_secs(20)
        };
        if self.state.last_poll.elapsed() >= interval {
            self.spawn_refresh(ctx.clone(), false);
        }
        ctx.request_repaint_after(interval);
    }

    fn maybe_heartbeat(&mut self, ctx: &egui::Context) {
        const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);

        if self.state.heartbeat_in_flight
            || self.state.last_heartbeat.elapsed() < HEARTBEAT_INTERVAL
        {
            ctx.request_repaint_after(HEARTBEAT_INTERVAL);
            return;
        }
        let Some(config) = self.state.config.clone() else {
            return;
        };
        let owns_session = self
            .state
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.session.as_ref())
            .is_some_and(|session| session.client_id == config.client_id);
        if !owns_session {
            return;
        }

        self.state.heartbeat_in_flight = true;
        self.state.last_heartbeat = Instant::now();
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = usb_bridge_client_core::api::ApiClient::new(config.server_url.clone())
                .and_then(|api| api.heartbeat(&config.client_id))
                .map_err(|error| error.to_string());
            let _ = sender.send(WorkerMessage::HeartbeatFinished(result));
            ctx.request_repaint();
        });
    }

    fn join_operation_thread(&mut self) {
        if let Some(thread) = self.operation_thread.take() {
            let _ = thread.join();
        }
    }
}

impl eframe::App for BridgeApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_messages(ctx);
        self.maybe_poll(ctx);
        self.maybe_heartbeat(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        match main_view::show(ui, &mut self.state) {
            ViewAction::None => {}
            ViewAction::Refresh => self.spawn_refresh(ui.ctx().clone(), true),
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
                !self.state.usb_transition_in_progress,
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
        self.join_operation_thread();

        if let Some(config) = self.state.config.clone() {
            let usbip = WindowsUsbip::new(config.usbip_path.clone(), false);
            let mut messages = Vec::new();
            let result = connection::disconnect_owned(&config, &usbip, |message| {
                messages.push(message);
            });
            for message in messages {
                self.log(message);
            }
            match result {
                Ok(()) => self.log("USB devices detached during shutdown".into()),
                Err(error) => self.log(format!(
                    "ERROR: failed to detach USB devices during shutdown: {error}"
                )),
            }
        }

        self.log_sender.take();
        if let Some(thread) = self.log_thread.take() {
            let _ = thread.join();
        }
    }
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
