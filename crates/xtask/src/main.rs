use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let subcommand = args.first().map(String::as_str);

    match subcommand {
        Some("fmt") => run_fmt(),
        Some("test") => cargo(&[
            "test",
            "--workspace",
            "--all-features",
            "--exclude",
            "xtask",
        ]),
        Some("test-all") => cargo(&[
            "test",
            "--workspace",
            "--all-features",
            "--exclude",
            "xtask",
            "--",
            "--include-ignored",
        ]),
        Some("build") => cargo(&["build", "--workspace", "--exclude", "xtask"]),
        Some("check") => cargo(&[
            "check",
            "--workspace",
            "--all-features",
            "--exclude",
            "xtask",
        ]),
        Some("help") | Some("--help") | Some("-h") => {
            print_help();
            ExitCode::SUCCESS
        }
        Some(unknown) => {
            eprintln!("不明なコマンド: {unknown}");
            eprintln!();
            print_help();
            ExitCode::FAILURE
        }
        None => {
            print_help();
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    eprintln!("rip-zip 開発タスクランナー");
    eprintln!();
    eprintln!("使い方: cargo xtask <COMMAND>");
    eprintln!();
    eprintln!("コマンド:");
    eprintln!("  fmt       コード整形 + Lint チェック");
    eprintln!("  test      ユニットテスト実行");
    eprintln!("  test-all  全テスト実行（#[ignore] 付きテスト含む）");
    eprintln!("  build     ワークスペース全体をビルド");
    eprintln!("  check     型チェック");
    eprintln!("  help      このヘルプを表示");
}

// コード整形 + Lint チェック
fn run_fmt() -> ExitCode {
    if !run_cargo(&["fmt", "--all"]) {
        return ExitCode::FAILURE;
    }
    cargo(&[
        "clippy",
        "--workspace",
        "--all-targets",
        "--all-features",
        "--",
        "-D",
        "warnings",
    ])
}

// cargo コマンドの結果を ExitCode に変換する
fn cargo(args: &[&str]) -> ExitCode {
    if run_cargo(args) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

// cargo コマンドを実行し、成功したかを返す
fn run_cargo(args: &[&str]) -> bool {
    let status = Command::new("cargo")
        .args(args)
        .status()
        .expect("cargo コマンドの実行に失敗しました");
    status.success()
}
