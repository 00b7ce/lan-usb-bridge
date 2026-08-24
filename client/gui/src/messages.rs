use usb_bridge_client_core::{config::Config, connection::Snapshot};

pub enum WorkerMessage {
    ConfigLoaded(Result<Config, String>),
    Refreshed {
        result: Result<Snapshot, String>,
        show_feedback: bool,
    },
    Progress(String),
    OperationFinished(Result<String, String>),
    ConfigSaved(Result<Config, String>),
    HeartbeatFinished(Result<(), String>),
}
