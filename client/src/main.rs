use usb_bridge_protocol::SelectionRequest;

fn main() {
    let selection = SelectionRequest::default();
    println!(
        "USB Bridge Windows client scaffold ({} selected devices)",
        selection.devices.len()
    );
    println!("Server API and usbip-win2 integration are not implemented yet.");
}
