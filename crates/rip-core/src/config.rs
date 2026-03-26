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

/// ZIP展開時の圧縮比率上限（1000倍）
///
/// この比率を超えるエントリはzip bomb疑いとしてスキップする。
/// 通常のテキストファイルの圧縮比率は5-10倍程度であり、
/// 1000倍は十分な余裕を持ちつつzip bombを検出可能な閾値。
pub const MAX_COMPRESSION_RATIO: u64 = 1000;

/// setuidビット
pub const SETUID_BIT: u32 = 0o4000;

/// setgidビット
pub const SETGID_BIT: u32 = 0o2000;

/// stickyビット
pub const STICKY_BIT: u32 = 0o1000;

/// ファイルのパーミッション上限
pub const MAX_FILE_PERMISSIONS: u32 = 0o755;

/// ディレクトリのパーミッション上限
pub const MAX_DIR_PERMISSIONS: u32 = 0o755;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_file_size_equals_1gb() {
        assert_eq!(MAX_FILE_SIZE, 1024 * 1024 * 1024);
    }

    #[test]
    fn max_total_size_equals_4gb() {
        assert_eq!(MAX_TOTAL_SIZE, 4 * 1024 * 1024 * 1024);
    }

    #[test]
    fn max_file_count_equals_100000() {
        assert_eq!(MAX_FILE_COUNT, 100_000);
    }

    #[test]
    fn max_filename_length_equals_zip_spec_limit_65535() {
        assert_eq!(MAX_FILENAME_LENGTH, 65535);
    }

    #[test]
    fn max_walk_depth_equals_100() {
        assert_eq!(MAX_WALK_DEPTH, 100);
    }

    #[test]
    fn max_compression_ratio_equals_1000() {
        assert_eq!(MAX_COMPRESSION_RATIO, 1000);
    }

    #[test]
    fn setuid_bit_equals_octal_4000() {
        assert_eq!(SETUID_BIT, 0o4000);
    }

    #[test]
    fn setgid_bit_equals_octal_2000() {
        assert_eq!(SETGID_BIT, 0o2000);
    }

    #[test]
    fn sticky_bit_equals_octal_1000() {
        assert_eq!(STICKY_BIT, 0o1000);
    }

    #[test]
    fn max_file_permissions_equals_octal_755() {
        assert_eq!(MAX_FILE_PERMISSIONS, 0o755);
    }

    #[test]
    fn max_dir_permissions_equals_octal_755() {
        assert_eq!(MAX_DIR_PERMISSIONS, 0o755);
    }
}
