# Configuration

USBデバイスは`/sys/bus/usb/devices`から動的に列挙します。固定のデバイス一覧は不要です。

Web UIで保存した選択は`../data/selection.json`へ自動保存されます。このファイルには
現在選択しているUSB Bus IDだけが入り、Gitの管理対象には含めません。

将来、シリアル番号やVID/PIDを使った再識別規則と、安全性ポリシーをこのディレクトリへ追加します。
