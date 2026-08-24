use std::{
    collections::{BTreeSet, VecDeque},
    time::Instant,
};

use usb_bridge_client_core::{config::Config, connection::Snapshot};

#[derive(Clone, Default)]
pub struct SettingsDraft {
    pub server_url: String,
    pub client_id: String,
    pub usbip_path: String,
}

impl From<&Config> for SettingsDraft {
    fn from(config: &Config) -> Self {
        Self {
            server_url: config.server_url.to_string(),
            client_id: config.client_id.clone(),
            usbip_path: config.usbip_path.display().to_string(),
        }
    }
}

pub struct AppState {
    pub config: Option<Config>,
    pub snapshot: Option<Snapshot>,
    pub busy: bool,
    pub usb_transition_in_progress: bool,
    pub progress: String,
    pub last_error: Option<String>,
    pub logs: VecDeque<String>,
    pub selected_devices: BTreeSet<String>,
    pub settings: SettingsDraft,
    pub show_settings: bool,
    pub show_logs: bool,
    pub last_poll: Instant,
    pub last_heartbeat: Instant,
    pub heartbeat_in_flight: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            config: None,
            snapshot: None,
            busy: true,
            usb_transition_in_progress: false,
            progress: "設定を読み込んでいます".into(),
            last_error: None,
            logs: VecDeque::new(),
            selected_devices: BTreeSet::new(),
            settings: SettingsDraft::default(),
            show_settings: false,
            show_logs: false,
            last_poll: Instant::now(),
            last_heartbeat: Instant::now(),
            heartbeat_in_flight: false,
        }
    }
}

impl AppState {
    pub fn push_log(&mut self, message: String) {
        if self.logs.len() >= 200 {
            self.logs.pop_front();
        }
        self.logs.push_back(message);
    }
}
