use std::fs;
use std::fs::File;
use std::path::Path;

use rip_adapters::walkdir_walker::WalkDirWalker;
use rip_adapters::zip_archiver::ZipWriterArchiver;
use rip_core::config::MAX_FILENAME_LENGTH;
use rip_core::error::ZipError;
use rip_core::path_utils::{get_zip_path, sanitize_filename};
use rip_core::types::ZipEvent;
use rip_core::zip_creator;
use tempfile::TempDir;
use zip::ZipArchive;

// --- テストヘルパー ---

/// 実アダプターを使用してZIPを作成するヘルパー
fn create_zip_with_adapters(
    source_dir: &Path,
    target_zip: &Path,
    use_zip64: bool,
) -> Result<rip_core::types::ZipStats, ZipError> {
    let walker = WalkDirWalker;
    let mut archiver = ZipWriterArchiver::new();
    zip_creator::create_zip(
        &walker,
        &mut archiver,
        source_dir,
        target_zip,
        use_zip64,
        &|_| {},
    )
}

// ========== 国際化テスト ==========

#[test]
fn test_japanese_filename() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_dir = temp_dir.path().join("テスト");
    fs::create_dir(&test_dir)?;

    let test_file_path = test_dir.join("日本語.txt");
    fs::write(&test_file_path, "テストデータ")?;

    let zip_path = temp_dir.path().join("test.zip");
    create_zip_with_adapters(&test_dir, &zip_path, false)?;

    assert!(zip_path.exists());

    let zip_file = File::open(&zip_path)?;
    let archive = ZipArchive::new(zip_file)?;

    let file_names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();
    assert!(file_names.contains(&"日本語.txt".to_string()));

    Ok(())
}

#[test]
fn test_complex_japanese_filename() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_dir = temp_dir.path().join("テスト");
    fs::create_dir(&test_dir)?;

    let test_file_path = test_dir.join("🗾日本語_テスト！＃＄％.txt");
    fs::write(&test_file_path, "テストデータ")?;

    let zip_path = temp_dir.path().join("test.zip");
    create_zip_with_adapters(&test_dir, &zip_path, false)?;

    let zip_file = File::open(&zip_path)?;
    let archive = ZipArchive::new(zip_file)?;

    let file_names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();
    assert!(file_names.contains(&"🗾日本語_テスト！＃＄％.txt".to_string()));

    Ok(())
}

#[test]
fn test_nested_japanese_directories() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let base_dir = temp_dir.path().join("テスト");
    fs::create_dir(&base_dir)?;

    let nested_dir = base_dir
        .join("フォルダー1")
        .join("フォルダー2")
        .join("フォルダー3");
    fs::create_dir_all(&nested_dir)?;

    let test_file_path = nested_dir.join("テスト.txt");
    fs::write(&test_file_path, "テストデータ")?;

    let zip_path = temp_dir.path().join("test.zip");
    create_zip_with_adapters(&base_dir, &zip_path, false)?;

    let zip_file = File::open(&zip_path)?;
    let archive = ZipArchive::new(zip_file)?;

    let file_names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();
    assert!(file_names.iter().any(|name| name.ends_with("テスト.txt")));

    Ok(())
}

#[test]
fn test_cross_platform_compatibility() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_dir = temp_dir.path().join("クロスプラットフォーム");
    fs::create_dir(&test_dir)?;

    let filename = if cfg!(windows) {
        "Windows用ファイル　テスト.txt"
    } else {
        "macOS用ファイル テスト.txt"
    };

    let test_file_path = test_dir.join(filename);
    fs::write(&test_file_path, "テストデータ")?;

    let subdir = test_dir.join("サブフォルダー");
    fs::create_dir(&subdir)?;
    let subfile_path = subdir.join("テスト.txt");
    fs::write(&subfile_path, "サブディレクトリのテストデータ")?;

    let zip_path = temp_dir.path().join("cross_platform_test.zip");
    create_zip_with_adapters(&test_dir, &zip_path, false)?;

    let zip_file = File::open(&zip_path)?;
    let archive = ZipArchive::new(zip_file)?;

    let file_names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();

    assert!(file_names.iter().all(|name| !name.contains('\\')));
    assert!(file_names.iter().any(|name| name.contains(filename)));
    assert!(file_names.iter().any(|name| name.ends_with("テスト.txt")));

    Ok(())
}

#[test]
fn test_platform_specific_filenames() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_dir = temp_dir.path().join("プラットフォーム互換性");
    fs::create_dir(&test_dir)?;

    let nfd_filename = "がぎぐげご_NFD.txt";
    let nfc_filename = "がぎぐげご_NFC.txt";

    fs::write(test_dir.join(nfd_filename), "NFDテスト")?;
    fs::write(test_dir.join(nfc_filename), "NFCテスト")?;

    let zip_path = temp_dir.path().join("platform_test.zip");
    create_zip_with_adapters(&test_dir, &zip_path, false)?;

    let zip_file = File::open(&zip_path)?;
    let archive = ZipArchive::new(zip_file)?;
    let file_names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();

    assert!(
        file_names.iter().any(|name| name.contains("がぎぐげご")),
        "ファイル名 'がぎぐげご' が見つかりません"
    );

    Ok(())
}

#[test]
fn test_very_long_paths() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_dir = temp_dir.path().join("長いパス");
    fs::create_dir(&test_dir)?;

    let mut current_dir = test_dir.clone();
    for i in 1..20 {
        current_dir = current_dir.join(format!("深いディレクトリ_{}", i));
        fs::create_dir(&current_dir)?;
    }

    fs::write(current_dir.join("テスト.txt"), "深い階層のテスト")?;

    let zip_path = temp_dir.path().join("long_paths_test.zip");
    create_zip_with_adapters(&test_dir, &zip_path, false)?;

    let zip_file = File::open(&zip_path)?;
    let archive = ZipArchive::new(zip_file)?;

    assert!(archive
        .file_names()
        .any(|name| name.ends_with("テスト.txt")));

    Ok(())
}

#[test]
fn test_simulated_cross_platform_paths() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_dir = temp_dir.path().join("クロスプラットフォーム");
    fs::create_dir(&test_dir)?;

    // Windowsスタイルのパス
    let win_style_name = "Windows\\スタイル\\パス.txt";
    let win_path = test_dir.join(win_style_name.replace('\\', "/"));
    fs::create_dir_all(win_path.parent().unwrap())?;
    fs::write(&win_path, "Windowsスタイル")?;

    // macOS/UNIXスタイルのパス
    let unix_style_name = "macOS/スタイル/パス.txt";
    let unix_path = test_dir.join(unix_style_name);
    fs::create_dir_all(unix_path.parent().unwrap())?;
    fs::write(&unix_path, "macOSスタイル")?;

    let zip_path = temp_dir.path().join("cross_platform.zip");
    create_zip_with_adapters(&test_dir, &zip_path, false)?;

    let zip_file = File::open(&zip_path)?;
    let archive = ZipArchive::new(zip_file)?;
    let file_names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();

    assert!(file_names.iter().all(|name| !name.contains('\\')));
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

// ========== ZIP64テスト ==========

#[test]
fn test_zip64_cli_option() {
    let temp_dir = TempDir::new().unwrap();
    let test_dir = temp_dir.path().join("zip64test");
    fs::create_dir(&test_dir).unwrap();
    fs::write(test_dir.join("test.txt"), b"test").unwrap();

    let zip_path = get_zip_path(&test_dir);
    assert!(create_zip_with_adapters(&test_dir, &zip_path, true).is_ok());
}

// ========== セキュリティテスト ==========

#[test]
fn test_path_traversal_protection() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_dir = temp_dir.path().join("traversal_test");
    fs::create_dir(&test_dir)?;

    fs::write(test_dir.join("normal.txt"), "normal content")?;

    let subdir = test_dir.join("subdir");
    fs::create_dir(&subdir)?;
    fs::write(subdir.join("file.txt"), "subdir content")?;

    let zip_path = temp_dir.path().join("test.zip");
    create_zip_with_adapters(&test_dir, &zip_path, false)?;

    let zip_file = File::open(&zip_path)?;
    let archive = ZipArchive::new(zip_file)?;

    for name in archive.file_names() {
        assert!(!name.contains(".."), "Path traversal detected: {}", name);
        assert!(!name.starts_with('/'), "Absolute path detected: {}", name);
    }

    Ok(())
}

#[test]
fn test_file_size_limit() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_dir = temp_dir.path().join("size_test");
    fs::create_dir(&test_dir)?;

    fs::write(test_dir.join("small.txt"), "small content")?;

    let zip_path = temp_dir.path().join("test_no_zip64.zip");
    create_zip_with_adapters(&test_dir, &zip_path, false)?;

    let zip_file = File::open(&zip_path)?;
    let archive = ZipArchive::new(zip_file)?;
    assert_eq!(archive.len(), 1);

    let zip_path_64 = temp_dir.path().join("test_with_zip64.zip");
    create_zip_with_adapters(&test_dir, &zip_path_64, true)?;

    let zip_file_64 = File::open(&zip_path_64)?;
    let archive_64 = ZipArchive::new(zip_file_64)?;
    assert_eq!(archive_64.len(), 1);

    Ok(())
}

#[test]
fn test_file_count_limit() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_dir = temp_dir.path().join("count_test");
    fs::create_dir(&test_dir)?;

    for i in 0..100 {
        fs::write(
            test_dir.join(format!("file_{:05}.txt", i)),
            format!("content {}", i),
        )?;
    }

    let zip_path = temp_dir.path().join("test.zip");
    create_zip_with_adapters(&test_dir, &zip_path, false)?;

    let zip_file = File::open(&zip_path)?;
    let archive = ZipArchive::new(zip_file)?;
    assert_eq!(archive.len(), 100);

    Ok(())
}

#[test]
#[cfg(unix)]
fn test_symlink_handling() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let temp_dir = TempDir::new()?;
    let test_dir = temp_dir.path().join("symlink_test");
    fs::create_dir(&test_dir)?;

    let normal_file = test_dir.join("normal.txt");
    fs::write(&normal_file, "normal content")?;

    let symlink_path = test_dir.join("symlink.txt");
    symlink(&normal_file, &symlink_path)?;

    let zip_path = temp_dir.path().join("test.zip");
    create_zip_with_adapters(&test_dir, &zip_path, false)?;

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

    let result = create_zip_with_adapters(&non_existent, &zip_path, false);
    assert!(result.is_err(), "Should fail for non-existent path");

    if let Err(ZipError::Validation(msg)) = result {
        assert!(msg.contains("does not exist"));
    } else {
        panic!("Expected Validation error for non-existent path");
    }

    // ファイルパス（ディレクトリではない）
    let file_path = temp_dir.path().join("file.txt");
    fs::write(&file_path, "content")?;

    let result = create_zip_with_adapters(&file_path, &zip_path, false);
    assert!(result.is_err(), "Should fail for non-directory path");

    if let Err(ZipError::Validation(msg)) = result {
        assert!(msg.contains("not a directory"));
    } else {
        panic!("Expected Validation error for non-directory path");
    }

    Ok(())
}

// ========== 制限値テスト ==========

#[test]
fn test_filename_length_limit() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_dir = temp_dir.path().join("length_test");
    fs::create_dir(&test_dir)?;

    fs::write(test_dir.join("normal.txt"), "normal")?;

    let long_name = "a".repeat(200) + ".txt";
    fs::write(test_dir.join(&long_name), "long name")?;

    let zip_path = temp_dir.path().join("test.zip");
    create_zip_with_adapters(&test_dir, &zip_path, false)?;

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
fn test_filename_sanitization() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_dir = temp_dir.path().join("sanitize_test");
    fs::create_dir(&test_dir)?;

    fs::write(test_dir.join("normal.txt"), "normal")?;

    // sanitize_filename関数のテスト
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
    let zip_path = get_zip_path(&test_dir);
    assert!(zip_path.to_string_lossy().ends_with(".zip"));

    Ok(())
}

#[test]
fn test_recursion_depth_limit() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_dir = temp_dir.path().join("depth_test");
    fs::create_dir(&test_dir)?;

    let mut current = test_dir.clone();
    for i in 0..50 {
        current = current.join(format!("level_{:03}", i));
        fs::create_dir(&current)?;
    }

    fs::write(current.join("deep_file.txt"), "deep content")?;

    let mut mid = test_dir.clone();
    for i in 0..25 {
        mid = mid.join(format!("level_{:03}", i));
    }
    fs::write(mid.join("mid_file.txt"), "mid content")?;

    let zip_path = temp_dir.path().join("test.zip");
    create_zip_with_adapters(&test_dir, &zip_path, false)?;

    let zip_file = File::open(&zip_path)?;
    let archive = ZipArchive::new(zip_file)?;

    assert_eq!(archive.len(), 2, "Should contain 2 files");

    let file_names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();
    assert!(file_names.iter().any(|n| n.ends_with("deep_file.txt")));
    assert!(file_names.iter().any(|n| n.ends_with("mid_file.txt")));

    Ok(())
}

#[test]
fn test_total_size_limit() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_dir = temp_dir.path().join("total_size_test");
    fs::create_dir(&test_dir)?;

    for i in 0..10 {
        fs::write(
            test_dir.join(format!("file_{}.txt", i)),
            format!("content {}", i),
        )?;
    }

    let zip_path = temp_dir.path().join("test.zip");
    create_zip_with_adapters(&test_dir, &zip_path, false)?;

    let zip_file = File::open(&zip_path)?;
    let archive = ZipArchive::new(zip_file)?;
    assert_eq!(archive.len(), 10);

    Ok(())
}

#[test]
#[cfg(unix)]
fn test_permission_preservation() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = TempDir::new()?;
    let test_dir = temp_dir.path().join("perm_test");
    fs::create_dir(&test_dir)?;

    let exec_file = test_dir.join("executable.sh");
    fs::write(&exec_file, "#!/bin/bash\necho test")?;
    fs::set_permissions(&exec_file, fs::Permissions::from_mode(0o755))?;

    let normal_file = test_dir.join("normal.txt");
    fs::write(&normal_file, "normal content")?;
    fs::set_permissions(&normal_file, fs::Permissions::from_mode(0o644))?;

    let zip_path = temp_dir.path().join("test.zip");
    create_zip_with_adapters(&test_dir, &zip_path, false)?;

    assert!(zip_path.exists());

    let zip_file = File::open(&zip_path)?;
    let archive = ZipArchive::new(zip_file)?;
    assert_eq!(archive.len(), 2, "Should contain 2 files");

    Ok(())
}

// ========== ZipEventコールバックテスト ==========

#[test]
fn test_zip_events_are_emitted() -> Result<(), Box<dyn std::error::Error>> {
    use std::cell::RefCell;

    let temp_dir = TempDir::new()?;
    let test_dir = temp_dir.path().join("event_test");
    fs::create_dir(&test_dir)?;
    fs::write(test_dir.join("file1.txt"), "content1")?;
    fs::write(test_dir.join("file2.txt"), "content2")?;

    let events: RefCell<Vec<String>> = RefCell::new(Vec::new());
    let zip_path = temp_dir.path().join("test.zip");

    let walker = WalkDirWalker;
    let mut archiver = ZipWriterArchiver::new();

    zip_creator::create_zip(
        &walker,
        &mut archiver,
        &test_dir,
        &zip_path,
        false,
        &|event| match event {
            ZipEvent::ArchiveStarted { .. } => events.borrow_mut().push("started".to_string()),
            ZipEvent::FileAdded { name, .. } => events.borrow_mut().push(format!("added:{}", name)),
            ZipEvent::ArchiveCompleted { .. } => events.borrow_mut().push("completed".to_string()),
            _ => {}
        },
    )?;

    let captured = events.borrow();
    assert!(captured.first().map(|s| s.as_str()) == Some("started"));
    assert!(captured.last().map(|s| s.as_str()) == Some("completed"));
    assert_eq!(
        captured.iter().filter(|s| s.starts_with("added:")).count(),
        2
    );

    Ok(())
}

// ========== 低速テスト（#[ignore]） ==========

#[test]
#[ignore = "very slow: creates 100,001 files"]
fn test_file_count_limit_exceeded() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_dir = temp_dir.path().join("file_count_test");
    fs::create_dir(&test_dir)?;

    for i in 0..=100_000 {
        let file_path = test_dir.join(format!("f{}.txt", i));
        File::create(file_path)?;
    }

    let zip_path = temp_dir.path().join("test.zip");
    let result = create_zip_with_adapters(&test_dir, &zip_path, false);

    assert!(result.is_err(), "Expected error for too many files");

    if let Err(ZipError::Validation(msg)) = result {
        assert!(
            msg.contains("Too many files") || msg.contains("100000"),
            "Expected file count error, got: {}",
            msg
        );
    } else {
        panic!("Expected Validation error for file count limit");
    }

    Ok(())
}

#[test]
#[cfg(unix)]
#[ignore = "very slow: creates 1GB+ sparse file"]
fn test_file_size_limit_large() -> Result<(), Box<dyn std::error::Error>> {
    use rip_core::config::MAX_FILE_SIZE;
    use std::os::unix::fs::FileExt;

    let temp_dir = TempDir::new()?;
    let test_dir = temp_dir.path().join("size_test");
    fs::create_dir(&test_dir)?;

    let large_file_path = test_dir.join("large.bin");
    let large_file = File::create(&large_file_path)?;

    let size: u64 = MAX_FILE_SIZE + 1;
    large_file.write_at(b"X", size - 1)?;

    fs::write(test_dir.join("small.txt"), "small")?;

    // ZIP64なし（大きいファイルはスキップ）
    let zip_path_no64 = temp_dir.path().join("test_no_zip64.zip");
    create_zip_with_adapters(&test_dir, &zip_path_no64, false)?;

    let zip_file = File::open(&zip_path_no64)?;
    let archive = ZipArchive::new(zip_file)?;

    assert_eq!(archive.len(), 1, "Should contain only small file");

    let file_names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();
    assert!(file_names.contains(&"small.txt".to_string()));
    assert!(!file_names.iter().any(|n| n.contains("large")));

    // ZIP64あり（すべて含まれる）
    let zip_path_64 = temp_dir.path().join("test_with_zip64.zip");
    create_zip_with_adapters(&test_dir, &zip_path_64, true)?;

    let zip_file_64 = File::open(&zip_path_64)?;
    let archive_64 = ZipArchive::new(zip_file_64)?;

    assert_eq!(archive_64.len(), 2, "Should contain both files with ZIP64");

    Ok(())
}

#[test]
#[ignore = "very slow: creates 101-level deep directory structure"]
fn test_recursion_depth_exceeded() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let test_dir = temp_dir.path().join("depth_test");
    fs::create_dir(&test_dir)?;

    let mut current_dir = test_dir.clone();
    for i in 0..=100 {
        current_dir = current_dir.join(format!("d{}", i));
        fs::create_dir(&current_dir)?;
    }

    // レベル50: 範囲内
    let mut level_50 = test_dir.clone();
    for i in 0..50 {
        level_50 = level_50.join(format!("d{}", i));
    }
    fs::write(level_50.join("file_level50.txt"), "level 50")?;

    // レベル99: 境界
    let mut level_99 = test_dir.clone();
    for i in 0..99 {
        level_99 = level_99.join(format!("d{}", i));
    }
    fs::write(level_99.join("file_level99.txt"), "level 99")?;

    // レベル100: 超過
    let mut level_100 = test_dir.clone();
    for i in 0..100 {
        level_100 = level_100.join(format!("d{}", i));
    }
    fs::write(level_100.join("file_level100.txt"), "level 100")?;

    let zip_path = temp_dir.path().join("test.zip");
    create_zip_with_adapters(&test_dir, &zip_path, false)?;

    let zip_file = File::open(&zip_path)?;
    let archive = ZipArchive::new(zip_file)?;

    let file_names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();

    assert!(
        file_names.iter().any(|n| n.ends_with("file_level50.txt")),
        "Level 50 file should be included"
    );
    assert!(
        file_names.iter().any(|n| n.ends_with("file_level99.txt")),
        "Level 99 file should be included"
    );
    assert!(
        !file_names.iter().any(|n| n.ends_with("file_level100.txt")),
        "Level 100 file should be skipped (exceeds max_depth)"
    );

    Ok(())
}
