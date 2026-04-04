use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::HookBridgeError;

pub trait FileSystem {
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
}

#[derive(Debug, Default)]
pub struct OsFileSystem;

impl FileSystem for OsFileSystem {
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
}

#[derive(Debug, Default)]
pub struct FakeFileSystem {
    existing: Vec<PathBuf>,
}

impl FakeFileSystem {
    #[must_use]
    pub fn with_existing(paths: Vec<PathBuf>) -> Self {
        Self { existing: paths }
    }
}

impl FileSystem for FakeFileSystem {
    fn exists(&self, path: &Path) -> Result<bool, HookBridgeError> {
        Ok(self.existing.iter().any(|item| item == path))
    }

    fn read_to_string(&self, _path: &Path) -> Result<String, HookBridgeError> {
        Ok(String::new())
    }

    fn write_all(&self, _path: &Path, _content: &[u8]) -> Result<(), HookBridgeError> {
        Ok(())
    }

    fn create_dir_all(&self, _path: &Path) -> Result<(), HookBridgeError> {
        Ok(())
    }

    fn rename(&self, _from: &Path, _to: &Path) -> Result<(), HookBridgeError> {
        Ok(())
    }

    fn remove_file_if_exists(&self, _path: &Path) -> Result<(), HookBridgeError> {
        Ok(())
    }
}

/// Atomically writes bytes by writing to a sibling temp file first then renaming.
///
/// # Errors
///
/// Returns an error if any filesystem operation fails.
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

    let tmp = unique_tmp_path(path);
    fs.write_all(&tmp, content)?;
    fs.rename(&tmp, path)
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
    use std::path::PathBuf;

    use super::{FakeFileSystem, FileSystem, OsFileSystem, atomic_write};
    use crate::error::HookBridgeError;

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

        assert_eq!(fs.write_all(&path, b"ok"), Ok(()));
        assert_eq!(fs.create_dir_all(&path), Ok(()));
        assert_eq!(fs.rename(&path, &path), Ok(()));
        assert_eq!(fs.remove_file_if_exists(&path), Ok(()));
    }

    #[test]
    fn os_filesystem_round_trips_file_operations() {
        let temp_result = tempfile::tempdir();
        assert!(temp_result.is_ok(), "tempdir creation should succeed");
        let Ok(temp) = temp_result else {
            return;
        };
        let fs = OsFileSystem;
        let dir = temp.path().join("nested");
        let original = dir.join("one.txt");
        let renamed = dir.join("two.txt");

        assert_eq!(fs.exists(&original), Ok(false));
        assert_eq!(fs.create_dir_all(&dir), Ok(()));
        assert_eq!(fs.write_all(&original, b"hello"), Ok(()));
        assert_eq!(fs.exists(&original), Ok(true));
        assert_eq!(fs.read_to_string(&original), Ok("hello".to_string()));
        assert_eq!(fs.rename(&original, &renamed), Ok(()));
        assert_eq!(fs.read_to_string(&renamed), Ok("hello".to_string()));
        assert_eq!(fs.remove_file_if_exists(&renamed), Ok(()));
        assert_eq!(fs.exists(&renamed), Ok(false));
    }

    #[test]
    fn atomic_write_persists_content() {
        let temp_result = tempfile::tempdir();
        assert!(temp_result.is_ok(), "tempdir creation should succeed");
        let Ok(temp) = temp_result else {
            return;
        };
        let fs = OsFileSystem;
        let path = temp.path().join("hooks.json");

        assert_eq!(atomic_write(&fs, &path, br#"{"ok":true}"#), Ok(()));
        assert_eq!(fs.read_to_string(&path), Ok(r#"{"ok":true}"#.to_string()));
    }

    #[test]
    fn atomic_write_rejects_path_without_parent() {
        assert_eq!(
            atomic_write(&FakeFileSystem::default(), std::path::Path::new("/"), b"{}"),
            Err(HookBridgeError::ConfigValidation {
                message: "path '/' has no parent directory".to_string(),
            })
        );
    }

    #[test]
    fn os_filesystem_surfaces_io_errors() {
        let temp_result = tempfile::tempdir();
        assert!(temp_result.is_ok(), "tempdir creation should succeed");
        let Ok(temp) = temp_result else {
            return;
        };
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
}
