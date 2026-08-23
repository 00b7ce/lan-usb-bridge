use usb_bridge_protocol::UsbDevice;

use crate::error::{ClientError, Result};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyLevel {
    Allowed,
    Warning,
    Prohibited,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DevicePolicy {
    pub level: PolicyLevel,
    pub label: &'static str,
    pub detail: &'static str,
}

pub fn evaluate(device: &UsbDevice) -> DevicePolicy {
    if let Some(class_name) = prohibited_class(device) {
        return DevicePolicy {
            level: PolicyLevel::Prohibited,
            label: "禁止",
            detail: class_name,
        };
    }
    if is_ftdi(device) {
        return DevicePolicy {
            level: PolicyLevel::Warning,
            label: "WARNING",
            detail: "FTDI: usbip-win2互換性問題の可能性",
        };
    }
    if !device.selectable || device.risk != "normal" {
        return DevicePolicy {
            level: if device.selectable {
                PolicyLevel::Warning
            } else {
                PolicyLevel::Prohibited
            },
            label: if device.selectable {
                "WARNING"
            } else {
                "禁止"
            },
            detail: "サーバーのデバイスポリシー",
        };
    }
    DevicePolicy {
        level: PolicyLevel::Allowed,
        label: "利用可能",
        detail: "転送許可",
    }
}

pub fn ensure_allowed(device: &UsbDevice) -> Result<()> {
    if let Some(class_name) = prohibited_class(device) {
        return Err(ClientError::ProhibitedDevice {
            bus_id: device.bus_id.clone(),
            class_name,
        });
    }
    if !device.selectable {
        return Err(ClientError::DeviceUnavailable(device.bus_id.clone()));
    }
    Ok(())
}

pub fn prohibited_class(device: &UsbDevice) -> Option<&'static str> {
    for (code, name) in [("08", "ストレージ"), ("01", "オーディオ"), ("0e", "ビデオ")]
    {
        if device
            .interface_classes
            .iter()
            .any(|class| class.eq_ignore_ascii_case(code))
        {
            return Some(name);
        }
    }
    None
}

pub fn is_ftdi(device: &UsbDevice) -> bool {
    device.vendor_id.eq_ignore_ascii_case("0403")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(vendor: &str, classes: &[&str]) -> UsbDevice {
        UsbDevice {
            bus_id: "1-1".into(),
            vendor_id: vendor.into(),
            product_id: "0001".into(),
            manufacturer: None,
            product: None,
            serial_number: None,
            device_class: "00".into(),
            interface_classes: classes.iter().map(|v| (*v).into()).collect(),
            drivers: Vec::new(),
            parent_hub: None,
            selected: false,
            selectable: true,
            risk: "normal".into(),
            warning: None,
            status: "available".into(),
        }
    }

    #[test]
    fn prohibits_risky_classes_in_composites() {
        assert_eq!(
            evaluate(&device("1234", &["03", "08"])).level,
            PolicyLevel::Prohibited
        );
        assert_eq!(
            evaluate(&device("1234", &["01"])).level,
            PolicyLevel::Prohibited
        );
        assert_eq!(
            evaluate(&device("1234", &["0E"])).level,
            PolicyLevel::Prohibited
        );
    }

    #[test]
    fn warns_for_ftdi() {
        assert_eq!(
            evaluate(&device("0403", &["ff"])).level,
            PolicyLevel::Warning
        );
    }
}
