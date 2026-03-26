use std::io::Write;
use std::path::Path;

use crate::error::ZipError;
use crate::types::{FileEntry, ZipEntryInfo};

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

/// ZIPアーカイブ読み取りの抽象化（2フェーズ分離設計）
///
/// コンストラクタでZIPファイルを開き、アーカイブ状態を保持するステートフルなトレイト。
/// scan（事前スキャン）とextract_entry（個別エントリ展開）を
/// 独立したメソッドとして提供する。
/// zipクレートなどの外部ライブラリをアダプター層で隔離するためのトレイト。
pub trait ZipReader {
    /// ZIPアーカイブ内のエントリ一覧を事前スキャンする
    ///
    /// バリデーション（合計サイズ・ファイル数・重複検出等）のために
    /// 展開前にメタデータを取得する。
    fn scan(&mut self) -> Result<Vec<ZipEntryInfo>, ZipError>;

    /// 指定エントリのデータを展開してwriterに書き込む
    ///
    /// 返り値は書き込んだバイト数。
    fn extract_entry(&mut self, entry_name: &str, writer: &mut dyn Write) -> Result<u64, ZipError>;
}

/// ファイルシステム書き込みの抽象化
///
/// 展開時のファイルシステム操作をアダプター層で隔離するためのトレイト。
/// テスト時にはFake実装に差し替えて安全にテストする。
pub trait FileWriter {
    /// ディレクトリを再帰的に作成する
    fn create_dir_all(&self, path: &Path) -> Result<(), ZipError>;

    /// ファイルを書き込む（バイトデータを書き込み、パーミッションを設定）
    ///
    /// 返り値は書き込んだバイト数。
    fn write_file(&self, path: &Path, data: &[u8], permissions: u32) -> Result<u64, ZipError>;

    /// パスが存在するかを判定する
    fn exists(&self, path: &Path) -> bool;

    /// パスがシンボリックリンクかを判定する
    fn is_symlink(&self, path: &Path) -> bool;
}
