# USB Bridge

Raspberry Piに接続したUSBデバイスをPC間で切り替えるための管理サービスです。
データ転送にはLinux USB/IPを利用し、このアプリはデバイス検出、排他制御、
一括接続・解放とWeb UIを担当する予定です。

## Workspace構成

```text
usb_bridge/
├─ server/            Raspberry Pi用サーバーとWeb UI
├─ client/            Windowsクライアント
├─ crates/protocol/   APIの共有データ型
├─ compose.yml
└─ Cargo.toml         Workspace定義
```

Workspace全体の確認:

```bash
cargo check --workspace
```

Windowsクライアントの起動:

```powershell
cargo run --package usb-bridge-client
```

常用構成ではPiのsysfsを読み取り専用で参照し、USBハブを除くUSBデバイスを
動的に列挙します。USB/IPの`bind`/`unbind`は、ホスト上で動作する権限分離された
`usb-bridge-host-agent`を明示的に有効化した場合だけ実行します。

## 起動

本番相当:

```bash
docker compose -f app/usb_bridge/compose.yml up --build
```

開発用（Rustソース変更時に自動再起動）:

```bash
docker compose \
  -f app/usb_bridge/compose.yml \
  -f app/usb_bridge/compose.dev.yml \
  up --build
```

開発構成はUSBを誤操作しないよう`mock`バックエンドを使用します。

ブラウザーで次を開くと、USBデバイスの個別選択と親ハブ単位の一括選択ができます。

```text
http://127.0.0.1:8080/
```

選択結果は`data/selection.json`へ保存されます。

標準ではホストの`127.0.0.1:8080`だけで待ち受けます。LANへ直接公開する場合は、
信頼できるネットワークであることを確認してから`.env`へ次を設定します。

```dotenv
USB_BRIDGE_BIND_ADDRESS=0.0.0.0
USB_BRIDGE_PORT=8080
RUST_LOG=usb_bridge_server=info
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

取得要求の例:

```bash
curl -X POST http://127.0.0.1:8080/api/acquire \
  -H 'content-type: application/json' \
  -d '{"client_id":"gaming-pc-1","devices":[]}'
```

`devices`が空の場合はWeb UIで保存した選択を利用します。

## セキュリティ方針

- Web/APIコンテナは非rootかつ`no-new-privileges`で動かします。
- USB sysfsのシンボリックリンク解決に必要な`/sys`全体を読み取り専用でマウントします。
- ストレージ、ネットワーク、オーディオ、映像クラスには警告を表示します。
- USB/IPのroot操作は、後から小さなホストヘルパーとして分離します。
- 任意コマンドではなく、登録済みUSBパスの`bind`/`unbind`だけを許可します。
- USB/IPのTCP 3240をインターネットへ公開しません。

## ライセンス

このプロジェクトは[Apache License 2.0](LICENSE)で公開されています。
