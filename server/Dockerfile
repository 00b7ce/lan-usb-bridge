FROM rust:bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY client ./client
COPY crates/protocol ./crates/protocol
COPY server ./server
RUN cargo build --release --locked --package usb-bridge-server

FROM debian:bookworm-slim AS runtime

COPY --from=builder /build/target/release/usb-bridge-server /usr/local/bin/usb-bridge-server

USER 10001:10001
EXPOSE 8080
ENV LISTEN_ADDRESS=0.0.0.0:8080 \
    RUST_LOG=usb_bridge_server=info \
    USB_BACKEND=sysfs \
    USB_SYSFS_ROOT=/host/sys/bus/usb/devices \
    SELECTION_FILE=/data/selection.json

ENTRYPOINT ["/usr/local/bin/usb-bridge-server"]
