use std::{ffi::OsString, path::PathBuf, process::Command};

use crate::{
    error::{ClientError, Result},
    usbip::{CommandOutput, UsbipRunner},
};

pub struct WindowsUsbip {
    executable: PathBuf,
    dry_run: bool,
}

impl WindowsUsbip {
    pub fn new(executable: PathBuf, dry_run: bool) -> Self {
        Self {
            executable,
            dry_run,
        }
    }

    fn execute<I, S>(&self, args: I) -> Result<CommandOutput>
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
        if self.dry_run {
            return Ok(CommandOutput {
                stdout: format!(
                    "dry-run: {} {}",
                    self.executable.display(),
                    args.iter()
                        .map(|a| a.to_string_lossy())
                        .collect::<Vec<_>>()
                        .join(" ")
                ),
                stderr: String::new(),
                code: Some(0),
            });
        }
        let output = Command::new(&self.executable)
            .args(&args)
            .output()
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    ClientError::UsbipNotFound
                } else {
                    ClientError::UsbipIo(error)
                }
            })?;
        let result = CommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).trim().to_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            code: output.status.code(),
        };
        if output.status.success() {
            Ok(result)
        } else {
            let message = if result.stderr.is_empty() {
                result.stdout.clone()
            } else {
                result.stderr.clone()
            };
            let lower = message.to_ascii_lowercase();
            let admin_hint = if result.code == Some(5)
                || lower.contains("access is denied")
                || lower.contains("permission")
                || lower.contains("administrator")
                || lower.contains("elevation")
            {
                "（管理者としてWindows Terminalを起動して再試行してください）"
            } else {
                ""
            };
            Err(ClientError::UsbipFailed {
                code: result.code,
                message,
                admin_hint,
            })
        }
    }

    fn port_for_bus_id(output: &str, bus_id: &str) -> Option<String> {
        let mut port = None;
        for line in output.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("Port ") {
                port = rest
                    .split(':')
                    .next()
                    .map(|p| p.trim_start_matches('0').to_owned())
                    .map(|p| if p.is_empty() { "0".into() } else { p });
            }
            if (trimmed.contains(&format!("/{bus_id}"))
                || trimmed.contains(&format!("busid={bus_id}")))
                && port.is_some()
            {
                return port;
            }
        }
        None
    }
}

impl UsbipRunner for WindowsUsbip {
    fn status(&self) -> Result<CommandOutput> {
        self.execute(["port"])
    }
    fn list(&self, host: &str) -> Result<CommandOutput> {
        self.execute(["list", "-r", host])
    }
    fn attach(&self, host: &str, bus_id: &str) -> Result<CommandOutput> {
        self.execute(["attach", "-r", host, "-b", bus_id])
    }
    fn detach_bus_id(&self, bus_id: &str) -> Result<CommandOutput> {
        if self.dry_run {
            return self.execute(["detach", "-p", "<resolved-port>"]);
        }
        let status = self.status()?;
        let port = Self::port_for_bus_id(&status.stdout, bus_id)
            .ok_or_else(|| ClientError::UsbipPortNotFound(bus_id.into()))?;
        self.execute(["detach", "-p", &port])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_port_for_bus_id() {
        let output = "Port 01: <Port in Use>\n  device\n  8-1 -> usbip://192.0.2.1:3240/1-1.5 (remote bus/dev)\n";
        assert_eq!(
            WindowsUsbip::port_for_bus_id(output, "1-1.5").as_deref(),
            Some("1")
        );
    }
    #[test]
    fn dry_run_does_not_execute() {
        let runner = WindowsUsbip::new(PathBuf::from("missing.exe"), true);
        assert!(
            runner
                .attach("example.test", "1-2")
                .unwrap()
                .stdout
                .contains("attach -r example.test -b 1-2")
        );
    }
}
