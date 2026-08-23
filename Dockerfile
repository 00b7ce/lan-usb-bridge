FROM rust:bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY web ./web
RUN cargo build --release --locked

FROM debian:bookworm-slim AS runtime

COPY --from=builder /build/target/release/usb-bridge /usr/local/bin/usb-bridge

USER 10001:10001
EXPOSE 8080
ENV LISTEN_ADDRESS=0.0.0.0:8080 \
    RUST_LOG=usb_bridge=info \
    USB_BACKEND=sysfs \
    USB_SYSFS_ROOT=/host/sys/bus/usb/devices \
    SELECTION_FILE=/data/selection.json

ENTRYPOINT ["/usr/local/bin/usb-bridge"]
