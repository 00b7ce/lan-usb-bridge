# LAN USB Bridge Server

The server is a Rust/Axum service for Raspberry Pi and other Linux USB/IP hosts. It
provides USB device discovery, saved Web UI selection, exclusive client sessions, and
controlled USB/IP export through a privileged host agent.

## Responsibilities

- Enumerate USB devices from Linux sysfs
- Reject hubs and prohibited interface classes
- Save the Web UI selection in `data/selection.json`
- Allow one client ID to own a session containing one or more devices
- Add devices to an existing session owned by the same client
- Release selected devices or the whole session
- Delegate validated `usbip bind` and `usbip unbind` operations to the host agent

The API process does not run arbitrary privileged commands. With
`USB_CONTROL_BACKEND=host-agent`, it sends a restricted request over a Unix socket to
`usb-bridge-host-agent`. See [host/README.md](host/README.md).

## Run with Docker Compose

From the repository root:

```bash
cp .env.example .env
docker compose up -d --build
docker compose ps
curl http://127.0.0.1:8080/health
```

`.env.example` uses the mock control backend and binds the management API to localhost.
For real transfers, configure the host agent and set:

```dotenv
USB_BRIDGE_BIND_ADDRESS=0.0.0.0
USB_BRIDGE_PORT=8080
USB_BRIDGE_UID=1000
USB_BRIDGE_GID=<usb-bridge-group-gid>
USB_CONTROL_BACKEND=host-agent
RUST_LOG=usb_bridge_server=info
```

Use a numeric GID in the actual `.env` file. LAN exposure is appropriate only on a
trusted network.

For development with mock devices and automatic Rust rebuilds:

```bash
docker compose -f compose.yml -f compose.dev.yml up --build
```

## API

```text
GET  /health
GET  /api/devices
POST /api/selection
GET  /api/session
POST /api/acquire
POST /api/release
```

An empty acquire device list uses the saved Web UI selection. Repeated acquire calls
from the current owner add devices. A release request with device IDs removes only
those devices; an empty list releases the full session.

## Environment variables

| Variable | Default | Purpose |
|---|---|---|
| `LISTEN_ADDRESS` | `0.0.0.0:8080` | Address inside the container |
| `USB_BACKEND` | `sysfs` | `sysfs` or `mock` device enumeration |
| `USB_CONTROL_BACKEND` | `mock` | `mock` or `host-agent` USB control |
| `USB_SYSFS_ROOT` | `/host/sys/bus/usb/devices` | Mounted sysfs path |
| `HOST_AGENT_SOCKET` | `/run/lan-usb-bridge/host-agent.sock` | Host-agent Unix socket |
| `SELECTION_FILE` | `/data/selection.json` | Saved Web UI selection |
| `RUST_LOG` | `usb_bridge_server=info` in Compose | Logging filter |

Compose additionally uses `USB_BRIDGE_BIND_ADDRESS`, `USB_BRIDGE_PORT`,
`USB_BRIDGE_UID`, and `USB_BRIDGE_GID` for host publishing and container identity.

## Security model

- The API container runs as a non-root UID/GID.
- `/sys` is mounted read-only.
- The container filesystem is read-only and uses `no-new-privileges`.
- The host-agent socket is mounted read-only into the container; clients only need to
  connect to it.
- The host agent validates bus-ID syntax, confirms the device exists, rejects hubs and
  blocked classes, and exposes only bind/unbind actions.
- TCP 8080 and 3240 must not be exposed to the Internet.

## Known limitations

- Session state is in memory and is not recovered after a server restart.
- A client crash does not currently expire or release its session automatically.
- A restart can leave kernel USB/IP bind state inconsistent with the in-memory session.
- Hot-unplug recovery is incomplete.
- The API has no authentication or TLS.
