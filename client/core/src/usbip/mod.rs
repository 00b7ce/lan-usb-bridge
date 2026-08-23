mod windows;

use crate::error::Result;

#[derive(Clone, Debug)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub code: Option<i32>,
}

pub trait UsbipRunner: Send + Sync {
    fn is_dry_run(&self) -> bool {
        false
    }
    fn status(&self) -> Result<CommandOutput>;
    fn list(&self, host: &str) -> Result<CommandOutput>;
    fn attach(&self, host: &str, bus_id: &str) -> Result<CommandOutput>;
    fn stop_attach(&self, host: &str, bus_id: &str) -> Result<CommandOutput>;
    fn stop_all(&self) -> Result<CommandOutput>;
    fn detach_bus_id(&self, bus_id: &str) -> Result<CommandOutput>;
    fn attached_port(&self, bus_id: &str) -> Result<Option<String>>;
}

pub use windows::WindowsUsbip;
