# LAN USB Bridge Windows Client

LAN USB Bridgeサーバーの利用権管理APIと、Windows用USB/IPクライアント
[usbip-win2](https://github.com/vadimgrn/usbip-win2) を扱うCLIです。

> 現在のサーバーはUSBデバイス列挙とセッション管理までです。Linux側の
> `usbip bind` / `unbind` や `usbipd` の公開処理はまだ実装されていません。
> そのため、本CLIだけで実機USB転送が完結する段階ではありません。

## 必要環境とビルド

- Rust（Rust 2024 edition対応のstable toolchain）
- Windows 10 x64 version 1903以降、またはusbip-win2対応のWindows 11 ARM64
- LAN USB BridgeサーバーへのHTTP(S)接続
- USB転送にはusbip-win2と対応ドライバー

```powershell
cargo build --release --package usb-bridge-client
```

生成物は `target\release\usb-bridge-client.exe` です。

## usbip-win2の準備

1. [usbip-win2公式Releases](https://github.com/vadimgrn/usbip-win2/releases) の案内に従ってインストールします。
2. 公式READMEの注意どおり、事前にWindowsの復元ポイントを作成してください。インストール中はUSBデバイスが一時的に再起動します。
3. `usbip.exe` をPATHへ追加するか、`--usbip-path "C:\Program Files\USBip\usbip.exe"` で指定します。

本クライアントが使う公式CLI形式は `list -r HOST`、`attach -r HOST -b BUS_ID`、
`port`、`detach -p PORT` です。`detach BUS_ID` は `port` 出力から対応ポートを検索します。

## 設定

初回起動時にユーザー別設定ディレクトリへ `config.json` を作り、ランダムな
`client_id` を永続化します。Windowsでは通常 `%APPDATA%\lan-usb-bridge\LAN USB Bridge\config\config.json`
相当です（実際のパスはWindowsのKnown Folderに従います）。

```json
{
  "server_url": "http://192.168.1.20:8080",
  "client_id": "gaming-pc-1",
  "usbip_path": "C:\\Program Files\\USBip\\usbip.exe"
}
```

優先順位はコマンドライン、環境変数、設定ファイル、既定値です。

| 設定 | コマンドライン | 環境変数 | 既定値 |
|---|---|---|---|
| Server URL | `--server-url` | `USB_BRIDGE_SERVER_URL` | `http://127.0.0.1:8080` |
| Client ID | `--client-id` | `USB_BRIDGE_CLIENT_ID` | 永続生成 |
| usbip.exe | `--usbip-path` | `USB_BRIDGE_USBIP_PATH` | `usbip.exe` (PATH検索) |

TLS証明書検証は無効化しません。USB/IP接続先にはServer URLのホストだけを使います。

### デバイスポリシー

- USB Mass Storage（interface class `08`）は禁止
- USB Audio（interface class `01`）は禁止
- USB Video（interface class `0e`）は禁止
- FTDI（vendor ID `0403`）はusbip-win2の既知の互換性問題があるためWARNING

禁止デバイスは一覧には理由付きで表示しますが、APIのacquireとUSB/IP attachを拒否します。
複合デバイスは、いずれかのインターフェースが禁止クラスならデバイス全体を禁止します。

## CLI例

```powershell
usb-bridge-client health
usb-bridge-client devices
usb-bridge-client session
usb-bridge-client acquire 1-1.2 1-1.3
usb-bridge-client release
usb-bridge-client usbip status
usb-bridge-client usbip list
usb-bridge-client usbip attach 1-1.2
usb-bridge-client usbip detach 1-1.2
usb-bridge-client --dry-run usbip attach 1-1.2
```

`usbip attach` はデバイスを確認し、APIのacquire成功後だけ実行します。attach失敗時は
セッションをベストエフォートで解放します。別client_idのセッションを強制取得・解放しません。

## 管理者権限

usbip-win2のインストールとドライバー操作には管理者権限が必要です。環境によって
attach/detachにも昇格が必要です。アクセス拒否を検出すると、管理者としてWindows
Terminalを起動し直す案内を表示します。API参照コマンドには不要です。

## 未実装

- GUI、常駐、自動再接続
- 認証・トークン（現行サーバーAPIにも未実装）
- Linuxサーバー側のUSB/IP bind/unbindとusbipd公開制御
- 複数デバイスの一括attachと途中失敗時の完全なロールバック
- usbip-win2の永続再接続（stash）管理

## トラブルシューティング

- `usbip-win2が見つかりません`: PATHまたは `--usbip-path` を確認します。
- `接続できません`: Server URL、PiのIP、ポート、ファイアウォールを確認します。
- `HTTP 409`: 別セッションが使用中です。強制取得はしません。
- `usbip list` に表示されない: 現行サーバーはbindを行いません。Pi側でのUSB/IP公開が別途必要です。
- `attach` がアクセス拒否: 管理者としてWindows Terminalを起動します。
- ストレージ機器: detach前に書き込みを止め、安全に取り外せる状態を確認します。
- 実行予定だけ確認する: `--dry-run` を使います。APIのacquire/releaseも変更しません。
