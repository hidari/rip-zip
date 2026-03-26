use std::path::Path;

use crate::config::{
    MAX_COMPRESSION_RATIO, MAX_DIR_PERMISSIONS, MAX_FILENAME_LENGTH, MAX_FILE_COUNT,
    MAX_FILE_PERMISSIONS, MAX_FILE_SIZE, MAX_TOTAL_SIZE, SETGID_BIT, SETUID_BIT, STICKY_BIT,
};
use crate::error::ZipError;
use crate::types::ZipEntryInfo;

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

/// 圧縮比率がzip bomb疑いの閾値を超えているかチェックする
///
/// compressed_sizeが0の場合、uncompressed_sizeも0なら正常（空エントリ）、
/// uncompressed_sizeが0より大きければ不審とみなす。
// TODO(#31): Phase 2のzip_extractorで使用予定。消費者実装後にallow(dead_code)を除去する。
#[allow(dead_code)]
pub(crate) fn is_suspicious_compression_ratio(compressed: u64, uncompressed: u64) -> bool {
    if compressed == 0 {
        return uncompressed > 0;
    }
    // 整数除算の切り捨てによる閾値付近のzip bomb見逃しを防ぐため、
    // 乗算で比較する。オーバーフロー時はfalse（閾値が天文学的に大きい）
    compressed
        .checked_mul(MAX_COMPRESSION_RATIO)
        .is_some_and(|threshold| uncompressed > threshold)
}

/// パーミッションをサニタイズする
///
/// setuid/setgid/stickyビットを除去し、上限マスクを適用する。
// TODO(#31): Phase 2のzip_extractorで使用予定。消費者実装後にallow(dead_code)を除去する。
#[allow(dead_code)]
pub(crate) fn sanitize_permissions(permissions: u32, is_dir: bool) -> u32 {
    // 特殊ビット（setuid, setgid, sticky）を除去
    let without_special = permissions & !(SETUID_BIT | SETGID_BIT | STICKY_BIT);
    // 上限マスクを適用
    let max_perms = if is_dir {
        MAX_DIR_PERMISSIONS
    } else {
        MAX_FILE_PERMISSIONS
    };
    without_special & max_perms
}

/// 展開対象ZIPファイルの存在と読み取り可能性を検証する
///
/// 存在確認・ファイル種別確認の後、File::openで読み取り可能性を検証する。
/// Windowsではディレクトリに対するFile::openが失敗するため、
/// is_file()チェックをopen前に行いクロスプラットフォームで一貫したエラーを返す。
// TODO(#31): Phase 2のzip_extractorで使用予定。消費者実装後にallow(dead_code)を除去する。
#[allow(dead_code)]
pub(crate) fn validate_source_zip(zip_path: &Path) -> Result<(), ZipError> {
    if !zip_path.exists() {
        return Err(ZipError::Validation(format!(
            "ZIP file does not exist: {}",
            zip_path.display()
        )));
    }

    if !zip_path.is_file() {
        return Err(ZipError::Validation(format!(
            "Not a file: {}",
            zip_path.display()
        )));
    }

    // 読み取り可能性をFile::openで確認
    std::fs::File::open(zip_path).map_err(|e| {
        ZipError::Validation(format!(
            "Cannot read ZIP file: {}: {}",
            zip_path.display(),
            e
        ))
    })?;

    Ok(())
}

/// ZIPエントリ一覧から重複エントリ名を検出する
///
/// 返り値は重複しているエントリ名のリスト（ソート済み）。
// TODO(#31): Phase 2のzip_extractorで使用予定。消費者実装後にallow(dead_code)を除去する。
#[allow(dead_code)]
pub(crate) fn find_duplicate_entries(entries: &[ZipEntryInfo]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut duplicates = std::collections::HashSet::new();
    for entry in entries {
        if !seen.insert(&entry.name) {
            duplicates.insert(entry.name.clone());
        }
    }
    let mut result: Vec<String> = duplicates.into_iter().collect();
    result.sort();
    result
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

        /// 設計判断の文書化: URLエンコードされたパスを復号しない
        ///
        /// 入力はファイルシステムからのパスであり、URLではない。
        /// "%2e%2e" はリテラルなファイル名として扱い、トラバーサルとして検出しない。
        #[test]
        fn treats_percent_encoded_dots_as_literal_filename() {
            // URLエンコードされた ".." (%2e%2e) はリテラルなファイル名
            assert!(!has_path_traversal(Path::new("%2e%2e/etc/passwd")));
            assert!(!has_path_traversal(Path::new("foo/%2e%2e/bar")));
        }

        #[test]
        fn treats_percent_encoded_slash_as_literal_filename() {
            // URLエンコードされた "/" (%2f) はリテラルなファイル名
            assert!(!has_path_traversal(Path::new("foo%2fbar")));
        }

        #[test]
        fn invisible_chars_in_dot_dot_do_not_create_traversal() {
            // 不可視文字を含む ".." 風のセグメントはPathコンポーネントとして
            // ParentDirにならないためトラバーサルとして検出されない
            // （サニタイズは別レイヤーで処理される）
            assert!(!has_path_traversal(Path::new(".\u{200B}./etc/passwd")));
            assert!(!has_path_traversal(Path::new(".\u{FEFF}./etc/passwd")));
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
        fn returns_false_at_exact_limit_with_multibyte_chars() {
            // マルチバイト文字でちょうど65535バイトになるケース
            // "あ" は3バイト、65535 / 3 = 21845文字
            let name = "あ".repeat(21845); // 21845 * 3 = 65535 bytes
            assert_eq!(name.len(), MAX_FILENAME_LENGTH);
            assert!(!is_filename_too_long(&name));
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

    mod compression_ratio {
        use super::*;

        #[test]
        fn returns_false_for_normal_ratio() {
            // 10:1の比率は正常
            assert!(!is_suspicious_compression_ratio(100, 1000));
        }

        #[test]
        fn returns_false_at_exact_limit_1000x() {
            // ちょうど1000倍は不審ではない（境界値）
            assert!(!is_suspicious_compression_ratio(1, 1000));
        }

        #[test]
        fn returns_true_above_1000x() {
            assert!(is_suspicious_compression_ratio(1, 1001));
        }

        #[test]
        fn returns_true_for_extreme_ratio() {
            // 典型的なzip bomb: 小さな圧縮サイズ、巨大な展開サイズ
            assert!(is_suspicious_compression_ratio(1, 1_000_000));
        }

        #[test]
        fn returns_false_for_zero_compressed_zero_uncompressed() {
            // 空エントリ: 両方ゼロは正常
            assert!(!is_suspicious_compression_ratio(0, 0));
        }

        #[test]
        fn returns_true_for_zero_compressed_nonzero_uncompressed() {
            // 圧縮サイズ0なのに展開後にコンテンツがある: 不審
            assert!(is_suspicious_compression_ratio(0, 100));
        }

        #[test]
        fn returns_false_for_equal_sizes() {
            // 1:1の比率（無圧縮格納）
            assert!(!is_suspicious_compression_ratio(1000, 1000));
        }

        #[test]
        fn returns_false_when_compressed_larger_than_uncompressed() {
            // 圧縮でサイズが増加するケース（小さいファイルで起こりうる）
            assert!(!is_suspicious_compression_ratio(200, 100));
        }

        #[test]
        fn detects_fractional_ratio_above_threshold() {
            // 2001/2 = 1000.5倍。整数除算では1000に切り捨てられ見逃すケース
            // 乗算比較により正しく検出される
            assert!(is_suspicious_compression_ratio(2, 2001));
        }

        #[test]
        fn returns_false_when_checked_mul_overflows() {
            // compressed * 1000がu64をオーバーフローする場合はfalse
            assert!(!is_suspicious_compression_ratio(u64::MAX, u64::MAX));
        }
    }

    mod permissions_sanitization {
        use super::*;

        #[test]
        fn preserves_normal_file_permissions() {
            assert_eq!(sanitize_permissions(0o644, false), 0o644);
        }

        #[test]
        fn preserves_normal_dir_permissions() {
            assert_eq!(sanitize_permissions(0o755, true), 0o755);
        }

        #[test]
        fn removes_setuid_bit() {
            assert_eq!(sanitize_permissions(0o4755, false), 0o755);
        }

        #[test]
        fn removes_setgid_bit() {
            assert_eq!(sanitize_permissions(0o2755, false), 0o755);
        }

        #[test]
        fn removes_sticky_bit() {
            assert_eq!(sanitize_permissions(0o1755, false), 0o755);
        }

        #[test]
        fn removes_all_special_bits_combined() {
            assert_eq!(sanitize_permissions(0o7755, false), 0o755);
        }

        #[test]
        fn caps_file_permissions_at_755() {
            // 0o777はファイルの上限0o755にキャップされる
            assert_eq!(sanitize_permissions(0o777, false), 0o755);
        }

        #[test]
        fn caps_dir_permissions_at_755() {
            assert_eq!(sanitize_permissions(0o777, true), 0o755);
        }

        #[test]
        fn handles_zero_permissions() {
            assert_eq!(sanitize_permissions(0o000, false), 0o000);
        }

        #[test]
        fn removes_special_bits_and_caps_simultaneously() {
            // 0o4777 -> setuid除去 -> 0o777 -> 上限0o755にキャップ
            assert_eq!(sanitize_permissions(0o4777, false), 0o755);
        }

        #[test]
        fn preserves_read_only_permissions() {
            assert_eq!(sanitize_permissions(0o444, false), 0o444);
        }

        #[test]
        fn preserves_execute_bit_within_limit() {
            assert_eq!(sanitize_permissions(0o755, false), 0o755);
        }
    }

    mod validate_source_zip {
        use super::*;

        #[test]
        fn rejects_nonexistent_path() {
            let result = validate_source_zip(Path::new("/nonexistent/path/test.zip"));
            assert!(
                matches!(result, Err(ZipError::Validation(msg)) if msg.contains("does not exist"))
            );
        }

        #[test]
        fn rejects_directory_path() {
            let dir = tempfile::TempDir::new().unwrap();
            let result = validate_source_zip(dir.path());
            assert!(matches!(result, Err(ZipError::Validation(msg)) if msg.contains("Not a file")));
        }

        #[test]
        fn accepts_valid_file() {
            let dir = tempfile::TempDir::new().unwrap();
            let file_path = dir.path().join("test.zip");
            std::fs::write(&file_path, "dummy content").unwrap();
            let result = validate_source_zip(&file_path);
            assert!(result.is_ok());
        }
    }

    mod duplicate_entries {
        use super::*;

        fn make_entry(name: &str) -> ZipEntryInfo {
            ZipEntryInfo {
                name: name.to_string(),
                compressed_size: 100,
                uncompressed_size: 200,
                is_dir: false,
                is_symlink: false,
                unix_permissions: Some(0o644),
            }
        }

        #[test]
        fn returns_empty_for_no_duplicates() {
            let entries = vec![
                make_entry("a.txt"),
                make_entry("b.txt"),
                make_entry("c.txt"),
            ];
            assert!(find_duplicate_entries(&entries).is_empty());
        }

        #[test]
        fn detects_single_duplicate() {
            let entries = vec![
                make_entry("a.txt"),
                make_entry("b.txt"),
                make_entry("a.txt"),
            ];
            assert_eq!(find_duplicate_entries(&entries), vec!["a.txt"]);
        }

        #[test]
        fn detects_multiple_duplicates() {
            let entries = vec![
                make_entry("a.txt"),
                make_entry("b.txt"),
                make_entry("a.txt"),
                make_entry("b.txt"),
            ];
            let result = find_duplicate_entries(&entries);
            // ソート済みなのでa.txt, b.txtの順
            assert_eq!(result, vec!["a.txt", "b.txt"]);
        }

        #[test]
        fn returns_empty_for_empty_input() {
            let entries: Vec<ZipEntryInfo> = vec![];
            assert!(find_duplicate_entries(&entries).is_empty());
        }

        #[test]
        fn returns_single_entry_per_duplicate_even_with_triples() {
            let entries = vec![
                make_entry("a.txt"),
                make_entry("a.txt"),
                make_entry("a.txt"),
            ];
            assert_eq!(find_duplicate_entries(&entries), vec!["a.txt"]);
        }

        #[test]
        fn treats_different_paths_as_distinct() {
            let entries = vec![make_entry("dir1/a.txt"), make_entry("dir2/a.txt")];
            assert!(find_duplicate_entries(&entries).is_empty());
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
