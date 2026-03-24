use std::fmt;
use std::io;
use std::path::StripPrefixError;

/// ZIP操作における統一エラー型
///
/// 外部ライブラリ（zip, walkdir）のエラー型には依存せず、
/// 文字列としてエラーメッセージを保持する。
/// adapters側でヘルパー関数を使って変換する。
#[derive(Debug)]
pub enum ZipError {
    /// I/Oエラー
    Io(io::Error),
    /// パスプレフィックス除去エラー
    StripPrefix(StripPrefixError),
    /// アーカイバ固有のエラー（外部ライブラリのエラーを文字列化して保持）
    Archive(String),
    /// ファイルウォーカー固有のエラー（外部ライブラリのエラーを文字列化して保持）
    Walk(String),
    /// バリデーションエラー（セキュリティチェック等）
    Validation(String),
}

impl From<io::Error> for ZipError {
    fn from(err: io::Error) -> Self {
        ZipError::Io(err)
    }
}

impl From<StripPrefixError> for ZipError {
    fn from(err: StripPrefixError) -> Self {
        ZipError::StripPrefix(err)
    }
}

impl fmt::Display for ZipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ZipError::Io(err) => write!(f, "IO error: {}", err),
            ZipError::StripPrefix(err) => write!(f, "Path error: {}", err),
            ZipError::Archive(msg) => write!(f, "Archive error: {}", msg),
            ZipError::Walk(msg) => write!(f, "Walk error: {}", msg),
            ZipError::Validation(msg) => write!(f, "Validation error: {}", msg),
        }
    }
}

impl std::error::Error for ZipError {}

#[cfg(test)]
mod tests {
    /// From トレイト変換の仕様
    mod conversion {
        use super::super::*;

        #[test]
        fn io_error_converts_from_std() {
            let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
            let zip_err: ZipError = io_err.into();

            assert!(matches!(zip_err, ZipError::Io(_)));
            assert!(zip_err.to_string().contains("file not found"));
        }

        #[test]
        fn strip_prefix_error_converts_from_std() {
            use std::path::Path;
            // strip_prefix で実際にエラーを生成
            let err = Path::new("a/b").strip_prefix("c").unwrap_err();
            let zip_err: ZipError = err.into();

            assert!(matches!(zip_err, ZipError::StripPrefix(_)));
            assert!(zip_err.to_string().contains("Path error"));
        }

        #[test]
        fn archive_variant_preserves_original_message() {
            let zip_err = ZipError::Archive("corrupted archive".to_string());
            assert!(zip_err.to_string().contains("corrupted archive"));
        }

        #[test]
        fn walk_variant_preserves_original_message() {
            let zip_err = ZipError::Walk("permission denied".to_string());
            assert!(zip_err.to_string().contains("permission denied"));
        }

        #[test]
        fn validation_variant_preserves_original_message() {
            let zip_err = ZipError::Validation("too many files".to_string());
            assert!(zip_err.to_string().contains("too many files"));
        }

        #[test]
        fn zip_error_implements_std_error_trait() {
            // ZipError が Box<dyn std::error::Error> に変換可能であることを確認
            let zip_err = ZipError::Archive("test error".to_string());
            let boxed: Box<dyn std::error::Error> = Box::new(zip_err);
            assert!(boxed.to_string().contains("test error"));
        }
    }

    /// Display フォーマットの仕様
    mod display {
        use super::super::*;

        #[test]
        fn prefixes_each_variant_correctly() {
            let cases = vec![
                (ZipError::Io(io::Error::other("test")), "IO error: test"),
                (
                    ZipError::Archive("archive msg".to_string()),
                    "Archive error: archive msg",
                ),
                (
                    ZipError::Walk("walk msg".to_string()),
                    "Walk error: walk msg",
                ),
                (
                    ZipError::Validation("validation msg".to_string()),
                    "Validation error: validation msg",
                ),
            ];

            for (err, expected) in cases {
                assert_eq!(err.to_string(), expected);
            }
        }
    }
}
