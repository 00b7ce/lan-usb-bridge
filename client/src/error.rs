use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("設定エラー: {0}")]
    Config(String),
    #[error("設定ファイル {path} を読み書きできません: {source}")]
    ConfigIo {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("サーバーへ接続できません ({url}): {source}")]
    Connection { url: String, source: reqwest::Error },
    #[error("サーバー応答をJSONとして解析できません ({url}): {source}")]
    Json { url: String, source: reqwest::Error },
    #[error("サーバーが HTTP {status} を返しました: {message}")]
    Http {
        status: reqwest::StatusCode,
        message: String,
    },
    #[error(
        "usbip-win2が見つかりません。公式インストーラーで導入し、usbip.exeをPATHへ追加するか --usbip-path を指定してください"
    )]
    UsbipNotFound,
    #[error("usbip-win2の実行に失敗しました: {0}")]
    UsbipIo(#[source] std::io::Error),
    #[error("usbip-win2が終了コード {code:?} で失敗しました: {message}{admin_hint}")]
    UsbipFailed {
        code: Option<i32>,
        message: String,
        admin_hint: &'static str,
    },
    #[error("BUS_ID {0} に対応する接続済みUSB/IPポートが見つかりません")]
    UsbipPortNotFound(String),
    #[error("セッションは別のクライアント {0} が使用中です。強制取得・強制解放は行いません")]
    SessionOwnedByOther(String),
    #[error(
        "このクライアントには既存セッションがあります。重複取得は行いません。先に session または release を確認してください"
    )]
    SessionAlreadyExists,
    #[error("デバイス {0} はサーバーの一覧に存在しないか選択できません")]
    DeviceUnavailable(String),
}

pub type Result<T> = std::result::Result<T, ClientError>;
