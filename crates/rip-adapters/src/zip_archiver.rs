use std::fs::File;
use std::io;
use std::path::Path;

use rip_core::error::ZipError;
use rip_core::traits::ZipArchiver;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::error_convert::from_zip_error;

/// zipクレートを使用したZipArchiver実装
///
/// ZipWriterをラップし、create → add_file (N回) → finish のライフサイクルを管理する。
/// writerとoptionsは常に同時に存在するため、単一のOptionで管理する。
pub struct ZipWriterArchiver {
    state: Option<(ZipWriter<File>, SimpleFileOptions)>,
}

impl ZipWriterArchiver {
    pub fn new() -> Self {
        Self { state: None }
    }

    fn active_state(&mut self) -> Result<(&mut ZipWriter<File>, &SimpleFileOptions), ZipError> {
        let (writer, options) = self.state.as_mut().ok_or_else(|| {
            ZipError::Archive("Archiver not initialized. Call create() first.".to_string())
        })?;
        Ok((writer, options))
    }
}

impl Default for ZipWriterArchiver {
    fn default() -> Self {
        Self::new()
    }
}

impl ZipArchiver for ZipWriterArchiver {
    fn create(&mut self, target_zip: &Path, use_zip64: bool) -> Result<(), ZipError> {
        let zip_file = File::create(target_zip)?;
        let writer = ZipWriter::new(zip_file);

        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .large_file(use_zip64);

        self.state = Some((writer, options));
        Ok(())
    }

    fn add_file(
        &mut self,
        name: &str,
        source_path: &Path,
        unix_permissions: u32,
    ) -> Result<(), ZipError> {
        let (writer, options) = self.active_state()?;

        let file_options = options.unix_permissions(unix_permissions);
        writer
            .start_file(name, file_options)
            .map_err(from_zip_error)?;

        let mut file = File::open(source_path)?;
        io::copy(&mut file, writer)?;

        Ok(())
    }

    fn finish(&mut self) -> Result<(), ZipError> {
        let (writer, _) = self.state.take().ok_or_else(|| {
            ZipError::Archive("Archiver not initialized. Call create() first.".to_string())
        })?;

        writer.finish().map_err(from_zip_error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn create_and_finish_produces_valid_zip() {
        let dir = tempfile::TempDir::new().unwrap();
        let zip_path = dir.path().join("test.zip");

        let mut archiver = ZipWriterArchiver::new();
        archiver.create(&zip_path, false).unwrap();
        archiver.finish().unwrap();

        // 生成されたZIPファイルが読み取れることを確認
        let file = File::open(&zip_path).unwrap();
        let archive = zip::ZipArchive::new(file).unwrap();
        assert_eq!(archive.len(), 0);
    }

    #[test]
    fn add_file_stores_content_correctly() {
        let dir = tempfile::TempDir::new().unwrap();
        let source_file = dir.path().join("input.txt");
        let content = "hello, zip!";
        fs::write(&source_file, content).unwrap();

        let zip_path = dir.path().join("output.zip");
        let mut archiver = ZipWriterArchiver::new();
        archiver.create(&zip_path, false).unwrap();
        archiver.add_file("input.txt", &source_file, 0o644).unwrap();
        archiver.finish().unwrap();

        // ZIPの中身を検証
        let file = File::open(&zip_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        assert_eq!(archive.len(), 1);

        let mut entry = archive.by_name("input.txt").unwrap();
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut entry, &mut buf).unwrap();
        assert_eq!(buf, content);
    }

    #[test]
    fn add_file_preserves_unicode_filename() {
        let dir = tempfile::TempDir::new().unwrap();
        let source_file = dir.path().join("テスト.txt");
        fs::write(&source_file, "日本語").unwrap();

        let zip_path = dir.path().join("unicode.zip");
        let mut archiver = ZipWriterArchiver::new();
        archiver.create(&zip_path, false).unwrap();
        archiver
            .add_file("日本語/テスト.txt", &source_file, 0o644)
            .unwrap();
        archiver.finish().unwrap();

        let file = File::open(&zip_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let entry = archive.by_name("日本語/テスト.txt").unwrap();
        assert_eq!(entry.name(), "日本語/テスト.txt");
    }

    #[cfg(unix)]
    #[test]
    fn add_file_preserves_unix_permissions() {
        let dir = tempfile::TempDir::new().unwrap();
        let source_file = dir.path().join("exec.sh");
        fs::write(&source_file, "#!/bin/bash").unwrap();

        let zip_path = dir.path().join("perms.zip");
        let mut archiver = ZipWriterArchiver::new();
        archiver.create(&zip_path, false).unwrap();
        archiver.add_file("exec.sh", &source_file, 0o755).unwrap();
        archiver.finish().unwrap();

        let file = File::open(&zip_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let entry = archive.by_name("exec.sh").unwrap();
        assert_eq!(entry.unix_mode().unwrap() & 0o777, 0o755);
    }

    #[test]
    fn add_file_without_create_returns_error() {
        let dir = tempfile::TempDir::new().unwrap();
        let source_file = dir.path().join("input.txt");
        fs::write(&source_file, "content").unwrap();

        let mut archiver = ZipWriterArchiver::new();
        let result = archiver.add_file("input.txt", &source_file, 0o644);
        assert!(matches!(result, Err(ZipError::Archive(_))));
    }

    #[test]
    fn finish_without_create_returns_error() {
        let mut archiver = ZipWriterArchiver::new();
        let result = archiver.finish();
        assert!(matches!(result, Err(ZipError::Archive(_))));
    }

    #[test]
    fn zip64_option_is_applied() {
        let dir = tempfile::TempDir::new().unwrap();
        let source_file = dir.path().join("file.txt");
        fs::write(&source_file, "content").unwrap();

        let zip_path = dir.path().join("zip64.zip");
        let mut archiver = ZipWriterArchiver::new();
        archiver.create(&zip_path, true).unwrap();
        archiver.add_file("file.txt", &source_file, 0o644).unwrap();
        archiver.finish().unwrap();

        // ZIP64で作成されたファイルが正常に読み取れることを確認
        let file = File::open(&zip_path).unwrap();
        let archive = zip::ZipArchive::new(file).unwrap();
        assert_eq!(archive.len(), 1);
    }

    #[test]
    fn multiple_files_are_stored() {
        let dir = tempfile::TempDir::new().unwrap();
        let zip_path = dir.path().join("multi.zip");

        let file1 = dir.path().join("file1.txt");
        let file2 = dir.path().join("file2.txt");
        fs::write(&file1, "content1").unwrap();
        fs::write(&file2, "content2").unwrap();

        let mut archiver = ZipWriterArchiver::new();
        archiver.create(&zip_path, false).unwrap();
        archiver.add_file("file1.txt", &file1, 0o644).unwrap();
        archiver.add_file("file2.txt", &file2, 0o644).unwrap();
        archiver.finish().unwrap();

        let file = File::open(&zip_path).unwrap();
        let archive = zip::ZipArchive::new(file).unwrap();
        assert_eq!(archive.len(), 2);
    }
}
