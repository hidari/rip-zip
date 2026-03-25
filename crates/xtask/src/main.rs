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
        Some("audit") => run_audit(),
        Some("help") | Some("--help") | Some("-h") => {
            print_help();
            ExitCode::SUCCESS
        }
        Some(unknown) => {
            eprintln!("unknown command: {unknown}");
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
    eprintln!("rip-zip development task runner");
    eprintln!();
    eprintln!("Usage: cargo xtask <COMMAND>");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  fmt       Format code + lint check");
    eprintln!("  test      Run unit tests");
    eprintln!("  test-all  Run all tests (including #[ignore])");
    eprintln!("  build     Build entire workspace");
    eprintln!("  check     Type check");
    eprintln!("  audit     Run security audit (cargo-deny)");
    eprintln!("  help      Show this help");
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

// セキュリティ監査（cargo-deny）
fn run_audit() -> ExitCode {
    let status = Command::new("cargo")
        .args(["deny", "check"])
        .status()
        .expect(
            "failed to execute cargo deny. Is cargo-deny installed? Run: cargo install cargo-deny",
        );
    if status.success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
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
        .expect("failed to execute cargo command");
    status.success()
}
