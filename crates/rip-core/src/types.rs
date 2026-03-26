use std::fmt;
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

/// ZIPエントリのメタデータを表すドメイン型
///
/// ZIPアーカイブ内の各エントリの情報を保持する。
/// アダプター層で外部ライブラリのエントリから変換される。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipEntryInfo {
    /// エントリ名（ZIPアーカイブ内のパス）
    pub name: String,
    /// 圧縮後のサイズ（バイト）
    pub compressed_size: u64,
    /// 展開後のサイズ（バイト）
    pub uncompressed_size: u64,
    /// ディレクトリエントリかどうか
    pub is_dir: bool,
    /// シンボリックリンクエントリかどうか
    pub is_symlink: bool,
    /// Unixパーミッション（格納されていない場合はNone）
    pub unix_permissions: Option<u32>,
}

/// ZIP展開の統計情報
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ExtractStats {
    /// 展開されたファイル数
    pub file_count: usize,
    /// 展開された合計サイズ（バイト）
    pub total_size: u64,
    /// スキップされたエントリ数
    pub skipped_count: usize,
}

/// ZIP展開のオプション
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ExtractOptions {
    /// 既存ファイルを上書きするかどうか
    pub overwrite: bool,
}

/// ファイルがスキップされた理由を表す型安全な列挙型
///
/// 文字列マッチングではなくパターンマッチで理由を判別できる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileSkipReason {
    /// パスに親ディレクトリ参照（..）が含まれている（パストラバーサル攻撃の防止）
    PathTraversal,
    /// ファイル名がZIP仕様の最大長を超えている
    FilenameTooLong,
    /// ファイルサイズが制限（1GB）を超えている（zip64未使用時）
    ExceedsFileSizeLimit,
    /// 圧縮比率が異常に高い（zip bomb疑い）
    SuspiciousCompressionRatio,
    /// シンボリックリンクエントリ（セキュリティリスク）
    SymlinkEntry,
    /// ZIP内に同名のエントリが重複している
    DuplicateEntry,
    /// 展開先に既にファイルが存在する（--overwrite未指定時）
    ExistingFile,
}

impl fmt::Display for FileSkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileSkipReason::PathTraversal => {
                write!(f, "path contains parent directory reference")
            }
            FileSkipReason::FilenameTooLong => write!(f, "filename too long"),
            FileSkipReason::ExceedsFileSizeLimit => write!(f, "exceeds 1GB limit"),
            FileSkipReason::SuspiciousCompressionRatio => {
                write!(f, "suspicious compression ratio (possible zip bomb)")
            }
            FileSkipReason::SymlinkEntry => write!(f, "symlink entry"),
            FileSkipReason::DuplicateEntry => write!(f, "duplicate entry name"),
            FileSkipReason::ExistingFile => write!(f, "file already exists"),
        }
    }
}

impl FileSkipReason {
    /// 常に表示すべきスキップ理由かどうか
    ///
    /// サイズ制限超過はverboseモードに関わらず表示する。
    /// ユーザーが--zip64の使用を検討できるようにするため。
    pub fn is_always_visible(&self) -> bool {
        matches!(
            self,
            FileSkipReason::ExceedsFileSizeLimit | FileSkipReason::SuspiciousCompressionRatio
        )
    }
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
    /// ファイルがスキップされた（型安全な理由付き）
    FileSkipped {
        name: String,
        reason: FileSkipReason,
    },
    /// ZIP作成が開始された
    ArchiveStarted { target: PathBuf },
    /// アーカイブの作成が完了した
    ArchiveCompleted { stats: ZipStats },
    /// ZIPエントリパスがサニタイズにより変更された
    PathSanitized { original: String, sanitized: String },
    /// ファイルが展開された
    FileExtracted { name: String, size: u64 },
    /// ZIP展開が開始された
    ExtractionStarted { source: PathBuf },
    /// ZIP展開が完了した
    ExtractionCompleted { stats: ExtractStats },
    /// パーミッションがサニタイズされた
    PermissionsSanitized {
        name: String,
        original: u32,
        sanitized: u32,
    },
    /// エントリがスキップされた（展開時用、型安全な理由付き）
    EntrySkipped {
        name: String,
        reason: FileSkipReason,
    },
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

    /// ZipEntryInfo の仕様
    mod zip_entry_info {
        use super::super::*;

        #[test]
        fn clone_preserves_all_fields() {
            let entry = ZipEntryInfo {
                name: "dir/file.txt".to_string(),
                compressed_size: 100,
                uncompressed_size: 5000,
                is_dir: false,
                is_symlink: false,
                unix_permissions: Some(0o644),
            };
            let cloned = entry.clone();
            assert_eq!(cloned, entry);
        }

        #[test]
        fn equality_compares_all_fields() {
            let a = ZipEntryInfo {
                name: "a.txt".to_string(),
                compressed_size: 10,
                uncompressed_size: 100,
                is_dir: false,
                is_symlink: false,
                unix_permissions: Some(0o644),
            };
            let b = ZipEntryInfo {
                name: "a.txt".to_string(),
                compressed_size: 10,
                uncompressed_size: 100,
                is_dir: false,
                is_symlink: false,
                unix_permissions: Some(0o644),
            };
            assert_eq!(a, b);
        }

        #[test]
        fn different_names_are_not_equal() {
            let a = ZipEntryInfo {
                name: "a.txt".to_string(),
                compressed_size: 10,
                uncompressed_size: 100,
                is_dir: false,
                is_symlink: false,
                unix_permissions: None,
            };
            let b = ZipEntryInfo {
                name: "b.txt".to_string(),
                compressed_size: 10,
                uncompressed_size: 100,
                is_dir: false,
                is_symlink: false,
                unix_permissions: None,
            };
            assert_ne!(a, b);
        }

        #[test]
        fn none_permissions_represents_missing_metadata() {
            let entry = ZipEntryInfo {
                name: "file.txt".to_string(),
                compressed_size: 0,
                uncompressed_size: 0,
                is_dir: false,
                is_symlink: false,
                unix_permissions: None,
            };
            assert!(entry.unix_permissions.is_none());
        }
    }

    /// ExtractStats の仕様
    mod extract_stats {
        use super::super::*;

        #[test]
        fn default_initializes_to_zero() {
            let stats = ExtractStats::default();
            assert_eq!(stats.file_count, 0);
            assert_eq!(stats.total_size, 0);
            assert_eq!(stats.skipped_count, 0);
        }
    }

    /// ExtractOptions の仕様
    mod extract_options {
        use super::super::*;

        #[test]
        fn default_does_not_overwrite() {
            let options = ExtractOptions::default();
            assert!(!options.overwrite);
        }
    }

    /// FileSkipReason の仕様
    mod file_skip_reason {
        use super::super::*;

        /// Display の仕様
        mod display {
            use super::*;

            #[test]
            fn path_traversal_displays_expected_message() {
                assert_eq!(
                    FileSkipReason::PathTraversal.to_string(),
                    "path contains parent directory reference"
                );
            }

            #[test]
            fn filename_too_long_displays_expected_message() {
                assert_eq!(
                    FileSkipReason::FilenameTooLong.to_string(),
                    "filename too long"
                );
            }

            #[test]
            fn exceeds_file_size_limit_displays_expected_message() {
                assert_eq!(
                    FileSkipReason::ExceedsFileSizeLimit.to_string(),
                    "exceeds 1GB limit"
                );
            }

            #[test]
            fn suspicious_compression_ratio_displays_expected_message() {
                assert_eq!(
                    FileSkipReason::SuspiciousCompressionRatio.to_string(),
                    "suspicious compression ratio (possible zip bomb)"
                );
            }

            #[test]
            fn symlink_entry_displays_expected_message() {
                assert_eq!(FileSkipReason::SymlinkEntry.to_string(), "symlink entry");
            }

            #[test]
            fn duplicate_entry_displays_expected_message() {
                assert_eq!(
                    FileSkipReason::DuplicateEntry.to_string(),
                    "duplicate entry name"
                );
            }

            #[test]
            fn existing_file_displays_expected_message() {
                assert_eq!(
                    FileSkipReason::ExistingFile.to_string(),
                    "file already exists"
                );
            }
        }

        /// 表示ポリシーの仕様
        mod visibility {
            use super::*;

            #[test]
            fn exceeds_file_size_limit_is_always_visible() {
                assert!(FileSkipReason::ExceedsFileSizeLimit.is_always_visible());
            }

            #[test]
            fn path_traversal_is_not_always_visible() {
                assert!(!FileSkipReason::PathTraversal.is_always_visible());
            }

            #[test]
            fn filename_too_long_is_not_always_visible() {
                assert!(!FileSkipReason::FilenameTooLong.is_always_visible());
            }

            #[test]
            fn suspicious_compression_ratio_is_always_visible() {
                assert!(FileSkipReason::SuspiciousCompressionRatio.is_always_visible());
            }

            #[test]
            fn symlink_entry_is_not_always_visible() {
                assert!(!FileSkipReason::SymlinkEntry.is_always_visible());
            }

            #[test]
            fn duplicate_entry_is_not_always_visible() {
                assert!(!FileSkipReason::DuplicateEntry.is_always_visible());
            }

            #[test]
            fn existing_file_is_not_always_visible() {
                assert!(!FileSkipReason::ExistingFile.is_always_visible());
            }
        }

        /// 等価比較の仕様
        mod equality {
            use super::*;

            #[test]
            fn same_variants_are_equal() {
                assert_eq!(FileSkipReason::PathTraversal, FileSkipReason::PathTraversal);
                assert_eq!(
                    FileSkipReason::FilenameTooLong,
                    FileSkipReason::FilenameTooLong
                );
                assert_eq!(
                    FileSkipReason::ExceedsFileSizeLimit,
                    FileSkipReason::ExceedsFileSizeLimit
                );
                assert_eq!(
                    FileSkipReason::SuspiciousCompressionRatio,
                    FileSkipReason::SuspiciousCompressionRatio
                );
                assert_eq!(FileSkipReason::SymlinkEntry, FileSkipReason::SymlinkEntry);
                assert_eq!(
                    FileSkipReason::DuplicateEntry,
                    FileSkipReason::DuplicateEntry
                );
                assert_eq!(FileSkipReason::ExistingFile, FileSkipReason::ExistingFile);
            }

            #[test]
            fn different_variants_are_not_equal() {
                assert_ne!(
                    FileSkipReason::PathTraversal,
                    FileSkipReason::FilenameTooLong
                );
                assert_ne!(
                    FileSkipReason::PathTraversal,
                    FileSkipReason::ExceedsFileSizeLimit
                );
                assert_ne!(
                    FileSkipReason::FilenameTooLong,
                    FileSkipReason::ExceedsFileSizeLimit
                );
                assert_ne!(
                    FileSkipReason::SuspiciousCompressionRatio,
                    FileSkipReason::SymlinkEntry
                );
                assert_ne!(FileSkipReason::DuplicateEntry, FileSkipReason::ExistingFile);
            }
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
                reason: FileSkipReason::ExceedsFileSizeLimit,
            };
            let _ = ZipEvent::ArchiveStarted {
                target: PathBuf::from("/tmp/out.zip"),
            };
            let _ = ZipEvent::ArchiveCompleted {
                stats: ZipStats::default(),
            };
            let _ = ZipEvent::PathSanitized {
                original: "dir/\u{200B}file.txt".to_string(),
                sanitized: "dir/file.txt".to_string(),
            };
            let _ = ZipEvent::FileExtracted {
                name: "file.txt".to_string(),
                size: 1024,
            };
            let _ = ZipEvent::ExtractionStarted {
                source: PathBuf::from("/tmp/test.zip"),
            };
            let _ = ZipEvent::ExtractionCompleted {
                stats: ExtractStats::default(),
            };
            let _ = ZipEvent::PermissionsSanitized {
                name: "file.txt".to_string(),
                original: 0o4755,
                sanitized: 0o755,
            };
            let _ = ZipEvent::EntrySkipped {
                name: "evil.txt".to_string(),
                reason: FileSkipReason::SuspiciousCompressionRatio,
            };
        }
    }
}
