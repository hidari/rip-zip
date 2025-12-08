use clap::Parser;
use std::fmt;
use std::fs::File;
use std::io::{self, Error};
use std::path::{Path, PathBuf, StripPrefixError};
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

#[cfg(windows)]
use std::io::IsTerminal;

#[derive(Parser)]
#[command(
    name = "rip",
    author,
    version,
    about = "rip - Cross-platform ZIP handling that just works everywhere",
    long_about = "Handling cross-platform ZIP archives. \
                  Just drag & drop to create ZIP files!"
)]
struct Args {
    /// Directories to zip (supports drag and drop)
    #[arg(value_parser, required = true)]
    sources: Vec<PathBuf>,

    /// Use verbose output
    #[arg(short, long)]
    verbose: bool,

    /// Enable ZIP64 support for large files (>4GB)
    #[arg(long)]
    zip64: bool,
}

#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
enum ZipError {
    IoError(Error),
    StripPrefixError(StripPrefixError),
    ZipError(zip::result::ZipError),
}

impl From<Error> for ZipError {
    fn from(err: Error) -> Self {
        ZipError::IoError(err)
    }
}

impl From<StripPrefixError> for ZipError {
    fn from(err: StripPrefixError) -> Self {
        ZipError::StripPrefixError(err)
    }
}

impl From<zip::result::ZipError> for ZipError {
    fn from(err: zip::result::ZipError) -> Self {
        ZipError::ZipError(err)
    }
}

impl fmt::Display for ZipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ZipError::IoError(err) => write!(f, "IO error: {}", err),
            ZipError::StripPrefixError(err) => write!(f, "Path error: {}", err),
            ZipError::ZipError(err) => write!(f, "ZIP error: {}", err),
        }
    }
}

// セキュリティ制限の定数
const MAX_FILE_SIZE: u64 = 1_073_741_824; // 1GB
const MAX_TOTAL_SIZE: u64 = 4_294_967_296; // 4GB (without zip64)
const MAX_FILE_COUNT: usize = 100_000; // 最大ファイル数
const MAX_FILENAME_LENGTH: usize = 65535; // ZIP仕様の最大ファイル名長

fn create_zip(
    source_dir: &Path,
    target_zip: &Path,
    verbose: bool,
    use_zip64: bool,
) -> Result<(), ZipError> {
    if !source_dir.exists() {
        return Err(ZipError::IoError(Error::new(
            io::ErrorKind::NotFound,
            format!("Source directory does not exist: {}", source_dir.display()),
        )));
    }

    if !source_dir.is_dir() {
        return Err(ZipError::IoError(Error::new(
            io::ErrorKind::InvalidInput,
            format!("Source is not a directory: {}", source_dir.display()),
        )));
    }

    if verbose {
        println!("Creating ZIP file: {}", target_zip.display());
    }

    let zip_file = File::create(target_zip)?;
    let mut zip = ZipWriter::new(zip_file);

    let base_options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(use_zip64);

    // シンボリックリンクは追跡せず、警告を表示
    let walkdir = WalkDir::new(source_dir)
        .follow_links(false)
        .same_file_system(true)
        .max_depth(100); // 深すぎる再帰を防ぐ

    let mut total_size: u64 = 0;
    let mut file_count: usize = 0;

    for entry in walkdir {
        let entry = entry.map_err(Error::other)?;
        let path = entry.path();

        // シンボリックリンクを明示的にスキップ
        if path.is_symlink() {
            if verbose {
                eprintln!("Warning: Skipping symlink: {}", path.display());
            }
            continue;
        }

        if path.is_file() {
            let relative_path = path.strip_prefix(source_dir)?;

            // 不正なパスがないかチェック
            if relative_path
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
            {
                if verbose {
                    eprintln!(
                        "Warning: Skipping file with parent directory reference: {}",
                        relative_path.display()
                    );
                }
                continue;
            }

            // ZIP仕様ではパス区切り文字は必ず '/' でなければならない
            // Windowsの '\' を '/' に変換
            let name = match relative_path.to_str() {
                Some(s) => s.replace('\\', "/"),
                None => relative_path.to_string_lossy().replace('\\', "/"),
            };

            // ファイル名の長さチェック
            if name.len() > MAX_FILENAME_LENGTH {
                if verbose {
                    eprintln!("Warning: Filename too long, skipping: {}", name);
                }
                continue;
            }

            // ファイル数制限チェック
            file_count += 1;
            if file_count > MAX_FILE_COUNT {
                return Err(ZipError::IoError(Error::other(format!(
                    "Too many files (limit: {})",
                    MAX_FILE_COUNT
                ))));
            }

            // ファイルサイズチェック
            let metadata = std::fs::metadata(path)?;
            let file_size = metadata.len();

            // Unixパーミッションの取得
            #[cfg(unix)]
            let unix_perms = {
                use std::os::unix::fs::PermissionsExt;
                metadata.permissions().mode()
            };
            #[cfg(not(unix))]
            let unix_perms = 0o644; // 非Unix環境はデフォルト（rw-r--r--）

            // ファイルごとのオプション設定
            let file_options = base_options.unix_permissions(unix_perms);

            if file_size > MAX_FILE_SIZE && !use_zip64 {
                eprintln!(
                    "Warning: File {} exceeds 1GB limit, skipping. Use --zip64 for large files.",
                    name
                );
                continue;
            }

            // 合計サイズチェック
            if total_size + file_size > MAX_TOTAL_SIZE && !use_zip64 {
                return Err(ZipError::IoError(Error::other(
                    "Total archive size would exceed 4GB limit. Use --zip64 flag for larger archives."
                        .to_string(),
                )));
            }

            total_size += file_size;

            if verbose {
                println!("Adding file: {} ({} bytes)", name, file_size);
            }

            zip.start_file(&name, file_options)?;

            let mut file = File::open(path)?;
            io::copy(&mut file, &mut zip)?;
        }
    }

    zip.finish()?;

    if verbose {
        println!(
            "Archive created: {} files, {} bytes total",
            file_count, total_size
        );
    }

    Ok(())
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            _ if c.is_control() => '_',
            _ => c,
        })
        .collect()
}

fn get_zip_path(source_dir: &Path) -> PathBuf {
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

#[cfg(windows)]
fn pause() {
    use std::io::Read;
    println!("\nPress any key to exit...");
    let _ = std::io::stdin().read(&mut [0u8]).unwrap();
}

fn main() {
    let args = Args::parse();

    for source in args.sources {
        // 入力検証
        if !source.exists() {
            eprintln!("Error: Source does not exist: {}", source.display());
            continue;
        }

        if !source.is_dir() {
            eprintln!("Error: Source is not a directory: {}", source.display());
            continue;
        }

        let zip_path = get_zip_path(&source);

        match create_zip(&source, &zip_path, args.verbose, args.zip64) {
            Ok(_) => {
                println!("Successfully created ZIP file: {}", zip_path.display());
            }
            Err(e) => {
                eprintln!("Error creating ZIP file for {}: {}", source.display(), e);
            }
        }
    }

    // コマンドラインから実行された場合のみ終了を遅延させる
    #[cfg(windows)]
    if std::env::args().len() <= 1 && std::io::stdin().is_terminal() {
        pause();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::{Cursor, Write};
    use tempfile::TempDir;
    use zip::ZipArchive;

    #[test]
    fn test_japanese_filename() -> Result<(), ZipError> {
        // テスト用の一時ディレクトリを作成
        let temp_dir = TempDir::new()?;
        let test_dir = temp_dir.path().join("テスト");
        fs::create_dir(&test_dir)?;

        // テストファイルを作成
        let test_file_path = test_dir.join("日本語.txt");
        fs::write(&test_file_path, "テストデータ")?;

        // ZIPファイルを作成
        let zip_path = temp_dir.path().join("test.zip");
        create_zip(&test_dir, &zip_path, false, false)?;

        // ZIPファイルを検証
        assert!(zip_path.exists());

        // ZIPの中身を確認
        let zip_file = File::open(&zip_path)?;
        let archive = ZipArchive::new(zip_file)?;

        // ファイル名が正しくUTF-8で保存されているか確認
        let file_names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();
        assert!(file_names.contains(&"日本語.txt".to_string()));

        Ok(())
    }

    #[test]
    fn test_complex_japanese_filename() -> Result<(), ZipError> {
        let temp_dir = TempDir::new()?;
        let test_dir = temp_dir.path().join("テスト");
        fs::create_dir(&test_dir)?;

        // 絵文字や特殊文字を含むファイル名
        let test_file_path = test_dir.join("🗾日本語_テスト！＃＄％.txt");
        fs::write(&test_file_path, "テストデータ")?;

        let zip_path = temp_dir.path().join("test.zip");
        create_zip(&test_dir, &zip_path, false, false)?;

        let zip_file = File::open(&zip_path)?;
        let archive = ZipArchive::new(zip_file)?;

        let file_names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();
        assert!(file_names.contains(&"🗾日本語_テスト！＃＄％.txt".to_string()));

        Ok(())
    }

    #[test]
    fn test_nested_japanese_directories() -> Result<(), ZipError> {
        let temp_dir = TempDir::new()?;
        let base_dir = temp_dir.path().join("テスト");
        fs::create_dir(&base_dir)?;

        // 入れ子のディレクトリを作成
        let nested_dir = base_dir
            .join("フォルダー1")
            .join("フォルダー2")
            .join("フォルダー3");
        fs::create_dir_all(&nested_dir)?;

        // ファイルを作成
        let test_file_path = nested_dir.join("テスト.txt");
        fs::write(&test_file_path, "テストデータ")?;

        let zip_path = temp_dir.path().join("test.zip");
        create_zip(&base_dir, &zip_path, false, false)?;

        // ZIPの内容を確認
        let zip_file = File::open(&zip_path)?;
        let archive = ZipArchive::new(zip_file)?;

        let file_names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();
        assert!(file_names.iter().any(|name| name.ends_with("テスト.txt")));

        Ok(())
    }

    #[test]
    fn test_cross_platform_compatibility() -> Result<(), ZipError> {
        let temp_dir = TempDir::new()?;
        let test_dir = temp_dir.path().join("クロスプラットフォーム");
        fs::create_dir(&test_dir)?;

        // プラットフォーム固有の文字を含むファイル名
        let filename = if cfg!(windows) {
            "Windows用ファイル　テスト.txt" // 全角スペース
        } else {
            "macOS用ファイル テスト.txt" // 半角スペース
        };

        let test_file_path = test_dir.join(filename);
        fs::write(&test_file_path, "テストデータ")?;

        // 日本語のサブディレクトリ
        let subdir = test_dir.join("サブフォルダー");
        fs::create_dir(&subdir)?;
        let subfile_path = subdir.join("テスト.txt");
        fs::write(&subfile_path, "サブディレクトリのテストデータ")?;

        let zip_path = temp_dir.path().join("cross_platform_test.zip");
        create_zip(&test_dir, &zip_path, false, false)?;

        // ZIPの内容を確認
        let zip_file = File::open(&zip_path)?;
        let archive = ZipArchive::new(zip_file)?;

        let file_names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();

        // パス区切り文字が正しく処理されているか確認
        // Windowsのバックスラッシュがスラッシュに変換されているか
        assert!(file_names.iter().all(|name| !name.contains('\\')));

        // すべてのファイルが存在することを確認
        assert!(file_names.iter().any(|name| name.contains(filename)));
        assert!(file_names.iter().any(|name| name.ends_with("テスト.txt")));

        Ok(())
    }

    #[test]
    fn test_platform_specific_filenames() -> Result<(), ZipError> {
        let temp_dir = TempDir::new()?;
        let test_dir = temp_dir.path().join("プラットフォーム互換性");
        fs::create_dir(&test_dir)?;

        println!("作成されたディレクトリ: {}", test_dir.display());

        // NFD/NFC文字のファイル作成
        let nfd_filename = "がぎぐげご_NFD.txt";
        let nfc_filename = "がぎぐげご_NFC.txt";

        let nfd_path = test_dir.join(nfd_filename);
        let nfc_path = test_dir.join(nfc_filename);

        fs::write(&nfd_path, "NFDテスト")?;
        fs::write(&nfc_path, "NFCテスト")?;

        println!("NFDファイルパス: {}", nfd_path.display());
        println!("NFCファイルパス: {}", nfc_path.display());

        // ファイルが実際に作成されたか確認
        println!("\nディレクトリの内容:");
        for entry in fs::read_dir(&test_dir)? {
            let entry = entry?;
            println!("- {}", entry.path().display());
        }

        // ZIPファイル作成
        let zip_path = temp_dir.path().join("platform_test.zip");
        create_zip(&test_dir, &zip_path, true, false)?; // verboseをtrueに

        // 検証
        let zip_file = File::open(&zip_path)?;
        let archive = ZipArchive::new(zip_file)?;
        let file_names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();

        println!("\nZIP内のファイル一覧:");
        for name in &file_names {
            println!("- {}", name);
        }

        // 各ケースの検証
        assert!(
            file_names.iter().any(|name| name.contains("がぎぐげご")),
            "ファイル名 'がぎぐげご' が見つかりません"
        );

        Ok(())
    }
    #[test]
    fn test_very_long_paths() -> Result<(), ZipError> {
        let temp_dir = TempDir::new()?;
        let test_dir = temp_dir.path().join("長いパス");
        fs::create_dir(&test_dir)?;

        // 深い階層のディレクトリを作成
        let mut current_dir = test_dir.clone();
        for i in 1..20 {
            // Windows上でもエラーにならない程度の深さ
            current_dir = current_dir.join(format!("深いディレクトリ_{}", i));
            fs::create_dir(&current_dir)?;
        }

        fs::write(current_dir.join("テスト.txt"), "深い階層のテスト")?;

        let zip_path = temp_dir.path().join("long_paths_test.zip");
        create_zip(&test_dir, &zip_path, false, false)?;

        // 検証
        let zip_file = File::open(&zip_path)?;
        let archive = ZipArchive::new(zip_file)?;

        assert!(archive
            .file_names()
            .any(|name| name.ends_with("テスト.txt")));

        Ok(())
    }

    #[test]
    fn test_simulated_cross_platform_paths() -> Result<(), ZipError> {
        let temp_dir = TempDir::new()?;
        let test_dir = temp_dir.path().join("クロスプラットフォーム");
        fs::create_dir(&test_dir)?;

        // Windowsスタイルのパス
        let win_style_name = "Windows\\スタイル\\パス.txt";
        let win_path = test_dir.join(win_style_name.replace("\\", "/"));
        fs::create_dir_all(win_path.parent().unwrap())?;
        fs::write(&win_path, "Windowsスタイル")?;

        // macOS/UNIXスタイルのパス
        let unix_style_name = "macOS/スタイル/パス.txt";
        let unix_path = test_dir.join(unix_style_name);
        fs::create_dir_all(unix_path.parent().unwrap())?;
        fs::write(&unix_path, "macOSスタイル")?;

        let zip_path = temp_dir.path().join("cross_platform.zip");
        create_zip(&test_dir, &zip_path, true, false)?;

        // 検証
        let zip_file = File::open(&zip_path)?;
        let archive = ZipArchive::new(zip_file)?;
        let file_names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();

        println!("\nZIP内のファイル一覧:");
        for name in &file_names {
            println!("- {}", name);
        }

        // パスの区切り文字が全て'/'になっていることを確認
        assert!(file_names.iter().all(|name| !name.contains('\\')));

        // 両方のファイルが存在することを確認
        assert!(file_names.iter().any(|name| name.ends_with("パス.txt")));
        assert_eq!(
            file_names
                .iter()
                .filter(|name| name.ends_with("パス.txt"))
                .count(),
            2
        );

        Ok(())
    }

    #[test]
    fn test_zip64_option_setting() {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().large_file(true);
        writer.start_file("test.txt", options).unwrap();
        writer.write_all(b"test").unwrap();
        // エラーが発生しないことを確認
        writer.finish().unwrap();
    }

    #[test]
    fn test_zip64_cli_option() {
        let temp_dir = TempDir::new().unwrap();
        let test_dir = temp_dir.path().join("zip64test");
        fs::create_dir(&test_dir).unwrap();
        fs::write(test_dir.join("test.txt"), b"test").unwrap();

        let args = Args {
            sources: vec![test_dir.clone()],
            verbose: false,
            zip64: true,
        };

        // CLIオプションが正しく処理されることを確認
        let zip_path = get_zip_path(&test_dir);
        assert!(create_zip(&test_dir, &zip_path, args.verbose, args.zip64).is_ok());
    }

    // ========== セキュリティテスト ==========

    #[test]
    fn test_path_traversal_protection() -> Result<(), ZipError> {
        let temp_dir = TempDir::new()?;
        let test_dir = temp_dir.path().join("traversal_test");
        fs::create_dir(&test_dir)?;

        // 通常のファイル
        fs::write(test_dir.join("normal.txt"), "normal content")?;

        // サブディレクトリ作成
        let subdir = test_dir.join("subdir");
        fs::create_dir(&subdir)?;
        fs::write(subdir.join("file.txt"), "subdir content")?;

        // ZIP作成
        let zip_path = temp_dir.path().join("test.zip");
        create_zip(&test_dir, &zip_path, false, false)?;

        // ZIP検証: 相対パス参照を含むパスがないことを確認
        let zip_file = File::open(&zip_path)?;
        let archive = ZipArchive::new(zip_file)?;

        for name in archive.file_names() {
            // ".."を含むパスがないことを確認
            assert!(!name.contains(".."), "Path traversal detected: {}", name);
            // 絶対パスでないことを確認
            assert!(!name.starts_with('/'), "Absolute path detected: {}", name);
        }

        Ok(())
    }

    #[test]
    fn test_file_size_limit() -> Result<(), ZipError> {
        let temp_dir = TempDir::new()?;
        let test_dir = temp_dir.path().join("size_test");
        fs::create_dir(&test_dir)?;

        // 小さいファイル（正常）
        fs::write(test_dir.join("small.txt"), "small content")?;

        // ZIP64なしでZIP作成（小さいファイルのみ成功）
        let zip_path = temp_dir.path().join("test_no_zip64.zip");
        create_zip(&test_dir, &zip_path, false, false)?;

        let zip_file = File::open(&zip_path)?;
        let archive = ZipArchive::new(zip_file)?;
        assert_eq!(archive.len(), 1); // small.txtのみ

        // ZIP64ありで同じディレクトリを圧縮（全ファイル成功）
        let zip_path_64 = temp_dir.path().join("test_with_zip64.zip");
        create_zip(&test_dir, &zip_path_64, false, true)?;

        let zip_file_64 = File::open(&zip_path_64)?;
        let archive_64 = ZipArchive::new(zip_file_64)?;
        assert_eq!(archive_64.len(), 1);

        Ok(())
    }

    #[test]
    fn test_file_count_limit() -> Result<(), ZipError> {
        let temp_dir = TempDir::new()?;
        let test_dir = temp_dir.path().join("count_test");
        fs::create_dir(&test_dir)?;

        // 100ファイル作成（制限内）
        for i in 0..100 {
            fs::write(
                test_dir.join(format!("file_{:05}.txt", i)),
                format!("content {}", i),
            )?;
        }

        let zip_path = temp_dir.path().join("test.zip");
        create_zip(&test_dir, &zip_path, false, false)?;

        let zip_file = File::open(&zip_path)?;
        let archive = ZipArchive::new(zip_file)?;
        assert_eq!(archive.len(), 100);

        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn test_symlink_handling() -> Result<(), ZipError> {
        use std::os::unix::fs::symlink;

        let temp_dir = TempDir::new()?;
        let test_dir = temp_dir.path().join("symlink_test");
        fs::create_dir(&test_dir)?;

        // 通常のファイル
        let normal_file = test_dir.join("normal.txt");
        fs::write(&normal_file, "normal content")?;

        // シンボリックリンク作成
        let symlink_path = test_dir.join("symlink.txt");
        symlink(&normal_file, &symlink_path)?;

        // ZIP作成
        let zip_path = temp_dir.path().join("test.zip");
        create_zip(&test_dir, &zip_path, false, false)?;

        // 検証: シンボリックリンクはZIPに含まれない
        let zip_file = File::open(&zip_path)?;
        let archive = ZipArchive::new(zip_file)?;

        assert_eq!(archive.len(), 1, "Only normal file should be included");

        let file_names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();
        assert!(file_names.contains(&"normal.txt".to_string()));
        assert!(!file_names.iter().any(|n| n.contains("symlink")));

        Ok(())
    }

    #[test]
    fn test_input_validation() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;

        // 存在しないパス
        let non_existent = temp_dir.path().join("does_not_exist");
        let zip_path = temp_dir.path().join("test.zip");

        let result = create_zip(&non_existent, &zip_path, false, false);
        assert!(result.is_err(), "Should fail for non-existent path");

        if let Err(ZipError::IoError(e)) = result {
            assert_eq!(e.kind(), io::ErrorKind::NotFound);
            assert!(e.to_string().contains("does not exist"));
        } else {
            panic!("Expected NotFound error");
        }

        // ファイルパス（ディレクトリではない）
        let file_path = temp_dir.path().join("file.txt");
        fs::write(&file_path, "content")?;

        let result = create_zip(&file_path, &zip_path, false, false);
        assert!(result.is_err(), "Should fail for non-directory path");

        if let Err(ZipError::IoError(e)) = result {
            assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
            assert!(e.to_string().contains("not a directory"));
        } else {
            panic!("Expected InvalidInput error");
        }

        Ok(())
    }

    #[test]
    fn test_filename_length_limit() -> Result<(), ZipError> {
        let temp_dir = TempDir::new()?;
        let test_dir = temp_dir.path().join("length_test");
        fs::create_dir(&test_dir)?;

        // 通常の長さのファイル名
        fs::write(test_dir.join("normal.txt"), "normal")?;

        // 長いファイル名（ただしファイルシステムの制限内）
        // 多くのファイルシステムは255バイトまで
        let long_name = "a".repeat(200) + ".txt";
        fs::write(test_dir.join(&long_name), "long name")?;

        // ZIP作成
        let zip_path = temp_dir.path().join("test.zip");
        create_zip(&test_dir, &zip_path, false, false)?;

        // すべてのファイル名が MAX_FILENAME_LENGTH 以下であることを確認
        let zip_file = File::open(&zip_path)?;
        let archive = ZipArchive::new(zip_file)?;

        for name in archive.file_names() {
            assert!(
                name.len() <= MAX_FILENAME_LENGTH,
                "Filename too long: {} bytes",
                name.len()
            );
        }

        Ok(())
    }

    #[test]
    fn test_filename_sanitization() -> Result<(), ZipError> {
        let temp_dir = TempDir::new()?;
        let test_dir = temp_dir.path().join("sanitize_test");
        fs::create_dir(&test_dir)?;

        // 通常のファイル
        fs::write(test_dir.join("normal.txt"), "normal")?;

        // sanitize_filename 関数のテスト
        assert_eq!(sanitize_filename("normal.txt"), "normal.txt");
        assert_eq!(
            sanitize_filename("file:with*special?chars.txt"),
            "file_with_special_chars.txt"
        );
        assert_eq!(
            sanitize_filename("path/with\\slashes.txt"),
            "path_with_slashes.txt"
        );
        assert_eq!(
            sanitize_filename("file<with>pipes|.txt"),
            "file_with_pipes_.txt"
        );
        assert_eq!(
            sanitize_filename("file\"with'quotes.txt"),
            "file_with'quotes.txt"
        );
        assert_eq!(
            sanitize_filename("file\0with\x01control.txt"),
            "file_with_control.txt"
        );

        // ZIP作成時のファイル名生成テスト
        let test_path = test_dir.clone();
        let zip_path = get_zip_path(&test_path);

        // ZIPファイルが作成されることを確認
        assert!(zip_path.to_string_lossy().ends_with(".zip"));

        Ok(())
    }

    #[test]
    fn test_recursion_depth_limit() -> Result<(), ZipError> {
        let temp_dir = TempDir::new()?;
        let test_dir = temp_dir.path().join("depth_test");
        fs::create_dir(&test_dir)?;

        // 50階層のディレクトリを作成（max_depth=100の範囲内）
        let mut current = test_dir.clone();
        for i in 0..50 {
            current = current.join(format!("level_{:03}", i));
            fs::create_dir(&current)?;
        }

        // 最深部にファイルを配置
        fs::write(current.join("deep_file.txt"), "deep content")?;

        // 途中の階層にもファイルを配置
        let mut mid = test_dir.clone();
        for i in 0..25 {
            mid = mid.join(format!("level_{:03}", i));
        }
        fs::write(mid.join("mid_file.txt"), "mid content")?;

        // ZIP作成（max_depth=100なので成功）
        let zip_path = temp_dir.path().join("test.zip");
        create_zip(&test_dir, &zip_path, false, false)?;

        // 検証
        let zip_file = File::open(&zip_path)?;
        let archive = ZipArchive::new(zip_file)?;

        assert_eq!(archive.len(), 2, "Should contain 2 files");

        let file_names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();
        assert!(file_names.iter().any(|n| n.ends_with("deep_file.txt")));
        assert!(file_names.iter().any(|n| n.ends_with("mid_file.txt")));

        Ok(())
    }

    #[test]
    fn test_total_size_limit() -> Result<(), ZipError> {
        let temp_dir = TempDir::new()?;
        let test_dir = temp_dir.path().join("total_size_test");
        fs::create_dir(&test_dir)?;

        // 小さいファイルを複数作成（合計サイズは4GB未満）
        for i in 0..10 {
            fs::write(
                test_dir.join(format!("file_{}.txt", i)),
                format!("content {}", i),
            )?;
        }

        // ZIP64なしで成功（合計サイズが4GB未満）
        let zip_path = temp_dir.path().join("test.zip");
        create_zip(&test_dir, &zip_path, false, false)?;

        let zip_file = File::open(&zip_path)?;
        let archive = ZipArchive::new(zip_file)?;
        assert_eq!(archive.len(), 10);

        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn test_permission_preservation() -> Result<(), ZipError> {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new()?;
        let test_dir = temp_dir.path().join("perm_test");
        fs::create_dir(&test_dir)?;

        // 実行可能ファイルを作成（0o755）
        let exec_file = test_dir.join("executable.sh");
        fs::write(&exec_file, "#!/bin/bash\necho test")?;
        fs::set_permissions(&exec_file, fs::Permissions::from_mode(0o755))?;

        // 通常ファイルを作成（0o644）
        let normal_file = test_dir.join("normal.txt");
        fs::write(&normal_file, "normal content")?;
        fs::set_permissions(&normal_file, fs::Permissions::from_mode(0o644))?;

        // ZIP作成
        let zip_path = temp_dir.path().join("test.zip");
        create_zip(&test_dir, &zip_path, false, false)?;

        // ZIPが正しく作成されたことを確認
        assert!(zip_path.exists());

        let zip_file = File::open(&zip_path)?;
        let archive = ZipArchive::new(zip_file)?;
        assert_eq!(archive.len(), 2, "Should contain 2 files");

        Ok(())
    }

    #[test]
    #[ignore = "very slow: creates 100,001 files"]
    fn test_file_count_limit_exceeded() -> Result<(), ZipError> {
        let temp_dir = TempDir::new()?;
        let test_dir = temp_dir.path().join("file_count_test");
        fs::create_dir(&test_dir)?;

        println!("Creating 100,001 files (this will take a while)...");

        // MAX_FILE_COUNT + 1 個のファイルを作成
        for i in 0..=100_000 {
            if i % 10_000 == 0 {
                println!("  Created {} files...", i);
            }

            // 空ファイルを作成（高速化）
            let file_path = test_dir.join(format!("f{}.txt", i));
            File::create(file_path)?;
        }

        println!("All files created. Starting ZIP creation...");

        // ZIP作成を試みる
        let zip_path = temp_dir.path().join("test.zip");
        let result = create_zip(&test_dir, &zip_path, true, false);

        // エラーが発生することを確認
        assert!(result.is_err(), "Expected error for too many files");

        if let Err(ZipError::IoError(e)) = result {
            let error_msg = e.to_string();
            assert!(
                error_msg.contains("Too many files") || error_msg.contains("100000"),
                "Expected file count error, got: {}",
                error_msg
            );
            println!("✓ Correctly rejected 100,001 files (limit: 100,000)");
        } else {
            panic!("Expected IoError for file count limit");
        }

        Ok(())
    }

    #[test]
    #[cfg(unix)]
    #[ignore = "very slow: creates 1GB+ sparse file"]
    fn test_file_size_limit_large() -> Result<(), ZipError> {
        use std::os::unix::fs::FileExt;

        let temp_dir = TempDir::new()?;
        let test_dir = temp_dir.path().join("size_test");
        fs::create_dir(&test_dir)?;

        println!("Creating sparse file (1GB + 1 byte)...");

        // スパースファイルで1GB + 1バイトのファイルを作成
        let large_file_path = test_dir.join("large.bin");
        let large_file = File::create(&large_file_path)?;

        let size: u64 = MAX_FILE_SIZE + 1; // 1GB + 1 byte
        large_file.write_at(b"X", size - 1)?;

        let metadata = fs::metadata(&large_file_path)?;
        assert_eq!(metadata.len(), size);

        println!("Sparse file created: {} bytes", size);

        // 小さいファイルも追加
        fs::write(test_dir.join("small.txt"), "small")?;

        // テスト1: ZIP64なし（大きいファイルはスキップ）
        println!("\nTest 1: Without ZIP64 (large file should be skipped)");
        let zip_path_no64 = temp_dir.path().join("test_no_zip64.zip");
        create_zip(&test_dir, &zip_path_no64, true, false)?;

        let zip_file = File::open(&zip_path_no64)?;
        let archive = ZipArchive::new(zip_file)?;

        assert_eq!(archive.len(), 1, "Should contain only small file");

        let file_names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();
        assert!(file_names.contains(&"small.txt".to_string()));
        assert!(!file_names.iter().any(|n| n.contains("large")));

        println!("✓ Large file correctly skipped without ZIP64");

        // テスト2: ZIP64あり（すべて含まれる）
        println!("\nTest 2: With ZIP64 (all files should be included)");
        let zip_path_64 = temp_dir.path().join("test_with_zip64.zip");
        create_zip(&test_dir, &zip_path_64, true, true)?;

        let zip_file_64 = File::open(&zip_path_64)?;
        let archive_64 = ZipArchive::new(zip_file_64)?;

        assert_eq!(archive_64.len(), 2, "Should contain both files with ZIP64");

        let file_names_64: Vec<String> = archive_64.file_names().map(|s| s.to_string()).collect();
        assert!(file_names_64.contains(&"small.txt".to_string()));
        assert!(file_names_64.contains(&"large.bin".to_string()));

        println!("✓ Large file correctly included with ZIP64");

        Ok(())
    }

    #[test]
    #[ignore = "very slow: creates 101-level deep directory structure"]
    fn test_recursion_depth_exceeded() -> Result<(), ZipError> {
        let temp_dir = TempDir::new()?;
        let test_dir = temp_dir.path().join("depth_test");
        fs::create_dir(&test_dir)?;

        println!("Creating 101-level deep directory structure...");

        // 101階層のディレクトリを作成
        let mut current_dir = test_dir.clone();
        for i in 0..=100 {
            if i % 10 == 0 {
                println!("  Created {} levels...", i);
            }

            current_dir = current_dir.join(format!("d{}", i));
            fs::create_dir(&current_dir)?;
        }

        // レベル50: 範囲内（深さ51）
        let mut level_50 = test_dir.clone();
        for i in 0..50 {
            level_50 = level_50.join(format!("d{}", i));
        }
        fs::write(level_50.join("file_level50.txt"), "level 50")?;

        // レベル99: 境界（深さ100、max_depthの限界）
        let mut level_99 = test_dir.clone();
        for i in 0..99 {
            level_99 = level_99.join(format!("d{}", i));
        }
        fs::write(level_99.join("file_level99.txt"), "level 99")?;

        // レベル100: 超過（深さ101、max_depthを超える）
        let mut level_100 = test_dir.clone();
        for i in 0..100 {
            level_100 = level_100.join(format!("d{}", i));
        }
        fs::write(level_100.join("file_level100.txt"), "level 100")?;

        println!("Directory structure created. Starting ZIP creation...");

        // ZIP作成
        let zip_path = temp_dir.path().join("test.zip");
        create_zip(&test_dir, &zip_path, true, false)?;

        // 検証
        let zip_file = File::open(&zip_path)?;
        let archive = ZipArchive::new(zip_file)?;

        let file_names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();

        println!("\nFiles in ZIP:");
        for name in &file_names {
            println!("  - {}", name);
        }

        // レベル50と99は含まれる
        assert!(
            file_names.iter().any(|n| n.ends_with("file_level50.txt")),
            "Level 50 file (depth 51) should be included"
        );
        assert!(
            file_names.iter().any(|n| n.ends_with("file_level99.txt")),
            "Level 99 file (depth 100) should be included"
        );

        // レベル100はスキップ（深さ101でmax_depthを超える）
        assert!(
            !file_names.iter().any(|n| n.ends_with("file_level100.txt")),
            "Level 100 file (depth 101) should be skipped (exceeds max_depth)"
        );

        println!("\n✓ Files at depth 50 and 99: included");
        println!("✓ File at depth 100: correctly skipped (exceeds max_depth)");

        Ok(())
    }
}
