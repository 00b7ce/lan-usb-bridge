use std::{
    env, fs,
    path::{Path, PathBuf},
};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::{ClientError, Result};

const DEFAULT_SERVER_URL: &str = "http://127.0.0.1:8080";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct StoredConfig {
    server_url: Option<String>,
    client_id: Option<String>,
    usbip_path: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub server_url: Url,
    pub client_id: String,
    pub usbip_path: PathBuf,
    pub dry_run: bool,
}

#[derive(Default)]
pub struct Overrides {
    pub server_url: Option<String>,
    pub client_id: Option<String>,
    pub usbip_path: Option<PathBuf>,
    pub dry_run: bool,
}

pub fn default_path() -> Result<PathBuf> {
    project_dirs().map(|dirs| dirs.config_dir().join("config.json"))
}

pub fn local_data_dir() -> Result<PathBuf> {
    project_dirs().map(|dirs| dirs.data_local_dir().to_owned())
}

fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("net", "lan-usb-bridge", "LAN USB Bridge")
        .ok_or_else(|| ClientError::Config("ユーザー設定ディレクトリを特定できません".into()))
}

pub fn load(overrides: Overrides) -> Result<Config> {
    load_from(&default_path()?, overrides, |name| env::var(name).ok())
}

pub fn save(config: &Config) -> Result<()> {
    let stored = StoredConfig {
        server_url: Some(config.server_url.to_string()),
        client_id: Some(config.client_id.clone()),
        usbip_path: Some(config.usbip_path.clone()),
    };
    persist(&default_path()?, &stored)
}

pub fn from_values(server_url: &str, client_id: &str, usbip_path: PathBuf) -> Result<Config> {
    if client_id.trim().is_empty() {
        return Err(ClientError::Config("client_idは空にできません".into()));
    }
    Ok(Config {
        server_url: validate_server_url(server_url)?,
        client_id: client_id.trim().to_owned(),
        usbip_path,
        dry_run: false,
    })
}

fn load_from<F>(path: &Path, overrides: Overrides, env_get: F) -> Result<Config>
where
    F: Fn(&str) -> Option<String>,
{
    let mut stored: StoredConfig = if path.exists() {
        let bytes = fs::read(path).map_err(|source| ClientError::ConfigIo {
            path: path.into(),
            source,
        })?;
        serde_json::from_slice(&bytes).map_err(|error| {
            ClientError::Config(format!("{} のJSONが不正です: {error}", path.display()))
        })?
    } else {
        StoredConfig::default()
    };

    let generated = stored.client_id.as_deref().is_none_or(str::is_empty);
    if generated {
        stored.client_id = Some(format!("windows-{}", uuid::Uuid::new_v4()));
        persist(path, &stored)?;
    }

    let server = overrides
        .server_url
        .or_else(|| env_get("USB_BRIDGE_SERVER_URL"))
        .or(stored.server_url)
        .unwrap_or_else(|| DEFAULT_SERVER_URL.into());
    let client_id = overrides
        .client_id
        .or_else(|| env_get("USB_BRIDGE_CLIENT_ID"))
        .or(stored.client_id)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ClientError::Config("client_idは空にできません".into()))?;
    let usbip_path = overrides
        .usbip_path
        .or_else(|| env_get("USB_BRIDGE_USBIP_PATH").map(PathBuf::from))
        .or(stored.usbip_path)
        .unwrap_or_else(|| PathBuf::from("usbip.exe"));
    Ok(Config {
        server_url: validate_server_url(&server)?,
        client_id,
        usbip_path,
        dry_run: overrides.dry_run,
    })
}

fn persist(path: &Path, stored: &StoredConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| ClientError::ConfigIo {
            path: parent.into(),
            source,
        })?;
    }
    let data = serde_json::to_vec_pretty(stored)
        .map_err(|error| ClientError::Config(error.to_string()))?;
    fs::write(path, data).map_err(|source| ClientError::ConfigIo {
        path: path.into(),
        source,
    })
}

fn validate_server_url(value: &str) -> Result<Url> {
    let mut url = Url::parse(value)
        .map_err(|error| ClientError::Config(format!("サーバーURLが不正です: {error}")))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(ClientError::Config(
            "サーバーURLにはhttpまたはhttpsの絶対URLを指定してください".into(),
        ));
    }
    if url.query().is_some() || url.fragment().is_some() || !matches!(url.path(), "" | "/") {
        return Err(ClientError::Config(
            "サーバーURLにはscheme・host・portだけを指定してください".into(),
        ));
    }
    url.set_path("/");
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precedence_is_cli_then_env_then_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        fs::write(
            &path,
            r#"{"server_url":"http://file:1","client_id":"file-id","usbip_path":"file.exe"}"#,
        )
        .unwrap();
        let config = load_from(
            &path,
            Overrides {
                server_url: Some("https://cli:3".into()),
                ..Default::default()
            },
            |key| match key {
                "USB_BRIDGE_SERVER_URL" => Some("http://env:2".into()),
                "USB_BRIDGE_CLIENT_ID" => Some("env-id".into()),
                _ => None,
            },
        )
        .unwrap();
        assert_eq!(config.server_url.as_str(), "https://cli:3/");
        assert_eq!(config.client_id, "env-id");
        assert_eq!(config.usbip_path, PathBuf::from("file.exe"));
    }

    #[test]
    fn generated_id_is_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let first = load_from(&path, Overrides::default(), |_| None).unwrap();
        let second = load_from(&path, Overrides::default(), |_| None).unwrap();
        assert_eq!(first.client_id, second.client_id);
    }
}
