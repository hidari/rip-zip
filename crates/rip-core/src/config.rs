/// 個別ファイルの最大サイズ（1GB）
pub const MAX_FILE_SIZE: u64 = 1_073_741_824;

/// アーカイブ全体の最大サイズ（4GB、ZIP64なしの場合）
pub const MAX_TOTAL_SIZE: u64 = 4_294_967_296;

/// アーカイブ内の最大ファイル数
pub const MAX_FILE_COUNT: usize = 100_000;

/// ZIP仕様の最大ファイル名長（バイト数）
pub const MAX_FILENAME_LENGTH: usize = 65535;

/// ディレクトリ走査の最大深度
pub const MAX_WALK_DEPTH: usize = 100;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_file_size_is_1gb() {
        assert_eq!(MAX_FILE_SIZE, 1024 * 1024 * 1024);
    }

    #[test]
    fn max_total_size_is_4gb() {
        assert_eq!(MAX_TOTAL_SIZE, 4 * 1024 * 1024 * 1024);
    }

    #[test]
    fn max_file_count_is_100k() {
        assert_eq!(MAX_FILE_COUNT, 100_000);
    }

    #[test]
    fn max_filename_length_is_zip_spec_max() {
        assert_eq!(MAX_FILENAME_LENGTH, 65535);
    }

    #[test]
    fn max_walk_depth_prevents_excessive_recursion() {
        assert_eq!(MAX_WALK_DEPTH, 100);
    }
}
