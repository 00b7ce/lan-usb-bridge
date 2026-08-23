# USB Bridge Server

Raspberry Pi上で動作するRust/Axumサーバーです。USBデバイスの列挙、選択保存、
クライアントセッションの排他管理とWeb UIを提供します。

現時点ではsysfsを読み取り専用で参照し、USB/IPのbind/unbindは行いません。
