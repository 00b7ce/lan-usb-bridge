use std::time::Duration;

use reqwest::blocking::{Client, RequestBuilder, Response};
use serde::{Serialize, de::DeserializeOwned};
use url::Url;
use usb_bridge_protocol::{
    AcquireRequest, ErrorResponse, HealthResponse, HeartbeatRequest, ReleaseRequest, Session,
    SessionResponse, UsbDevice,
};

use crate::error::{ClientError, Result};

pub struct ApiClient {
    base_url: Url,
    http: Client,
}

impl ApiClient {
    pub fn new(base_url: Url) -> Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|error| {
                ClientError::Config(format!("HTTPクライアントを初期化できません: {error}"))
            })?;
        Ok(Self { base_url, http })
    }

    pub fn health(&self) -> Result<HealthResponse> {
        self.get("health")
    }
    pub fn devices(&self) -> Result<Vec<UsbDevice>> {
        self.get("api/devices")
    }
    pub fn session(&self) -> Result<SessionResponse> {
        self.get("api/session")
    }

    pub fn acquire(&self, client_id: &str, devices: Vec<String>) -> Result<Session> {
        self.post_json(
            "api/acquire",
            &AcquireRequest {
                client_id: client_id.into(),
                devices,
            },
        )
    }

    pub fn release(&self, client_id: &str) -> Result<()> {
        self.release_devices(client_id, Vec::new())
    }

    pub fn heartbeat(&self, client_id: &str) -> Result<()> {
        let response = self.send(self.http.post(self.endpoint("api/heartbeat")?).json(
            &HeartbeatRequest {
                client_id: client_id.into(),
            },
        ))?;
        self.ensure_success(response).map(|_| ())
    }

    pub fn release_devices(&self, client_id: &str, devices: Vec<String>) -> Result<()> {
        let response = self.send(self.http.post(self.endpoint("api/release")?).json(
            &ReleaseRequest {
                client_id: client_id.into(),
                devices,
            },
        ))?;
        self.ensure_success(response).map(|_| ())
    }

    fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = self.endpoint(path)?;
        let response = self.send(self.http.get(url.clone()))?;
        self.decode(url, response)
    }

    fn post_json<B: Serialize, T: DeserializeOwned>(&self, path: &str, body: &B) -> Result<T> {
        let url = self.endpoint(path)?;
        let response = self.send(self.http.post(url.clone()).json(body))?;
        self.decode(url, response)
    }

    fn endpoint(&self, path: &str) -> Result<Url> {
        self.base_url
            .join(path)
            .map_err(|error| ClientError::Config(format!("API URLを構築できません: {error}")))
    }

    fn send(&self, request: RequestBuilder) -> Result<Response> {
        let url = request
            .try_clone()
            .and_then(|request| request.build().ok())
            .map(|request| request.url().to_string())
            .unwrap_or_else(|| self.base_url.to_string());
        request
            .send()
            .map_err(|source| ClientError::Connection { url, source })
    }

    fn ensure_success(&self, response: Response) -> Result<Response> {
        if response.status().is_success() {
            return Ok(response);
        }
        let status = response.status();
        let text = response.text().unwrap_or_default();
        let message = serde_json::from_str::<ErrorResponse>(&text)
            .map(|error| error.error)
            .unwrap_or_else(|_| {
                if text.trim().is_empty() {
                    status
                        .canonical_reason()
                        .unwrap_or("不明なHTTPエラー")
                        .into()
                } else {
                    text
                }
            });
        Err(ClientError::Http { status, message })
    }

    fn decode<T: DeserializeOwned>(&self, url: Url, response: Response) -> Result<T> {
        self.ensure_success(response)?
            .json()
            .map_err(|source| ClientError::Json {
                url: url.to_string(),
                source,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_http::{Header, Response as TinyResponse, Server};

    fn server(body: &'static str, status: u16) -> String {
        let server = Server::http("127.0.0.1:0").unwrap();
        let address = format!("http://{}", server.server_addr());
        std::thread::spawn(move || {
            let request = server.recv().unwrap();
            let response = TinyResponse::from_string(body)
                .with_status_code(status)
                .with_header(Header::from_bytes("content-type", "application/json").unwrap());
            request.respond(response).unwrap();
        });
        address
    }

    #[test]
    fn parses_health_without_real_server() {
        let api = ApiClient::new(
            Url::parse(&server(r#"{"status":"ok","backend":"mock"}"#, 200)).unwrap(),
        )
        .unwrap();
        assert_eq!(api.health().unwrap().backend, "mock");
    }

    #[test]
    fn exposes_server_error_message() {
        let api = ApiClient::new(Url::parse(&server(r#"{"error":"busy"}"#, 409)).unwrap()).unwrap();
        assert!(api.health().unwrap_err().to_string().contains("busy"));
    }

    #[test]
    fn identifies_invalid_json() {
        let api = ApiClient::new(Url::parse(&server("not json", 200)).unwrap()).unwrap();
        assert!(
            api.health()
                .unwrap_err()
                .to_string()
                .contains("JSONとして解析")
        );
    }
}
