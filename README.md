# LAN USB Bridge

LAN USB Bridge shares USB devices connected to a Linux host, such as a Raspberry Pi,
with Windows PCs over a trusted local network. Linux USB/IP carries the USB traffic;
this project adds device discovery, exclusive session management, safe bind/unbind
operations, and native Windows controls around it.

> [!WARNING]
> This is an experimental project. USB/IP operates below the application layer, and a
> driver or network failure can interrupt a device immediately. Do not expose TCP 3240
> or the management API to the Internet. Storage, audio, and video devices are blocked
> by the current safety policy.

## Current status

The end-to-end path has been verified with a Raspberry Pi server, Windows 11,
usbip-win2 0.9.7.7, and an M5Stack Atom-S3 USB serial device. Windows received the
device as a COM port and completed bidirectional serial communication.

usbip-win2 0.9.7.8 did not work with the tested Atom-S3 environment. Version 0.9.7.7
is therefore the currently tested client version, not a claim that every 0.9.7.8
installation is broken.

Implemented features include:

- Dynamic USB enumeration from Linux sysfs
- A non-root API/Web container and a small privileged host agent
- Validated `usbip bind` and `usbip unbind` operations only
- One exclusive client session with incremental acquire and partial release
- Individual and checkbox-based batch attach/detach in the Windows GUI
- Automatic detach and session release on normal GUI exit
- USB topology grouping for display without product-specific bundle rules
- Rollback after multi-device attach failure
- Automatic restoration of remaining Windows attachments when usbip-win2 drops more
  ports than the requested detach
- Persistent client settings, periodic refresh, and rotating local logs

## Architecture

```text
Windows GUI / CLI
  |  HTTP API :8080                 USB/IP :3240
  |--------------------------------------+
                                         |
Raspberry Pi / Linux                     |
  +-- usb-bridge-server (container)       |
  |     +-- sysfs discovery              |
  |     +-- selection/session API        |
  |     `-- Unix socket -----------------+-- usb-bridge-host-agent (root)
  |                                            `-- usbip bind/unbind
  `-- usbipd ------------------------------------ USB device
```

The API container mounts `/sys` read-only and runs as a non-root user. Only the host
agent runs with root privileges. The host agent accepts validated USB bus IDs over a
Unix socket and does not execute arbitrary commands.

## Repository layout

```text
lan-usb-bridge/
├── server/              Axum API, Web UI, host integration, and host agent
├── client/              Windows CLI
│   ├── core/            API, configuration, policy, and USB/IP control
│   └── gui/             Native egui/eframe/glow Windows GUI
├── crates/protocol/     Shared API types
├── compose.yml
└── Cargo.toml           Rust workspace
```

## Requirements

### Raspberry Pi / Linux server

- A Linux host with USB/IP kernel support and the distribution's USB/IP tools
- Docker Engine with Docker Compose
- systemd for the supplied host services
- Rust stable when building the host agent on the host
- A trusted LAN that permits Windows to reach TCP 8080 and 3240

### Windows client

- Windows 10 x64 version 1903 or later, or a usbip-win2-supported Windows 11 ARM64 system
- [usbip-win2](https://github.com/vadimgrn/usbip-win2) and its drivers
  - Version 0.9.7.7 is the version verified by this project
  - Create a Windows restore point before installing or changing the driver
- Rust stable when building from source

## Server setup

Run these commands from the repository root on the Linux server.

1. Install the distribution's USB/IP tools and make sure `usbip` and `usbipd` are
   available. On Raspberry Pi OS/Debian this package is normally named `usbip`.
2. Create the restricted host-agent group and build the agent:

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

3. Create `.env`. Replace the UID and GID with values reported by `id -u` and
   `getent group usb-bridge`:

```dotenv
USB_BRIDGE_BIND_ADDRESS=0.0.0.0
USB_BRIDGE_PORT=8080
USB_BRIDGE_UID=1000
USB_BRIDGE_GID=983
USB_CONTROL_BACKEND=host-agent
RUST_LOG=usb_bridge_server=info
```

4. Sign out and back in if you just added your account to the `usb-bridge` group, then
   start the API container:

```bash
docker compose up -d --build
docker compose ps
curl http://127.0.0.1:8080/health
```

The default `.env.example` binds the API to localhost and uses the mock control backend.
LAN binding and `host-agent` must be enabled explicitly. See
[server/host/README.md](server/host/README.md) for host integration details.

For development without real USB operations:

```bash
docker compose -f compose.yml -f compose.dev.yml up --build
```

## Windows client setup

Install usbip-win2, then either add `usbip.exe` to `PATH` or configure its absolute
path. Build and run the native GUI:

```powershell
cargo build --release --package usb-bridge-gui
.\target\release\usb-bridge-gui.exe
```

The first run creates a per-user configuration file. It is normally under:

```text
%APPDATA%\lan-usb-bridge\LAN USB Bridge\config\config.json
```

Example:

```json
{
  "server_url": "http://192.168.10.8:8080/",
  "client_id": "gaming-pc-1",
  "usbip_path": "C:\\Program Files\\USBip\\usbip.exe"
}
```

In the GUI, select one or more devices with the checkboxes and use **Attach selected**
or **Detach selected**. Each device also has its own attach/detach button. If a selection
contains both attached and detached devices, each batch button operates only on the
applicable subset.

The CLI provides the same core workflow:

```powershell
usb-bridge-client health
usb-bridge-client devices
usb-bridge-client session
usb-bridge-client usbip list
usb-bridge-client usbip attach 2-2
usb-bridge-client usbip detach 2-2
usb-bridge-client release
```

Some systems require an elevated Windows Terminal for driver attach/detach operations.
Read-only API commands normally do not require elevation.

## Device policy

The server and client reject a whole composite device when any interface has one of
these classes:

| Interface class | Device type | Policy |
|---|---|---|
| `08` | Mass storage | Blocked |
| `01` | Audio | Blocked |
| `0e` | Video | Blocked |

FTDI devices with vendor ID `0403` are allowed with a compatibility warning. Other
devices are not guaranteed to work merely because they pass the policy.

## API

```text
GET  /health
GET  /api/devices
POST /api/selection
GET  /api/session
POST /api/acquire
POST /api/release
```

Acquire a device:

```bash
curl -X POST http://127.0.0.1:8080/api/acquire \
  -H 'content-type: application/json' \
  -d '{"client_id":"gaming-pc-1","devices":["2-2"]}'
```

The same client can call acquire again to add devices to its session. A different
client receives HTTP 409 while the session is owned.

Release selected devices while keeping the rest of the session:

```bash
curl -X POST http://127.0.0.1:8080/api/release \
  -H 'content-type: application/json' \
  -d '{"client_id":"gaming-pc-1","devices":["2-2"]}'
```

An empty `devices` array, or an older request containing only `client_id`, releases the
entire session. An empty acquire array uses the selection saved by the Web UI.

## Known limitations

- The management API has no authentication or TLS. Use it only on a trusted LAN.
- USB/IP traffic on TCP 3240 is not encrypted by this project.
- Session state is held in memory. Server restarts can forget a session while a device
  remains bound.
- A normal GUI exit detaches devices and releases its session. There is no lease,
  heartbeat, or automatic server-side release after forced termination, a client
  crash, or power loss.
- Hot-unplug and reconnect recovery is not complete.
- Only one client ID can own a server session at a time, although that session can
  contain multiple devices.
- usbip-win2 compatibility varies by device and version. Only 0.9.7.7 has been verified
  in the current Atom-S3 test environment.
- On the tested 0.9.7.7 installation, detaching one port could temporarily remove other
  imported ports. The client detects this and reattaches devices that remain in the
  server session, causing a brief interruption.

## Troubleshooting

- **The API is unreachable:** Check the Pi address, `docker compose ps`, port 8080, and
  the host firewall.
- **`usbip list` is empty:** Confirm `usbipd` and the host agent are running, TCP 3240 is
  reachable, and the API acquired/bound the device.
- **Attach is denied:** Retry the Windows client from an elevated terminal.
- **HTTP 409:** Another client ID owns the current session. The application does not
  forcefully steal it.
- **A device stays exported after a failure:** Stop using the device, then release it
  through the owning client or run a controlled `usbip unbind --busid <BUS_ID>` on the
  server.
- **The device enumerates but does not function:** Check the usbip-win2 version, Windows
  Device Manager, `usbip.exe port`, `journalctl -u usbipd`, and host-agent logs.

## Development

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
```

Windows release artifacts:

```powershell
cargo build --release --package usb-bridge-client
cargo build --release --package usb-bridge-gui
```

## License

Licensed under the [Apache License 2.0](LICENSE).
