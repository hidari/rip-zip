use std::path::{Path, PathBuf};

/// ファイル名から危険な文字を安全な文字に置換する
///
/// ZIPファイル名として不正な文字や制御文字をアンダースコアに置き換える。
pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            _ if c.is_control() => '_',
            _ => c,
        })
        .collect()
}

/// ソースディレクトリから出力ZIPファイルのパスを決定する
///
/// ディレクトリ名をサニタイズし、同名のZIPファイルが既に存在する場合は
/// 連番を付与する（例: `dir (1).zip`, `dir (2).zip`）。
pub fn get_zip_path(source_dir: &Path) -> PathBuf {
    let dir_name = source_dir
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("archive"))
        .to_string_lossy();
    let safe_name = sanitize_filename(&dir_name);

    let mut zip_path = source_dir
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();

    zip_path.push(format!("{}.zip", safe_name));

    // 同名のZIPファイルが存在する場合は連番を付ける
    let mut counter = 1;
    let original_zip_path = zip_path.clone();
    while zip_path.exists() {
        zip_path = original_zip_path.with_file_name(format!("{} ({}).zip", safe_name, counter));
        counter += 1;
    }

    zip_path
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- sanitize_filename ---

    #[test]
    fn sanitize_filename_replaces_dangerous_characters() {
        assert_eq!(sanitize_filename("file\\name"), "file_name");
        assert_eq!(sanitize_filename("file/name"), "file_name");
        assert_eq!(sanitize_filename("file:name"), "file_name");
        assert_eq!(sanitize_filename("file*name"), "file_name");
        assert_eq!(sanitize_filename("file?name"), "file_name");
        assert_eq!(sanitize_filename("file\"name"), "file_name");
        assert_eq!(sanitize_filename("file<name"), "file_name");
        assert_eq!(sanitize_filename("file>name"), "file_name");
        assert_eq!(sanitize_filename("file|name"), "file_name");
        assert_eq!(sanitize_filename("file\0name"), "file_name");
    }

    #[test]
    fn sanitize_filename_replaces_control_characters() {
        assert_eq!(sanitize_filename("file\x01name"), "file_name");
        assert_eq!(sanitize_filename("file\x1fname"), "file_name");
    }

    #[test]
    fn sanitize_filename_preserves_normal_characters() {
        assert_eq!(sanitize_filename("normal_file.txt"), "normal_file.txt");
        assert_eq!(sanitize_filename("日本語ファイル"), "日本語ファイル");
        assert_eq!(sanitize_filename("file-name.rs"), "file-name.rs");
    }

    #[test]
    fn sanitize_filename_handles_empty_string() {
        assert_eq!(sanitize_filename(""), "");
    }

    #[test]
    fn sanitize_filename_replaces_multiple_dangerous_chars() {
        assert_eq!(sanitize_filename("a\\b/c:d"), "a_b_c_d");
    }

    // --- get_zip_path ---

    #[test]
    fn get_zip_path_creates_zip_extension() {
        let dir = tempfile::TempDir::new().unwrap();
        let source = dir.path().join("my_project");
        std::fs::create_dir(&source).unwrap();

        let result = get_zip_path(&source);
        assert_eq!(
            result.file_name().unwrap().to_str().unwrap(),
            "my_project.zip"
        );
    }

    #[test]
    fn get_zip_path_adds_counter_for_existing_zip() {
        let dir = tempfile::TempDir::new().unwrap();
        let source = dir.path().join("project");
        std::fs::create_dir(&source).unwrap();

        // 既存のZIPファイルを作成
        let existing_zip = dir.path().join("project.zip");
        std::fs::write(&existing_zip, "dummy").unwrap();

        let result = get_zip_path(&source);
        assert_eq!(
            result.file_name().unwrap().to_str().unwrap(),
            "project (1).zip"
        );
    }

    #[test]
    fn get_zip_path_increments_counter_for_multiple_existing() {
        let dir = tempfile::TempDir::new().unwrap();
        let source = dir.path().join("docs");
        std::fs::create_dir(&source).unwrap();

        std::fs::write(dir.path().join("docs.zip"), "dummy").unwrap();
        std::fs::write(dir.path().join("docs (1).zip"), "dummy").unwrap();

        let result = get_zip_path(&source);
        assert_eq!(
            result.file_name().unwrap().to_str().unwrap(),
            "docs (2).zip"
        );
    }

    #[test]
    fn get_zip_path_sanitizes_directory_name() {
        let dir = tempfile::TempDir::new().unwrap();
        let source = dir.path().join("my:project");
        std::fs::create_dir(&source).unwrap();

        let result = get_zip_path(&source);
        assert_eq!(
            result.file_name().unwrap().to_str().unwrap(),
            "my_project.zip"
        );
    }
}
