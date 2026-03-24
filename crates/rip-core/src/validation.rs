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

    // --- validate_source_dir ---

    #[test]
    fn validate_source_dir_returns_error_for_nonexistent_path() {
        let result = validate_source_dir(Path::new("/nonexistent/path/that/does/not/exist"));
        assert!(matches!(result, Err(ZipError::Validation(msg)) if msg.contains("does not exist")));
    }

    #[test]
    fn validate_source_dir_returns_error_for_file_path() {
        let dir = tempfile::TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "content").unwrap();

        let result = validate_source_dir(&file_path);
        assert!(
            matches!(result, Err(ZipError::Validation(msg)) if msg.contains("not a directory"))
        );
    }

    #[test]
    fn validate_source_dir_succeeds_for_valid_directory() {
        let dir = tempfile::TempDir::new().unwrap();
        let result = validate_source_dir(dir.path());
        assert!(result.is_ok());
    }

    // --- has_path_traversal ---

    #[test]
    fn has_path_traversal_detects_parent_dir_component() {
        assert!(has_path_traversal(Path::new("../etc/passwd")));
        assert!(has_path_traversal(Path::new("foo/../../bar")));
        assert!(has_path_traversal(Path::new("foo/../bar")));
    }

    #[test]
    fn has_path_traversal_allows_normal_paths() {
        assert!(!has_path_traversal(Path::new("foo/bar/baz.txt")));
        assert!(!has_path_traversal(Path::new("file.txt")));
        assert!(!has_path_traversal(Path::new("deeply/nested/path/file.rs")));
    }

    // --- normalize_path_separator ---

    #[test]
    fn normalize_path_separator_converts_backslashes() {
        let path = PathBuf::from("foo\\bar\\baz.txt");
        let result = normalize_path_separator(&path);
        assert_eq!(result, "foo/bar/baz.txt");
    }

    #[test]
    fn normalize_path_separator_preserves_forward_slashes() {
        let path = PathBuf::from("foo/bar/baz.txt");
        let result = normalize_path_separator(&path);
        assert_eq!(result, "foo/bar/baz.txt");
    }

    // --- is_filename_too_long ---

    #[test]
    fn is_filename_too_long_returns_false_for_normal_name() {
        assert!(!is_filename_too_long("short_name.txt"));
    }

    #[test]
    fn is_filename_too_long_returns_false_at_exact_limit() {
        let name = "a".repeat(MAX_FILENAME_LENGTH);
        assert!(!is_filename_too_long(&name));
    }

    #[test]
    fn is_filename_too_long_returns_true_over_limit() {
        let name = "a".repeat(MAX_FILENAME_LENGTH + 1);
        assert!(is_filename_too_long(&name));
    }

    // --- check_file_count ---

    #[test]
    fn check_file_count_allows_within_limit() {
        assert!(check_file_count(1).is_ok());
        assert!(check_file_count(MAX_FILE_COUNT).is_ok());
    }

    #[test]
    fn check_file_count_rejects_over_limit() {
        let result = check_file_count(MAX_FILE_COUNT + 1);
        assert!(matches!(result, Err(ZipError::Validation(msg)) if msg.contains("Too many files")));
    }

    // --- should_skip_large_file ---

    #[test]
    fn should_skip_large_file_skips_when_over_limit_without_zip64() {
        assert!(should_skip_large_file(MAX_FILE_SIZE + 1, false));
    }

    #[test]
    fn should_skip_large_file_allows_when_within_limit() {
        assert!(!should_skip_large_file(MAX_FILE_SIZE, false));
        assert!(!should_skip_large_file(100, false));
    }

    #[test]
    fn should_skip_large_file_allows_large_with_zip64() {
        assert!(!should_skip_large_file(MAX_FILE_SIZE + 1, true));
    }

    // --- check_total_size ---

    #[test]
    fn check_total_size_allows_within_limit() {
        assert!(check_total_size(0, MAX_TOTAL_SIZE, false).is_ok());
    }

    #[test]
    fn check_total_size_rejects_over_limit() {
        let result = check_total_size(MAX_TOTAL_SIZE, 1, false);
        assert!(matches!(result, Err(ZipError::Validation(msg)) if msg.contains("4GB limit")));
    }

    #[test]
    fn check_total_size_allows_over_limit_with_zip64() {
        assert!(check_total_size(MAX_TOTAL_SIZE, 1, true).is_ok());
    }
}
