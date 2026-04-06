use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};
use std::{cell::RefCell, collections::BTreeMap};

use atomic_write_file::AtomicWriteFile;

use crate::error::HookBridgeError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsEntryType {
    File,
    Directory,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FsMetadata {
    pub entry_type: FsEntryType,
    pub readonly: bool,
}

pub trait FileSystem {
    /// Returns the absolute current working directory used for relative path resolution.
    ///
    /// # Errors
    ///
    /// Returns an error when the current working directory cannot be resolved.
    fn current_dir(&self) -> Result<PathBuf, HookBridgeError>;
    /// Checks whether the given path exists.
    ///
    /// # Errors
    ///
    /// Returns an error when the underlying implementation fails to query path status.
    fn exists(&self, path: &Path) -> Result<bool, HookBridgeError>;
    /// Reads an entire file into a string.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read.
    fn read_to_string(&self, path: &Path) -> Result<String, HookBridgeError>;
    /// Writes all bytes into a file.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be written.
    fn write_all(&self, path: &Path, content: &[u8]) -> Result<(), HookBridgeError>;
    /// Creates a directory and all missing parent directories.
    ///
    /// # Errors
    ///
    /// Returns an error when the directory cannot be created.
    fn create_dir_all(&self, path: &Path) -> Result<(), HookBridgeError>;
    /// Renames a file or directory.
    ///
    /// # Errors
    ///
    /// Returns an error when the rename operation fails.
    fn rename(&self, from: &Path, to: &Path) -> Result<(), HookBridgeError>;
    /// Deletes a file if it exists.
    ///
    /// # Errors
    ///
    /// Returns an error when deletion fails for reasons other than missing file.
    fn remove_file_if_exists(&self, path: &Path) -> Result<(), HookBridgeError>;
    /// Returns metadata for a path without following symlinks.
    ///
    /// # Errors
    ///
    /// Returns an error when querying metadata fails for reasons other than missing path.
    fn metadata(&self, path: &Path) -> Result<Option<FsMetadata>, HookBridgeError>;

    /// Atomically writes bytes into a file.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be written atomically.
    fn atomic_write_all(&self, path: &Path, content: &[u8]) -> Result<(), HookBridgeError>;
}

#[derive(Debug, Default)]
pub struct OsFileSystem;

impl FileSystem for OsFileSystem {
    fn current_dir(&self) -> Result<PathBuf, HookBridgeError> {
        std::env::current_dir().map_err(|error| HookBridgeError::Process {
            message: format!("failed to resolve current working directory: {error}"),
        })
    }

    fn exists(&self, path: &Path) -> Result<bool, HookBridgeError> {
        match fs_err::metadata(path) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
            Err(error) => Err(HookBridgeError::Io {
                operation: "exists",
                path: path.to_path_buf(),
                kind: error.kind(),
            }),
        }
    }

    fn read_to_string(&self, path: &Path) -> Result<String, HookBridgeError> {
        fs_err::read_to_string(path).map_err(|error| HookBridgeError::Io {
            operation: "read_to_string",
            path: path.to_path_buf(),
            kind: error.kind(),
        })
    }

    fn write_all(&self, path: &Path, content: &[u8]) -> Result<(), HookBridgeError> {
        fs_err::write(path, content).map_err(|error| HookBridgeError::Io {
            operation: "write",
            path: path.to_path_buf(),
            kind: error.kind(),
        })
    }

    fn create_dir_all(&self, path: &Path) -> Result<(), HookBridgeError> {
        fs_err::create_dir_all(path).map_err(|error| HookBridgeError::Io {
            operation: "create_dir_all",
            path: path.to_path_buf(),
            kind: error.kind(),
        })
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), HookBridgeError> {
        fs_err::rename(from, to).map_err(|error| HookBridgeError::Io {
            operation: "rename",
            path: from.to_path_buf(),
            kind: error.kind(),
        })
    }

    fn remove_file_if_exists(&self, path: &Path) -> Result<(), HookBridgeError> {
        match fs_err::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(HookBridgeError::Io {
                operation: "remove_file",
                path: path.to_path_buf(),
                kind: error.kind(),
            }),
        }
    }

    fn metadata(&self, path: &Path) -> Result<Option<FsMetadata>, HookBridgeError> {
        match fs_err::symlink_metadata(path) {
            Ok(metadata) => {
                let file_type = metadata.file_type();
                let entry_type = if file_type.is_file() {
                    FsEntryType::File
                } else if file_type.is_dir() {
                    FsEntryType::Directory
                } else {
                    FsEntryType::Other
                };
                Ok(Some(FsMetadata {
                    entry_type,
                    readonly: metadata.permissions().readonly(),
                }))
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(HookBridgeError::Io {
                operation: "metadata",
                path: path.to_path_buf(),
                kind: error.kind(),
            }),
        }
    }

    fn atomic_write_all(&self, path: &Path, content: &[u8]) -> Result<(), HookBridgeError> {
        write_through_atomic_write_file(path, content)
    }
}

#[derive(Debug, Default)]
pub struct FakeFileSystem {
    files: RefCell<BTreeMap<PathBuf, Vec<u8>>>,
}

impl FakeFileSystem {
    #[must_use]
    pub fn with_existing(paths: Vec<PathBuf>) -> Self {
        let files = paths.into_iter().map(|path| (path, Vec::new())).collect();
        Self {
            files: RefCell::new(files),
        }
    }
}

impl FileSystem for FakeFileSystem {
    fn current_dir(&self) -> Result<PathBuf, HookBridgeError> {
        Ok(PathBuf::from("/tmp/hook-bridge-fake-fs"))
    }

    fn exists(&self, path: &Path) -> Result<bool, HookBridgeError> {
        Ok(self.files.borrow().contains_key(path))
    }

    fn read_to_string(&self, path: &Path) -> Result<String, HookBridgeError> {
        let Some(content) = self.files.borrow().get(path).cloned() else {
            return Ok(String::new());
        };
        String::from_utf8(content).map_err(|_| HookBridgeError::Io {
            operation: "read_to_string",
            path: path.to_path_buf(),
            kind: ErrorKind::InvalidData,
        })
    }

    fn write_all(&self, path: &Path, content: &[u8]) -> Result<(), HookBridgeError> {
        self.files
            .borrow_mut()
            .insert(path.to_path_buf(), content.to_vec());
        Ok(())
    }

    fn create_dir_all(&self, _path: &Path) -> Result<(), HookBridgeError> {
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), HookBridgeError> {
        let content = self
            .files
            .borrow_mut()
            .remove(from)
            .ok_or(HookBridgeError::Io {
                operation: "rename",
                path: from.to_path_buf(),
                kind: ErrorKind::NotFound,
            })?;
        self.files.borrow_mut().insert(to.to_path_buf(), content);
        Ok(())
    }

    fn remove_file_if_exists(&self, path: &Path) -> Result<(), HookBridgeError> {
        self.files.borrow_mut().remove(path);
        Ok(())
    }

    fn metadata(&self, path: &Path) -> Result<Option<FsMetadata>, HookBridgeError> {
        if self.files.borrow().contains_key(path) {
            Ok(Some(FsMetadata {
                entry_type: FsEntryType::File,
                readonly: false,
            }))
        } else {
            Ok(None)
        }
    }

    fn atomic_write_all(&self, path: &Path, content: &[u8]) -> Result<(), HookBridgeError> {
        atomic_write_via_temp_file(self, path, content)
    }
}

/// Atomically writes bytes while ensuring parent directories exist.
///
/// # Errors
///
/// Returns an error if path validation, directory creation, or the atomic write fails.
pub fn atomic_write(
    fs: &dyn FileSystem,
    path: &Path,
    content: &[u8],
) -> Result<(), HookBridgeError> {
    let parent = path
        .parent()
        .ok_or_else(|| HookBridgeError::ConfigValidation {
            message: format!("path '{}' has no parent directory", path.display()),
        })?;
    fs.create_dir_all(parent)?;
    fs.atomic_write_all(path, content)
}

fn write_through_atomic_write_file(path: &Path, content: &[u8]) -> Result<(), HookBridgeError> {
    let mut file = AtomicWriteFile::options()
        .open(path)
        .map_err(|error: std::io::Error| HookBridgeError::Io {
            operation: "atomic_write::open",
            path: path.to_path_buf(),
            kind: error.kind(),
        })?;
    file.write_all(content)
        .map_err(|error: std::io::Error| HookBridgeError::Io {
            operation: "atomic_write::write",
            path: path.to_path_buf(),
            kind: error.kind(),
        })?;
    file.commit()
        .map_err(|error: std::io::Error| HookBridgeError::Io {
            operation: "atomic_write::commit",
            path: path.to_path_buf(),
            kind: error.kind(),
        })
}

fn atomic_write_via_temp_file(
    fs: &dyn FileSystem,
    path: &Path,
    content: &[u8],
) -> Result<(), HookBridgeError> {
    let tmp = unique_tmp_path(path);
    if let Err(error) = fs.write_all(&tmp, content) {
        let _ = fs.remove_file_if_exists(&tmp);
        return Err(error);
    }

    if let Err(error) = fs.rename(&tmp, path) {
        let _ = fs.remove_file_if_exists(&tmp);
        return Err(error);
    }

    Ok(())
}

fn unique_tmp_path(path: &Path) -> PathBuf {
    static NEXT_TMP_ID: AtomicU64 = AtomicU64::new(1);

    let id = NEXT_TMP_ID.fetch_add(1, Ordering::Relaxed);
    let pid = process::id();
    let mut candidate = path.to_path_buf();
    let extension = format!("tmp.hook_bridge.{pid}.{id}");
    candidate.set_extension(extension);
    candidate
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::ErrorKind;
    use std::path::{Path, PathBuf};

    use super::{
        FakeFileSystem, FileSystem, FsEntryType, FsMetadata, OsFileSystem, atomic_write,
        unique_tmp_path,
    };
    use crate::error::HookBridgeError;

    #[allow(clippy::expect_used, reason = "test fixture setup should fail fast")]
    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir creation should succeed")
    }

    #[test]
    fn fake_filesystem_can_simulate_existence_checks() {
        let path = PathBuf::from("/tmp/mock");
        let fs = FakeFileSystem::with_existing(vec![path.clone()]);

        let exists_result = fs.exists(&path);

        assert!(matches!(exists_result, Ok(true)));
    }

    #[test]
    fn fake_filesystem_noop_operations_succeed() {
        let fs = FakeFileSystem::default();
        let path = PathBuf::from("/tmp/mock");

        assert!(fs.current_dir().is_ok_and(|cwd| cwd.is_absolute()));
        assert_eq!(fs.read_to_string(&path), Ok(String::new()));
        assert_eq!(fs.write_all(&path, b"ok"), Ok(()));
        assert_eq!(fs.create_dir_all(&path), Ok(()));
        assert_eq!(fs.rename(&path, &path), Ok(()));
        assert_eq!(fs.remove_file_if_exists(&path), Ok(()));
        assert_eq!(fs.metadata(&path), Ok(None));
    }

    #[test]
    fn os_filesystem_round_trips_file_operations() {
        let temp = tempdir();
        let fs = OsFileSystem;
        let dir = temp.path().join("nested");
        let original = dir.join("one.txt");
        let renamed = dir.join("two.txt");

        assert_eq!(fs.exists(&original), Ok(false));
        assert_eq!(fs.create_dir_all(&dir), Ok(()));
        assert_eq!(fs.write_all(&original, b"hello"), Ok(()));
        assert_eq!(fs.exists(&original), Ok(true));
        assert_eq!(
            fs.metadata(&original),
            Ok(Some(FsMetadata {
                entry_type: FsEntryType::File,
                readonly: false,
            }))
        );
        assert_eq!(fs.read_to_string(&original), Ok("hello".to_string()));
        assert_eq!(fs.rename(&original, &renamed), Ok(()));
        assert_eq!(fs.read_to_string(&renamed), Ok("hello".to_string()));
        assert_eq!(fs.remove_file_if_exists(&renamed), Ok(()));
        assert_eq!(fs.exists(&renamed), Ok(false));
        assert_eq!(fs.metadata(&renamed), Ok(None));
    }

    #[test]
    fn atomic_write_persists_content() {
        let temp = tempdir();
        let fs = OsFileSystem;
        let path = temp.path().join("hooks.json");

        assert_eq!(atomic_write(&fs, &path, br#"{"ok":true}"#), Ok(()));
        assert_eq!(fs.read_to_string(&path), Ok(r#"{"ok":true}"#.to_string()));
    }

    #[test]
    fn atomic_write_overwrites_existing_file() {
        let temp = tempdir();
        let fs = OsFileSystem;
        let path = temp.path().join("hooks.json");
        assert!(fs::write(&path, b"old").is_ok());

        assert_eq!(atomic_write(&fs, &path, b"new"), Ok(()));
        assert_eq!(fs.read_to_string(&path), Ok("new".to_string()));
    }

    #[test]
    fn atomic_write_rejects_path_without_parent() {
        assert_eq!(
            atomic_write(&FakeFileSystem::default(), Path::new("/"), b"{}"),
            Err(HookBridgeError::ConfigValidation {
                message: "path '/' has no parent directory".to_string(),
            })
        );
    }

    #[test]
    fn atomic_write_uses_fake_filesystem_atomic_path() {
        let fs = FakeFileSystem::default();
        let target = Path::new("/tmp/hooks.json");

        assert_eq!(atomic_write(&fs, target, br#"{"ok":true}"#), Ok(()));
        assert_eq!(fs.read_to_string(target), Ok(r#"{"ok":true}"#.to_string()));
    }

    #[test]
    fn unique_tmp_path_stays_in_parent_and_uses_hook_bridge_extension() {
        let target = Path::new("/tmp/hooks.json");
        let tmp = unique_tmp_path(target);

        assert_eq!(tmp.parent(), target.parent());
        assert_ne!(tmp, PathBuf::new());
        assert!(
            tmp.file_name()
                .is_some_and(|name| name.to_string_lossy().contains(".tmp.hook_bridge."))
        );
    }

    #[test]
    fn os_filesystem_remove_file_surfaces_non_not_found_errors() {
        let temp = tempdir();
        let fs = OsFileSystem;
        let directory = temp.path().join("dir");
        let mkdir_result = fs_err::create_dir_all(&directory);
        assert!(
            mkdir_result.is_ok(),
            "fixture directory should be creatable"
        );

        assert!(matches!(
            fs.remove_file_if_exists(&directory),
            Err(HookBridgeError::Io {
                operation: "remove_file",
                ..
            })
        ));
    }

    #[test]
    fn os_filesystem_surfaces_io_errors() {
        let temp = tempdir();
        let fs = OsFileSystem;
        let missing = temp.path().join("missing.txt");
        let invalid_parent = missing.join("child.txt");

        assert!(matches!(
            fs.read_to_string(&missing),
            Err(HookBridgeError::Io {
                operation: "read_to_string",
                ..
            })
        ));
        assert!(matches!(
            fs.write_all(&invalid_parent, b"nope"),
            Err(HookBridgeError::Io {
                operation: "write",
                ..
            })
        ));
        assert_eq!(fs.remove_file_if_exists(&missing), Ok(()));

        let blocked_dir = temp.path().join("blocked");
        let write_blocked_dir = fs::write(&blocked_dir, b"file");
        assert!(write_blocked_dir.is_ok(), "fixture file should be writable");
        assert!(matches!(
            fs.create_dir_all(&blocked_dir.join("child")),
            Err(HookBridgeError::Io {
                operation: "create_dir_all",
                ..
            })
        ));
        assert!(matches!(
            fs.exists(&blocked_dir.join("child")),
            Err(HookBridgeError::Io {
                operation: "exists",
                ..
            })
        ));
        assert!(matches!(
            fs.rename(&missing, &blocked_dir.join("renamed.txt")),
            Err(HookBridgeError::Io {
                operation: "rename",
                ..
            })
        ));
    }

    #[test]
    fn os_filesystem_read_to_string_surfaces_invalid_data() {
        let temp = tempdir();
        let fs = OsFileSystem;
        let path = temp.path().join("binary.bin");
        assert!(
            fs::write(&path, [0xff]).is_ok(),
            "fixture file should be writable"
        );

        assert!(matches!(
            fs.read_to_string(&path),
            Err(HookBridgeError::Io {
                operation: "read_to_string",
                kind: ErrorKind::InvalidData,
                ..
            })
        ));
    }

    #[test]
    fn os_filesystem_rename_missing_source_surfaces_not_found() {
        let temp = tempdir();
        let fs = OsFileSystem;
        let from = temp.path().join("missing.txt");
        let to = temp.path().join("renamed.txt");

        assert!(matches!(
            fs.rename(&from, &to),
            Err(HookBridgeError::Io {
                operation: "rename",
                kind: ErrorKind::NotFound,
                ..
            })
        ));
    }
}
