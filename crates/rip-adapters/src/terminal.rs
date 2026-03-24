use rip_core::traits::Terminal;

/// is-terminalクレートを使用したTerminal実装
pub struct IsTerminalAdapter;

impl Terminal for IsTerminalAdapter {
    fn is_stdin_terminal(&self) -> bool {
        use is_terminal::IsTerminal;
        std::io::stdin().is_terminal()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_stdin_terminal_returns_without_panic() {
        let adapter = IsTerminalAdapter;
        // CI環境ではfalseになるため、呼び出せることだけを確認
        let _ = adapter.is_stdin_terminal();
    }
}
