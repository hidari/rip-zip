use std::fs;
use std::io::{self, Read};
use std::path::Path;

use rip_core::error::ZipError;
use rip_core::traits::FileWriter;

/// 標準ファイルシステムを使用したFileWriter実装
///
/// std::fsの操作をラップし、ファイル書き込みとディレクトリ作成を提供する。
pub struct FsFileWriter;

impl FileWriter for FsFileWriter {
    fn create_dir_all(&self, path: &Path) -> Result<(), ZipError> {
        fs::create_dir_all(path)?;
        Ok(())
    }

    fn write_file(
        &self,
        path: &Path,
        reader: &mut dyn Read,
        permissions: u32,
    ) -> Result<u64, ZipError> {
        let mut file = fs::File::create(path)?;
        let bytes = io::copy(reader, &mut file)?;

        // File::set_permissions()はUnixではfchmod(2)ベースのため、
        // パスベースのfs::set_permissions()よりもTOCTOU耐性が高い
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(permissions & 0o777))?;
        }

        #[cfg(not(unix))]
        let _ = permissions;

        Ok(bytes)
    }

    fn exists(&self, path: &Path) -> bool {
        // symlink_metadata()を使用してリンク自体の存在を検出する
        // path.exists()はシンボリックリンクを追跡するため、
        // dangling symlinkをfalseとして返してしまう
        path.symlink_metadata().is_ok()
    }

    fn is_symlink(&self, path: &Path) -> bool {
        // symlink_metadata()を使用してリンク自体のメタデータを取得する
        // （通常のmetadata()はシンボリックリンクを追跡してしまう）
        path.symlink_metadata()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    mod create_dir_all {
        use super::*;

        #[test]
        fn creates_single_directory() {
            // 単一レベルのディレクトリを作成できること
            let dir = tempfile::TempDir::new().unwrap();
            let target = dir.path().join("new_dir");

            let writer = FsFileWriter;
            writer.create_dir_all(&target).unwrap();

            assert!(target.is_dir());
        }

        #[test]
        fn creates_nested_directories() {
            // ネストしたディレクトリを再帰的に作成できること
            let dir = tempfile::TempDir::new().unwrap();
            let target = dir.path().join("a").join("b").join("c");

            let writer = FsFileWriter;
            writer.create_dir_all(&target).unwrap();

            assert!(target.is_dir());
        }

        #[test]
        fn succeeds_for_existing_directory() {
            // 既存ディレクトリに対して再実行してもエラーにならないこと（冪等性）
            let dir = tempfile::TempDir::new().unwrap();
            let target = dir.path().join("existing");
            fs::create_dir(&target).unwrap();

            let writer = FsFileWriter;
            writer.create_dir_all(&target).unwrap();

            assert!(target.is_dir());
        }
    }

    mod write_file {
        use super::*;

        #[test]
        fn writes_data_from_reader_to_file() {
            // readerからデータを読み取りファイルに書き込めること
            let dir = tempfile::TempDir::new().unwrap();
            let target = dir.path().join("output.txt");
            let content = b"hello, file writer!";

            let writer = FsFileWriter;
            let mut reader = Cursor::new(content);
            writer.write_file(&target, &mut reader, 0o644).unwrap();

            let written = fs::read(&target).unwrap();
            assert_eq!(written, content);
        }

        #[test]
        fn returns_correct_byte_count() {
            // 書き込んだバイト数が正しく返されること
            let dir = tempfile::TempDir::new().unwrap();
            let target = dir.path().join("output.txt");
            let content = b"exactly 27 bytes of content!";

            let writer = FsFileWriter;
            let mut reader = Cursor::new(content);
            let bytes = writer.write_file(&target, &mut reader, 0o644).unwrap();

            assert_eq!(bytes, content.len() as u64);
        }

        #[test]
        fn writes_zero_bytes_successfully() {
            // 0バイトの書き込みが成功すること
            let dir = tempfile::TempDir::new().unwrap();
            let target = dir.path().join("empty.txt");

            let writer = FsFileWriter;
            let mut reader = Cursor::new(b"");
            let bytes = writer.write_file(&target, &mut reader, 0o644).unwrap();

            assert_eq!(bytes, 0);
            assert_eq!(fs::read(&target).unwrap().len(), 0);
        }

        #[cfg(unix)]
        #[test]
        fn sets_unix_permissions_on_file() {
            // Unixパーミッションが正しく設定されること
            use std::os::unix::fs::PermissionsExt;

            let dir = tempfile::TempDir::new().unwrap();
            let target = dir.path().join("exec.sh");

            let writer = FsFileWriter;
            let mut reader = Cursor::new(b"#!/bin/bash");
            writer.write_file(&target, &mut reader, 0o755).unwrap();

            let metadata = fs::metadata(&target).unwrap();
            let mode = metadata.permissions().mode() & 0o777;
            assert_eq!(mode, 0o755);
        }

        #[test]
        fn returns_io_error_for_nonexistent_parent() {
            // 存在しないディレクトリへの書き込みがIoエラーを返すこと
            let writer = FsFileWriter;
            let mut reader = Cursor::new(b"data");
            let result = writer.write_file(
                Path::new("/nonexistent/parent/file.txt"),
                &mut reader,
                0o644,
            );

            assert!(result.is_err());
        }
    }

    mod exists {
        use super::*;

        #[test]
        fn returns_true_for_existing_file() {
            // 存在するファイルに対してtrueを返すこと
            let dir = tempfile::TempDir::new().unwrap();
            let file_path = dir.path().join("file.txt");
            fs::write(&file_path, "content").unwrap();

            let writer = FsFileWriter;
            assert!(writer.exists(&file_path));
        }

        #[test]
        fn returns_false_for_nonexistent_path() {
            // 存在しないパスに対してfalseを返すこと
            let writer = FsFileWriter;
            assert!(!writer.exists(Path::new("/nonexistent/path")));
        }

        #[test]
        fn returns_true_for_directory() {
            // ディレクトリに対してtrueを返すこと
            let dir = tempfile::TempDir::new().unwrap();

            let writer = FsFileWriter;
            assert!(writer.exists(dir.path()));
        }

        #[cfg(unix)]
        #[test]
        fn returns_true_for_dangling_symlink() {
            // dangling symlinkに対してもtrueを返すこと（セキュリティ上重要）
            let dir = tempfile::TempDir::new().unwrap();
            let link = dir.path().join("dangling_link");
            std::os::unix::fs::symlink("/nonexistent/target", &link).unwrap();

            let writer = FsFileWriter;
            assert!(writer.exists(&link));
        }
    }

    mod is_symlink {
        use super::*;

        #[cfg(unix)]
        #[test]
        fn returns_true_for_symlink() {
            // シンボリックリンクに対してtrueを返すこと
            let dir = tempfile::TempDir::new().unwrap();
            let target = dir.path().join("target.txt");
            fs::write(&target, "content").unwrap();

            let link = dir.path().join("link.txt");
            std::os::unix::fs::symlink(&target, &link).unwrap();

            let writer = FsFileWriter;
            assert!(writer.is_symlink(&link));
        }

        #[test]
        fn returns_false_for_regular_file() {
            // 通常ファイルに対してfalseを返すこと
            let dir = tempfile::TempDir::new().unwrap();
            let file_path = dir.path().join("file.txt");
            fs::write(&file_path, "content").unwrap();

            let writer = FsFileWriter;
            assert!(!writer.is_symlink(&file_path));
        }

        #[test]
        fn returns_false_for_nonexistent_path() {
            // 存在しないパスに対してfalseを返すこと
            let writer = FsFileWriter;
            assert!(!writer.is_symlink(Path::new("/nonexistent/path")));
        }
    }
}
