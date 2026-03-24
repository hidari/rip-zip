use std::path::Path;

use rip_core::config::MAX_WALK_DEPTH;
use rip_core::error::ZipError;
use rip_core::traits::FileWalker;
use rip_core::types::FileEntry;
use walkdir::WalkDir;

use crate::error_convert::from_walkdir_error;

/// walkdirクレートを使用したFileWalker実装
///
/// シンボリックリンクは追跡せず、同一ファイルシステム内のみを走査する。
/// 深度はMAX_WALK_DEPTHで制限される。
pub struct WalkDirWalker;

impl FileWalker for WalkDirWalker {
    fn walk(&self, source_dir: &Path) -> Box<dyn Iterator<Item = Result<FileEntry, ZipError>>> {
        let source_dir = source_dir.to_path_buf();
        let walkdir = WalkDir::new(&source_dir)
            .follow_links(false)
            .same_file_system(true)
            .max_depth(MAX_WALK_DEPTH);

        let iter = walkdir.into_iter().filter_map(move |entry_result| {
            match entry_result {
                Err(err) => Some(Err(from_walkdir_error(err))),
                Ok(entry) => {
                    let path = entry.path().to_path_buf();

                    // ソースディレクトリ自体はスキップ
                    if path == source_dir {
                        return None;
                    }

                    // walkdir::DirEntryのキャッシュ済みfile_typeを使い、冗長なsyscallを回避
                    let file_type = entry.file_type();
                    let is_symlink = file_type.is_symlink();
                    let is_file = file_type.is_file();

                    let relative_path = match path.strip_prefix(&source_dir) {
                        Ok(rel) => rel.to_path_buf(),
                        Err(err) => return Some(Err(ZipError::StripPrefix(err))),
                    };

                    let (size, unix_permissions) = if is_file && !is_symlink {
                        // entry.metadata()はwalkdirがキャッシュしたstatを再利用する
                        match entry.metadata() {
                            Ok(metadata) => {
                                let size = metadata.len();
                                #[cfg(unix)]
                                let perms = {
                                    use std::os::unix::fs::PermissionsExt;
                                    metadata.permissions().mode()
                                };
                                #[cfg(not(unix))]
                                let perms = 0o644;
                                (size, perms)
                            }
                            Err(err) => return Some(Err(from_walkdir_error(err))),
                        }
                    } else {
                        (0, 0o644)
                    };

                    Some(Ok(FileEntry {
                        path,
                        relative_path,
                        is_symlink,
                        is_file,
                        size,
                        unix_permissions,
                    }))
                }
            }
        });

        Box::new(iter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    mod file_discovery {
        use super::*;

        #[test]
        fn returns_entries_for_regular_files() {
            let dir = tempfile::TempDir::new().unwrap();
            fs::write(dir.path().join("file1.txt"), "hello").unwrap();
            fs::write(dir.path().join("file2.txt"), "world").unwrap();

            let walker = WalkDirWalker;
            let entries: Vec<_> = walker
                .walk(dir.path())
                .filter_map(Result::ok)
                .filter(|e| e.is_file)
                .collect();

            assert_eq!(entries.len(), 2);
            assert!(entries.iter().all(|e| e.is_file));
            assert!(entries.iter().all(|e| !e.is_symlink));
        }

        #[test]
        fn sets_correct_relative_paths_for_nested_files() {
            let dir = tempfile::TempDir::new().unwrap();
            let sub = dir.path().join("sub");
            fs::create_dir(&sub).unwrap();
            fs::write(sub.join("nested.txt"), "content").unwrap();

            let walker = WalkDirWalker;
            let entries: Vec<_> = walker
                .walk(dir.path())
                .filter_map(Result::ok)
                .filter(|e| e.is_file)
                .collect();

            assert_eq!(entries.len(), 1);
            assert_eq!(
                entries[0].relative_path,
                std::path::PathBuf::from("sub/nested.txt")
            );
        }

        #[test]
        fn traverses_deeply_nested_directory_structure() {
            let dir = tempfile::TempDir::new().unwrap();
            let deep = dir.path().join("a").join("b").join("c");
            fs::create_dir_all(&deep).unwrap();
            fs::write(deep.join("deep.txt"), "deep").unwrap();

            let walker = WalkDirWalker;
            let file_entries: Vec<_> = walker
                .walk(dir.path())
                .filter_map(Result::ok)
                .filter(|e| e.is_file)
                .collect();

            assert_eq!(file_entries.len(), 1);
            assert_eq!(
                file_entries[0].relative_path,
                std::path::PathBuf::from("a/b/c/deep.txt")
            );
        }

        #[test]
        fn returns_empty_iterator_for_empty_directory() {
            let dir = tempfile::TempDir::new().unwrap();

            let walker = WalkDirWalker;
            let entries: Vec<_> = walker.walk(dir.path()).filter_map(Result::ok).collect();

            assert!(entries.is_empty());
        }
    }

    mod metadata {
        use super::*;

        #[test]
        fn returns_accurate_file_sizes() {
            let dir = tempfile::TempDir::new().unwrap();
            let content = "hello world";
            fs::write(dir.path().join("file.txt"), content).unwrap();

            let walker = WalkDirWalker;
            let entries: Vec<_> = walker
                .walk(dir.path())
                .filter_map(Result::ok)
                .filter(|e| e.is_file)
                .collect();

            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].size, content.len() as u64);
        }

        #[cfg(unix)]
        #[test]
        fn returns_unix_permissions_from_filesystem() {
            use std::os::unix::fs::PermissionsExt;

            let dir = tempfile::TempDir::new().unwrap();
            let file = dir.path().join("file.txt");
            fs::write(&file, "content").unwrap();
            fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();

            let walker = WalkDirWalker;
            let entries: Vec<_> = walker
                .walk(dir.path())
                .filter_map(Result::ok)
                .filter(|e| e.is_file)
                .collect();

            assert_eq!(entries.len(), 1);
            // modeはファイルタイプビットを含むため、下位12ビットのみ比較
            assert_eq!(entries[0].unix_permissions & 0o777, 0o755);
        }
    }

    mod filtering {
        use super::*;

        #[test]
        fn detects_symlinks_with_is_symlink_flag() {
            let dir = tempfile::TempDir::new().unwrap();
            let target = dir.path().join("target.txt");
            fs::write(&target, "content").unwrap();

            let link = dir.path().join("link.txt");
            #[cfg(unix)]
            std::os::unix::fs::symlink(&target, &link).unwrap();
            #[cfg(not(unix))]
            {
                // Windows環境ではシンボリックリンクテストをスキップ
                return;
            }

            let walker = WalkDirWalker;
            let entries: Vec<_> = walker.walk(dir.path()).filter_map(Result::ok).collect();

            let symlink_entry = entries.iter().find(|e| e.path == link);
            assert!(symlink_entry.is_some());
            assert!(symlink_entry.unwrap().is_symlink);
        }

        #[test]
        fn excludes_source_directory_from_results() {
            let dir = tempfile::TempDir::new().unwrap();
            fs::write(dir.path().join("file.txt"), "content").unwrap();

            let walker = WalkDirWalker;
            let entries: Vec<_> = walker.walk(dir.path()).filter_map(Result::ok).collect();

            // ソースディレクトリ自体はエントリに含まれない
            assert!(entries.iter().all(|e| e.path != dir.path()));
        }

        #[test]
        fn includes_hidden_files() {
            let dir = tempfile::TempDir::new().unwrap();
            fs::write(dir.path().join(".hidden"), "secret").unwrap();

            let walker = WalkDirWalker;
            let entries: Vec<_> = walker
                .walk(dir.path())
                .filter_map(Result::ok)
                .filter(|e| e.is_file)
                .collect();

            assert_eq!(entries.len(), 1);
            assert_eq!(
                entries[0].relative_path,
                std::path::PathBuf::from(".hidden")
            );
        }
    }
}
