use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    config,
    error::{ClientError, Result},
};

const MAX_LOG_BYTES: u64 = 1_048_576;

pub struct FileLogger {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileLogger {
    pub fn new() -> Result<Self> {
        let directory = config::local_data_dir()?.join("logs");
        fs::create_dir_all(&directory).map_err(|source| ClientError::ConfigIo {
            path: directory.clone(),
            source,
        })?;
        Ok(Self {
            path: directory.join("gui.log"),
            lock: Mutex::new(()),
        })
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn append(&self, level: &str, message: &str) -> Result<()> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| ClientError::Config("ログロックが破損しました".into()))?;
        if self
            .path
            .metadata()
            .map(|metadata| metadata.len())
            .unwrap_or(0)
            >= MAX_LOG_BYTES
        {
            let rotated = self.path.with_extension("log.old");
            let _ = fs::remove_file(&rotated);
            fs::rename(&self.path, &rotated).map_err(|source| ClientError::ConfigIo {
                path: self.path.clone(),
                source,
            })?;
        }
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let sanitized = message.replace(['\r', '\n'], " ");
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|source| ClientError::ConfigIo {
                path: self.path.clone(),
                source,
            })?;
        writeln!(file, "{seconds} [{level}] {sanitized}").map_err(|source| ClientError::ConfigIo {
            path: self.path.clone(),
            source,
        })
    }
}
