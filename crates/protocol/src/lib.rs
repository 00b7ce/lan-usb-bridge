use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub backend: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UsbDevice {
    pub bus_id: String,
    pub vendor_id: String,
    pub product_id: String,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial_number: Option<String>,
    pub device_class: String,
    pub interface_classes: Vec<String>,
    pub drivers: Vec<String>,
    pub parent_hub: Option<String>,
    pub selected: bool,
    pub selectable: bool,
    pub risk: String,
    pub warning: Option<String>,
    pub status: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Session {
    pub client_id: String,
    pub devices: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AcquireRequest {
    pub client_id: String,
    #[serde(default)]
    pub devices: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReleaseRequest {
    pub client_id: String,
    #[serde(default)]
    pub devices: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HeartbeatRequest {
    pub client_id: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SelectionRequest {
    #[serde(default)]
    pub devices: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SelectionResponse {
    pub devices: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SessionResponse {
    pub session: Option<Session>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostControlAction {
    Bind,
    Unbind,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HostControlRequest {
    pub action: HostControlAction,
    pub devices: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HostControlResponse {
    pub success: bool,
    pub error: Option<String>,
}
