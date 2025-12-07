# セキュリティレビュー報告書

**プロジェクト**: rip-zip
**レビュー日**: 2025-12-07
**レビュアー**: Claude Security Review

## エグゼクティブサマリー

rip-zipのソースコードに対してセキュリティレビューを実施しました。**3つの重大な脆弱性**と**4つの中程度の問題**、**3つの軽微な問題**が発見されました。

### 重大度の分類
- 🔴 **Critical (重大)**: 3件 - 即座に修正が必要
- 🟠 **High (高)**: 4件 - 早急に修正が必要
- 🟡 **Medium (中)**: 3件 - 修正を推奨

---

## 🔴 Critical - 重大な脆弱性

### 1. ファイルサイズ制限が実装されていない (CWE-400: リソース枯渇)

**場所**: `src/main.rs:68-127` (`create_zip`関数)

**問題**:
READMEには「Individual file size limit: 1GB」「Total ZIP size: 4GB」と記載されていますが、実際のコードには**いかなるサイズ制限も実装されていません**。

```rust
// line 120-121: サイズチェックなしでファイル全体をコピー
let mut file = File::open(path)?;
io::copy(&mut file, &mut zip)?;
```

**影響**:
- 攻撃者が巨大なファイルを含むディレクトリを圧縮させることで、メモリ枯渇やディスク領域の消費を引き起こす可能性
- DoS攻撃のベクトルとなる
- システムリソースの予期しない消費

**推奨される修正**:
```rust
const MAX_FILE_SIZE: u64 = 1_073_741_824; // 1GB
const MAX_TOTAL_SIZE: u64 = 4_294_967_296; // 4GB (without zip64)

let mut total_size: u64 = 0;

// ファイル追加時にチェック
let metadata = file.metadata()?;
let file_size = metadata.len();

if file_size > MAX_FILE_SIZE && !use_zip64 {
    eprintln!("Warning: File {} exceeds 1GB limit, skipping", name);
    continue;
}

if total_size + file_size > MAX_TOTAL_SIZE && !use_zip64 {
    return Err(ZipError::IoError(Error::new(
        io::ErrorKind::Other,
        "Total archive size would exceed 4GB limit. Use --zip64 flag for larger archives."
    )));
}

total_size += file_size;
```

---

### 2. ファイル名サニタイズが無効化されている (CWE-73: ファイル名の外部制御)

**場所**: `src/main.rs:139-169` (`get_zip_path`関数)

**問題**:
`sanitize_filename`関数が呼び出されているにもかかわらず、その結果が使用されていません。

```rust
// line 144: サニタイズを実行
let safe_name = sanitize_filename(&dir_name);

// line 146-151: safe_nameを使ってパスを構築
zip_path.push(format!("{}.zip", safe_name));

// line 153-158: ⚠️ 直後に上書きして、元のdir_nameを使用！
let mut zip_path = source_dir
    .parent()
    .unwrap_or_else(|| Path::new("."))
    .to_path_buf();
zip_path.push(format!("{}.zip", dir_name));  // サニタイズされていない！
```

**影響**:
- 特殊文字を含むファイル名による予期しない動作
- Windowsでは `<>:"/\|?*` などの文字がファイルシステムエラーを引き起こす
- パストラバーサルやコマンドインジェクションのリスク

**推奨される修正**:
```rust
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

    // safe_nameを使用（dir_nameではなく）
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
```

---

### 3. ファイル数制限がない (CWE-770: 制限またはスロットリングなしのリソース割り当て)

**場所**: `src/main.rs:94-123`

**問題**:
圧縮できるファイルの数に制限がありません。悪意のあるユーザーが数百万の小さなファイルを含むディレクトリを圧縮させることが可能です。

**影響**:
- Zip bomb攻撃のベクトル
- メモリとCPUリソースの枯渇
- システムの応答停止

**推奨される修正**:
```rust
const MAX_FILE_COUNT: usize = 100_000;

let mut file_count: usize = 0;

for entry in walkdir {
    // ...
    if path.is_file() {
        file_count += 1;
        if file_count > MAX_FILE_COUNT {
            return Err(ZipError::IoError(Error::new(
                io::ErrorKind::Other,
                format!("Too many files (limit: {})", MAX_FILE_COUNT)
            )));
        }
        // ...
    }
}
```

---

## 🟠 High - 高リスク

### 4. パストラバーサル攻撃の不完全な防御

**場所**: `src/main.rs:102-107`

**問題**:
`..` コンポーネントのチェックはありますが、警告が表示されず、サイレントにスキップされます。

```rust
if relative_path
    .components()
    .any(|component| matches!(component, std::path::Component::ParentDir))
{
    continue;  // ⚠️ サイレントにスキップ
}
```

**推奨される修正**:
```rust
if relative_path
    .components()
    .any(|component| matches!(component, std::path::Component::ParentDir))
{
    if verbose {
        eprintln!("Warning: Skipping file with parent directory reference: {}",
                  relative_path.display());
    }
    continue;
}
```

---

### 5. 入力検証の欠如

**場所**: `src/main.rs:178-192` (`main`関数)

**問題**:
コマンドライン引数として渡されたパスが実際にディレクトリであるかの検証がありません。

**推奨される修正**:
```rust
for source in args.sources {
    if !source.exists() {
        eprintln!("Error: Source does not exist: {}", source.display());
        continue;
    }

    if !source.is_dir() {
        eprintln!("Error: Source is not a directory: {}", source.display());
        continue;
    }

    let zip_path = get_zip_path(&source);
    // ...
}
```

---

### 6. シンボリックリンクの不適切な処理

**場所**: `src/main.rs:88-92`

**問題**:
`.follow_links(false)`は設定されていますが、シンボリックリンクに遭遇した際の動作が不明確です。

**推奨される修正**:
```rust
for entry in walkdir {
    let entry = entry.map_err(|e| Error::new(io::ErrorKind::Other, e))?;
    let path = entry.path();

    // シンボリックリンクを明示的にスキップ
    if path.is_symlink() {
        if verbose {
            eprintln!("Warning: Skipping symlink: {}", path.display());
        }
        continue;
    }

    if path.is_file() {
        // ...
    }
}
```

---

### 7. ファイル名の長さ制限がない

**場所**: `src/main.rs:109-112`

**問題**:
ZIP仕様ではファイル名の長さは65,535バイトまでですが、チェックがありません。

**推奨される修正**:
```rust
const MAX_FILENAME_LENGTH: usize = 65535;

let name = match relative_path.to_str() {
    Some(s) => s.to_string(),
    None => relative_path.to_string_lossy().into_owned(),
};

if name.len() > MAX_FILENAME_LENGTH {
    if verbose {
        eprintln!("Warning: Filename too long, skipping: {}", name);
    }
    continue;
}
```

---

## 🟡 Medium - 中リスク

### 8. 依存関係のセキュリティ

**場所**: `Cargo.toml:17`

**問題**:
`atty = "0.2.14"` は非推奨のクレートで、メンテナンスされていません。

**推奨される修正**:
```toml
# atty = "0.2.14"  # 非推奨
is-terminal = "0.4"  # 推奨される代替
```

コードの変更:
```rust
// 古い:
#[cfg(windows)]
if std::env::args().len() <= 1 && atty::is(atty::Stream::Stdin) {
    pause();
}

// 新しい:
#[cfg(windows)]
if std::env::args().len() <= 1 && std::io::stdin().is_terminal() {
    pause();
}
```

---

### 9. Unixパーミッションのハードコーディング

**場所**: `src/main.rs:85`

**問題**:
すべてのファイルに `0o755` (rwxr-xr-x) が設定されますが、元のファイルのパーミッションは保持されません。

**推奨される修正**:
```rust
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

// ファイルごとに元のパーミッションを取得
#[cfg(unix)]
let permissions = metadata.permissions().mode();
#[cfg(not(unix))]
let permissions = 0o644;

let options = SimpleFileOptions::default()
    .compression_method(zip::CompressionMethod::Deflated)
    .unix_permissions(permissions)
    .large_file(use_zip64);
```

---

### 10. `sanitize_filename`の重複チェック

**場所**: `src/main.rs:129-137`

**問題**:
スラッシュとバックスラッシュのチェックが重複しています。

```rust
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            _ if c.is_control() || c == '/' || c == '\\' => '_',  // ⚠️ 重複
            _ => c,
        })
        .collect()
}
```

**推奨される修正**:
```rust
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            _ if c.is_control() => '_',
            _ => c,
        })
        .collect()
}
```

---

## 推奨される追加のセキュリティ対策

### 1. セキュリティテストの追加

```rust
#[test]
fn test_path_traversal_protection() {
    // パストラバーサル攻撃のテスト
}

#[test]
fn test_file_size_limits() {
    // ファイルサイズ制限のテスト
}

#[test]
fn test_symlink_handling() {
    // シンボリックリンク処理のテスト
}
```

### 2. 依存関係の定期的な監査

```bash
# cargo-auditの使用を推奨
cargo install cargo-audit
cargo audit
```

### 3. Fuzzing

長期的には、`cargo-fuzz`を使用してファズテストを実施することを推奨します。

---

## まとめ

このツールは基本的なセキュリティ対策（シンボリックリンクの非追跡、パストラバーサルチェック）を実装していますが、**リソース制限の欠如**が重大な問題です。

### 優先度別の修正推奨順序:

1. **即座に修正すべき (Critical)**:
   - ファイルサイズ制限の実装
   - ファイル名サニタイズの修正
   - ファイル数制限の追加

2. **早急に修正すべき (High)**:
   - パストラバーサル警告の追加
   - 入力検証の追加
   - シンボリックリンク処理の明示化
   - ファイル名長さ制限

3. **修正を推奨 (Medium)**:
   - attyクレートの置き換え
   - パーミッション保持の改善
   - sanitize_filename関数のリファクタリング

修正後は、セキュリティテストを追加し、`cargo audit`での定期的な依存関係チェックを実施することを強く推奨します。
