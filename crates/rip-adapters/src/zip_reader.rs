use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use rip_core::error::ZipError;
use rip_core::traits::ZipReader;
use rip_core::types::ZipEntryInfo;
use zip::ZipArchive;

use crate::error_convert::from_zip_error;

/// zipクレートを使用したZipReader実装
///
/// コンストラクタでZIPファイルを開きアーカイブ状態を保持する。
/// scan（事前スキャン）とextract_entry（個別エントリ展開）は
/// 保持済みのアーカイブに対して操作するため、毎回のファイルオープンが不要。
pub struct ZipArchiveReader {
    archive: ZipArchive<File>,
    path: PathBuf,
}

impl ZipArchiveReader {
    /// ZIPファイルを開いてリーダーを作成する
    pub fn new(zip_path: &Path) -> Result<Self, ZipError> {
        let file = File::open(zip_path)?;
        let archive = ZipArchive::new(file).map_err(from_zip_error)?;
        Ok(Self {
            archive,
            path: zip_path.to_path_buf(),
        })
    }
}

/// unix_mode()のファイルタイプビットからシンボリックリンクかどうかを判定する
///
/// S_IFMT (0o170000) でマスクし、S_IFLNK (0o120000) と比較する
fn is_symlink_mode(mode: Option<u32>) -> bool {
    match mode {
        Some(mode) => (mode & 0o170000) == 0o120000,
        None => false,
    }
}

impl ZipReader for ZipArchiveReader {
    fn source_path(&self) -> &Path {
        &self.path
    }

    fn scan(&mut self) -> Result<Vec<ZipEntryInfo>, ZipError> {
        let mut entries = Vec::with_capacity(self.archive.len());
        for i in 0..self.archive.len() {
            let entry = self.archive.by_index(i).map_err(from_zip_error)?;
            let unix_mode = entry.unix_mode();

            entries.push(ZipEntryInfo {
                name: entry.name().to_string(),
                compressed_size: entry.compressed_size(),
                uncompressed_size: entry.size(),
                is_dir: entry.is_dir(),
                is_symlink: is_symlink_mode(unix_mode),
                unix_permissions: unix_mode.map(|m| m & 0o777),
            });
        }

        Ok(entries)
    }

    fn extract_entry(&mut self, entry_name: &str, writer: &mut dyn Write) -> Result<u64, ZipError> {
        let mut entry = self.archive.by_name(entry_name).map_err(from_zip_error)?;

        let bytes = io::copy(&mut entry, writer)?;
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    /// テスト用ZIPファイルを作成するヘルパー
    /// entries: (エントリ名, 内容, Unixパーミッション)のスライス
    fn create_test_zip(
        dir: &Path,
        name: &str,
        entries: &[(&str, &[u8], u32)],
    ) -> std::path::PathBuf {
        let zip_path = dir.join(name);
        let file = File::create(&zip_path).unwrap();
        let mut writer = ZipWriter::new(file);

        for (entry_name, content, permissions) in entries {
            let options = SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .unix_permissions(*permissions);
            writer.start_file(*entry_name, options).unwrap();
            writer.write_all(content).unwrap();
        }

        writer.finish().unwrap();
        zip_path
    }

    /// テスト用のディレクトリエントリを含むZIPファイルを作成するヘルパー
    fn create_zip_with_directory(dir: &Path, name: &str) -> std::path::PathBuf {
        let zip_path = dir.join(name);
        let file = File::create(&zip_path).unwrap();
        let mut writer = ZipWriter::new(file);

        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        writer.add_directory("subdir/", options).unwrap();

        let file_options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o644);
        writer.start_file("subdir/file.txt", file_options).unwrap();
        writer.write_all(b"content").unwrap();

        writer.finish().unwrap();
        zip_path
    }

    // --- ZipArchiveReaderの仕様テスト ---
    // ZipArchiveReaderが提供するscan()とextract_entry()の仕様を検証するテスト群。

    mod scan {
        use super::*;

        #[test]
        fn returns_single_entry_for_single_file_zip() {
            // 単一ファイルのZIPをスキャンして1つのエントリ情報を取得できること
            let dir = tempfile::TempDir::new().unwrap();
            let zip_path = create_test_zip(
                dir.path(),
                "test.zip",
                &[("hello.txt", b"hello world", 0o644)],
            );

            let mut reader = ZipArchiveReader::new(&zip_path).unwrap();
            let entries = reader.scan().unwrap();

            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].name, "hello.txt");
            assert!(!entries[0].is_dir);
            assert!(!entries[0].is_symlink);
        }

        #[test]
        fn returns_all_entries_for_multi_file_zip() {
            // 複数ファイルのZIPをスキャンして全エントリ情報を取得できること
            let dir = tempfile::TempDir::new().unwrap();
            let zip_path = create_test_zip(
                dir.path(),
                "test.zip",
                &[
                    ("a.txt", b"aaa", 0o644),
                    ("b.txt", b"bbb", 0o644),
                    ("c.txt", b"ccc", 0o644),
                ],
            );

            let mut reader = ZipArchiveReader::new(&zip_path).unwrap();
            let entries = reader.scan().unwrap();

            assert_eq!(entries.len(), 3);
            let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
            assert!(names.contains(&"a.txt"));
            assert!(names.contains(&"b.txt"));
            assert!(names.contains(&"c.txt"));
        }

        #[test]
        fn marks_directory_entries_as_is_dir() {
            // ディレクトリエントリのis_dirが正しく設定されること
            let dir = tempfile::TempDir::new().unwrap();
            let zip_path = create_zip_with_directory(dir.path(), "test.zip");

            let mut reader = ZipArchiveReader::new(&zip_path).unwrap();
            let entries = reader.scan().unwrap();

            let dir_entry = entries.iter().find(|e| e.name == "subdir/").unwrap();
            assert!(dir_entry.is_dir);

            let file_entry = entries
                .iter()
                .find(|e| e.name == "subdir/file.txt")
                .unwrap();
            assert!(!file_entry.is_dir);
        }

        #[test]
        fn returns_empty_vec_for_empty_zip() {
            // 空のZIPをスキャンすると空のVecを返すこと
            let dir = tempfile::TempDir::new().unwrap();
            let zip_path = create_test_zip(dir.path(), "empty.zip", &[]);

            let mut reader = ZipArchiveReader::new(&zip_path).unwrap();
            let entries = reader.scan().unwrap();
            assert!(entries.is_empty());
        }

        #[test]
        fn reports_correct_uncompressed_size() {
            // 展開後のサイズが正しく設定されること
            let dir = tempfile::TempDir::new().unwrap();
            let content = b"hello world, this is test content";
            let zip_path = create_test_zip(dir.path(), "test.zip", &[("data.txt", content, 0o644)]);

            let mut reader = ZipArchiveReader::new(&zip_path).unwrap();
            let entries = reader.scan().unwrap();

            assert_eq!(entries[0].uncompressed_size, content.len() as u64);
        }

        #[cfg(unix)]
        #[test]
        fn includes_unix_permissions_when_available() {
            // Unixパーミッションがエントリに反映されること
            let dir = tempfile::TempDir::new().unwrap();
            let zip_path = create_test_zip(
                dir.path(),
                "test.zip",
                &[("exec.sh", b"#!/bin/bash", 0o755)],
            );

            let mut reader = ZipArchiveReader::new(&zip_path).unwrap();
            let entries = reader.scan().unwrap();

            assert_eq!(entries[0].unix_permissions, Some(0o755));
        }

        #[test]
        fn returns_io_error_for_nonexistent_path() {
            // 存在しないファイルパスを指定するとコンストラクタでIoエラーを返すこと
            let result = ZipArchiveReader::new(Path::new("/nonexistent/path/to/file.zip"));
            assert!(result.is_err());
        }

        #[test]
        fn regular_file_entry_is_not_detected_as_symlink() {
            // ZipWriter::start_file()で作成した通常ファイルはsymlinkとして検出されないこと
            // （start_file()はS_IFREGを自動付与するため）
            let dir = tempfile::TempDir::new().unwrap();
            let zip_path =
                create_test_zip(dir.path(), "test.zip", &[("file.txt", b"content", 0o644)]);

            let mut reader = ZipArchiveReader::new(&zip_path).unwrap();
            let entries = reader.scan().unwrap();

            assert!(!entries[0].is_symlink);
        }
    }

    // --- is_symlink_mode関数のユニットテスト ---
    // ZipWriter::start_file()ではsymlinkエントリを作成できないため、
    // is_symlink_mode()関数を直接テストする。

    mod symlink_detection {
        use super::*;

        #[test]
        fn returns_true_for_symlink_mode() {
            // S_IFLNK (0o120000) のファイルタイプビットが設定されていればtrueを返すこと
            assert!(is_symlink_mode(Some(0o120777)));
            assert!(is_symlink_mode(Some(0o120644)));
            assert!(is_symlink_mode(Some(0o120000)));
        }

        #[test]
        fn returns_false_for_regular_file_mode() {
            // S_IFREG (0o100000) のファイルタイプビットが設定されていればfalseを返すこと
            assert!(!is_symlink_mode(Some(0o100644)));
            assert!(!is_symlink_mode(Some(0o100755)));
        }

        #[test]
        fn returns_false_for_directory_mode() {
            // S_IFDIR (0o040000) のファイルタイプビットが設定されていればfalseを返すこと
            assert!(!is_symlink_mode(Some(0o040755)));
        }

        #[test]
        fn returns_false_for_none() {
            // Noneの場合はfalseを返すこと
            assert!(!is_symlink_mode(None));
        }

        #[test]
        fn returns_false_for_zero() {
            // 0の場合はfalseを返すこと（ファイルタイプビットが未設定）
            assert!(!is_symlink_mode(Some(0)));
        }
    }

    mod source_path {
        use super::*;

        #[test]
        fn returns_the_path_used_to_create_reader() {
            // コンストラクタに渡したパスがsource_path()で取得できること
            let dir = tempfile::TempDir::new().unwrap();
            let zip_path = create_test_zip(dir.path(), "test.zip", &[("a.txt", b"hello", 0o644)]);

            let reader = ZipArchiveReader::new(&zip_path).unwrap();
            assert_eq!(reader.source_path(), &zip_path);
        }
    }

    mod extract_entry {
        use super::*;

        #[test]
        fn extracts_entry_data_to_writer() {
            // エントリのデータをwriterに正しく書き込めること
            let dir = tempfile::TempDir::new().unwrap();
            let content = b"hello, extraction!";
            let zip_path = create_test_zip(dir.path(), "test.zip", &[("msg.txt", content, 0o644)]);

            let mut reader = ZipArchiveReader::new(&zip_path).unwrap();
            let mut buf = Vec::new();
            reader.extract_entry("msg.txt", &mut buf).unwrap();

            assert_eq!(buf, content);
        }

        #[test]
        fn returns_correct_byte_count() {
            // 展開したバイト数が正しく返されること
            let dir = tempfile::TempDir::new().unwrap();
            let content = b"exactly 26 bytes of content";
            let zip_path = create_test_zip(dir.path(), "test.zip", &[("data.txt", content, 0o644)]);

            let mut reader = ZipArchiveReader::new(&zip_path).unwrap();
            let mut buf = Vec::new();
            let bytes = reader.extract_entry("data.txt", &mut buf).unwrap();

            assert_eq!(bytes, content.len() as u64);
        }

        #[test]
        fn returns_archive_error_for_missing_entry() {
            // 存在しないエントリ名を指定するとArchiveエラーを返すこと
            let dir = tempfile::TempDir::new().unwrap();
            let zip_path = create_test_zip(dir.path(), "test.zip", &[("a.txt", b"hello", 0o644)]);

            let mut reader = ZipArchiveReader::new(&zip_path).unwrap();
            let mut buf = Vec::new();
            let result = reader.extract_entry("nonexistent.txt", &mut buf);

            assert!(result.is_err());
        }

        #[test]
        fn extracts_zero_byte_file() {
            // 0バイトファイルの展開が成功すること
            let dir = tempfile::TempDir::new().unwrap();
            let zip_path = create_test_zip(dir.path(), "test.zip", &[("empty.txt", b"", 0o644)]);

            let mut reader = ZipArchiveReader::new(&zip_path).unwrap();
            let mut buf = Vec::new();
            let bytes = reader.extract_entry("empty.txt", &mut buf).unwrap();

            assert_eq!(bytes, 0);
            assert!(buf.is_empty());
        }

        #[test]
        fn extracts_unicode_named_entry() {
            // Unicodeファイル名のエントリを展開できること
            let dir = tempfile::TempDir::new().unwrap();
            let content = "日本語コンテンツ".as_bytes();
            let zip_path = create_test_zip(
                dir.path(),
                "test.zip",
                &[("テスト/データ.txt", content, 0o644)],
            );

            let mut reader = ZipArchiveReader::new(&zip_path).unwrap();
            let mut buf = Vec::new();
            reader.extract_entry("テスト/データ.txt", &mut buf).unwrap();

            assert_eq!(buf, content);
        }
    }

    // --- zip crate API契約テスト（読み取り系） ---
    // zip crate 8.4.0 の読み取り系APIの存在・挙動を確認するテスト群。
    // バージョンアップ時にAPI互換性の破損を自動検出する。

    mod zip_crate_read_contract {
        use super::*;

        #[test]
        fn zip_archive_new_opens_valid_zip_file() {
            // ZipArchive::newでファイルからアーカイブを開けること
            let dir = tempfile::TempDir::new().unwrap();
            let zip_path = create_test_zip(dir.path(), "test.zip", &[("a.txt", b"hello", 0o644)]);

            let file = File::open(&zip_path).unwrap();
            let archive = ZipArchive::new(file).unwrap();
            assert_eq!(archive.len(), 1);
        }

        #[test]
        fn zip_archive_by_index_returns_entry() {
            // by_indexでインデックスからエントリを取得できること
            let dir = tempfile::TempDir::new().unwrap();
            let zip_path = create_test_zip(dir.path(), "test.zip", &[("a.txt", b"hello", 0o644)]);

            let file = File::open(&zip_path).unwrap();
            let mut archive = ZipArchive::new(file).unwrap();
            let entry = archive.by_index(0).unwrap();
            assert_eq!(entry.name(), "a.txt");
        }

        #[test]
        fn zip_archive_by_name_returns_entry() {
            // by_nameで名前からエントリを取得できること
            let dir = tempfile::TempDir::new().unwrap();
            let zip_path = create_test_zip(dir.path(), "test.zip", &[("a.txt", b"hello", 0o644)]);

            let file = File::open(&zip_path).unwrap();
            let mut archive = ZipArchive::new(file).unwrap();
            let entry = archive.by_name("a.txt").unwrap();
            assert_eq!(entry.name(), "a.txt");
        }

        #[test]
        fn zip_entry_provides_size_metadata() {
            // エントリからcompressed_sizeとsizeを取得できること
            let dir = tempfile::TempDir::new().unwrap();
            let zip_path = create_test_zip(
                dir.path(),
                "test.zip",
                &[("data.txt", b"some content here", 0o644)],
            );

            let file = File::open(&zip_path).unwrap();
            let mut archive = ZipArchive::new(file).unwrap();
            let entry = archive.by_index(0).unwrap();

            // size()は展開後のサイズを返す
            assert_eq!(entry.size(), 17);
            // compressed_sizeは圧縮後のサイズ（Deflateなのでサイズは変動するが取得可能）
            let _ = entry.compressed_size();
        }

        #[test]
        fn zip_entry_is_dir_detects_directory_entries() {
            // is_dir()でディレクトリエントリを判定できること
            let dir = tempfile::TempDir::new().unwrap();
            let zip_path = create_zip_with_directory(dir.path(), "test.zip");

            let file = File::open(&zip_path).unwrap();
            let mut archive = ZipArchive::new(file).unwrap();

            // by_nameは&mut selfを取るため、各エントリの結果を先に取得してドロップする
            let is_dir = archive.by_name("subdir/").unwrap().is_dir();
            assert!(is_dir);

            let is_file = !archive.by_name("subdir/file.txt").unwrap().is_dir();
            assert!(is_file);
        }

        #[cfg(unix)]
        #[test]
        fn zip_entry_unix_mode_returns_permissions() {
            // unix_mode()でパーミッション情報を取得できること
            let dir = tempfile::TempDir::new().unwrap();
            let zip_path = create_test_zip(
                dir.path(),
                "test.zip",
                &[("exec.sh", b"#!/bin/bash", 0o755)],
            );

            let file = File::open(&zip_path).unwrap();
            let mut archive = ZipArchive::new(file).unwrap();
            let entry = archive.by_index(0).unwrap();

            let mode = entry.unix_mode().unwrap();
            // 下位9ビットのパーミッション部分が0o755であること
            assert_eq!(mode & 0o777, 0o755);
        }

        #[cfg(unix)]
        #[test]
        fn zip_writer_start_file_overrides_file_type_bits_with_regular_file() {
            // ZipWriter::start_file()はunix_permissions()で指定したファイルタイプビットを
            // S_IFREG (0o100000) で上書きする。
            // そのため、start_file()ではsymlinkエントリを作成できない。
            // 実際のsymlinkエントリは外部ツールで作成されたZIPにのみ存在する。
            let dir = tempfile::TempDir::new().unwrap();
            let zip_path = dir.path().join("type_bits.zip");
            let file = File::create(&zip_path).unwrap();
            let mut writer = ZipWriter::new(file);

            // S_IFLNK (0o120000) | 0o777 を設定してもstart_file()がS_IFREGで上書きする
            let symlink_mode = 0o120000 | 0o777;
            let options = SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored)
                .unix_permissions(symlink_mode);
            writer.start_file("link", options).unwrap();
            writer.write_all(b"target").unwrap();
            writer.finish().unwrap();

            let file = File::open(&zip_path).unwrap();
            let mut archive = ZipArchive::new(file).unwrap();
            let entry = archive.by_index(0).unwrap();

            let mode = entry.unix_mode().unwrap();
            // start_file()がS_IFREG (0o100000)を自動付与することを確認
            assert_eq!(
                mode & 0o170000,
                0o100000,
                "start_file() should set S_IFREG file type bits. Got: {:#o}",
                mode
            );
            // パーミッション部分は保持される
            assert_eq!(mode & 0o777, 0o777);
        }

        #[test]
        fn zip_archive_by_name_returns_error_for_missing_entry() {
            // by_nameで存在しないエントリを指定するとエラーになること
            let dir = tempfile::TempDir::new().unwrap();
            let zip_path = create_test_zip(dir.path(), "test.zip", &[("a.txt", b"hello", 0o644)]);

            let file = File::open(&zip_path).unwrap();
            let mut archive = ZipArchive::new(file).unwrap();
            let result = archive.by_name("nonexistent.txt");
            assert!(result.is_err());
        }

        #[test]
        fn zip_entry_supports_read_trait() {
            // エントリからReadトレイトでデータを読み取れること
            let dir = tempfile::TempDir::new().unwrap();
            let zip_path = create_test_zip(dir.path(), "test.zip", &[("a.txt", b"hello", 0o644)]);

            let file = File::open(&zip_path).unwrap();
            let mut archive = ZipArchive::new(file).unwrap();
            let mut entry = archive.by_name("a.txt").unwrap();

            let mut buf = Vec::new();
            entry.read_to_end(&mut buf).unwrap();
            assert_eq!(buf, b"hello");
        }

        #[test]
        fn zip_archive_supports_by_index_then_by_name_on_same_instance() {
            // 同一ZipArchiveインスタンスでby_index（scan相当）の後に
            // by_name（extract_entry相当）が正常に動作すること
            let dir = tempfile::TempDir::new().unwrap();
            let zip_path = create_test_zip(
                dir.path(),
                "test.zip",
                &[("a.txt", b"hello", 0o644), ("b.txt", b"world", 0o644)],
            );

            let file = File::open(&zip_path).unwrap();
            let mut archive = ZipArchive::new(file).unwrap();

            // by_indexで全エントリをスキャン（scan相当）
            let mut names = Vec::new();
            for i in 0..archive.len() {
                let entry = archive.by_index(i).unwrap();
                names.push(entry.name().to_string());
            }
            assert_eq!(names, vec!["a.txt", "b.txt"]);

            // by_nameでエントリを読み取り（extract_entry相当）
            let mut buf = Vec::new();
            let mut entry = archive.by_name("b.txt").unwrap();
            entry.read_to_end(&mut buf).unwrap();
            assert_eq!(buf, b"world");
        }
    }
}
