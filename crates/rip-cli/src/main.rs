use std::path::PathBuf;

use clap::Parser;

use rip_adapters::walkdir_walker::WalkDirWalker;
use rip_adapters::zip_archiver::ZipWriterArchiver;
use rip_core::path_utils::get_zip_path;
use rip_core::types::ZipEvent;
use rip_core::zip_creator;

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

    /// Allow large files by lifting size restrictions (>1GB per file, >4GB total)
    #[arg(long)]
    zip64: bool,
}

fn main() {
    let args = Args::parse();

    for source in &args.sources {
        let zip_path = get_zip_path(source);

        let walker = WalkDirWalker;
        let mut archiver = ZipWriterArchiver::new();

        let verbose = args.verbose;
        let on_event = move |event: ZipEvent| {
            handle_event(event, verbose);
        };

        match zip_creator::create_zip(
            &walker,
            &mut archiver,
            source,
            &zip_path,
            args.zip64,
            &on_event,
        ) {
            Ok(_) => {
                println!("Successfully created ZIP file: {}", zip_path.display());
            }
            Err(e) => {
                // 不完全なZIPファイルがディスクに残らないようにクリーンアップ
                let _ = std::fs::remove_file(&zip_path);
                eprintln!("Error creating ZIP file for {}: {}", source.display(), e);
            }
        }
    }

    // コマンドラインから実行された場合のみ終了を遅延させる
    #[cfg(windows)]
    if std::env::args().len() <= 1 {
        use rip_adapters::terminal::IsTerminalAdapter;
        use rip_core::traits::Terminal;
        let terminal = IsTerminalAdapter;
        if terminal.is_stdin_terminal() {
            pause();
        }
    }
}

/// ZipEventを受け取り、適切な出力を行う
fn handle_event(event: ZipEvent, verbose: bool) {
    match event {
        ZipEvent::ArchiveStarted { target } => {
            if verbose {
                println!("Creating ZIP file: {}", target.display());
            }
        }
        ZipEvent::FileAdded { name, size } => {
            if verbose {
                println!("Adding file: {} ({} bytes)", name, size);
            }
        }
        ZipEvent::SymlinkSkipped { path } => {
            if verbose {
                eprintln!("Warning: Skipping symlink: {}", path.display());
            }
        }
        ZipEvent::FileSkipped { name, reason } => {
            // サイズ制限超過は常に表示（ユーザーが--zip64の使用を検討できるように）
            // それ以外はverbose時のみ表示
            if reason.is_always_visible() || verbose {
                let hint = if reason.is_always_visible() {
                    ". Use --zip64 for large files"
                } else {
                    ""
                };
                eprintln!("Warning: Skipping {}: {}{}", name, reason, hint);
            }
        }
        ZipEvent::ArchiveCompleted { stats } => {
            if verbose {
                println!(
                    "Archive created: {} files, {} bytes total",
                    stats.file_count, stats.total_size
                );
            }
        }
        ZipEvent::PathSanitized {
            original,
            sanitized,
        } => {
            eprintln!(
                "Warning: Sanitized entry path: '{}' -> '{}'",
                original, sanitized
            );
        }
    }
}

#[cfg(windows)]
fn pause() {
    use std::io::Read;
    println!("\nPress any key to exit...");
    let _ = std::io::stdin().read(&mut [0u8]).unwrap();
}
