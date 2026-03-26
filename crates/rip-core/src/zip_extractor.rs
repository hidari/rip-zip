use std::collections::HashSet;
use std::io::Cursor;
use std::path::Path;

use crate::error::ZipError;
use crate::path_utils;
use crate::traits::{FileWriter, ZipReader};
use crate::types::{ExtractOptions, ExtractStats, FileSkipReason, ZipEvent};
use crate::validation;

/// ZIPアーカイブを展開する
///
/// トレイト経由で外部ライブラリに依存せず、バリデーションとフロー制御を行う。
/// 副作用（ログ出力等）はon_eventコールバックで呼び出し元に委譲する。
/// 脅威モデルで定義された7脅威すべてに対策した多層防御パイプライン。
pub fn extract_zip(
    reader: &dyn ZipReader,
    writer: &dyn FileWriter,
    source_zip: &Path,
    target_dir: &Path,
    options: &ExtractOptions,
    on_event: &dyn Fn(ZipEvent),
) -> Result<ExtractStats, ZipError> {
    // Phase 1: バリデーション
    validation::validate_source_zip(source_zip)?;

    on_event(ZipEvent::ExtractionStarted {
        source: source_zip.to_path_buf(),
    });

    // Phase 2: 事前スキャン
    let entries = reader.scan(source_zip)?;

    // ファイル数チェック
    validation::check_file_count(entries.len())?;

    // 合計展開サイズの概算チェック（zip bomb早期検出）
    let total_uncompressed: u64 = entries.iter().map(|e| e.uncompressed_size).sum();
    validation::check_total_size(0, total_uncompressed, false)?;

    // 重複エントリ検出
    let duplicates = validation::find_duplicate_entries(&entries);
    let duplicate_set: HashSet<&str> = duplicates.iter().map(|s| s.as_str()).collect();

    // 展開先ディレクトリの作成とcanonicalizeによるbase_dir確定
    writer.create_dir_all(target_dir)?;
    let base_dir = target_dir.canonicalize().map_err(ZipError::Io)?;

    let mut stats = ExtractStats::default();
    let mut seen_names: HashSet<String> = HashSet::new();

    // Phase 3: 各エントリの展開ループ（9段階セキュリティチェック）
    for entry in &entries {
        // a. パストラバーサル検出
        if validation::has_path_traversal(Path::new(&entry.name)) {
            on_event(ZipEvent::EntrySkipped {
                name: entry.name.clone(),
                reason: FileSkipReason::PathTraversal,
            });
            stats.skipped_count += 1;
            continue;
        }

        // b-c. パス正規化 + サニタイズ
        let sanitized_name = path_utils::sanitize_zip_entry_path(&entry.name);
        if sanitized_name != entry.name {
            on_event(ZipEvent::PathSanitized {
                original: entry.name.clone(),
                sanitized: sanitized_name.clone(),
            });
        }

        // d. ファイル名長チェック
        if validation::is_filename_too_long(&sanitized_name) {
            on_event(ZipEvent::EntrySkipped {
                name: sanitized_name,
                reason: FileSkipReason::FilenameTooLong,
            });
            stats.skipped_count += 1;
            continue;
        }

        // e. 個別ファイルサイズチェック（展開時は常に1GB制限を適用）
        if validation::should_skip_large_file(entry.uncompressed_size, false) {
            on_event(ZipEvent::EntrySkipped {
                name: sanitized_name,
                reason: FileSkipReason::ExceedsFileSizeLimit,
            });
            stats.skipped_count += 1;
            continue;
        }

        // f. 圧縮比率チェック（zip bomb検出）
        if validation::is_suspicious_compression_ratio(
            entry.compressed_size,
            entry.uncompressed_size,
        ) {
            on_event(ZipEvent::EntrySkipped {
                name: sanitized_name,
                reason: FileSkipReason::SuspiciousCompressionRatio,
            });
            stats.skipped_count += 1;
            continue;
        }

        // g. symlinkエントリ検出
        if entry.is_symlink {
            on_event(ZipEvent::EntrySkipped {
                name: sanitized_name,
                reason: FileSkipReason::SymlinkEntry,
            });
            stats.skipped_count += 1;
            continue;
        }

        // 重複エントリチェック（事前スキャンで検出された重複の2回目以降をスキップ）
        if duplicate_set.contains(entry.name.as_str()) && !seen_names.insert(sanitized_name.clone())
        {
            on_event(ZipEvent::EntrySkipped {
                name: sanitized_name,
                reason: FileSkipReason::DuplicateEntry,
            });
            stats.skipped_count += 1;
            continue;
        }
        // 重複でない場合もseen_namesに追加
        if !duplicate_set.contains(entry.name.as_str()) {
            seen_names.insert(sanitized_name.clone());
        }

        // h. 展開先パスのセキュリティチェック
        let dest_path = base_dir.join(&sanitized_name);

        // prefix チェック（主防御）
        if !dest_path.starts_with(&base_dir) {
            stats.skipped_count += 1;
            continue;
        }

        // ディレクトリエントリの処理
        if entry.is_dir {
            writer.create_dir_all(&dest_path)?;

            // canonicalizeによる最終確認（symlink防御）
            if let Ok(canonical) = dest_path.canonicalize() {
                if !canonical.starts_with(&base_dir) {
                    stats.skipped_count += 1;
                    continue;
                }
            }
            continue;
        }

        // ファイルエントリの処理: 親ディレクトリの作成
        if let Some(parent) = dest_path.parent() {
            writer.create_dir_all(parent)?;

            // canonicalizeによる最終確認（symlink防御）
            if let Ok(canonical_parent) = parent.canonicalize() {
                if !canonical_parent.starts_with(&base_dir) {
                    stats.skipped_count += 1;
                    continue;
                }
            }
        }

        // i. 既存ファイル上書きチェック
        if !options.overwrite && writer.exists(&dest_path) {
            on_event(ZipEvent::EntrySkipped {
                name: sanitized_name,
                reason: FileSkipReason::ExistingFile,
            });
            stats.skipped_count += 1;
            continue;
        }

        // j. パーミッション検証・マスク適用
        let default_perms = if entry.is_dir { 0o755 } else { 0o644 };
        let raw_perms = entry.unix_permissions.unwrap_or(default_perms);
        let sanitized_perms = validation::sanitize_permissions(raw_perms, entry.is_dir);

        if sanitized_perms != raw_perms {
            on_event(ZipEvent::PermissionsSanitized {
                name: sanitized_name.clone(),
                original: raw_perms,
                sanitized: sanitized_perms,
            });
        }

        // k. ファイル展開（Vec<u8> + Cursorでブリッジ）
        let mut buffer = Vec::new();
        reader.extract_entry(source_zip, &entry.name, &mut buffer)?;
        let mut cursor = Cursor::new(buffer);
        let bytes_written = writer.write_file(&dest_path, &mut cursor, sanitized_perms)?;

        // l. 累計サイズ更新
        stats.total_size += bytes_written;
        stats.file_count += 1;

        // m. イベント通知
        on_event(ZipEvent::FileExtracted {
            name: sanitized_name,
            size: bytes_written,
        });
    }

    // Phase 4: 完了処理
    on_event(ZipEvent::ExtractionCompleted {
        stats: stats.clone(),
    });

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{MAX_FILE_COUNT, MAX_FILE_SIZE};
    use crate::types::ZipEntryInfo;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::path::PathBuf;

    // --- Fake実装 ---

    struct FakeZipReader {
        entries: Vec<ZipEntryInfo>,
        entry_data: HashMap<String, Vec<u8>>,
    }

    impl FakeZipReader {
        fn new(entries: Vec<ZipEntryInfo>, entry_data: HashMap<String, Vec<u8>>) -> Self {
            Self {
                entries,
                entry_data,
            }
        }
    }

    impl ZipReader for FakeZipReader {
        fn scan(&self, _zip_path: &Path) -> Result<Vec<ZipEntryInfo>, ZipError> {
            Ok(self.entries.clone())
        }

        fn extract_entry(
            &self,
            _zip_path: &Path,
            entry_name: &str,
            writer: &mut dyn Write,
        ) -> Result<u64, ZipError> {
            let data = self
                .entry_data
                .get(entry_name)
                .ok_or_else(|| ZipError::Archive(format!("Entry not found: {}", entry_name)))?;
            writer.write_all(data).map_err(ZipError::Io)?;
            Ok(data.len() as u64)
        }
    }

    struct FakeFileWriter {
        dirs_created: RefCell<Vec<PathBuf>>,
        files_written: RefCell<Vec<(PathBuf, Vec<u8>, u32)>>,
        existing_paths: HashSet<PathBuf>,
    }

    impl FakeFileWriter {
        fn new() -> Self {
            Self {
                dirs_created: RefCell::new(Vec::new()),
                files_written: RefCell::new(Vec::new()),
                existing_paths: HashSet::new(),
            }
        }

        fn with_existing_paths(existing_paths: HashSet<PathBuf>) -> Self {
            Self {
                dirs_created: RefCell::new(Vec::new()),
                files_written: RefCell::new(Vec::new()),
                existing_paths,
            }
        }
    }

    impl FileWriter for FakeFileWriter {
        fn create_dir_all(&self, path: &Path) -> Result<(), ZipError> {
            self.dirs_created.borrow_mut().push(path.to_path_buf());
            Ok(())
        }

        fn write_file(
            &self,
            path: &Path,
            reader: &mut dyn Read,
            permissions: u32,
        ) -> Result<u64, ZipError> {
            let mut data = Vec::new();
            reader.read_to_end(&mut data).map_err(ZipError::Io)?;
            let size = data.len() as u64;
            self.files_written
                .borrow_mut()
                .push((path.to_path_buf(), data, permissions));
            Ok(size)
        }

        fn exists(&self, path: &Path) -> bool {
            self.existing_paths.contains(path)
        }

        fn is_symlink(&self, _path: &Path) -> bool {
            false
        }
    }

    // --- ヘルパー関数 ---

    fn make_file_entry(name: &str, data: &[u8]) -> (ZipEntryInfo, String, Vec<u8>) {
        let entry = ZipEntryInfo {
            name: name.to_string(),
            compressed_size: data.len() as u64,
            uncompressed_size: data.len() as u64,
            is_dir: false,
            is_symlink: false,
            unix_permissions: Some(0o644),
        };
        (entry, name.to_string(), data.to_vec())
    }

    fn make_dir_entry(name: &str) -> ZipEntryInfo {
        ZipEntryInfo {
            name: name.to_string(),
            compressed_size: 0,
            uncompressed_size: 0,
            is_dir: true,
            is_symlink: false,
            unix_permissions: Some(0o755),
        }
    }

    fn make_symlink_entry(name: &str) -> ZipEntryInfo {
        ZipEntryInfo {
            name: name.to_string(),
            compressed_size: 0,
            uncompressed_size: 0,
            is_dir: false,
            is_symlink: true,
            unix_permissions: Some(0o777),
        }
    }

    /// テスト用のセットアップヘルパー
    /// ZIPファイル（ダミー）とtargetディレクトリを実FS上に作成し、パスを返す
    fn setup_test_dirs() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        let zip_path = dir.path().join("test.zip");
        std::fs::write(&zip_path, "dummy").unwrap();
        let target = dir.path().join("output");
        std::fs::create_dir_all(&target).unwrap();
        (dir, zip_path, target)
    }

    // --- テスト ---

    mod source_validation {
        use super::*;

        #[test]
        fn rejects_nonexistent_zip() {
            let reader = FakeZipReader::new(vec![], HashMap::new());
            let writer = FakeFileWriter::new();

            let result = extract_zip(
                &reader,
                &writer,
                Path::new("/nonexistent/test.zip"),
                Path::new("/tmp/output"),
                &ExtractOptions::default(),
                &|_| {},
            );

            assert!(
                matches!(result, Err(ZipError::Validation(msg)) if msg.contains("does not exist"))
            );
        }
    }

    mod normal_extraction {
        use super::*;

        #[test]
        fn extracts_single_file_with_correct_stats() {
            let (_dir, zip_path, target) = setup_test_dirs();

            let (entry, name, data) = make_file_entry("hello.txt", b"Hello, World!");
            let reader = FakeZipReader::new(vec![entry], HashMap::from([(name, data)]));
            let writer = FakeFileWriter::new();

            let stats = extract_zip(
                &reader,
                &writer,
                &zip_path,
                &target,
                &ExtractOptions::default(),
                &|_| {},
            )
            .unwrap();

            assert_eq!(stats.file_count, 1);
            assert_eq!(stats.total_size, 13); // "Hello, World!" = 13 bytes
            assert_eq!(stats.skipped_count, 0);

            let files = writer.files_written.borrow();
            assert_eq!(files.len(), 1);
            assert!(files[0].0.ends_with("hello.txt"));
            assert_eq!(files[0].1, b"Hello, World!");
            assert_eq!(files[0].2, 0o644);
        }

        #[test]
        fn extracts_multiple_files() {
            let (_dir, zip_path, target) = setup_test_dirs();

            let (e1, n1, d1) = make_file_entry("file1.txt", b"aaa");
            let (e2, n2, d2) = make_file_entry("sub/file2.rs", b"bbbbb");
            let reader = FakeZipReader::new(vec![e1, e2], HashMap::from([(n1, d1), (n2, d2)]));
            let writer = FakeFileWriter::new();

            let stats = extract_zip(
                &reader,
                &writer,
                &zip_path,
                &target,
                &ExtractOptions::default(),
                &|_| {},
            )
            .unwrap();

            assert_eq!(stats.file_count, 2);
            assert_eq!(stats.total_size, 8);
        }

        #[test]
        fn creates_directory_entries() {
            let (_dir, zip_path, target) = setup_test_dirs();

            let dir_entry = make_dir_entry("subdir/");
            let (file_entry, name, data) = make_file_entry("subdir/file.txt", b"content");
            let reader =
                FakeZipReader::new(vec![dir_entry, file_entry], HashMap::from([(name, data)]));
            let writer = FakeFileWriter::new();

            let stats = extract_zip(
                &reader,
                &writer,
                &zip_path,
                &target,
                &ExtractOptions::default(),
                &|_| {},
            )
            .unwrap();

            // ディレクトリエントリはfile_countに含まれない
            assert_eq!(stats.file_count, 1);

            // ディレクトリが作成されていることを確認
            let dirs = writer.dirs_created.borrow();
            assert!(dirs.iter().any(|d| d.ends_with("subdir")));
        }

        #[test]
        fn returns_zero_stats_for_empty_zip() {
            let (_dir, zip_path, target) = setup_test_dirs();

            let reader = FakeZipReader::new(vec![], HashMap::new());
            let writer = FakeFileWriter::new();

            let stats = extract_zip(
                &reader,
                &writer,
                &zip_path,
                &target,
                &ExtractOptions::default(),
                &|_| {},
            )
            .unwrap();

            assert_eq!(stats.file_count, 0);
            assert_eq!(stats.total_size, 0);
            assert_eq!(stats.skipped_count, 0);
        }

        #[test]
        fn creates_parent_directories_for_nested_files() {
            let (_dir, zip_path, target) = setup_test_dirs();

            let (entry, name, data) = make_file_entry("a/b/c/deep.txt", b"deep");
            let reader = FakeZipReader::new(vec![entry], HashMap::from([(name, data)]));
            let writer = FakeFileWriter::new();

            extract_zip(
                &reader,
                &writer,
                &zip_path,
                &target,
                &ExtractOptions::default(),
                &|_| {},
            )
            .unwrap();

            // 親ディレクトリ "a/b/c" が作成されている
            let dirs = writer.dirs_created.borrow();
            assert!(dirs.iter().any(|d| d.to_string_lossy().contains("a/b/c")));
        }
    }

    mod security {
        use super::*;

        #[test]
        fn skips_path_traversal_entries() {
            let (_dir, zip_path, target) = setup_test_dirs();

            let traversal_entry = ZipEntryInfo {
                name: "../etc/passwd".to_string(),
                compressed_size: 100,
                uncompressed_size: 100,
                is_dir: false,
                is_symlink: false,
                unix_permissions: Some(0o644),
            };
            let (safe_entry, name, data) = make_file_entry("safe.txt", b"safe");
            let reader = FakeZipReader::new(
                vec![traversal_entry, safe_entry],
                HashMap::from([(name, data)]),
            );
            let writer = FakeFileWriter::new();
            let skipped: RefCell<Vec<(String, FileSkipReason)>> = RefCell::new(Vec::new());

            let stats = extract_zip(
                &reader,
                &writer,
                &zip_path,
                &target,
                &ExtractOptions::default(),
                &|event| {
                    if let ZipEvent::EntrySkipped { name, reason } = event {
                        skipped.borrow_mut().push((name, reason));
                    }
                },
            )
            .unwrap();

            assert_eq!(stats.file_count, 1);
            assert_eq!(stats.skipped_count, 1);
            let skipped = skipped.borrow();
            assert_eq!(skipped[0].1, FileSkipReason::PathTraversal);
        }

        #[test]
        fn skips_entries_with_suspicious_compression_ratio() {
            let (_dir, zip_path, target) = setup_test_dirs();

            let bomb_entry = ZipEntryInfo {
                name: "bomb.txt".to_string(),
                compressed_size: 1,
                uncompressed_size: 1001, // 1001倍 > 1000倍閾値
                is_dir: false,
                is_symlink: false,
                unix_permissions: Some(0o644),
            };
            let reader = FakeZipReader::new(vec![bomb_entry], HashMap::new());
            let writer = FakeFileWriter::new();
            let skipped: RefCell<Vec<FileSkipReason>> = RefCell::new(Vec::new());

            let stats = extract_zip(
                &reader,
                &writer,
                &zip_path,
                &target,
                &ExtractOptions::default(),
                &|event| {
                    if let ZipEvent::EntrySkipped { reason, .. } = event {
                        skipped.borrow_mut().push(reason);
                    }
                },
            )
            .unwrap();

            assert_eq!(stats.skipped_count, 1);
            assert_eq!(
                skipped.borrow()[0],
                FileSkipReason::SuspiciousCompressionRatio
            );
        }

        #[test]
        fn skips_symlink_entries() {
            let (_dir, zip_path, target) = setup_test_dirs();

            let symlink = make_symlink_entry("evil_link");
            let (safe, name, data) = make_file_entry("safe.txt", b"ok");
            let reader = FakeZipReader::new(vec![symlink, safe], HashMap::from([(name, data)]));
            let writer = FakeFileWriter::new();

            let stats = extract_zip(
                &reader,
                &writer,
                &zip_path,
                &target,
                &ExtractOptions::default(),
                &|_| {},
            )
            .unwrap();

            assert_eq!(stats.file_count, 1);
            assert_eq!(stats.skipped_count, 1);
        }

        #[test]
        fn skips_filenames_exceeding_max_length() {
            let (_dir, zip_path, target) = setup_test_dirs();

            let long_name = "a".repeat(65536);
            let long_entry = ZipEntryInfo {
                name: long_name,
                compressed_size: 10,
                uncompressed_size: 10,
                is_dir: false,
                is_symlink: false,
                unix_permissions: Some(0o644),
            };
            let reader = FakeZipReader::new(vec![long_entry], HashMap::new());
            let writer = FakeFileWriter::new();

            let stats = extract_zip(
                &reader,
                &writer,
                &zip_path,
                &target,
                &ExtractOptions::default(),
                &|_| {},
            )
            .unwrap();

            assert_eq!(stats.skipped_count, 1);
            assert_eq!(stats.file_count, 0);
        }

        #[test]
        fn skips_files_exceeding_size_limit() {
            let (_dir, zip_path, target) = setup_test_dirs();

            let large_entry = ZipEntryInfo {
                name: "huge.bin".to_string(),
                compressed_size: MAX_FILE_SIZE + 1,
                uncompressed_size: MAX_FILE_SIZE + 1,
                is_dir: false,
                is_symlink: false,
                unix_permissions: Some(0o644),
            };
            let reader = FakeZipReader::new(vec![large_entry], HashMap::new());
            let writer = FakeFileWriter::new();

            let stats = extract_zip(
                &reader,
                &writer,
                &zip_path,
                &target,
                &ExtractOptions::default(),
                &|_| {},
            )
            .unwrap();

            assert_eq!(stats.skipped_count, 1);
            assert_eq!(stats.file_count, 0);
        }

        #[test]
        fn skips_duplicate_entries_second_occurrence() {
            let (_dir, zip_path, target) = setup_test_dirs();

            let (e1, n1, d1) = make_file_entry("dup.txt", b"first");
            let (e2, _n2, _d2) = make_file_entry("dup.txt", b"second");
            let reader = FakeZipReader::new(vec![e1, e2], HashMap::from([(n1, d1)]));
            let writer = FakeFileWriter::new();
            let skipped: RefCell<Vec<FileSkipReason>> = RefCell::new(Vec::new());

            let stats = extract_zip(
                &reader,
                &writer,
                &zip_path,
                &target,
                &ExtractOptions::default(),
                &|event| {
                    if let ZipEvent::EntrySkipped { reason, .. } = event {
                        skipped.borrow_mut().push(reason);
                    }
                },
            )
            .unwrap();

            // 1回目は展開、2回目はスキップ
            assert_eq!(stats.file_count, 1);
            assert_eq!(stats.skipped_count, 1);
            assert_eq!(skipped.borrow()[0], FileSkipReason::DuplicateEntry);
        }

        #[test]
        fn sanitizes_entry_path_and_emits_event() {
            let (_dir, zip_path, target) = setup_test_dirs();

            let entry_name = "dir/file:name.txt";
            let entry = ZipEntryInfo {
                name: entry_name.to_string(),
                compressed_size: 4,
                uncompressed_size: 4,
                is_dir: false,
                is_symlink: false,
                unix_permissions: Some(0o644),
            };
            let reader = FakeZipReader::new(
                vec![entry],
                HashMap::from([(entry_name.to_string(), b"data".to_vec())]),
            );
            let writer = FakeFileWriter::new();
            let sanitized_events: RefCell<Vec<(String, String)>> = RefCell::new(Vec::new());

            extract_zip(
                &reader,
                &writer,
                &zip_path,
                &target,
                &ExtractOptions::default(),
                &|event| {
                    if let ZipEvent::PathSanitized {
                        original,
                        sanitized,
                    } = event
                    {
                        sanitized_events.borrow_mut().push((original, sanitized));
                    }
                },
            )
            .unwrap();

            let events = sanitized_events.borrow();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].0, "dir/file:name.txt");
            assert_eq!(events[0].1, "dir/file_name.txt");

            // サニタイズ後のパスでファイルが書き込まれている
            let files = writer.files_written.borrow();
            assert!(files[0].0.to_string_lossy().contains("file_name.txt"));
        }

        #[test]
        fn rejects_archive_exceeding_file_count_limit() {
            let (_dir, zip_path, target) = setup_test_dirs();

            let entries: Vec<ZipEntryInfo> = (0..=MAX_FILE_COUNT)
                .map(|i| ZipEntryInfo {
                    name: format!("file_{}.txt", i),
                    compressed_size: 1,
                    uncompressed_size: 1,
                    is_dir: false,
                    is_symlink: false,
                    unix_permissions: Some(0o644),
                })
                .collect();

            let reader = FakeZipReader::new(entries, HashMap::new());
            let writer = FakeFileWriter::new();

            let result = extract_zip(
                &reader,
                &writer,
                &zip_path,
                &target,
                &ExtractOptions::default(),
                &|_| {},
            );

            assert!(
                matches!(result, Err(ZipError::Validation(msg)) if msg.contains("Too many files"))
            );
        }
    }

    mod overwrite_control {
        use super::*;

        #[test]
        fn skips_existing_file_without_overwrite_flag() {
            let (_dir, zip_path, target) = setup_test_dirs();

            let (entry, name, data) = make_file_entry("existing.txt", b"new");
            let reader = FakeZipReader::new(vec![entry], HashMap::from([(name, data)]));

            // 展開先に既にファイルが存在する状態をシミュレート
            let existing = target.canonicalize().unwrap().join("existing.txt");
            let writer = FakeFileWriter::with_existing_paths(HashSet::from([existing]));
            let skipped: RefCell<Vec<FileSkipReason>> = RefCell::new(Vec::new());

            let stats = extract_zip(
                &reader,
                &writer,
                &zip_path,
                &target,
                &ExtractOptions { overwrite: false },
                &|event| {
                    if let ZipEvent::EntrySkipped { reason, .. } = event {
                        skipped.borrow_mut().push(reason);
                    }
                },
            )
            .unwrap();

            assert_eq!(stats.file_count, 0);
            assert_eq!(stats.skipped_count, 1);
            assert_eq!(skipped.borrow()[0], FileSkipReason::ExistingFile);
        }

        #[test]
        fn overwrites_existing_file_with_overwrite_flag() {
            let (_dir, zip_path, target) = setup_test_dirs();

            let (entry, name, data) = make_file_entry("existing.txt", b"new content");
            let reader = FakeZipReader::new(vec![entry], HashMap::from([(name, data)]));

            let existing = target.canonicalize().unwrap().join("existing.txt");
            let writer = FakeFileWriter::with_existing_paths(HashSet::from([existing]));

            let stats = extract_zip(
                &reader,
                &writer,
                &zip_path,
                &target,
                &ExtractOptions { overwrite: true },
                &|_| {},
            )
            .unwrap();

            assert_eq!(stats.file_count, 1);
            assert_eq!(stats.skipped_count, 0);
        }
    }

    mod permissions {
        use super::*;

        #[test]
        fn uses_default_644_for_none_permissions() {
            let (_dir, zip_path, target) = setup_test_dirs();

            let entry = ZipEntryInfo {
                name: "file.txt".to_string(),
                compressed_size: 4,
                uncompressed_size: 4,
                is_dir: false,
                is_symlink: false,
                unix_permissions: None, // Windows生成ZIPなど
            };
            let reader = FakeZipReader::new(
                vec![entry],
                HashMap::from([("file.txt".to_string(), b"data".to_vec())]),
            );
            let writer = FakeFileWriter::new();

            extract_zip(
                &reader,
                &writer,
                &zip_path,
                &target,
                &ExtractOptions::default(),
                &|_| {},
            )
            .unwrap();

            let files = writer.files_written.borrow();
            assert_eq!(files[0].2, 0o644);
        }

        #[test]
        fn sanitizes_dangerous_permissions_and_emits_event() {
            let (_dir, zip_path, target) = setup_test_dirs();

            let entry = ZipEntryInfo {
                name: "setuid.bin".to_string(),
                compressed_size: 4,
                uncompressed_size: 4,
                is_dir: false,
                is_symlink: false,
                unix_permissions: Some(0o4755), // setuidビット付き
            };
            let reader = FakeZipReader::new(
                vec![entry],
                HashMap::from([("setuid.bin".to_string(), b"data".to_vec())]),
            );
            let writer = FakeFileWriter::new();
            let perm_events: RefCell<Vec<(String, u32, u32)>> = RefCell::new(Vec::new());

            extract_zip(
                &reader,
                &writer,
                &zip_path,
                &target,
                &ExtractOptions::default(),
                &|event| {
                    if let ZipEvent::PermissionsSanitized {
                        name,
                        original,
                        sanitized,
                    } = event
                    {
                        perm_events.borrow_mut().push((name, original, sanitized));
                    }
                },
            )
            .unwrap();

            let events = perm_events.borrow();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].1, 0o4755); // original
            assert_eq!(events[0].2, 0o755); // sanitized（setuid除去）

            // サニタイズ後のパーミッションで書き込まれている
            let files = writer.files_written.borrow();
            assert_eq!(files[0].2, 0o755);
        }

        #[test]
        fn preserves_safe_permissions_without_event() {
            let (_dir, zip_path, target) = setup_test_dirs();

            let entry = ZipEntryInfo {
                name: "safe.txt".to_string(),
                compressed_size: 4,
                uncompressed_size: 4,
                is_dir: false,
                is_symlink: false,
                unix_permissions: Some(0o644),
            };
            let reader = FakeZipReader::new(
                vec![entry],
                HashMap::from([("safe.txt".to_string(), b"data".to_vec())]),
            );
            let writer = FakeFileWriter::new();
            let perm_event_count: RefCell<usize> = RefCell::new(0);

            extract_zip(
                &reader,
                &writer,
                &zip_path,
                &target,
                &ExtractOptions::default(),
                &|event| {
                    if matches!(event, ZipEvent::PermissionsSanitized { .. }) {
                        *perm_event_count.borrow_mut() += 1;
                    }
                },
            )
            .unwrap();

            assert_eq!(*perm_event_count.borrow(), 0);
        }
    }

    mod events {
        use super::*;

        #[test]
        fn emits_extraction_started_and_completed() {
            let (_dir, zip_path, target) = setup_test_dirs();

            let (entry, name, data) = make_file_entry("file.txt", b"data");
            let reader = FakeZipReader::new(vec![entry], HashMap::from([(name, data)]));
            let writer = FakeFileWriter::new();
            let started: RefCell<bool> = RefCell::new(false);
            let completed: RefCell<bool> = RefCell::new(false);

            extract_zip(
                &reader,
                &writer,
                &zip_path,
                &target,
                &ExtractOptions::default(),
                &|event| match event {
                    ZipEvent::ExtractionStarted { .. } => *started.borrow_mut() = true,
                    ZipEvent::ExtractionCompleted { .. } => *completed.borrow_mut() = true,
                    _ => {}
                },
            )
            .unwrap();

            assert!(*started.borrow());
            assert!(*completed.borrow());
        }

        #[test]
        fn emits_events_in_correct_order() {
            let (_dir, zip_path, target) = setup_test_dirs();

            let symlink = make_symlink_entry("link");
            let (file_entry, name, data) = make_file_entry("file.txt", b"data");
            let reader =
                FakeZipReader::new(vec![symlink, file_entry], HashMap::from([(name, data)]));
            let writer = FakeFileWriter::new();
            let event_order: RefCell<Vec<String>> = RefCell::new(Vec::new());

            extract_zip(
                &reader,
                &writer,
                &zip_path,
                &target,
                &ExtractOptions::default(),
                &|event| {
                    let label = match event {
                        ZipEvent::ExtractionStarted { .. } => "started",
                        ZipEvent::EntrySkipped { .. } => "skipped",
                        ZipEvent::FileExtracted { .. } => "extracted",
                        ZipEvent::ExtractionCompleted { .. } => "completed",
                        _ => return,
                    };
                    event_order.borrow_mut().push(label.to_string());
                },
            )
            .unwrap();

            let order = event_order.borrow();
            assert_eq!(*order, vec!["started", "skipped", "extracted", "completed"]);
        }

        #[test]
        fn emits_file_extracted_with_correct_size() {
            let (_dir, zip_path, target) = setup_test_dirs();

            let (entry, name, data) = make_file_entry("file.txt", b"12345");
            let reader = FakeZipReader::new(vec![entry], HashMap::from([(name, data)]));
            let writer = FakeFileWriter::new();
            let extracted: RefCell<Vec<(String, u64)>> = RefCell::new(Vec::new());

            extract_zip(
                &reader,
                &writer,
                &zip_path,
                &target,
                &ExtractOptions::default(),
                &|event| {
                    if let ZipEvent::FileExtracted { name, size } = event {
                        extracted.borrow_mut().push((name, size));
                    }
                },
            )
            .unwrap();

            let extracted = extracted.borrow();
            assert_eq!(extracted.len(), 1);
            assert_eq!(extracted[0].0, "file.txt");
            assert_eq!(extracted[0].1, 5);
        }
    }
}
