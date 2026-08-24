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
    let existing = api.session()?.session;
    if let Some(session) = &existing
        && session.client_id != config.client_id
    {
        return Err(ClientError::SessionOwnedByOther(session.client_id.clone()));
    }
    let devices: Vec<&UsbDevice> = devices
        .iter()
        .filter(|device| {
            existing
                .as_ref()
                .is_none_or(|session| !session.devices.contains(&device.bus_id))
        })
        .collect();
    if devices.is_empty() {
        return existing.ok_or(ClientError::SessionAlreadyExists);
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
                &bus_ids,
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
                &bus_ids,
                &attached,
                Some(&device.bus_id),
            );
            return Err(error);
        }
        if !usbip.is_dry_run() {
            match wait_for_port(usbip, &device.bus_id, cancelled) {
                Ok(true) => {}
                Ok(false) => {
                    rollback(
                        &api,
                        usbip,
                        host,
                        &config.client_id,
                        &bus_ids,
                        &attached,
                        Some(&device.bus_id),
                    );
                    return if is_ftdi(device) {
                        Err(ClientError::FtdiCompatibility(device.bus_id.clone()))
                    } else {
                        Err(ClientError::UsbipPortNotFound(device.bus_id.clone()))
                    };
                }
                Err(error) => {
                    rollback(
                        &api,
                        usbip,
                        host,
                        &config.client_id,
                        &bus_ids,
                        &attached,
                        Some(&device.bus_id),
                    );
                    return Err(error);
                }
            }
        }
        attached.push(device.bus_id.clone());
        if let Err(error) = api.heartbeat(&config.client_id) {
            rollback(
                &api,
                usbip,
                host,
                &config.client_id,
                &bus_ids,
                &attached,
                None,
            );
            return Err(error);
        }
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
    let remaining: Vec<String> = session
        .as_ref()
        .map(|session| {
            session
                .devices
                .iter()
                .filter(|device| !devices.contains(device))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let mut detach_errors = Vec::new();
    let host = config
        .server_url
        .host_str()
        .ok_or_else(|| ClientError::Config("サーバーURLにホストがありません".into()))?;
    let mut stop_all_needed = false;
    for bus_id in devices {
        if usbip.stop_attach(host, bus_id).is_err() {
            stop_all_needed = true;
        }
    }
    if stop_all_needed {
        let _ = usbip.stop_all();
    }
    for bus_id in devices {
        progress(format!("{bus_id} を切断しています"));
        match usbip.detach_bus_id(bus_id) {
            Ok(_) | Err(ClientError::UsbipPortNotFound(_)) => {}
            Err(error) => detach_errors.push(error.to_string()),
        }
    }
    progress("サーバーの利用権を解放しています".into());
    api.release_devices(&config.client_id, devices.to_vec())?;
    for bus_id in remaining {
        match usbip.attached_port(&bus_id) {
            Ok(Some(_)) => {}
            Ok(None) => {
                progress(format!("{bus_id} のUSB/IP接続が失われたため復元しています"));
                if let Err(error) = usbip.attach(host, &bus_id) {
                    detach_errors.push(format!("{bus_id} の再接続に失敗: {error}"));
                    continue;
                }
                if !usbip.is_dry_run() && !wait_for_port(usbip, &bus_id, &AtomicBool::new(false))? {
                    detach_errors.push(format!("{bus_id} の再接続ポートを確認できません"));
                }
            }
            Err(error) => detach_errors.push(format!("{bus_id} の接続確認に失敗: {error}")),
        }
    }
    if detach_errors.is_empty() {
        Ok(())
    } else {
        Err(ClientError::Disconnect(detach_errors.join(" / ")))
    }
}

/// Detach every device owned by this client and release its server session.
///
/// This is intended for graceful client shutdown, where the caller may not
/// have a current UI snapshot of the devices in the session.
pub fn disconnect_owned<F>(config: &Config, usbip: &dyn UsbipRunner, progress: F) -> Result<()>
where
    F: FnMut(String),
{
    let api = ApiClient::new(config.server_url.clone())?;
    let Some(session) = api.session()?.session else {
        return Ok(());
    };
    if session.client_id != config.client_id {
        return Err(ClientError::SessionOwnedByOther(session.client_id));
    }

    disconnect_group(config, usbip, &session.devices, progress)
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
    acquired: &[String],
    attached: &[String],
    failed: Option<&str>,
) {
    let mut stop_all_needed = false;
    if let Some(bus_id) = failed {
        stop_all_needed |= usbip.stop_attach(host, bus_id).is_err();
    }
    for bus_id in attached.iter().rev() {
        stop_all_needed |= usbip.stop_attach(host, bus_id).is_err();
    }
    if stop_all_needed {
        let _ = usbip.stop_all();
    }
    for bus_id in attached.iter().rev() {
        let _ = usbip.detach_bus_id(bus_id);
    }
    let _ = api.release_devices(client_id, acquired.to_vec());
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        sync::{Arc, Mutex},
    };

    use tiny_http::{Header, Method, Response, Server, StatusCode};
    use url::Url;

    use super::*;
    use crate::usbip::CommandOutput;

    struct MockUsbip {
        calls: Mutex<Vec<String>>,
        attach_failure: Option<String>,
        stop_fails: bool,
    }

    impl MockUsbip {
        fn new(attach_fails: bool, stop_fails: bool) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                attach_failure: attach_fails.then(|| "*".into()),
                stop_fails,
            }
        }
        fn failing_on(bus_id: &str) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                attach_failure: Some(bus_id.into()),
                stop_fails: false,
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
            if self
                .attach_failure
                .as_deref()
                .is_some_and(|failed| failed == "*" || failed == bus_id)
            {
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
        fn attached_port(&self, bus_id: &str) -> Result<Option<String>> {
            Ok(Some(format!("port-{bus_id}")))
        }
    }

    struct DetachingAllUsbip {
        calls: Mutex<Vec<String>>,
        ports: Mutex<BTreeSet<String>>,
    }

    impl DetachingAllUsbip {
        fn new(devices: &[&str]) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                ports: Mutex::new(devices.iter().map(|device| (*device).to_owned()).collect()),
            }
        }

        fn record(&self, value: String) {
            self.calls.lock().unwrap().push(value);
        }
    }

    impl UsbipRunner for DetachingAllUsbip {
        fn status(&self) -> Result<CommandOutput> {
            Ok(MockUsbip::output())
        }
        fn list(&self, _host: &str) -> Result<CommandOutput> {
            Ok(MockUsbip::output())
        }
        fn attach(&self, _host: &str, bus_id: &str) -> Result<CommandOutput> {
            self.record(format!("attach:{bus_id}"));
            self.ports.lock().unwrap().insert(bus_id.to_owned());
            Ok(MockUsbip::output())
        }
        fn stop_attach(&self, _host: &str, bus_id: &str) -> Result<CommandOutput> {
            self.record(format!("stop:{bus_id}"));
            Ok(MockUsbip::output())
        }
        fn stop_all(&self) -> Result<CommandOutput> {
            self.record("stop-all".into());
            Ok(MockUsbip::output())
        }
        fn detach_bus_id(&self, bus_id: &str) -> Result<CommandOutput> {
            self.record(format!("detach:{bus_id}"));
            self.ports.lock().unwrap().clear();
            Ok(MockUsbip::output())
        }
        fn attached_port(&self, bus_id: &str) -> Result<Option<String>> {
            Ok(self
                .ports
                .lock()
                .unwrap()
                .contains(bus_id)
                .then(|| format!("port-{bus_id}")))
        }
    }

    fn device() -> UsbDevice {
        device_with_id("1-2")
    }

    fn device_with_id(bus_id: &str) -> UsbDevice {
        UsbDevice {
            bus_id: bus_id.into(),
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
        api_server_with_session(
            if session_present {
                r#"{"session":{"client_id":"client-a","devices":["1-2"]}}"#
            } else {
                r#"{"session":null}"#
            },
            request_count,
        )
    }

    fn api_server_with_session(
        session_body: &'static str,
        request_count: usize,
    ) -> (String, Arc<Mutex<Vec<String>>>) {
        api_server_with_session_and_heartbeat(session_body, request_count, 204)
    }

    fn api_server_with_session_and_heartbeat(
        session_body: &'static str,
        request_count: usize,
        heartbeat_status: u16,
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
                    (&Method::Get, "/api/session") => (200, session_body),
                    (&Method::Post, "/api/acquire") => {
                        (201, r#"{"client_id":"client-a","devices":["1-2","1-3"]}"#)
                    }
                    (&Method::Post, "/api/heartbeat") => {
                        (heartbeat_status, r#"{"error":"heartbeat failed"}"#)
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
    fn attaches_all_devices_in_a_multi_device_request() {
        let (base, _) = api_server(false, 5);
        let usbip = MockUsbip::new(false, false);
        let devices = [
            device_with_id("1-1.2.1"),
            device_with_id("1-1.2.2"),
            device_with_id("1-1.2.3"),
        ];

        connect_group(
            &config(base),
            &usbip,
            &devices,
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();

        assert_eq!(
            usbip.calls.lock().unwrap().as_slice(),
            ["attach:1-1.2.1", "attach:1-1.2.2", "attach:1-1.2.3"]
        );
    }

    #[test]
    fn adds_a_device_to_a_session_owned_by_the_same_client() {
        let (base, _) = api_server(true, 3);
        let usbip = MockUsbip::new(false, false);

        let session = connect_group(
            &config(base),
            &usbip,
            &[device_with_id("1-3")],
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();

        assert_eq!(session.devices, ["1-2", "1-3"]);
        assert_eq!(usbip.calls.lock().unwrap().as_slice(), ["attach:1-3"]);
    }

    #[test]
    fn second_device_failure_rolls_back_first_device() {
        let (base, requests) = api_server(false, 4);
        let usbip = MockUsbip::failing_on("1-1.2.2");
        let devices = [
            device_with_id("1-1.2.1"),
            device_with_id("1-1.2.2"),
            device_with_id("1-1.2.3"),
        ];

        assert!(
            connect_group(
                &config(base),
                &usbip,
                &devices,
                &AtomicBool::new(false),
                |_| {},
            )
            .is_err()
        );
        assert_eq!(
            usbip.calls.lock().unwrap().as_slice(),
            [
                "attach:1-1.2.1",
                "attach:1-1.2.2",
                "stop:1-1.2.2",
                "stop:1-1.2.1",
                "detach:1-1.2.1"
            ]
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
    fn heartbeat_failure_rolls_back_attached_devices() {
        let (base, requests) = api_server_with_session_and_heartbeat(r#"{"session":null}"#, 4, 500);
        let usbip = MockUsbip::new(false, false);

        assert!(
            connect_group(
                &config(base),
                &usbip,
                &[device()],
                &AtomicBool::new(false),
                |_| {},
            )
            .is_err()
        );
        assert_eq!(
            usbip.calls.lock().unwrap().as_slice(),
            ["attach:1-2", "stop:1-2", "detach:1-2"]
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

    #[test]
    fn disconnect_owned_uses_the_current_server_session() {
        let (base, requests) = api_server(true, 3);
        let usbip = MockUsbip::new(false, false);

        disconnect_owned(&config(base), &usbip, |_| {}).unwrap();

        assert_eq!(
            usbip.calls.lock().unwrap().as_slice(),
            ["stop:1-2", "detach:1-2"]
        );
        assert_eq!(
            requests.lock().unwrap().as_slice(),
            ["GET /api/session", "GET /api/session", "POST /api/release"]
        );
    }

    #[test]
    fn restores_remaining_devices_when_detach_clears_all_windows_ports() {
        let (base, _) = api_server_with_session(
            r#"{"session":{"client_id":"client-a","devices":["1-2","1-3"]}}"#,
            2,
        );
        let usbip = DetachingAllUsbip::new(&["1-2", "1-3"]);

        disconnect_group(&config(base), &usbip, &["1-2".into()], |_| {}).unwrap();

        assert_eq!(
            usbip.calls.lock().unwrap().as_slice(),
            ["stop:1-2", "detach:1-2", "attach:1-3"]
        );
        assert!(usbip.attached_port("1-3").unwrap().is_some());
    }
}
