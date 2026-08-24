# LAN USB Bridge Windows Client

The Windows client combines the LAN USB Bridge session-management API with
[usbip-win2](https://github.com/vadimgrn/usbip-win2). It is split into a shared core
library, a command-line client, and a native Windows GUI.

```text
client/
├── core/   API, configuration, device policy, USB/IP, and connection control
├── gui/    Native egui + eframe + glow GUI
└── src/    CLI
```

The server must run with `usb-bridge-host-agent` and `usbipd` enabled for real USB
transfers. usbip-win2 0.9.7.7 is the currently verified Windows client version.

## Requirements

- Rust stable with Rust 2024 edition support
- Windows 10 x64 version 1903 or later, or a usbip-win2-supported Windows 11 ARM64 system
- HTTP access to the LAN USB Bridge server
- usbip-win2 and its drivers for USB transfer

Build release binaries:

```powershell
cargo build --release --package usb-bridge-client
cargo build --release --package usb-bridge-gui
```

Artifacts:

```text
target\release\usb-bridge-client.exe
target\release\usb-bridge-gui.exe
```

The release GUI does not open a console window.

## Native GUI

```powershell
cargo run --release --package usb-bridge-gui
```

The GUI provides:

- Compact Windows-native dark UI
- Individual attach and detach controls for every USB device
- Checkbox-based batch attach and detach
- Automatic detach and server-session release on normal GUI exit
- A 10-second heartbeat that keeps the server lease active while the GUI owns a session
- Console-free `usbip.exe` child processes and a disabled UI during attach/detach transitions
- USB topology grouping for display only; devices under a hub remain individually controlled
- Statuses for available, attached, warning, blocked, owned by another PC, and errors
- Background workers for HTTP, `usbip.exe`, wait operations, settings, and log writes
- Five-second refresh while visible and twenty-second refresh while hidden or minimized
- Rotating per-user logs with a 1 MiB limit and one retained generation

The renderer uses the `accesskit`, `default_fonts`, and `glow` features of eframe
0.36.1. It does not use WebView, WASM, wgpu, or Node.js. Japanese UI text uses the
first available Windows font from Yu Gothic, Meiryo, and MS Gothic.

## Installing usbip-win2

1. Follow the instructions in the
   [official usbip-win2 releases](https://github.com/vadimgrn/usbip-win2/releases).
2. Create a Windows restore point first. Installing the driver temporarily restarts
   USB devices.
3. Add `usbip.exe` to `PATH`, or configure an absolute path such as
   `C:\Program Files\USBip\usbip.exe`.

The current test environment works with usbip-win2 0.9.7.7. Version 0.9.7.8 did not
work with the tested Atom-S3 device, although that does not establish a universal
0.9.7.8 incompatibility.

The client uses these usbip-win2 operations:

```text
list -r HOST
attach -r HOST -b BUS_ID
attach -r HOST -b BUS_ID --stop
port
detach -p PORT
```

The application resolves a remote bus ID to a Windows USB/IP port from `usbip.exe port`
before detaching. In the tested 0.9.7.7 environment, detaching one port could also drop
other imported ports. The client immediately reattaches devices that remain in the
server session.

## Configuration

The first run creates `config.json` in the per-user configuration directory and stores
a generated `client_id`. On Windows the normal location is:

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

Configuration precedence is command line, environment, configuration file, then the
default value.

| Setting | Command line | Environment | Default |
|---|---|---|---|
| Server URL | `--server-url` | `USB_BRIDGE_SERVER_URL` | `http://127.0.0.1:8080` |
| Client ID | `--client-id` | `USB_BRIDGE_CLIENT_ID` | Persisted generated value |
| usbip.exe | `--usbip-path` | `USB_BRIDGE_USBIP_PATH` | `usbip.exe` from `PATH` |

TLS certificate validation is not disabled. The USB/IP host is always derived from the
configured server URL.

## Device policy

- USB mass storage, interface class `08`: blocked
- USB audio, interface class `01`: blocked
- USB video, interface class `0e`: blocked
- FTDI, vendor ID `0403`: allowed with a compatibility warning

If any interface of a composite device is blocked, the entire device is blocked. A
blocked device remains visible with its reason, but acquire and attach are disabled.

## CLI examples

```powershell
usb-bridge-client health
usb-bridge-client devices
usb-bridge-client session
usb-bridge-client acquire 1-1.2 1-1.3
usb-bridge-client release
usb-bridge-client usbip status
usb-bridge-client usbip list
usb-bridge-client usbip attach 2-2
usb-bridge-client usbip detach 2-2
usb-bridge-client --dry-run usbip attach 2-2
```

`usbip attach` validates the device, acquires it through the API, and then runs
usbip-win2. An attach failure triggers best-effort rollback. The same client can add
devices to its session and release selected devices. A different `client_id` cannot
forcefully acquire or release the current session.

## Administrator privileges

Installing usbip-win2 and managing its drivers requires administrator privileges.
Depending on the system, attach and detach may also require elevation. When the client
detects access denial, it suggests restarting Windows Terminal as administrator.
Read-only API commands do not normally require elevation.

## Known limitations

- The server API has no authentication or access token.
- A normal GUI exit detaches devices and releases the server session. Forced
  termination, a Windows crash, or power loss can still leave the session active
  until the server-side lease expires (45 seconds by default).
- Hot-unplug and reconnect recovery is not complete.
- usbip-win2 persistent-device stash management is not implemented.
- Device compatibility depends on both usbip-win2 and the device driver stack.

## Troubleshooting

- **usbip-win2 is not found:** Check `PATH` or the configured `usbip_path`.
- **The server is unreachable:** Check the server URL, Pi address, port, and firewall.
- **HTTP 409:** Another client ID owns the session; the client does not steal it.
- **`usbip list` is empty:** Check the host agent, `usbipd`, TCP 3240, and the API acquire result.
- **Attach is denied:** Run the Windows client from an elevated terminal.
- **A device enumerates but does not work:** Check the usbip-win2 version, Device Manager,
  `usbip.exe port`, and the server logs.
- **Preview operations only:** Use `--dry-run`; it does not acquire or release API state.
