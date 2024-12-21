use std::fs::File;
use std::io::{self};
use zip::{ZipWriter};
use zip::write::SimpleFileOptions;
use walkdir::WalkDir;
use std::path::{Path, PathBuf};
use clap::Parser;
use std::fmt;

const MAX_FILE_SIZE: u64 = 1024 * 1024 * 1024;  // 1GB
const MAX_TOTAL_SIZE: u64 = 4 * 1024 * 1024 * 1024;  // 4GB

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
}

#[derive(Debug)]
enum ZipError {
    IoError(io::Error),
    StripPrefixError(std::path::StripPrefixError),
    ZipError(zip::result::ZipError),
}

impl From<io::Error> for ZipError {
    fn from(err: io::Error) -> Self {
        ZipError::IoError(err)
    }
}

impl From<std::path::StripPrefixError> for ZipError {
    fn from(err: std::path::StripPrefixError) -> Self {
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

fn create_zip(source_dir: &Path, target_zip: &Path, verbose: bool) -> Result<(), ZipError> {
    if !source_dir.exists() {
        return Err(ZipError::IoError(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Source directory does not exist: {}", source_dir.display())
        )));
    }

    if verbose {
        println!("Creating ZIP file: {}", target_zip.display());
    }

    let zip_file = File::create(target_zip)?;
    let mut zip = ZipWriter::new(zip_file);

    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);

    // シンボリックリンクは追跡せず、警告を表示
    let walkdir = WalkDir::new(source_dir)
        .follow_links(false)
        .same_file_system(true)
        .max_depth(100);  // 深すぎる再帰を防ぐ

    let mut total_size: u64 = 0;
    for entry in walkdir {
        let entry = entry.map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let path = entry.path();

        if path.is_file() {
            let relative_path = path.strip_prefix(source_dir)?;

            // 不正なパスがないかチェック
            if relative_path.components().any(|component|
                matches!(component, std::path::Component::ParentDir)
            ) {
                continue;
            }

            let name = match relative_path.to_str() {
                Some(s) => s.to_string(),
                None => relative_path.to_string_lossy().into_owned(),
            };

            if verbose {
                println!("Adding file: {}", name);
            }

            zip.start_file(&name, options)?;

            let metadata = std::fs::metadata(path)?;
            let file_size = metadata.len();

            // ファイルサイズチェック
            if file_size > MAX_FILE_SIZE {
                eprintln!("Skipping file larger than 1GB: {}", name);
                continue;
            }

            total_size += file_size;
            if total_size > MAX_TOTAL_SIZE {
                return Err(ZipError::IoError(io::Error::new(
                    io::ErrorKind::Other,
                    "Total size exceeds 4GB limit"
                )));
            }

            let mut file = File::open(path)?;
            io::copy(&mut file, &mut zip)?;
        }
    }

    zip.finish()?;
    Ok(())
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            _ if c.is_control() || c == '/' || c == '\\' => '_',
            _ => c
        })
        .collect()
}

fn get_zip_path(source_dir: &Path) -> PathBuf {
    let dir_name = source_dir.file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("archive"))
        .to_string_lossy();
    let safe_name = sanitize_filename(&dir_name);

    let mut zip_path = source_dir.parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();

    zip_path.push(format!("{}.zip", safe_name));


    let mut zip_path = source_dir.parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();

    zip_path.push(format!("{}.zip", dir_name));

    // 同名のZIPファイルが存在する場合は連番を付ける
    let mut counter = 1;
    let original_zip_path = zip_path.clone();
    while zip_path.exists() {
        zip_path = original_zip_path.with_file_name(
            format!("{} ({}).zip", safe_name, counter)
        );
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
        let zip_path = get_zip_path(&source);

        match create_zip(&source, &zip_path, args.verbose) {
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
    if std::env::args().len() <= 1 && atty::is(atty::Stream::Stdin) {
        pause();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_japanese_filename() -> Result<(), ZipError> {
        // テスト用の一時ディレクトリを作成
        let temp_dir = TempDir::new().unwrap();
        let test_dir = temp_dir.path().join("テスト");
        fs::create_dir(&test_dir)?;

        // テストファイルを作成
        let test_file_path = test_dir.join("日本語.txt");
        fs::write(&test_file_path, "テストデータ")?;

        // ZIPファイルを作成
        let zip_path = temp_dir.path().join("test.zip");
        create_zip(&test_dir, &zip_path, false)?;

        // ZIPファイルを検証
        assert!(zip_path.exists());

        // ZIPの中身を確認
        let zip_file = fs::File::open(&zip_path)?;
        let archive = zip::ZipArchive::new(zip_file)?;

        // ファイル名が正しくUTF-8で保存されているか確認
        let file_names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();
        assert!(file_names.contains(&"日本語.txt".to_string()));

        Ok(())
    }

    #[test]
    fn test_complex_japanese_filename() -> Result<(), ZipError> {
        let temp_dir = TempDir::new().unwrap();
        let test_dir = temp_dir.path().join("テスト");
        fs::create_dir(&test_dir)?;

        // 絵文字や特殊文字を含むファイル名
        let test_file_path = test_dir.join("🗾日本語_テスト！＃＄％.txt");
        fs::write(&test_file_path, "テストデータ")?;

        let zip_path = temp_dir.path().join("test.zip");
        create_zip(&test_dir, &zip_path, false)?;

        let zip_file = fs::File::open(&zip_path)?;
        let archive = zip::ZipArchive::new(zip_file)?;

        let file_names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();
        assert!(file_names.contains(&"🗾日本語_テスト！＃＄％.txt".to_string()));

        Ok(())
    }

    #[test]
    fn test_nested_japanese_directories() -> Result<(), ZipError> {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path().join("テスト");
        fs::create_dir(&base_dir)?;

        // 入れ子のディレクトリを作成
        let nested_dir = base_dir.join("フォルダー1").join("フォルダー2").join("フォルダー3");
        fs::create_dir_all(&nested_dir)?;

        // ファイルを作成
        let test_file_path = nested_dir.join("テスト.txt");
        fs::write(&test_file_path, "テストデータ")?;

        let zip_path = temp_dir.path().join("test.zip");
        create_zip(&base_dir, &zip_path, false)?;

        // ZIPの内容を確認
        let zip_file = fs::File::open(&zip_path)?;
        let archive = zip::ZipArchive::new(zip_file)?;

        let file_names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();
        assert!(file_names.iter().any(|name| name.ends_with("テスト.txt")));

        Ok(())
    }

    #[test]
    fn test_cross_platform_compatibility() -> Result<(), ZipError> {
        let temp_dir = TempDir::new().unwrap();
        let test_dir = temp_dir.path().join("クロスプラットフォーム");
        fs::create_dir(&test_dir)?;

        // プラットフォーム固有の文字を含むファイル名
        let filename = if cfg!(windows) {
            "Windows用ファイル　テスト.txt"  // 全角スペース
        } else {
            "macOS用ファイル テスト.txt"    // 半角スペース
        };

        let test_file_path = test_dir.join(filename);
        fs::write(&test_file_path, "テストデータ")?;

        // 日本語のサブディレクトリ
        let subdir = test_dir.join("サブフォルダー");
        fs::create_dir(&subdir)?;
        let subfile_path = subdir.join("テスト.txt");
        fs::write(&subfile_path, "サブディレクトリのテストデータ")?;

        let zip_path = temp_dir.path().join("cross_platform_test.zip");
        create_zip(&test_dir, &zip_path, false)?;

        // ZIPの内容を確認
        let zip_file = fs::File::open(&zip_path)?;
        let archive = zip::ZipArchive::new(zip_file)?;

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
        let temp_dir = TempDir::new().unwrap();
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
        create_zip(&test_dir, &zip_path, true)?;  // verboseをtrueに

        // 検証
        let zip_file = fs::File::open(&zip_path)?;
        let archive = zip::ZipArchive::new(zip_file)?;
        let file_names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();

        println!("\nZIP内のファイル一覧:");
        for name in &file_names {
            println!("- {}", name);
        }

        // 各ケースの検証
        assert!(file_names.iter().any(|name| name.contains("がぎぐげご")),
                "ファイル名 'がぎぐげご' が見つかりません");

        Ok(())
    }
    #[test]
    fn test_very_long_paths() -> Result<(), ZipError> {
        let temp_dir = TempDir::new().unwrap();
        let test_dir = temp_dir.path().join("長いパス");
        fs::create_dir(&test_dir)?;

        // 深い階層のディレクトリを作成
        let mut current_dir = test_dir.clone();
        for i in 1..20 {  // Windows上でもエラーにならない程度の深さ
            current_dir = current_dir.join(format!("深いディレクトリ_{}", i));
            fs::create_dir(&current_dir)?;
        }

        fs::write(current_dir.join("テスト.txt"), "深い階層のテスト")?;

        let zip_path = temp_dir.path().join("long_paths_test.zip");
        create_zip(&test_dir, &zip_path, false)?;

        // 検証
        let zip_file = fs::File::open(&zip_path)?;
        let archive = zip::ZipArchive::new(zip_file)?;

        assert!(archive.file_names().any(|name| name.ends_with("テスト.txt")));

        Ok(())
    }

    #[test]
    fn test_simulated_cross_platform_paths() -> Result<(), ZipError> {
        let temp_dir = TempDir::new().unwrap();
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
        create_zip(&test_dir, &zip_path, true)?;

        // 検証
        let zip_file = fs::File::open(&zip_path)?;
        let archive = zip::ZipArchive::new(zip_file)?;
        let file_names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();

        println!("\nZIP内のファイル一覧:");
        for name in &file_names {
            println!("- {}", name);
        }

        // パスの区切り文字が全て'/'になっていることを確認
        assert!(file_names.iter().all(|name| !name.contains('\\')));

        // 両方のファイルが存在することを確認
        assert!(file_names.iter().any(|name| name.ends_with("パス.txt")));
        assert_eq!(file_names.iter().filter(|name| name.ends_with("パス.txt")).count(), 2);

        Ok(())
    }
}
