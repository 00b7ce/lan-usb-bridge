use std::{
    collections::BTreeSet,
    env, fs,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
};
#[cfg(unix)]
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};
use tokio::{net::TcpListener, sync::RwLock};
use tracing::{info, warn};
use usb_bridge_protocol::{
    AcquireRequest, ErrorResponse, HealthResponse, HostControlAction, ReleaseRequest,
    SelectionRequest, SelectionResponse, Session, SessionResponse, UsbDevice,
};
#[cfg(unix)]
use usb_bridge_protocol::{HostControlRequest, HostControlResponse};

const INDEX_HTML: &str = include_str!("../web/index.html");

#[derive(Clone)]
struct AppState {
    backend: String,
    sysfs_root: PathBuf,
    selection_file: PathBuf,
    control_backend: String,
    #[cfg(unix)]
    host_agent_socket: PathBuf,
    selected: Arc<RwLock<BTreeSet<String>>>,
    session: Arc<RwLock<Option<Session>>>,
}

#[tokio::main]
async fn main() {
    if env::args().any(|argument| argument == "--healthcheck") {
        run_healthcheck();
        return;
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "usb_bridge=info".into()),
        )
        .init();

    let backend = env::var("USB_BACKEND").unwrap_or_else(|_| "sysfs".to_owned());
    if backend != "sysfs" && backend != "mock" {
        eprintln!("unsupported USB_BACKEND={backend}; use sysfs or mock");
        std::process::exit(2);
    }

    let sysfs_root = env::var("USB_SYSFS_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/host/sys/bus/usb/devices"));
    let selection_file = env::var("SELECTION_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/data/selection.json"));
    let selected = load_selection(&selection_file);
    let control_backend = env::var("USB_CONTROL_BACKEND").unwrap_or_else(|_| "mock".to_owned());
    if control_backend != "mock" && control_backend != "host-agent" {
        eprintln!("unsupported USB_CONTROL_BACKEND={control_backend}; use mock or host-agent");
        std::process::exit(2);
    }
    #[cfg(unix)]
    let host_agent_socket = env::var("HOST_AGENT_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/run/lan-usb-bridge/host-agent.sock"));

    let listen_address = env::var("LISTEN_ADDRESS").unwrap_or_else(|_| "0.0.0.0:8080".to_owned());
    let address: SocketAddr = listen_address
        .parse()
        .unwrap_or_else(|error| panic!("invalid LISTEN_ADDRESS {listen_address:?}: {error}"));

    let state = AppState {
        backend,
        sysfs_root,
        selection_file,
        control_backend,
        #[cfg(unix)]
        host_agent_socket,
        selected: Arc::new(RwLock::new(selected)),
        session: Arc::new(RwLock::new(None)),
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/api/devices", get(devices))
        .route("/api/selection", post(save_selection))
        .route("/api/session", get(get_session))
        .route("/api/acquire", post(acquire))
        .route("/api/release", post(release))
        .with_state(state);

    let listener = TcpListener::bind(address)
        .await
        .unwrap_or_else(|error| panic!("failed to bind {address}: {error}"));
    info!(%address, "USB Bridge API started");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("HTTP server failed");
}

fn run_healthcheck() {
    let address = env::var("HEALTHCHECK_ADDRESS").unwrap_or_else(|_| "127.0.0.1:8080".to_owned());
    let timeout = Duration::from_secs(2);
    let socket_address: SocketAddr = address
        .parse()
        .unwrap_or_else(|error| panic!("invalid HEALTHCHECK_ADDRESS {address:?}: {error}"));
    let mut stream = TcpStream::connect_timeout(&socket_address, timeout)
        .unwrap_or_else(|error| panic!("healthcheck connection failed: {error}"));
    stream
        .set_read_timeout(Some(timeout))
        .expect("failed to set timeout");
    stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("healthcheck request failed");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("healthcheck response failed");
    if !response.starts_with("HTTP/1.1 200") {
        panic!("healthcheck returned an unhealthy response");
    }
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_owned(),
        backend: state.backend,
    })
}

async fn devices(State(state): State<AppState>) -> impl IntoResponse {
    let selected = state.selected.read().await.clone();
    match enumerate_devices(&state, &selected) {
        Ok(devices) => Json(devices).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error }),
        )
            .into_response(),
    }
}

async fn save_selection(
    State(state): State<AppState>,
    Json(request): Json<SelectionRequest>,
) -> impl IntoResponse {
    let current = state.selected.read().await.clone();
    let available = match enumerate_devices(&state, &current) {
        Ok(devices) => devices,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error }),
            )
                .into_response();
        }
    };
    let selectable: BTreeSet<_> = available
        .iter()
        .filter(|device| device.selectable)
        .map(|device| device.bus_id.as_str())
        .collect();
    if request
        .devices
        .iter()
        .any(|device| !selectable.contains(device.as_str()))
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "selection contains an unavailable or blocked USB device".to_owned(),
            }),
        )
            .into_response();
    }

    let selected: BTreeSet<String> = request.devices.into_iter().collect();
    if let Err(error) = persist_selection(&state.selection_file, &selected) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error }),
        )
            .into_response();
    }
    *state.selected.write().await = selected.clone();
    Json(SelectionResponse {
        devices: selected.into_iter().collect(),
    })
    .into_response()
}

async fn get_session(State(state): State<AppState>) -> Json<SessionResponse> {
    Json(SessionResponse {
        session: state.session.read().await.clone(),
    })
}

async fn acquire(
    State(state): State<AppState>,
    Json(request): Json<AcquireRequest>,
) -> impl IntoResponse {
    if request.client_id.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "client_id must not be empty".to_owned(),
            }),
        )
            .into_response();
    }
    let selected = state.selected.read().await.clone();
    let available = match enumerate_devices(&state, &selected) {
        Ok(devices) => devices,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error }),
            )
                .into_response();
        }
    };
    let requested = if request.devices.is_empty() {
        selected.into_iter().collect::<Vec<_>>()
    } else {
        request.devices
    };
    let selectable: BTreeSet<_> = available
        .iter()
        .filter(|device| device.selectable)
        .map(|device| device.bus_id.as_str())
        .collect();
    if requested
        .iter()
        .any(|device| !selectable.contains(device.as_str()))
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "acquire contains an unavailable or prohibited USB device".to_owned(),
            }),
        )
            .into_response();
    }

    if requested.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "no USB devices were requested or selected".to_owned(),
            }),
        )
            .into_response();
    }
    let mut current = state.session.write().await;
    if let Some(session) = current.as_ref()
        && session.client_id != request.client_id
    {
        return (
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "USB devices are already acquired".to_owned(),
            }),
        )
            .into_response();
    }
    let already_acquired: BTreeSet<&str> = current
        .as_ref()
        .map(|session| session.devices.iter().map(String::as_str).collect())
        .unwrap_or_default();
    let additions: Vec<String> = requested
        .into_iter()
        .filter(|device| !already_acquired.contains(device.as_str()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if additions.is_empty() {
        return (
            StatusCode::OK,
            Json(current.as_ref().expect("existing session").clone()),
        )
            .into_response();
    }
    if let Err(error) = control_devices(&state, HostControlAction::Bind, &additions).await {
        return (StatusCode::BAD_GATEWAY, Json(ErrorResponse { error })).into_response();
    }
    let mut session = current.clone().unwrap_or(Session {
        client_id: request.client_id,
        devices: Vec::new(),
    });
    session.devices.extend(additions);
    session.devices.sort();
    session.devices.dedup();
    *current = Some(session.clone());
    (StatusCode::CREATED, Json(session)).into_response()
}

async fn release(
    State(state): State<AppState>,
    Json(request): Json<ReleaseRequest>,
) -> impl IntoResponse {
    let mut current = state.session.write().await;
    match current.as_ref() {
        Some(session) if session.client_id == request.client_id => {
            let targets = if request.devices.is_empty() {
                session.devices.clone()
            } else {
                let owned: BTreeSet<&str> = session.devices.iter().map(String::as_str).collect();
                if request
                    .devices
                    .iter()
                    .any(|device| !owned.contains(device.as_str()))
                {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(ErrorResponse {
                            error: "release contains a device not owned by this session".to_owned(),
                        }),
                    )
                        .into_response();
                }
                request.devices.clone()
            };
            if let Err(error) = control_devices(&state, HostControlAction::Unbind, &targets).await {
                return (StatusCode::BAD_GATEWAY, Json(ErrorResponse { error })).into_response();
            }
            let released: BTreeSet<&str> = targets.iter().map(String::as_str).collect();
            let mut remaining = session.clone();
            remaining
                .devices
                .retain(|device| !released.contains(device.as_str()));
            *current = (!remaining.devices.is_empty()).then_some(remaining);
            StatusCode::NO_CONTENT.into_response()
        }
        Some(_) => (
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "session is owned by another client".to_owned(),
            }),
        )
            .into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

async fn control_devices(
    state: &AppState,
    action: HostControlAction,
    devices: &[String],
) -> Result<(), String> {
    if state.control_backend == "mock" {
        return Ok(());
    }
    #[cfg(not(unix))]
    {
        let _ = (action, devices);
        Err("host-agent control is only supported on Unix".to_owned())
    }
    #[cfg(unix)]
    {
        let mut stream = UnixStream::connect(&state.host_agent_socket)
            .await
            .map_err(|error| {
                format!(
                    "failed to connect to host agent at {}: {error}",
                    state.host_agent_socket.display()
                )
            })?;
        let request = HostControlRequest {
            action,
            devices: devices.to_vec(),
        };
        let mut payload = serde_json::to_vec(&request)
            .map_err(|error| format!("failed to encode host-agent request: {error}"))?;
        payload.push(b'\n');
        stream
            .write_all(&payload)
            .await
            .map_err(|error| format!("failed to write host-agent request: {error}"))?;
        stream
            .shutdown()
            .await
            .map_err(|error| format!("failed to finish host-agent request: {error}"))?;
        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .await
            .map_err(|error| format!("failed to read host-agent response: {error}"))?;
        let response: HostControlResponse = serde_json::from_slice(&response)
            .map_err(|error| format!("invalid host-agent response: {error}"))?;
        if response.success {
            Ok(())
        } else {
            Err(response
                .error
                .unwrap_or_else(|| "host agent failed without an error message".to_owned()))
        }
    }
}

fn enumerate_devices(
    state: &AppState,
    selected: &BTreeSet<String>,
) -> Result<Vec<UsbDevice>, String> {
    if state.backend == "mock" {
        return Ok(mock_devices(selected));
    }
    let entries = fs::read_dir(&state.sysfs_root).map_err(|error| {
        format!(
            "failed to read USB sysfs at {}: {error}",
            state.sysfs_root.display()
        )
    })?;
    let mut devices = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let bus_id = entry.file_name().to_string_lossy().into_owned();
        if bus_id.contains(':') || !path.join("idVendor").is_file() {
            continue;
        }
        let device_class = read_attribute(&path, "bDeviceClass").unwrap_or_default();
        if device_class.eq_ignore_ascii_case("09") {
            continue;
        }
        let interface_classes = interface_attributes(&state.sysfs_root, &bus_id, "bInterfaceClass");
        let drivers = interface_drivers(&state.sysfs_root, &bus_id);
        let vendor_id = read_attribute(&path, "idVendor").unwrap_or_default();
        let (selectable, risk, warning) = classify_device(&vendor_id, &interface_classes, &drivers);
        devices.push(UsbDevice {
            vendor_id,
            product_id: read_attribute(&path, "idProduct").unwrap_or_default(),
            manufacturer: read_attribute(&path, "manufacturer"),
            product: read_attribute(&path, "product"),
            serial_number: read_attribute(&path, "serial"),
            parent_hub: parent_usb_path(&bus_id),
            selected: selected.contains(&bus_id),
            selectable,
            risk: risk.to_owned(),
            warning: warning.map(str::to_owned),
            status: "available".to_owned(),
            bus_id,
            device_class,
            interface_classes,
            drivers,
        });
    }
    devices.sort_by_key(|device| natural_bus_key(&device.bus_id));
    Ok(devices)
}

fn read_attribute(path: &Path, attribute: &str) -> Option<String> {
    fs::read_to_string(path.join(attribute))
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn interface_attributes(root: &Path, bus_id: &str, attribute: &str) -> Vec<String> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let prefix = format!("{bus_id}:");
    let values: BTreeSet<String> = entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        .filter_map(|entry| read_attribute(&entry.path(), attribute))
        .collect();
    values.into_iter().collect()
}

fn interface_drivers(root: &Path, bus_id: &str) -> Vec<String> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let prefix = format!("{bus_id}:");
    let drivers: BTreeSet<String> = entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        .filter_map(|entry| fs::read_link(entry.path().join("driver")).ok())
        .filter_map(|driver| {
            driver
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .collect();
    drivers.into_iter().collect()
}

fn classify_device(
    vendor_id: &str,
    interface_classes: &[String],
    drivers: &[String],
) -> (bool, &'static str, Option<&'static str>) {
    if interface_classes.iter().any(|class| class == "08") {
        return (
            false,
            "prohibited",
            Some("禁止: ストレージクラスは切断時にデータを破損する危険があるため転送できません"),
        );
    }
    if interface_classes.iter().any(|class| class == "01") {
        return (
            false,
            "prohibited",
            Some("禁止: オーディオクラスは現在の安全な対応範囲外です"),
        );
    }
    if interface_classes.iter().any(|class| class == "0e") {
        return (
            false,
            "prohibited",
            Some("禁止: ビデオクラスは現在の安全な対応範囲外です"),
        );
    }
    if vendor_id.eq_ignore_ascii_case("0403") {
        return (
            true,
            "caution",
            Some("WARNING: FTDI系デバイスはusbip-win2で互換性問題が報告されています"),
        );
    }
    if !drivers.iter().any(|driver| driver == "cdc_acm")
        && interface_classes
            .iter()
            .any(|class| matches!(class.as_str(), "02" | "0a" | "e0"))
    {
        return (
            true,
            "caution",
            Some("ネットワーク機器の場合、Piとの接続を失う可能性があります"),
        );
    }
    (true, "normal", None)
}

fn parent_usb_path(bus_id: &str) -> Option<String> {
    let (bus, ports) = bus_id.split_once('-')?;
    let (parent_ports, _) = ports.rsplit_once('.')?;
    Some(format!("{bus}-{parent_ports}"))
}

fn natural_bus_key(bus_id: &str) -> Vec<u32> {
    bus_id
        .split(['-', '.'])
        .filter_map(|part| part.parse::<u32>().ok())
        .collect()
}

fn load_selection(path: &Path) -> BTreeSet<String> {
    let Ok(contents) = fs::read_to_string(path) else {
        return BTreeSet::new();
    };
    match serde_json::from_str::<SelectionRequest>(&contents) {
        Ok(selection) => selection.devices.into_iter().collect(),
        Err(error) => {
            warn!(%error, path = %path.display(), "ignoring invalid selection file");
            BTreeSet::new()
        }
    }
}

fn persist_selection(path: &Path, selected: &BTreeSet<String>) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    let payload = SelectionRequest {
        devices: selected.iter().cloned().collect(),
    };
    let contents = serde_json::to_string_pretty(&payload)
        .map_err(|error| format!("failed to serialize selection: {error}"))?;
    fs::write(path, format!("{contents}\n"))
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
}

fn mock_devices(selected: &BTreeSet<String>) -> Vec<UsbDevice> {
    [
        ("1-1.2.1", "04da", "3f18", "HaritoraX Wireless", "02"),
        ("1-1.2.2", "04da", "3f18", "HaritoraX Wireless", "02"),
        ("1-1.2.3", "04da", "3f18", "HaritoraX Wireless", "02"),
        ("1-1.4", "1234", "0001", "USB Keyboard", "03"),
        ("1-1.5", "1234", "0002", "USB Mouse", "03"),
        ("1-1.6", "0781", "0001", "USB Storage", "08"),
        ("1-1.7", "1234", "0003", "USB Audio", "01"),
        ("1-1.8", "1234", "0004", "USB Camera", "0e"),
        ("1-1.9", "0403", "6001", "FTDI Serial Adapter", "ff"),
    ]
    .into_iter()
    .map(|(bus_id, vendor_id, product_id, product, class)| {
        let interface_classes = vec![class.to_owned()];
        let drivers = vec![if class == "03" { "usbhid" } else { "cdc_acm" }.to_owned()];
        let (selectable, risk, warning) = classify_device(vendor_id, &interface_classes, &drivers);
        UsbDevice {
            bus_id: bus_id.to_owned(),
            vendor_id: vendor_id.to_owned(),
            product_id: product_id.to_owned(),
            manufacturer: Some("Mock Device".to_owned()),
            product: Some(product.to_owned()),
            serial_number: None,
            device_class: "00".to_owned(),
            interface_classes,
            drivers,
            parent_hub: parent_usb_path(bus_id),
            selected: selected.contains(bus_id),
            selectable,
            risk: risk.to_owned(),
            warning: warning.map(str::to_owned),
            status: "available".to_owned(),
        }
    })
    .collect()
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler")
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
}

#[cfg(test)]
mod tests {
    use super::classify_device;

    #[test]
    fn prohibits_storage_audio_and_video_classes() {
        for class in ["08", "01", "0e"] {
            let result = classify_device("1234", &[class.to_owned()], &[]);
            assert!(!result.0, "class {class} must be prohibited");
            assert_eq!(result.1, "prohibited");
        }
    }

    #[test]
    fn warns_for_ftdi_vendor() {
        let result = classify_device("0403", &["ff".to_owned()], &[]);
        assert!(result.0);
        assert_eq!(result.1, "caution");
        assert!(result.2.unwrap().contains("FTDI"));
    }
}
