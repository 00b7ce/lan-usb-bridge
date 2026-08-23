use std::collections::BTreeMap;

use usb_bridge_protocol::UsbDevice;

#[derive(Clone, Debug)]
pub struct DeviceGroup {
    pub id: String,
    pub title: String,
    pub devices: Vec<UsbDevice>,
}

pub fn group_devices(devices: &[UsbDevice]) -> Vec<DeviceGroup> {
    let mut groups: BTreeMap<String, Vec<UsbDevice>> = BTreeMap::new();
    for device in devices {
        let key = device
            .parent_hub
            .clone()
            .unwrap_or_else(|| device.bus_id.clone());
        groups.entry(key).or_default().push(device.clone());
    }
    groups
        .into_iter()
        .map(|(id, mut devices)| {
            devices.sort_by(|left, right| left.bus_id.cmp(&right.bus_id));
            let title = if devices.len() > 1 {
                format!("USBハブ {id}")
            } else {
                "単体デバイス".to_owned()
            };
            DeviceGroup { id, title, devices }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: &str, parent: Option<&str>) -> UsbDevice {
        UsbDevice {
            bus_id: id.into(),
            vendor_id: "1".into(),
            product_id: "2".into(),
            manufacturer: None,
            product: None,
            serial_number: None,
            device_class: "00".into(),
            interface_classes: vec!["03".into()],
            drivers: vec![],
            parent_hub: parent.map(Into::into),
            selected: false,
            selectable: true,
            risk: "normal".into(),
            warning: None,
            status: "available".into(),
        }
    }

    #[test]
    fn groups_siblings_by_parent_hub() {
        let groups = group_devices(&[
            device("1-1.1", Some("1-1")),
            device("1-1.2", Some("1-1")),
            device("2-1", None),
        ]);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].devices.len(), 2);
    }
}
