# Configuration Data

USB devices are enumerated dynamically from `/sys/bus/usb/devices`; no fixed device
inventory is required.

The server Web UI stores its selected USB bus IDs in `../data/selection.json`. This
runtime file is excluded from Git. An acquire request with an empty `devices` array uses
the saved selection.

Bus IDs describe the current USB topology and can change when a device is moved to a
different port. Persistent matching by serial number or VID/PID is not implemented.
Future server-side identification or policy configuration can be added under this
directory.
