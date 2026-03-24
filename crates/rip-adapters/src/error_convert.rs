use rip_core::error::ZipError;

/// zip::result::ZipError から ZipError への変換
///
/// orphan ruleにより From トレイトは実装できないため、
/// ヘルパー関数として提供する。
pub fn from_zip_error(err: zip::result::ZipError) -> ZipError {
    ZipError::Archive(err.to_string())
}

/// walkdir::Error から ZipError への変換
pub fn from_walkdir_error(err: walkdir::Error) -> ZipError {
    ZipError::Walk(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_zip_error_converts_to_archive_variant() {
        let zip_err = zip::result::ZipError::FileNotFound;
        let result = from_zip_error(zip_err);
        assert!(matches!(result, ZipError::Archive(msg) if !msg.is_empty()));
    }

    #[test]
    fn from_walkdir_error_converts_to_walk_variant() {
        let dir = tempfile::TempDir::new().unwrap();
        let nonexistent = dir.path().join("definitely_nonexistent_subdir");
        let mut walker = walkdir::WalkDir::new(&nonexistent).into_iter();
        match walker.next() {
            Some(Err(err)) => {
                let result = from_walkdir_error(err);
                assert!(matches!(result, ZipError::Walk(msg) if !msg.is_empty()));
            }
            other => panic!(
                "Expected walkdir error for nonexistent path, got: {:?}",
                other.map(|r| r.map(|e| e.path().to_path_buf()))
            ),
        }
    }
}
