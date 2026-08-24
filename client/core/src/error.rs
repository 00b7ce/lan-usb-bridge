use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("configuration error: {0}")]
    Config(String),
    #[error("failed to read or write configuration file {path}: {source}")]
    ConfigIo {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to connect to the server ({url}): {source}")]
    Connection { url: String, source: reqwest::Error },
    #[error("failed to parse server response as JSON ({url}): {source}")]
    Json { url: String, source: reqwest::Error },
    #[error("server returned HTTP {status}: {message}")]
    Http {
        status: reqwest::StatusCode,
        message: String,
    },
    #[error(
        "usbip-win2 was not found; install it with the official installer and add usbip.exe to PATH, or select it in Settings"
    )]
    UsbipNotFound,
    #[error("failed to run usbip-win2: {0}")]
    UsbipIo(#[source] std::io::Error),
    #[error("usbip-win2 failed with exit code {code:?}: {message}{admin_hint}")]
    UsbipFailed {
        code: Option<i32>,
        message: String,
        admin_hint: &'static str,
    },
    #[error("no attached USB/IP port was found for BUS_ID {0}")]
    UsbipPortNotFound(String),
    #[error(
        "Windows could not enumerate FTDI device {0}; this may be a known compatibility issue between usbip-win2 and the FTDI driver"
    )]
    FtdiCompatibility(String),
    #[error(
        "the session is owned by another client ({0}); it will not be forcefully acquired or released"
    )]
    SessionOwnedByOther(String),
    #[error("this client already has a session; duplicate acquisition was skipped")]
    SessionAlreadyExists,
    #[error("device {0} is missing from the server list or is not selectable")]
    DeviceUnavailable(String),
    #[error(
        "device {bus_id} contains a blocked USB class ({class_name}) and cannot be acquired or attached"
    )]
    ProhibitedDevice {
        bus_id: String,
        class_name: &'static str,
    },
    #[error("the operation was cancelled because the application is exiting")]
    Cancelled,
    #[error("errors occurred while detaching devices: {0}")]
    Disconnect(String),
}

pub type Result<T> = std::result::Result<T, ClientError>;
