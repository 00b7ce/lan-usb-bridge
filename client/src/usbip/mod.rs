mod windows;

use crate::error::Result;

#[derive(Clone, Debug)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub code: Option<i32>,
}

pub trait UsbipRunner {
    fn status(&self) -> Result<CommandOutput>;
    fn list(&self, host: &str) -> Result<CommandOutput>;
    fn attach(&self, host: &str, bus_id: &str) -> Result<CommandOutput>;
    fn detach_bus_id(&self, bus_id: &str) -> Result<CommandOutput>;
}

pub use windows::WindowsUsbip;
