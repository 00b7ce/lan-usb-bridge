use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

use usb_bridge_protocol::{HealthResponse, Session, UsbDevice};

use crate::{
    api::ApiClient,
    config::Config,
    device_policy::{ensure_allowed, is_ftdi},
    error::{ClientError, Result},
    usbip::UsbipRunner,
};

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub health: HealthResponse,
    pub devices: Vec<UsbDevice>,
    pub session: Option<Session>,
}

pub fn refresh(config: &Config) -> Result<Snapshot> {
    let api = ApiClient::new(config.server_url.clone())?;
    Ok(Snapshot {
        health: api.health()?,
        devices: api.devices()?,
        session: api.session()?.session,
    })
}

pub fn connect_group<F>(
    config: &Config,
    usbip: &dyn UsbipRunner,
    devices: &[UsbDevice],
    cancelled: &AtomicBool,
    mut progress: F,
) -> Result<Session>
where
    F: FnMut(String),
{
    if devices.is_empty() {
        return Err(ClientError::Config("接続対象デバイスがありません".into()));
    }
    for device in devices {
        ensure_allowed(device)?;
    }
    let api = ApiClient::new(config.server_url.clone())?;
    if let Some(session) = api.session()?.session {
        return if session.client_id == config.client_id {
            Err(ClientError::SessionAlreadyExists)
        } else {
            Err(ClientError::SessionOwnedByOther(session.client_id))
        };
    }
    let bus_ids: Vec<String> = devices.iter().map(|device| device.bus_id.clone()).collect();
    progress("サーバーから利用権を取得しています".into());
    let session = api.acquire(&config.client_id, bus_ids.clone())?;
    let host = config
        .server_url
        .host_str()
        .ok_or_else(|| ClientError::Config("サーバーURLにホストがありません".into()))?;
    let mut attached = Vec::new();
    for device in devices {
        if cancelled.load(Ordering::Acquire) {
            rollback(
                &api,
                usbip,
                host,
                &config.client_id,
                &attached,
                Some(&device.bus_id),
            );
            return Err(ClientError::Cancelled);
        }
        progress(format!("{} をUSB/IP接続しています", device.bus_id));
        if let Err(error) = usbip.attach(host, &device.bus_id) {
            rollback(
                &api,
                usbip,
                host,
                &config.client_id,
                &attached,
                Some(&device.bus_id),
            );
            return Err(error);
        }
        if !usbip.is_dry_run() && !wait_for_port(usbip, &device.bus_id, cancelled)? {
            rollback(
                &api,
                usbip,
                host,
                &config.client_id,
                &attached,
                Some(&device.bus_id),
            );
            return if is_ftdi(device) {
                Err(ClientError::FtdiCompatibility(device.bus_id.clone()))
            } else {
                Err(ClientError::UsbipPortNotFound(device.bus_id.clone()))
            };
        }
        attached.push(device.bus_id.clone());
    }
    progress("接続しました".into());
    Ok(session)
}

pub fn disconnect_group<F>(
    config: &Config,
    usbip: &dyn UsbipRunner,
    devices: &[String],
    mut progress: F,
) -> Result<()>
where
    F: FnMut(String),
{
    let api = ApiClient::new(config.server_url.clone())?;
    let session = api.session()?.session;
    if let Some(owner) = &session
        && owner.client_id != config.client_id
    {
        return Err(ClientError::SessionOwnedByOther(owner.client_id.clone()));
    }
    let mut detach_errors = Vec::new();
    for bus_id in devices {
        progress(format!("{bus_id} を切断しています"));
        match usbip.detach_bus_id(bus_id) {
            Ok(_) | Err(ClientError::UsbipPortNotFound(_)) => {}
            Err(error) => detach_errors.push(error.to_string()),
        }
    }
    progress("サーバーの利用権を解放しています".into());
    api.release(&config.client_id)?;
    if detach_errors.is_empty() {
        Ok(())
    } else {
        Err(ClientError::Disconnect(detach_errors.join(" / ")))
    }
}

fn wait_for_port(usbip: &dyn UsbipRunner, bus_id: &str, cancelled: &AtomicBool) -> Result<bool> {
    for _ in 0..10 {
        if cancelled.load(Ordering::Acquire) {
            return Err(ClientError::Cancelled);
        }
        if usbip.attached_port(bus_id)?.is_some() {
            return Ok(true);
        }
        thread::sleep(Duration::from_millis(500));
    }
    Ok(false)
}

fn rollback(
    api: &ApiClient,
    usbip: &dyn UsbipRunner,
    host: &str,
    client_id: &str,
    attached: &[String],
    failed: Option<&str>,
) {
    if let Some(bus_id) = failed
        && usbip.stop_attach(host, bus_id).is_err()
    {
        let _ = usbip.stop_all();
    }
    for bus_id in attached.iter().rev() {
        let _ = usbip.detach_bus_id(bus_id);
    }
    let _ = api.release(client_id);
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tiny_http::{Header, Method, Response, Server, StatusCode};
    use url::Url;

    use super::*;
    use crate::usbip::CommandOutput;

    struct MockUsbip {
        calls: Mutex<Vec<String>>,
        attach_fails: bool,
        stop_fails: bool,
    }

    impl MockUsbip {
        fn new(attach_fails: bool, stop_fails: bool) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                attach_fails,
                stop_fails,
            }
        }
        fn record(&self, value: &str) {
            self.calls.lock().unwrap().push(value.into());
        }
        fn output() -> CommandOutput {
            CommandOutput {
                stdout: String::new(),
                stderr: String::new(),
                code: Some(0),
            }
        }
    }

    impl UsbipRunner for MockUsbip {
        fn status(&self) -> Result<CommandOutput> {
            Ok(Self::output())
        }
        fn list(&self, _host: &str) -> Result<CommandOutput> {
            Ok(Self::output())
        }
        fn attach(&self, _host: &str, bus_id: &str) -> Result<CommandOutput> {
            self.record(&format!("attach:{bus_id}"));
            if self.attach_fails {
                Err(ClientError::UsbipFailed {
                    code: Some(1),
                    message: "failed".into(),
                    admin_hint: "",
                })
            } else {
                Ok(Self::output())
            }
        }
        fn stop_attach(&self, _host: &str, bus_id: &str) -> Result<CommandOutput> {
            self.record(&format!("stop:{bus_id}"));
            if self.stop_fails {
                Err(ClientError::UsbipFailed {
                    code: Some(1),
                    message: "stop failed".into(),
                    admin_hint: "",
                })
            } else {
                Ok(Self::output())
            }
        }
        fn stop_all(&self) -> Result<CommandOutput> {
            self.record("stop-all");
            Ok(Self::output())
        }
        fn detach_bus_id(&self, bus_id: &str) -> Result<CommandOutput> {
            self.record(&format!("detach:{bus_id}"));
            Err(ClientError::UsbipPortNotFound(bus_id.into()))
        }
        fn attached_port(&self, _bus_id: &str) -> Result<Option<String>> {
            Ok(None)
        }
    }

    fn device() -> UsbDevice {
        UsbDevice {
            bus_id: "1-2".into(),
            vendor_id: "1234".into(),
            product_id: "0001".into(),
            manufacturer: None,
            product: Some("Test".into()),
            serial_number: None,
            device_class: "00".into(),
            interface_classes: vec!["03".into()],
            drivers: vec![],
            parent_hub: None,
            selected: false,
            selectable: true,
            risk: "normal".into(),
            warning: None,
            status: "available".into(),
        }
    }

    fn config(base: String) -> Config {
        Config {
            server_url: Url::parse(&base).unwrap(),
            client_id: "client-a".into(),
            usbip_path: "usbip.exe".into(),
            dry_run: false,
        }
    }

    fn api_server(
        session_present: bool,
        request_count: usize,
    ) -> (String, Arc<Mutex<Vec<String>>>) {
        let server = Server::http("127.0.0.1:0").unwrap();
        let address = format!("http://{}/", server.server_addr());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        thread::spawn(move || {
            for _ in 0..request_count {
                let request = server.recv().unwrap();
                let path = request.url().to_owned();
                seen.lock()
                    .unwrap()
                    .push(format!("{} {path}", request.method()));
                let (status, body) = match (request.method(), path.as_str()) {
                    (&Method::Get, "/api/session") if session_present => (
                        200,
                        r#"{"session":{"client_id":"client-a","devices":["1-2"]}}"#,
                    ),
                    (&Method::Get, "/api/session") => (200, r#"{"session":null}"#),
                    (&Method::Post, "/api/acquire") => {
                        (201, r#"{"client_id":"client-a","devices":["1-2"]}"#)
                    }
                    (&Method::Post, "/api/release") => (204, ""),
                    _ => (404, r#"{"error":"missing"}"#),
                };
                let mut response = Response::from_string(body).with_status_code(StatusCode(status));
                if !body.is_empty() {
                    response.add_header(
                        Header::from_bytes("content-type", "application/json").unwrap(),
                    );
                }
                request.respond(response).unwrap();
            }
        });
        (address, requests)
    }

    #[test]
    fn attach_failure_stops_attempt_and_releases_session() {
        let (base, requests) = api_server(false, 3);
        let usbip = MockUsbip::new(true, false);
        let result = connect_group(
            &config(base),
            &usbip,
            &[device()],
            &AtomicBool::new(false),
            |_| {},
        );
        assert!(result.is_err());
        assert_eq!(
            usbip.calls.lock().unwrap().as_slice(),
            ["attach:1-2", "stop:1-2"]
        );
        assert!(
            requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| request == "POST /api/release")
        );
    }

    #[test]
    fn failed_target_stop_falls_back_to_stop_all() {
        let (base, _) = api_server(false, 3);
        let usbip = MockUsbip::new(true, true);
        let _ = connect_group(
            &config(base),
            &usbip,
            &[device()],
            &AtomicBool::new(false),
            |_| {},
        );
        assert!(
            usbip
                .calls
                .lock()
                .unwrap()
                .iter()
                .any(|call| call == "stop-all")
        );
    }

    #[test]
    fn empty_port_still_releases_server_session() {
        let (base, requests) = api_server(true, 2);
        let usbip = MockUsbip::new(false, false);
        disconnect_group(&config(base), &usbip, &["1-2".into()], |_| {}).unwrap();
        assert!(
            requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| request == "POST /api/release")
        );
    }
}
