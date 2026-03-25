use std::path::{Path, PathBuf};

/// ファイル名に不要な不可視Unicode文字（零幅文字・BOM・双方向制御文字）かを判定する
///
/// これらはファイル名に含まれるべきではなく、セキュリティリスク（ファイル名偽装等）がある。
fn is_invisible_unicode(c: char) -> bool {
    matches!(
        c,
        '\u{200B}'..='\u{200D}'   // 零幅文字 (ZERO WIDTH SPACE, NON-JOINER, JOINER)
        | '\u{FEFF}'              // BOM / ZERO WIDTH NO-BREAK SPACE
        | '\u{2060}'              // WORD JOINER
        | '\u{200E}'..='\u{200F}' // LRM, RLM
        | '\u{202A}'..='\u{202E}' // 双方向埋め込み/オーバーライド
        | '\u{2066}'..='\u{2069}' // 双方向分離
        | '\u{2028}'..='\u{2029}' // LINE SEPARATOR, PARAGRAPH SEPARATOR
    )
}

/// Windows予約デバイス名かどうかを判定する（大文字小文字非区別）
///
/// Windowsではこれらの名前をファイル名として使用できない。
fn is_windows_reserved_name(name: &str) -> bool {
    let upper = name.to_uppercase();
    matches!(
        upper.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

/// ファイル名サニタイズのコアロジック（フォールバックなし）
///
/// 以下の処理を順に適用し、空文字列になった場合はNoneを返す:
/// 1. 不可視Unicode文字（零幅文字、BOM、双方向制御文字）を除去
/// 2. 危険文字・制御文字をアンダースコアに置換
/// 3. 末尾のドット・スペースを除去（Windows互換）
/// 4. Windows予約デバイス名をエスケープ
fn sanitize_filename_core(name: &str) -> Option<String> {
    // 1-2. 不可視Unicode文字を除去し、危険文字・制御文字をアンダースコアに置換
    let sanitized: String = name
        .chars()
        .filter(|c| !is_invisible_unicode(*c))
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            _ if c.is_control() => '_',
            _ => c,
        })
        .collect();

    // 3. 先頭スペース・末尾のドット/スペースを除去（Windows互換）
    let trimmed = sanitized
        .trim_start_matches(' ')
        .trim_end_matches(['.', ' ']);

    // 4. Windows予約デバイス名の処理（最初のドットで分割してstemを判定）
    let stem = trimmed.find('.').map_or(trimmed, |pos| &trimmed[..pos]);
    let result = if is_windows_reserved_name(stem) {
        match trimmed.find('.') {
            Some(pos) => format!("{}_{}", &trimmed[..pos], &trimmed[pos..]),
            None => format!("{}_", trimmed),
        }
    } else {
        trimmed.to_string()
    };

    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

/// ファイル名から危険な文字を安全な文字に置換する
///
/// ZIPファイル名として安全な文字列を生成する。
/// サニタイズ後に空になった場合は "archive" にフォールバックする。
pub fn sanitize_filename(name: &str) -> String {
    sanitize_filename_core(name).unwrap_or_else(|| "archive".to_string())
}

/// ZIPエントリパスの各セグメントをサニタイズする
///
/// バックスラッシュをフォワードスラッシュに正規化した後、
/// パスを `/` で分割し、各セグメントに `sanitize_filename_core` を適用後、
/// `/` で再結合する。空セグメント（連続スラッシュ）は除去する。
/// サニタイズ後に空になったセグメントは `_` に置換する。
pub fn sanitize_zip_entry_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let segments: Vec<String> = normalized
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|segment| sanitize_filename_core(segment).unwrap_or_else(|| "_".to_string()))
        .collect();

    if segments.is_empty() {
        "_".to_string()
    } else {
        segments.join("/")
    }
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

    /// sanitize_filename の仕様
    mod sanitize_filename {
        use super::*;

        /// 危険文字の置換仕様
        mod dangerous_characters {
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
            fn replaces_multiple_dangerous_chars_independently() {
                // 複数の危険文字が混在する場合、それぞれ独立に置換される
                assert_eq!(sanitize_filename("a\\b/c:d"), "a_b_c_d");
            }
        }

        /// 制御文字の置換仕様
        mod control_characters {
            use super::*;

            #[test]
            fn replaces_c0_control_characters_with_underscore() {
                // C0制御文字 (U+0001-U+001F) がアンダースコアに置換される
                assert_eq!(sanitize_filename("file\x01name"), "file_name");
                assert_eq!(sanitize_filename("file\x1fname"), "file_name");
            }

            #[test]
            fn replaces_del_character_with_underscore() {
                // DEL (U+007F) がアンダースコアに置換される
                assert_eq!(sanitize_filename("file\x7fname"), "file_name");
            }

            #[test]
            fn replaces_c1_control_characters_with_underscore() {
                // C1制御文字 (U+0080-U+009F) がアンダースコアに置換される
                assert_eq!(sanitize_filename("file\u{0080}name"), "file_name");
                assert_eq!(sanitize_filename("file\u{009F}name"), "file_name");
            }
        }

        /// 通常文字の保持仕様
        mod normal_characters {
            use super::*;

            #[test]
            fn preserves_ascii_and_unicode_characters() {
                // 通常のASCII文字およびUnicode文字はそのまま保持される
                assert_eq!(sanitize_filename("normal_file.txt"), "normal_file.txt");
                assert_eq!(sanitize_filename("日本語ファイル"), "日本語ファイル");
                assert_eq!(sanitize_filename("file-name.rs"), "file-name.rs");
            }

            #[test]
            fn preserves_emoji_characters() {
                // 絵文字はファイル名として有効
                assert_eq!(sanitize_filename("📁test📂"), "📁test📂");
            }

            #[test]
            fn preserves_combining_characters() {
                // 結合文字（例: e + combining acute accent）は保持される
                assert_eq!(
                    sanitize_filename("caf\u{0065}\u{0301}"),
                    "caf\u{0065}\u{0301}"
                );
            }
        }

        /// ゼロ幅Unicode文字の除去仕様
        mod zero_width_unicode {
            use super::*;

            #[test]
            fn strips_zero_width_space() {
                // U+200B ZERO WIDTH SPACE が除去される
                assert_eq!(sanitize_filename("file\u{200B}name"), "filename");
            }

            #[test]
            fn strips_zero_width_non_joiner() {
                // U+200C ZERO WIDTH NON-JOINER が除去される
                assert_eq!(sanitize_filename("file\u{200C}name"), "filename");
            }

            #[test]
            fn strips_zero_width_joiner() {
                // U+200D ZERO WIDTH JOINER が除去される
                assert_eq!(sanitize_filename("file\u{200D}name"), "filename");
            }

            #[test]
            fn strips_bom() {
                // U+FEFF BOM / ZERO WIDTH NO-BREAK SPACE が除去される
                assert_eq!(sanitize_filename("\u{FEFF}filename"), "filename");
            }

            #[test]
            fn strips_word_joiner() {
                // U+2060 WORD JOINER が除去される
                assert_eq!(sanitize_filename("file\u{2060}name"), "filename");
            }

            #[test]
            fn strips_multiple_zero_width_chars_from_mixed_input() {
                // 通常文字と混在する零幅文字がすべて除去される
                assert_eq!(
                    sanitize_filename("\u{200B}a\u{200C}b\u{200D}c\u{FEFF}"),
                    "abc"
                );
            }

            #[test]
            fn returns_fallback_for_only_zero_width_chars() {
                // 全てゼロ幅文字のみの場合はフォールバック
                assert_eq!(sanitize_filename("\u{200B}\u{200C}\u{200D}"), "archive");
            }
        }

        /// 双方向制御文字の除去仕様（ファイル名偽装攻撃の防止）
        mod bidi_control_characters {
            use super::*;

            #[test]
            fn strips_left_to_right_and_right_to_left_marks() {
                // U+200E LRM, U+200F RLM が除去される
                assert_eq!(sanitize_filename("file\u{200E}name"), "filename");
                assert_eq!(sanitize_filename("file\u{200F}name"), "filename");
            }

            #[test]
            fn strips_directional_embedding_and_override() {
                // U+202A-U+202E 双方向埋め込み/オーバーライドが除去される
                assert_eq!(sanitize_filename("file\u{202A}name"), "filename");
                assert_eq!(sanitize_filename("file\u{202E}name"), "filename");
            }

            #[test]
            fn strips_directional_isolate() {
                // U+2066-U+2069 双方向分離が除去される
                assert_eq!(sanitize_filename("file\u{2066}name"), "filename");
                assert_eq!(sanitize_filename("file\u{2069}name"), "filename");
            }

            #[test]
            fn neutralizes_rtl_filename_spoofing() {
                // RTLオーバーライドによるファイル名偽装が無害化される
                // "evil\u{202E}cod.exe" -> RTL除去後 "evilcod.exe"
                // ただし.はそのまま残る
                assert_eq!(sanitize_filename("evil\u{202E}cod.exe"), "evilcod.exe");
            }

            #[test]
            fn strips_line_and_paragraph_separators() {
                // U+2028 LINE SEPARATOR, U+2029 PARAGRAPH SEPARATOR が除去される
                assert_eq!(sanitize_filename("file\u{2028}name"), "filename");
                assert_eq!(sanitize_filename("file\u{2029}name"), "filename");
            }
        }

        /// Windows予約デバイス名の処理仕様
        mod windows_reserved_names {
            use super::*;

            #[test]
            fn appends_underscore_to_reserved_names() {
                // 予約名にアンダースコアを付与してデバイス名衝突を回避
                assert_eq!(sanitize_filename("CON"), "CON_");
                assert_eq!(sanitize_filename("PRN"), "PRN_");
                assert_eq!(sanitize_filename("AUX"), "AUX_");
                assert_eq!(sanitize_filename("NUL"), "NUL_");
            }

            #[test]
            fn appends_underscore_to_com_and_lpt_ports() {
                assert_eq!(sanitize_filename("COM1"), "COM1_");
                assert_eq!(sanitize_filename("COM9"), "COM9_");
                assert_eq!(sanitize_filename("LPT1"), "LPT1_");
                assert_eq!(sanitize_filename("LPT9"), "LPT9_");
            }

            #[test]
            fn handles_reserved_names_case_insensitively() {
                // 大文字小文字を区別しない
                assert_eq!(sanitize_filename("con"), "con_");
                assert_eq!(sanitize_filename("Con"), "Con_");
                assert_eq!(sanitize_filename("CON"), "CON_");
            }

            #[test]
            fn appends_underscore_before_extension_for_reserved_names() {
                // 拡張子付きの予約名: stemと拡張子の間に_を挿入
                assert_eq!(sanitize_filename("CON.txt"), "CON_.txt");
                assert_eq!(sanitize_filename("nul.zip"), "nul_.zip");
                assert_eq!(sanitize_filename("COM1.tar.gz"), "COM1_.tar.gz");
            }

            #[test]
            fn does_not_modify_non_reserved_names() {
                // 予約名に似ているが非該当のもの
                assert_eq!(sanitize_filename("CONX"), "CONX");
                assert_eq!(sanitize_filename("COM10"), "COM10");
                assert_eq!(sanitize_filename("LPTA"), "LPTA");
                assert_eq!(sanitize_filename("connect"), "connect");
            }

            #[test]
            fn detects_reserved_name_with_injected_invisible_chars() {
                // 零幅文字を注入してWindows予約名チェックを回避しようとする攻撃を無効化
                // パス1で不可視文字除去 → パス4で予約名検出
                assert_eq!(sanitize_filename("C\u{200B}ON"), "CON_");
                assert_eq!(sanitize_filename("N\u{FEFF}UL.txt"), "NUL_.txt");
            }
        }

        /// ドット・スペースのトリム仕様（Windows互換）
        mod dots_and_spaces {
            use super::*;

            #[test]
            fn trims_trailing_dots() {
                // Windowsでは末尾のドットが無視されるため除去する
                assert_eq!(sanitize_filename("file."), "file");
                assert_eq!(sanitize_filename("file..."), "file");
            }

            #[test]
            fn trims_trailing_spaces() {
                // Windowsでは末尾のスペースが無視されるため除去する
                assert_eq!(sanitize_filename("file "), "file");
                assert_eq!(sanitize_filename("file   "), "file");
            }

            #[test]
            fn trims_leading_spaces() {
                // Windowsでは先頭のスペースも無視されるため除去する
                assert_eq!(sanitize_filename("  file"), "file");
                assert_eq!(sanitize_filename("   leading.txt"), "leading.txt");
            }

            #[test]
            fn trims_mixed_trailing_dots_and_spaces() {
                assert_eq!(sanitize_filename("file. ."), "file");
            }

            #[test]
            fn preserves_dots_in_middle() {
                // 中間のドットは保持される
                assert_eq!(sanitize_filename("file.tar.gz"), "file.tar.gz");
            }
        }

        /// エッジケースの仕様
        mod edge_cases {
            use super::*;

            #[test]
            fn returns_fallback_for_empty_input() {
                // 空文字列はフォールバック名を返す
                assert_eq!(sanitize_filename(""), "archive");
            }

            #[test]
            fn returns_fallback_when_all_chars_are_invisible() {
                // 全文字が不可視の場合はフォールバック
                assert_eq!(sanitize_filename("\u{200B}\u{FEFF}\u{200E}"), "archive");
            }

            #[test]
            fn returns_fallback_for_only_dots() {
                // ドットのみの入力は末尾trim後に空になりフォールバック
                assert_eq!(sanitize_filename(".."), "archive");
                assert_eq!(sanitize_filename("..."), "archive");
            }

            #[test]
            fn returns_fallback_for_only_spaces() {
                // スペースのみの入力は末尾trim後に空になりフォールバック
                assert_eq!(sanitize_filename("   "), "archive");
            }

            #[test]
            fn handles_all_dangerous_characters() {
                // 全文字が危険文字 → 全てアンダースコアに置換
                assert_eq!(sanitize_filename("***"), "___");
            }
        }

        /// naughty stringsによる堅牢性テスト
        mod naughty_strings {
            use super::*;

            #[test]
            fn does_not_panic_on_script_injection() {
                let result = sanitize_filename("<script>alert(1)</script>");
                assert!(!result.is_empty());
                // <と>はアンダースコアに置換される
                assert!(!result.contains('<'));
                assert!(!result.contains('>'));
            }

            #[test]
            fn does_not_panic_on_sql_injection() {
                let result = sanitize_filename("' OR 1=1 --");
                assert!(!result.is_empty());
            }

            #[test]
            fn preserves_length_without_truncation() {
                // sanitize_filenameは長さ制限を行わない（切り詰めは呼び出し元の責務）
                let long_input = "a".repeat(100_000);
                let result = sanitize_filename(&long_input);
                assert_eq!(result.len(), 100_000);
            }

            #[test]
            fn does_not_panic_on_mixed_scripts() {
                // キリル文字、ギリシャ文字、中国語の混合
                let result = sanitize_filename("файл_αρχείο_文件");
                assert_eq!(result, "файл_αρχείο_文件");
            }

            #[test]
            fn does_not_panic_on_null_bytes_embedded() {
                let result = sanitize_filename("file\0\0\0name");
                assert_eq!(result, "file___name");
            }

            #[test]
            fn does_not_panic_on_replacement_character() {
                // U+FFFD REPLACEMENT CHARACTER は通常文字として保持
                let result = sanitize_filename("\u{FFFD}test");
                assert_eq!(result, "\u{FFFD}test");
            }

            #[test]
            fn does_not_panic_on_newlines_and_tabs() {
                // 改行・タブは制御文字として置換
                let result = sanitize_filename("file\n\t\rname");
                assert_eq!(result, "file___name");
            }
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

    /// sanitize_zip_entry_path の仕様
    mod sanitize_zip_entry_path {
        use super::*;

        /// 基本動作の仕様
        mod basic_behavior {
            use super::*;

            #[test]
            fn sanitizes_each_segment_independently() {
                // 各パスセグメントが個別にサニタイズされる
                assert_eq!(
                    sanitize_zip_entry_path("dir/file:name.txt"),
                    "dir/file_name.txt"
                );
            }

            #[test]
            fn preserves_already_safe_path() {
                // 安全なパスは変更されない
                assert_eq!(
                    sanitize_zip_entry_path("dir/sub/file.txt"),
                    "dir/sub/file.txt"
                );
            }

            #[test]
            fn handles_single_segment_path() {
                // 単一セグメントのパスはそのまま処理される
                assert_eq!(sanitize_zip_entry_path("file.txt"), "file.txt");
            }
        }

        /// 空セグメント除去の仕様
        mod empty_segment_removal {
            use super::*;

            #[test]
            fn removes_empty_segments_from_consecutive_slashes() {
                // 連続スラッシュは正規化される
                assert_eq!(sanitize_zip_entry_path("dir//file.txt"), "dir/file.txt");
            }

            #[test]
            fn removes_leading_slash() {
                // 先頭スラッシュは除去される
                assert_eq!(sanitize_zip_entry_path("/dir/file.txt"), "dir/file.txt");
            }

            #[test]
            fn removes_trailing_slash() {
                // 末尾スラッシュは除去される
                assert_eq!(sanitize_zip_entry_path("dir/file.txt/"), "dir/file.txt");
            }
        }

        /// サニタイズ後の空セグメントフォールバックの仕様
        mod fallback_behavior {
            use super::*;

            #[test]
            fn replaces_invisible_only_segment_with_underscore() {
                // 不可視文字のみのセグメントは "_" にフォールバック
                assert_eq!(
                    sanitize_zip_entry_path("dir/\u{200B}/file.txt"),
                    "dir/_/file.txt"
                );
            }

            #[test]
            fn replaces_dots_only_segment_with_underscore() {
                // ドットのみのセグメントはトリミングで空になり "_" にフォールバック
                assert_eq!(
                    sanitize_zip_entry_path("dir/.../file.txt"),
                    "dir/_/file.txt"
                );
            }

            #[test]
            fn returns_underscore_for_all_empty_segments() {
                // 全セグメントが空の場合 "_" にフォールバック
                assert_eq!(sanitize_zip_entry_path("///"), "_");
            }

            #[test]
            fn returns_underscore_for_empty_string() {
                // 空文字列は "_" にフォールバック
                assert_eq!(sanitize_zip_entry_path(""), "_");
            }

            #[test]
            fn replaces_single_dot_segment_with_underscore() {
                // "." はトリミングで空になり "_" にフォールバック
                assert_eq!(sanitize_zip_entry_path("."), "_");
            }

            #[test]
            fn replaces_double_dot_segment_with_underscore() {
                // ".." はトリミングで空になり "_" にフォールバック
                assert_eq!(sanitize_zip_entry_path(".."), "_");
            }
        }

        /// セキュリティ関連の仕様
        mod security {
            use super::*;

            #[test]
            fn sanitizes_windows_reserved_names_in_segments() {
                // Windows予約名はセグメント単位でサニタイズされる
                assert_eq!(sanitize_zip_entry_path("CON/file.txt"), "CON_/file.txt");
            }

            #[test]
            fn strips_invisible_unicode_from_segments() {
                // 不可視Unicode文字はセグメントから除去される
                assert_eq!(
                    sanitize_zip_entry_path("dir/fi\u{200B}le.txt"),
                    "dir/file.txt"
                );
            }

            #[test]
            fn sanitizes_dangerous_chars_in_segments() {
                // 危険文字はセグメント単位で置換される
                assert_eq!(
                    sanitize_zip_entry_path("my:dir/file*name.txt"),
                    "my_dir/file_name.txt"
                );
            }

            #[test]
            fn strips_bidi_control_chars_from_segments() {
                // RTL制御文字はセグメントから除去される
                assert_eq!(
                    sanitize_zip_entry_path("dir/\u{202E}evil.txt"),
                    "dir/evil.txt"
                );
            }

            #[test]
            fn normalizes_backslashes_as_path_separators() {
                // バックスラッシュはフォワードスラッシュに正規化される
                assert_eq!(
                    sanitize_zip_entry_path("dir\\sub\\file.txt"),
                    "dir/sub/file.txt"
                );
            }
        }

        /// "archive" セグメントの区別の仕様
        mod archive_segment {
            use super::*;

            #[test]
            fn preserves_literal_archive_segment() {
                // 元が "archive" というセグメントはそのまま保持される
                assert_eq!(
                    sanitize_zip_entry_path("archive/file.txt"),
                    "archive/file.txt"
                );
            }
        }
    }
}
