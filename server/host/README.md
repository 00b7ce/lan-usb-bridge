# Server host integration

Linuxの`usbip-core`、`usbip-host`、`usbipd`と、限定された権限で
`bind`/`unbind`を行うヘルパーはここに追加します。

現在のコンテナは安全な`mock`バックエンドだけを実装しており、ホストのUSB状態は変更しません。
