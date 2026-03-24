use std::path::Path;

use crate::error::ZipError;
use crate::traits::{FileWalker, ZipArchiver};
use crate::types::{ZipEvent, ZipStats};
use crate::validation;

/// ZIPアーカイブを作成する
///
/// トレイト経由で外部ライブラリに依存せず、バリデーションとフロー制御を行う。
/// 副作用（ログ出力等）はon_eventコールバックで呼び出し元に委譲する。
pub fn create_zip(
    walker: &dyn FileWalker,
    archiver: &mut dyn ZipArchiver,
    source_dir: &Path,
    target_zip: &Path,
    use_zip64: bool,
    on_event: &dyn Fn(ZipEvent),
) -> Result<ZipStats, ZipError> {
    validation::validate_source_dir(source_dir)?;

    on_event(ZipEvent::ArchiveStarted {
        target: target_zip.to_path_buf(),
    });

    archiver.create(target_zip, use_zip64)?;

    let mut stats = ZipStats::default();

    for entry_result in walker.walk(source_dir) {
        let entry = entry_result?;

        // シンボリックリンクをスキップ
        if entry.is_symlink {
            on_event(ZipEvent::SymlinkSkipped { path: entry.path });
            continue;
        }

        // ファイルのみ処理（ディレクトリはスキップ）
        if !entry.is_file {
            continue;
        }

        // パストラバーサル攻撃の検出
        if validation::has_path_traversal(&entry.relative_path) {
            on_event(ZipEvent::FileSkipped {
                name: entry.relative_path.display().to_string(),
                reason: "path contains parent directory reference".to_string(),
            });
            continue;
        }

        // パス区切り文字の正規化（ZIP仕様準拠）
        let name = validation::normalize_path_separator(&entry.relative_path);

        // ファイル名長チェック
        if validation::is_filename_too_long(&name) {
            on_event(ZipEvent::FileSkipped {
                name: name.clone(),
                reason: "filename too long".to_string(),
            });
            continue;
        }

        // ファイル数制限チェック
        stats.file_count += 1;
        validation::check_file_count(stats.file_count)?;

        // 個別ファイルサイズチェック
        if validation::should_skip_large_file(entry.size, use_zip64) {
            on_event(ZipEvent::FileSkipped {
                name: name.clone(),
                reason: "exceeds 1GB limit, skipping. Use --zip64 for large files.".to_string(),
            });
            stats.file_count -= 1;
            continue;
        }

        // 合計サイズチェック
        validation::check_total_size(stats.total_size, entry.size, use_zip64)?;

        stats.total_size += entry.size;

        archiver.add_file(&name, &entry.path, entry.unix_permissions)?;

        on_event(ZipEvent::FileAdded {
            name,
            size: entry.size,
        });
    }

    archiver.finish()?;

    on_event(ZipEvent::ArchiveCompleted {
        stats: stats.clone(),
    });

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{MAX_FILE_COUNT, MAX_FILE_SIZE};
    use crate::types::FileEntry;
    use std::cell::RefCell;
    use std::path::PathBuf;

    // --- Fake実装 ---

    struct FakeWalker {
        entries: RefCell<Vec<Result<FileEntry, ZipError>>>,
    }

    impl FakeWalker {
        fn new(entries: Vec<Result<FileEntry, ZipError>>) -> Self {
            Self {
                entries: RefCell::new(entries),
            }
        }
    }

    impl FileWalker for FakeWalker {
        fn walk(
            &self,
            _source_dir: &Path,
        ) -> Box<dyn Iterator<Item = Result<FileEntry, ZipError>>> {
            let entries = self.entries.borrow_mut().drain(..).collect::<Vec<_>>();
            Box::new(entries.into_iter())
        }
    }

    struct FakeArchiver {
        added_files: Vec<(String, PathBuf, u32)>,
        created: bool,
        finished: bool,
    }

    impl FakeArchiver {
        fn new() -> Self {
            Self {
                added_files: Vec::new(),
                created: false,
                finished: false,
            }
        }
    }

    impl ZipArchiver for FakeArchiver {
        fn create(&mut self, _target: &Path, _zip64: bool) -> Result<(), ZipError> {
            self.created = true;
            Ok(())
        }

        fn add_file(&mut self, name: &str, path: &Path, perms: u32) -> Result<(), ZipError> {
            self.added_files
                .push((name.to_string(), path.to_path_buf(), perms));
            Ok(())
        }

        fn finish(&mut self) -> Result<(), ZipError> {
            self.finished = true;
            Ok(())
        }
    }

    fn make_file_entry(relative: &str, size: u64) -> FileEntry {
        FileEntry {
            path: PathBuf::from("/fake/source").join(relative),
            relative_path: PathBuf::from(relative),
            is_symlink: false,
            is_file: true,
            size,
            unix_permissions: 0o644,
        }
    }

    fn make_symlink_entry(relative: &str) -> FileEntry {
        FileEntry {
            path: PathBuf::from("/fake/source").join(relative),
            relative_path: PathBuf::from(relative),
            is_symlink: true,
            is_file: false,
            size: 0,
            unix_permissions: 0o777,
        }
    }

    fn make_dir_entry(relative: &str) -> FileEntry {
        FileEntry {
            path: PathBuf::from("/fake/source").join(relative),
            relative_path: PathBuf::from(relative),
            is_symlink: false,
            is_file: false,
            size: 0,
            unix_permissions: 0o755,
        }
    }

    // --- テスト ---

    #[test]
    fn create_zip_returns_error_for_nonexistent_source() {
        let walker = FakeWalker::new(vec![]);
        let mut archiver = FakeArchiver::new();

        let result = create_zip(
            &walker,
            &mut archiver,
            Path::new("/nonexistent/dir"),
            Path::new("/tmp/out.zip"),
            false,
            &|_| {},
        );

        assert!(matches!(result, Err(ZipError::Validation(msg)) if msg.contains("does not exist")));
        // archiverのcreateは呼ばれていないことを確認
        assert!(!archiver.created);
    }

    #[test]
    fn create_zip_adds_normal_files() {
        let dir = tempfile::TempDir::new().unwrap();
        let source = dir.path().join("src");
        std::fs::create_dir(&source).unwrap();

        let walker = FakeWalker::new(vec![
            Ok(make_file_entry("file1.txt", 100)),
            Ok(make_file_entry("sub/file2.rs", 200)),
        ]);
        let mut archiver = FakeArchiver::new();
        let events: RefCell<Vec<String>> = RefCell::new(Vec::new());

        let result = create_zip(
            &walker,
            &mut archiver,
            &source,
            &dir.path().join("out.zip"),
            false,
            &|event| {
                if let ZipEvent::FileAdded { name, .. } = event {
                    events.borrow_mut().push(name);
                }
            },
        );

        let stats = result.unwrap();
        assert_eq!(stats.file_count, 2);
        assert_eq!(stats.total_size, 300);

        let added = &archiver.added_files;
        assert_eq!(added.len(), 2);
        assert_eq!(added[0].0, "file1.txt");
        assert_eq!(added[1].0, "sub/file2.rs");

        let event_names = events.borrow();
        assert_eq!(event_names.len(), 2);
    }

    #[test]
    fn create_zip_skips_symlinks() {
        let dir = tempfile::TempDir::new().unwrap();
        let source = dir.path().join("src");
        std::fs::create_dir(&source).unwrap();

        let walker = FakeWalker::new(vec![
            Ok(make_symlink_entry("link.txt")),
            Ok(make_file_entry("real.txt", 50)),
        ]);
        let mut archiver = FakeArchiver::new();
        let symlink_skipped: RefCell<bool> = RefCell::new(false);

        let result = create_zip(
            &walker,
            &mut archiver,
            &source,
            &dir.path().join("out.zip"),
            false,
            &|event| {
                if matches!(event, ZipEvent::SymlinkSkipped { .. }) {
                    *symlink_skipped.borrow_mut() = true;
                }
            },
        );

        let stats = result.unwrap();
        assert_eq!(stats.file_count, 1);
        assert!(*symlink_skipped.borrow());
    }

    #[test]
    fn create_zip_skips_directories() {
        let dir = tempfile::TempDir::new().unwrap();
        let source = dir.path().join("src");
        std::fs::create_dir(&source).unwrap();

        let walker = FakeWalker::new(vec![
            Ok(make_dir_entry("subdir")),
            Ok(make_file_entry("file.txt", 50)),
        ]);
        let mut archiver = FakeArchiver::new();

        let stats = create_zip(
            &walker,
            &mut archiver,
            &source,
            &dir.path().join("out.zip"),
            false,
            &|_| {},
        )
        .unwrap();

        assert_eq!(stats.file_count, 1);
        assert_eq!(archiver.added_files.len(), 1);
    }

    #[test]
    fn create_zip_skips_path_traversal() {
        let dir = tempfile::TempDir::new().unwrap();
        let source = dir.path().join("src");
        std::fs::create_dir(&source).unwrap();

        let mut traversal_entry = make_file_entry("../etc/passwd", 100);
        traversal_entry.relative_path = PathBuf::from("../etc/passwd");

        let walker = FakeWalker::new(vec![
            Ok(traversal_entry),
            Ok(make_file_entry("safe.txt", 50)),
        ]);
        let mut archiver = FakeArchiver::new();
        let skipped: RefCell<Vec<String>> = RefCell::new(Vec::new());

        let stats = create_zip(
            &walker,
            &mut archiver,
            &source,
            &dir.path().join("out.zip"),
            false,
            &|event| {
                if let ZipEvent::FileSkipped { name, .. } = event {
                    skipped.borrow_mut().push(name);
                }
            },
        )
        .unwrap();

        assert_eq!(stats.file_count, 1);
        assert_eq!(skipped.borrow().len(), 1);
    }

    #[test]
    fn create_zip_skips_long_filenames() {
        let dir = tempfile::TempDir::new().unwrap();
        let source = dir.path().join("src");
        std::fs::create_dir(&source).unwrap();

        let long_name = "a".repeat(65536);
        let walker = FakeWalker::new(vec![Ok(make_file_entry(&long_name, 50))]);
        let mut archiver = FakeArchiver::new();

        let stats = create_zip(
            &walker,
            &mut archiver,
            &source,
            &dir.path().join("out.zip"),
            false,
            &|_| {},
        )
        .unwrap();

        assert_eq!(stats.file_count, 0);
        assert!(archiver.added_files.is_empty());
    }

    #[test]
    fn create_zip_rejects_too_many_files() {
        let dir = tempfile::TempDir::new().unwrap();
        let source = dir.path().join("src");
        std::fs::create_dir(&source).unwrap();

        let entries: Vec<Result<FileEntry, ZipError>> = (0..=MAX_FILE_COUNT)
            .map(|i| Ok(make_file_entry(&format!("file_{}.txt", i), 1)))
            .collect();

        let walker = FakeWalker::new(entries);
        let mut archiver = FakeArchiver::new();

        let result = create_zip(
            &walker,
            &mut archiver,
            &source,
            &dir.path().join("out.zip"),
            false,
            &|_| {},
        );

        assert!(matches!(result, Err(ZipError::Validation(msg)) if msg.contains("Too many files")));
    }

    #[test]
    fn create_zip_skips_large_files_without_zip64() {
        let dir = tempfile::TempDir::new().unwrap();
        let source = dir.path().join("src");
        std::fs::create_dir(&source).unwrap();

        let walker = FakeWalker::new(vec![
            Ok(make_file_entry("big.bin", MAX_FILE_SIZE + 1)),
            Ok(make_file_entry("small.txt", 100)),
        ]);
        let mut archiver = FakeArchiver::new();

        let stats = create_zip(
            &walker,
            &mut archiver,
            &source,
            &dir.path().join("out.zip"),
            false,
            &|_| {},
        )
        .unwrap();

        assert_eq!(stats.file_count, 1);
        assert_eq!(stats.total_size, 100);
    }

    #[test]
    fn create_zip_allows_large_files_with_zip64() {
        let dir = tempfile::TempDir::new().unwrap();
        let source = dir.path().join("src");
        std::fs::create_dir(&source).unwrap();

        let walker = FakeWalker::new(vec![Ok(make_file_entry("big.bin", MAX_FILE_SIZE + 1))]);
        let mut archiver = FakeArchiver::new();

        let stats = create_zip(
            &walker,
            &mut archiver,
            &source,
            &dir.path().join("out.zip"),
            true,
            &|_| {},
        )
        .unwrap();

        assert_eq!(stats.file_count, 1);
    }

    #[test]
    fn create_zip_rejects_total_size_over_limit_without_zip64() {
        let dir = tempfile::TempDir::new().unwrap();
        let source = dir.path().join("src");
        std::fs::create_dir(&source).unwrap();

        // 個別ファイルサイズはMAX_FILE_SIZE以下だが、合計でMAX_TOTAL_SIZEを超えるケース
        // MAX_FILE_SIZE (1GB) * 5 = 5GB > MAX_TOTAL_SIZE (4GB)
        let walker = FakeWalker::new(vec![
            Ok(make_file_entry("file1.bin", MAX_FILE_SIZE)),
            Ok(make_file_entry("file2.bin", MAX_FILE_SIZE)),
            Ok(make_file_entry("file3.bin", MAX_FILE_SIZE)),
            Ok(make_file_entry("file4.bin", MAX_FILE_SIZE)),
            Ok(make_file_entry("file5.bin", MAX_FILE_SIZE)),
        ]);
        let mut archiver = FakeArchiver::new();

        let result = create_zip(
            &walker,
            &mut archiver,
            &source,
            &dir.path().join("out.zip"),
            false,
            &|_| {},
        );

        assert!(matches!(result, Err(ZipError::Validation(msg)) if msg.contains("4GB limit")));
    }

    #[test]
    fn create_zip_emits_archive_started_and_completed_events() {
        let dir = tempfile::TempDir::new().unwrap();
        let source = dir.path().join("src");
        std::fs::create_dir(&source).unwrap();

        let walker = FakeWalker::new(vec![Ok(make_file_entry("file.txt", 50))]);
        let mut archiver = FakeArchiver::new();
        let started: RefCell<bool> = RefCell::new(false);
        let completed: RefCell<bool> = RefCell::new(false);

        create_zip(
            &walker,
            &mut archiver,
            &source,
            &dir.path().join("out.zip"),
            false,
            &|event| match event {
                ZipEvent::ArchiveStarted { .. } => *started.borrow_mut() = true,
                ZipEvent::ArchiveCompleted { .. } => *completed.borrow_mut() = true,
                _ => {}
            },
        )
        .unwrap();

        assert!(*started.borrow());
        assert!(*completed.borrow());
    }

    #[test]
    fn create_zip_calls_archiver_lifecycle_correctly() {
        let dir = tempfile::TempDir::new().unwrap();
        let source = dir.path().join("src");
        std::fs::create_dir(&source).unwrap();

        let walker = FakeWalker::new(vec![Ok(make_file_entry("test.txt", 42))]);
        let mut archiver = FakeArchiver::new();

        create_zip(
            &walker,
            &mut archiver,
            &source,
            &dir.path().join("out.zip"),
            false,
            &|_| {},
        )
        .unwrap();

        assert!(archiver.created);
        assert!(archiver.finished);
        assert_eq!(archiver.added_files.len(), 1);
    }
}
