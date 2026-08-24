# LAN USB Bridge Protocol

This crate defines the serialized request, response, device, session, and host-control
types shared by the Linux server and Windows clients.

The protocol currently covers:

- Health and USB device responses
- Device selection
- Exclusive client sessions
- Incremental device acquire
- Partial or full device release
- Client heartbeat requests for session lease renewal
- Restricted host-agent bind/unbind requests

Changes to serialized fields must preserve compatibility where practical. For example,
`ReleaseRequest.devices` defaults to an empty list so older clients that send only a
`client_id` continue to release the full session.
