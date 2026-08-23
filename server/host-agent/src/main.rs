#[cfg(unix)]
use std::{env, path::PathBuf, process::Stdio};

#[cfg(unix)]
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    process::Command,
};
#[cfg(unix)]
use tracing::{error, info};
#[cfg(unix)]
use usb_bridge_protocol::{HostControlAction, HostControlRequest, HostControlResponse};

#[cfg(unix)]
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "usb_bridge_host_agent=info".into()),
        )
        .init();

    let socket = env::var("HOST_AGENT_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/run/lan-usb-bridge/host-agent.sock"));
    let sysfs = env::var("USB_SYSFS_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/sys/bus/usb/devices"));
    let usbip = env::var("USBIP_PATH").unwrap_or_else(|_| "/usr/sbin/usbip".to_owned());

    if let Some(parent) = socket.parent() {
        fs::create_dir_all(parent)
            .await
            .unwrap_or_else(|error| panic!("failed to create {}: {error}", parent.display()));
    }
    if fs::try_exists(&socket).await.unwrap_or(false) {
        fs::remove_file(&socket)
            .await
            .unwrap_or_else(|error| panic!("failed to remove stale socket: {error}"));
    }
    let listener = UnixListener::bind(&socket)
        .unwrap_or_else(|error| panic!("failed to bind {}: {error}", socket.display()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o660))
            .unwrap_or_else(|error| panic!("failed to set socket permissions: {error}"));
    }
    info!(path = %socket.display(), "USB/IP host agent started");

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let sysfs = sysfs.clone();
                let usbip = usbip.clone();
                tokio::spawn(async move {
                    if let Err(error) = handle(stream, sysfs, usbip).await {
                        error!(%error, "host-agent request failed");
                    }
                });
            }
            Err(error) => error!(%error, "failed to accept host-agent connection"),
        }
    }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("usb-bridge-host-agent is only supported on Unix");
    std::process::exit(2);
}

#[cfg(unix)]
async fn handle(mut stream: UnixStream, sysfs: PathBuf, usbip: String) -> Result<(), String> {
    let mut bytes = Vec::new();
    stream
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| format!("failed to read request: {error}"))?;
    let result = async {
        if bytes.len() > 64 * 1024 {
            return Err("request exceeds 64 KiB".to_owned());
        }
        let request: HostControlRequest =
            serde_json::from_slice(&bytes).map_err(|error| format!("invalid request: {error}"))?;
        if request.devices.is_empty() {
            return Err("device list must not be empty".to_owned());
        }
        for bus_id in &request.devices {
            validate_bus_id(&sysfs, bus_id).await?;
        }
        match request.action {
            HostControlAction::Bind => bind_all(&usbip, &request.devices).await,
            HostControlAction::Unbind => unbind_all(&usbip, &request.devices).await,
        }
    }
    .await;
    let response = match result {
        Ok(()) => HostControlResponse {
            success: true,
            error: None,
        },
        Err(error) => HostControlResponse {
            success: false,
            error: Some(error),
        },
    };
    let encoded = serde_json::to_vec(&response)
        .map_err(|error| format!("failed to encode response: {error}"))?;
    stream
        .write_all(&encoded)
        .await
        .map_err(|error| format!("failed to write response: {error}"))
}

#[cfg(unix)]
async fn validate_bus_id(sysfs: &std::path::Path, bus_id: &str) -> Result<(), String> {
    if !is_valid_bus_id(bus_id) {
        return Err(format!("invalid USB bus ID: {bus_id:?}"));
    }
    let path = sysfs.join(bus_id);
    if !fs::try_exists(path.join("idVendor"))
        .await
        .map_err(|error| format!("failed to inspect {bus_id}: {error}"))?
    {
        return Err(format!("USB device {bus_id} is not present"));
    }
    let class = fs::read_to_string(path.join("bDeviceClass"))
        .await
        .unwrap_or_default();
    if class.trim().eq_ignore_ascii_case("09") {
        return Err(format!("refusing to export USB hub {bus_id}"));
    }
    let mut entries = fs::read_dir(sysfs)
        .await
        .map_err(|error| format!("failed to inspect USB interfaces: {error}"))?;
    let interface_prefix = format!("{bus_id}:");
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| format!("failed to inspect USB interfaces: {error}"))?
    {
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(&interface_prefix)
        {
            continue;
        }
        let interface_class = fs::read_to_string(entry.path().join("bInterfaceClass"))
            .await
            .unwrap_or_default();
        if let Some(class_name) = prohibited_class(interface_class.trim()) {
            return Err(format!(
                "refusing to export {bus_id}: prohibited {class_name} interface class"
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn is_valid_bus_id(bus_id: &str) -> bool {
    !bus_id.is_empty()
        && bus_id.len() <= 32
        && bus_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'-' | b'.'))
        && bus_id.contains('-')
}

#[cfg(unix)]
fn prohibited_class(class: &str) -> Option<&'static str> {
    if class.eq_ignore_ascii_case("08") {
        Some("mass-storage")
    } else if class.eq_ignore_ascii_case("01") {
        Some("audio")
    } else if class.eq_ignore_ascii_case("0e") {
        Some("video")
    } else {
        None
    }
}

#[cfg(unix)]
async fn bind_all(usbip: &str, devices: &[String]) -> Result<(), String> {
    let mut bound: Vec<String> = Vec::new();
    for bus_id in devices {
        if let Err(error) = run_usbip(usbip, "bind", bus_id).await {
            for previous in bound.iter().rev() {
                let _ = run_usbip(usbip, "unbind", previous).await;
            }
            return Err(error);
        }
        bound.push(bus_id.clone());
    }
    Ok(())
}

#[cfg(unix)]
async fn unbind_all(usbip: &str, devices: &[String]) -> Result<(), String> {
    let mut errors = Vec::new();
    for bus_id in devices {
        if let Err(error) = run_usbip(usbip, "unbind", bus_id).await {
            errors.push(error);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(unix)]
async fn run_usbip(usbip: &str, operation: &str, bus_id: &str) -> Result<(), String> {
    let output = Command::new(usbip)
        .arg(operation)
        .arg("--busid")
        .arg(bus_id)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| format!("failed to execute {usbip}: {error}"))?;
    if output.status.success() {
        info!(%operation, %bus_id, "USB/IP operation completed");
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let detail = if stderr.is_empty() { stdout } else { stderr };
    Err(format!(
        "usbip {operation} failed for {bus_id} (exit {:?}): {detail}",
        output.status.code()
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::{is_valid_bus_id, prohibited_class};

    #[test]
    fn accepts_kernel_usb_bus_ids() {
        assert!(is_valid_bus_id("1-1.3"));
        assert!(is_valid_bus_id("10-2.12.4"));
    }

    #[test]
    fn rejects_interface_ids_and_command_arguments() {
        assert!(!is_valid_bus_id("1-1.3:1.0"));
        assert!(!is_valid_bus_id("--help"));
        assert!(!is_valid_bus_id("1-1;reboot"));
        assert!(!is_valid_bus_id("../1-1"));
    }

    #[test]
    fn recognizes_prohibited_classes_case_insensitively() {
        assert_eq!(prohibited_class("08"), Some("mass-storage"));
        assert_eq!(prohibited_class("01"), Some("audio"));
        assert_eq!(prohibited_class("0E"), Some("video"));
        assert_eq!(prohibited_class("03"), None);
        assert_eq!(prohibited_class("ff"), None);
    }
}
