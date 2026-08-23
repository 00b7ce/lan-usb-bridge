use usb_bridge_client_core::{config::Config, connection::Snapshot};

pub enum WorkerMessage {
    ConfigLoaded(Result<Config, String>),
    Refreshed(Result<Snapshot, String>),
    Progress(String),
    OperationFinished(Result<String, String>),
    ConfigSaved(Result<Config, String>),
}
