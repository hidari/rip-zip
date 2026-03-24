use std::path::Path;

use crate::error::ZipError;
use crate::types::FileEntry;

/// ファイルシステム走査の抽象化
///
/// walkdirなどの外部ライブラリをアダプター層で隔離するためのトレイト。
/// テスト時にはFake実装に差し替えて高速なユニットテストを実現する。
pub trait FileWalker {
    /// ソースディレクトリを走査し、FileEntryのイテレータを返す
    fn walk(&self, source_dir: &Path) -> Box<dyn Iterator<Item = Result<FileEntry, ZipError>>>;
}

/// ZIPアーカイブ書き込みの抽象化
///
/// zipクレートなどの外部ライブラリをアダプター層で隔離するためのトレイト。
/// create → add_file (N回) → finish のライフサイクルを持つ。
pub trait ZipArchiver {
    /// ZIPファイルへの書き込みを開始する
    ///
    /// ZIP64形式はzip crateが自動判定するため、呼び出し側での指定は不要。
    fn create(&mut self, target_zip: &Path) -> Result<(), ZipError>;

    /// ファイルをZIPに追加する
    fn add_file(
        &mut self,
        name: &str,
        source_path: &Path,
        unix_permissions: u32,
    ) -> Result<(), ZipError>;

    /// ZIPの書き込みを完了する
    fn finish(&mut self) -> Result<(), ZipError>;
}

/// ターミナル判定の抽象化
///
/// is-terminalクレートをアダプター層で隔離するためのトレイト。
pub trait Terminal {
    /// 標準入力がターミナルに接続されているかを判定する
    fn is_stdin_terminal(&self) -> bool;
}
