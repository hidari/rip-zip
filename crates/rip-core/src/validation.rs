use std::path::Path;

use crate::config::{MAX_FILENAME_LENGTH, MAX_FILE_COUNT, MAX_FILE_SIZE, MAX_TOTAL_SIZE};
use crate::error::ZipError;

/// ソースディレクトリの存在とディレクトリであることを検証する
pub(crate) fn validate_source_dir(source_dir: &Path) -> Result<(), ZipError> {
    if !source_dir.exists() {
        return Err(ZipError::Validation(format!(
            "Source directory does not exist: {}",
            source_dir.display()
        )));
    }

    if !source_dir.is_dir() {
        return Err(ZipError::Validation(format!(
            "Source is not a directory: {}",
            source_dir.display()
        )));
    }

    Ok(())
}

/// パストラバーサル攻撃を検出する
///
/// 相対パスに`..`（ParentDir）コンポーネントが含まれている場合、
/// ZIPスリップ攻撃の可能性があるためtrueを返す。
pub(crate) fn has_path_traversal(relative_path: &Path) -> bool {
    relative_path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
}

/// パス区切り文字をZIP仕様に準拠させる
///
/// ZIP仕様ではパス区切り文字は`/`でなければならない。
/// Windowsの`\`を`/`に変換する。
pub(crate) fn normalize_path_separator(relative_path: &Path) -> String {
    match relative_path.to_str() {
        Some(s) => s.replace('\\', "/"),
        None => relative_path.to_string_lossy().replace('\\', "/"),
    }
}

/// ファイル名がZIP仕様の最大長を超えているかチェックする
pub(crate) fn is_filename_too_long(name: &str) -> bool {
    name.len() > MAX_FILENAME_LENGTH
}

/// ファイル数が制限を超えていないかチェックする
pub(crate) fn check_file_count(count: usize) -> Result<(), ZipError> {
    if count > MAX_FILE_COUNT {
        return Err(ZipError::Validation(format!(
            "Too many files (limit: {})",
            MAX_FILE_COUNT
        )));
    }
    Ok(())
}

/// 個別ファイルがサイズ制限を超えているかチェックする
///
/// ZIP64が有効な場合はサイズ制限を適用しない。
/// trueを返した場合、呼び出し元はこのファイルをスキップすべき。
pub(crate) fn should_skip_large_file(file_size: u64, use_zip64: bool) -> bool {
    file_size > MAX_FILE_SIZE && !use_zip64
}

/// 合計サイズが制限を超えていないかチェックする
///
/// ZIP64が有効な場合はサイズ制限を適用しない。
pub(crate) fn check_total_size(
    current_total: u64,
    addition: u64,
    use_zip64: bool,
) -> Result<(), ZipError> {
    if current_total + addition > MAX_TOTAL_SIZE && !use_zip64 {
        return Err(ZipError::Validation(
            "Total archive size would exceed 4GB limit. Use --zip64 flag for larger archives."
                .to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    mod validate_source_dir {
        use super::*;

        #[test]
        fn rejects_nonexistent_path() {
            let result = validate_source_dir(Path::new("/nonexistent/path/that/does/not/exist"));
            assert!(
                matches!(result, Err(ZipError::Validation(msg)) if msg.contains("does not exist"))
            );
        }

        #[test]
        fn rejects_file_path_as_source() {
            let dir = tempfile::TempDir::new().unwrap();
            let file_path = dir.path().join("test.txt");
            std::fs::write(&file_path, "content").unwrap();

            let result = validate_source_dir(&file_path);
            assert!(
                matches!(result, Err(ZipError::Validation(msg)) if msg.contains("not a directory"))
            );
        }

        #[test]
        fn accepts_valid_directory() {
            let dir = tempfile::TempDir::new().unwrap();
            let result = validate_source_dir(dir.path());
            assert!(result.is_ok());
        }
    }

    mod path_traversal {
        use super::*;

        #[test]
        fn detects_parent_dir_component() {
            assert!(has_path_traversal(Path::new("../etc/passwd")));
            assert!(has_path_traversal(Path::new("foo/../bar")));
        }

        #[test]
        fn allows_normal_relative_paths() {
            assert!(!has_path_traversal(Path::new("foo/bar/baz.txt")));
            assert!(!has_path_traversal(Path::new("file.txt")));
            assert!(!has_path_traversal(Path::new("deeply/nested/path/file.rs")));
        }

        #[test]
        fn detects_deeply_nested_traversal() {
            // 複数段のトラバーサルも検出できることを確認
            assert!(has_path_traversal(Path::new("foo/../../bar")));
            assert!(has_path_traversal(Path::new("a/b/c/../../../d")));
        }

        #[test]
        fn allows_path_with_dots_in_filename() {
            // ドット始まりのファイル名やドットを含むファイル名は許可する
            assert!(!has_path_traversal(Path::new("..hidden")));
            assert!(!has_path_traversal(Path::new(".gitignore")));
            assert!(!has_path_traversal(Path::new("foo/..hidden/bar")));
            assert!(!has_path_traversal(Path::new("foo/.config/settings")));
        }
    }

    mod path_separator {
        use super::*;

        #[test]
        fn converts_backslashes_to_forward_slashes() {
            let path = PathBuf::from("foo\\bar\\baz.txt");
            let result = normalize_path_separator(&path);
            assert_eq!(result, "foo/bar/baz.txt");
        }

        #[test]
        fn preserves_forward_slashes() {
            let path = PathBuf::from("foo/bar/baz.txt");
            let result = normalize_path_separator(&path);
            assert_eq!(result, "foo/bar/baz.txt");
        }

        #[test]
        fn handles_mixed_separators() {
            // フォワードスラッシュとバックスラッシュの混合を正しく処理する
            let path = PathBuf::from("foo/bar\\baz");
            let result = normalize_path_separator(&path);
            assert_eq!(result, "foo/bar/baz");
        }
    }

    mod filename_length {
        use super::*;

        #[test]
        fn returns_false_for_normal_length_name() {
            assert!(!is_filename_too_long("short_name.txt"));
        }

        #[test]
        fn returns_false_at_exact_limit_65535_bytes() {
            let name = "a".repeat(MAX_FILENAME_LENGTH);
            assert!(!is_filename_too_long(&name));
        }

        #[test]
        fn returns_true_at_65536_bytes() {
            let name = "a".repeat(MAX_FILENAME_LENGTH + 1);
            assert!(is_filename_too_long(&name));
        }

        #[test]
        fn measures_byte_length_not_char_count() {
            // 日本語文字は1文字あたり3バイト（UTF-8）なので、
            // 文字数ではなくバイト長で判定されることを確認する
            let single_char = "あ";
            assert_eq!(single_char.len(), 3); // 3バイト/文字

            // MAX_FILENAME_LENGTH / 3 文字の日本語はバイト長が MAX_FILENAME_LENGTH 以下
            let chars_within_limit = MAX_FILENAME_LENGTH / 3;
            let name_within = "あ".repeat(chars_within_limit);
            assert!(!is_filename_too_long(&name_within));

            // 1文字追加するとバイト長が超過する場合を確認
            // chars_over_limit * 3 > MAX_FILENAME_LENGTH となる
            let chars_over_limit = (MAX_FILENAME_LENGTH / 3) + 1;
            let name_over = "あ".repeat(chars_over_limit);
            assert!(is_filename_too_long(&name_over));
        }
    }

    mod file_count {
        use super::*;

        #[test]
        fn accepts_count_within_limit() {
            assert!(check_file_count(1).is_ok());
        }

        #[test]
        fn accepts_exactly_100000_files() {
            // 境界値ちょうどMAX_FILE_COUNTは許可される
            assert!(check_file_count(MAX_FILE_COUNT).is_ok());
        }

        #[test]
        fn rejects_count_exceeding_100000() {
            let result = check_file_count(MAX_FILE_COUNT + 1);
            assert!(
                matches!(result, Err(ZipError::Validation(msg)) if msg.contains("Too many files"))
            );
        }
    }

    mod large_file {
        use super::*;

        #[test]
        fn skips_when_over_1gb_without_zip64() {
            assert!(should_skip_large_file(MAX_FILE_SIZE + 1, false));
        }

        #[test]
        fn allows_file_at_exact_1gb_limit() {
            assert!(!should_skip_large_file(MAX_FILE_SIZE, false));
            assert!(!should_skip_large_file(100, false));
        }

        #[test]
        fn allows_over_1gb_with_zip64_enabled() {
            assert!(!should_skip_large_file(MAX_FILE_SIZE + 1, true));
        }

        #[test]
        fn does_not_skip_zero_byte_file() {
            // 0バイトファイルはスキップされない
            assert!(!should_skip_large_file(0, false));
        }
    }

    mod total_size {
        use super::*;

        #[test]
        fn accepts_total_within_4gb_limit() {
            assert!(check_total_size(0, MAX_TOTAL_SIZE, false).is_ok());
        }

        #[test]
        fn rejects_total_exceeding_4gb_without_zip64() {
            let result = check_total_size(MAX_TOTAL_SIZE, 1, false);
            assert!(matches!(result, Err(ZipError::Validation(msg)) if msg.contains("4GB limit")));
        }

        #[test]
        fn allows_total_exceeding_4gb_with_zip64() {
            assert!(check_total_size(MAX_TOTAL_SIZE, 1, true).is_ok());
        }

        #[test]
        fn accepts_zero_total_size() {
            // 合計サイズ0は許可される
            assert!(check_total_size(0, 0, false).is_ok());
        }
    }
}
