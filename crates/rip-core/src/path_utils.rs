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
    if zip_path.exists() {
        let base = zip_path.clone();
        let mut counter = 1;
        while zip_path.exists() {
            zip_path = base.with_file_name(format!("{} ({}).zip", safe_name, counter));
            counter += 1;
        }
    }

    zip_path
}

#[cfg(test)]
mod tests {
    use super::*;

    mod sanitize_filename {
        use super::*;

        #[test]
        fn replaces_each_dangerous_character_with_underscore() {
            // 各危険文字が個別にアンダースコアへ置換されることを検証
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
        fn replaces_control_characters_with_underscore() {
            // ASCII制御文字がアンダースコアへ置換されることを検証
            assert_eq!(sanitize_filename("file\x01name"), "file_name");
            assert_eq!(sanitize_filename("file\x1fname"), "file_name");
        }

        #[test]
        fn preserves_normal_and_unicode_characters() {
            // 通常のASCII文字およびUnicode文字はそのまま保持される
            assert_eq!(sanitize_filename("normal_file.txt"), "normal_file.txt");
            assert_eq!(sanitize_filename("日本語ファイル"), "日本語ファイル");
            assert_eq!(sanitize_filename("file-name.rs"), "file-name.rs");
        }

        #[test]
        fn returns_empty_string_for_empty_input() {
            assert_eq!(sanitize_filename(""), "");
        }

        #[test]
        fn replaces_multiple_dangerous_chars_independently() {
            // 複数の危険文字が混在する場合、それぞれ独立に置換される
            assert_eq!(sanitize_filename("a\\b/c:d"), "a_b_c_d");
        }
    }

    mod get_zip_path {
        use super::*;

        #[test]
        fn appends_zip_extension_to_directory_name() {
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
        fn appends_counter_1_when_zip_already_exists() {
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
        fn increments_counter_for_multiple_existing_zips() {
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
        #[cfg(unix)]
        fn sanitizes_dangerous_chars_in_directory_name() {
            // Windowsでは `:` がファイル名に使えないためUnix限定
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
}
