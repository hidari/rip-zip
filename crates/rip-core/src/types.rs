use std::path::PathBuf;

/// ファイルシステムのエントリを表すドメイン型
///
/// walkdir::DirEntryに依存しない独自の型。
/// アダプター層で外部ライブラリのエントリから変換される。
#[derive(Debug, Clone)]
pub struct FileEntry {
    /// ファイルの絶対パス
    pub path: PathBuf,
    /// ソースディレクトリからの相対パス
    pub relative_path: PathBuf,
    /// シンボリックリンクかどうか
    pub is_symlink: bool,
    /// 通常ファイルかどうか
    pub is_file: bool,
    /// ファイルサイズ（バイト）
    pub size: u64,
    /// Unixパーミッション（非Unix環境では0o644）
    pub unix_permissions: u32,
}

/// ZIP作成の統計情報
#[derive(Debug, Default, Clone)]
pub struct ZipStats {
    /// 追加されたファイル数
    pub file_count: usize,
    /// 合計サイズ（バイト）
    pub total_size: u64,
}

/// ZIP作成中に発生するイベント
///
/// rip-coreは副作用を持たず、このイベントをコールバック経由で
/// 呼び出し元（rip-cli）に通知する。表示の制御は呼び出し元が行う。
pub enum ZipEvent {
    /// ファイルがアーカイブに追加された
    FileAdded { name: String, size: u64 },
    /// シンボリックリンクがスキップされた
    SymlinkSkipped { path: PathBuf },
    /// ファイルがスキップされた（理由付き）
    FileSkipped { name: String, reason: String },
    /// ZIP作成が開始された
    ArchiveStarted { target: PathBuf },
    /// アーカイブの作成が完了した
    ArchiveCompleted { stats: ZipStats },
}

#[cfg(test)]
mod tests {
    /// FileEntry の仕様
    mod file_entry {
        use super::super::*;

        #[test]
        fn clone_preserves_all_fields() {
            let entry = FileEntry {
                path: PathBuf::from("/tmp/test/file.txt"),
                relative_path: PathBuf::from("file.txt"),
                is_symlink: false,
                is_file: true,
                size: 1024,
                unix_permissions: 0o644,
            };
            let cloned = entry.clone();
            // 全フィールドが保持されていることを確認
            assert_eq!(cloned.path, entry.path);
            assert_eq!(cloned.relative_path, entry.relative_path);
            assert_eq!(cloned.is_symlink, entry.is_symlink);
            assert_eq!(cloned.is_file, entry.is_file);
            assert_eq!(cloned.size, entry.size);
            assert_eq!(cloned.unix_permissions, entry.unix_permissions);
        }
    }

    /// ZipStats の仕様
    mod zip_stats {
        use super::super::*;

        #[test]
        fn default_initializes_to_zero() {
            let stats = ZipStats::default();
            assert_eq!(stats.file_count, 0);
            assert_eq!(stats.total_size, 0);
        }
    }

    /// ZipEvent の仕様
    mod zip_event {
        use super::super::*;

        #[test]
        fn all_variants_are_constructable() {
            // 全バリアントが構築可能であることを確認
            let _ = ZipEvent::FileAdded {
                name: "test.txt".to_string(),
                size: 100,
            };
            let _ = ZipEvent::SymlinkSkipped {
                path: PathBuf::from("/tmp/link"),
            };
            let _ = ZipEvent::FileSkipped {
                name: "big.bin".to_string(),
                reason: "exceeds size limit".to_string(),
            };
            let _ = ZipEvent::ArchiveStarted {
                target: PathBuf::from("/tmp/out.zip"),
            };
            let _ = ZipEvent::ArchiveCompleted {
                stats: ZipStats::default(),
            };
        }
    }
}
