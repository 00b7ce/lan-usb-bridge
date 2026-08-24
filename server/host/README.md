# Server Host Integration

`usb-bridge-host-agent` is the only LAN USB Bridge component that runs as root. It
listens on a Unix socket and permits only `usbip bind` and `usbip unbind` for validated
USB bus IDs. The Web/API container remains non-root.

## Build and install

Run from the repository root on the Linux host:

```bash
sudo groupadd --system --force usb-bridge
sudo usermod --append --groups usb-bridge "$USER"

docker run --rm -v "$PWD:/workspace" -w /workspace rust:bookworm \
  cargo build --release --locked --package usb-bridge-host-agent

sudo install -o root -g root -m 0755 \
  target/release/usb-bridge-host-agent /usr/local/sbin/
sudo install -o root -g root -m 0644 \
  server/host/usb-bridge-host-agent.service server/host/usbipd.service \
  /etc/systemd/system/

sudo systemctl daemon-reload
sudo systemctl enable --now usb-bridge-host-agent.service usbipd.service
```

Verify both services:

```bash
systemctl --no-pager --full status \
  usb-bridge-host-agent.service usbipd.service
```

## Container permissions

Find the numeric GID of the restricted group:

```bash
getent group usb-bridge
```

Set the same numeric GID and enable real USB control in `.env`:

```dotenv
USB_CONTROL_BACKEND=host-agent
USB_BRIDGE_GID=983
```

Replace `983` with the value from the host. Sign out and back in after adding your user
to the group so the current shell receives the new membership.

## Runtime paths and commands

- Unix socket: `/run/lan-usb-bridge/host-agent.sock`
- Host executable: `/usr/local/sbin/usb-bridge-host-agent`
- Linux USB/IP utility: `/usr/sbin/usbip`
- USB/IP daemon: `/usr/sbin/usbipd --ipv4`

The host agent validates that each request:

- Contains at least one device
- Uses a kernel-style bus ID containing only digits, `-`, and `.`
- Refers to a present USB device rather than an interface or hub
- Does not contain a mass-storage, audio, or video interface
- Requests only bind or unbind

TCP 3240 should be reachable only from trusted LAN clients and must not be published to
the Internet.

## Diagnostics

```bash
/usr/sbin/usbip list -l
ss -lntp | grep ':3240'
journalctl -u usb-bridge-host-agent.service -u usbipd.service
```

Before manually running `usbip unbind`, confirm the exact bus ID and stop applications
using the remote device.
