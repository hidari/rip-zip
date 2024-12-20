use std::fs::File;
use std::io::{self, Write};
use zip::{write::FileOptions, ZipWriter};
use walkdir::WalkDir;
use std::path::{Path, PathBuf};
use clap::Parser;

const EFS_FLAG: u16 = 0x0800;
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

    let options = FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755)
        .general_purpose_flags(EFS_FLAG);

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

            let name = relative_path.to_string_lossy();

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
    // 安全でない文字を削除または置換
    name.chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ if c.is_control() => '_',
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
                eprintln!("Error creating ZIP file for {}: {:?}", source.display(), e);
            }
        }
    }

    // コマンドラインから実行された場合のみ終了を遅延させる
    #[cfg(windows)]
    if std::env::args().len() <= 1 && atty::is(atty::Stream::Stdin) {
        pause();
    }
}