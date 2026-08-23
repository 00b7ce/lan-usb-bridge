# Server host integration

`usb-bridge-host-agent`だけをrootでホスト上に置き、Unixソケット経由で検証済みの
USB BUS IDに対する`usbip bind/unbind`だけを許可します。Web/APIコンテナは非rootの
まま動作します。

## ビルドと配置

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

`usb-bridge`グループのGIDを確認し、Composeの`USB_BRIDGE_GID`と一致させます。

```bash
getent group usb-bridge
```

表示された数値GIDを`.env`へ設定してください。グループ追加を現在のシェルへ反映するには、
再ログインが必要です。

ホストエージェントと実転送を有効にするには`.env`へ追加します。

```dotenv
USB_CONTROL_BACKEND=host-agent
USB_BRIDGE_GID=<usb-bridgeグループのGID>
```

TCP 3240は信頼できるLANからだけ到達可能にし、インターネットへ公開しないでください。
