use std::path::{Path, PathBuf};

use hook_bridge::error::HookBridgeError;
use hook_bridge::runtime::fs::{
    FakeFileSystem, FileSystem, FsEntryType, FsMetadata, OsFileSystem, atomic_write,
};

#[test]
fn fake_filesystem_metadata_reports_existing_path_as_writable_file() {
    let path = PathBuf::from("/tmp/mock-hooks.json");
    let fs = FakeFileSystem::with_existing(vec![path.clone()]);

    assert_eq!(
        fs.metadata(&path),
        Ok(Some(FsMetadata {
            entry_type: FsEntryType::File,
            readonly: false,
        }))
    );
}

#[test]
fn os_filesystem_metadata_surfaces_non_not_found_error() {
    let temp = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
    let fs = OsFileSystem;
    let file_parent = temp.path().join("plain-file");
    assert!(std::fs::write(&file_parent, b"fixture").is_ok());

    let nested_path = file_parent.join("child.json");
    assert!(matches!(
        fs.metadata(&nested_path),
        Err(HookBridgeError::Io {
            operation: "metadata",
            ..
        })
    ));
}

#[cfg(unix)]
#[test]
fn os_filesystem_metadata_marks_symlink_as_other() {
    let temp = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
    let fs = OsFileSystem;
    let target = temp.path().join("target.txt");
    assert!(std::fs::write(&target, b"fixture").is_ok());

    let symlink = temp.path().join("target-link.txt");
    assert!(std::os::unix::fs::symlink(&target, &symlink).is_ok());

    assert!(matches!(
        fs.metadata(&symlink),
        Ok(Some(FsMetadata {
            entry_type: FsEntryType::Other,
            ..
        }))
    ));
}

#[derive(Clone, Copy)]
enum AtomicWriteFailure {
    Open,
    Write,
    Commit,
}

struct FailingAtomicFileSystem {
    os: OsFileSystem,
    failure: AtomicWriteFailure,
}

impl FailingAtomicFileSystem {
    fn new(failure: AtomicWriteFailure) -> Self {
        Self {
            os: OsFileSystem,
            failure,
        }
    }
}

impl FileSystem for FailingAtomicFileSystem {
    fn current_dir(&self) -> Result<PathBuf, HookBridgeError> {
        self.os.current_dir()
    }

    fn exists(&self, path: &Path) -> Result<bool, HookBridgeError> {
        self.os.exists(path)
    }

    fn read_to_string(&self, path: &Path) -> Result<String, HookBridgeError> {
        self.os.read_to_string(path)
    }

    fn write_all(&self, path: &Path, content: &[u8]) -> Result<(), HookBridgeError> {
        self.os.write_all(path, content)
    }

    fn create_dir_all(&self, path: &Path) -> Result<(), HookBridgeError> {
        self.os.create_dir_all(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), HookBridgeError> {
        self.os.rename(from, to)
    }

    fn remove_file_if_exists(&self, path: &Path) -> Result<(), HookBridgeError> {
        self.os.remove_file_if_exists(path)
    }

    fn metadata(&self, path: &Path) -> Result<Option<FsMetadata>, HookBridgeError> {
        self.os.metadata(path)
    }

    fn atomic_write_all(&self, path: &Path, content: &[u8]) -> Result<(), HookBridgeError> {
        let _ = content;
        match self.failure {
            AtomicWriteFailure::Open => Err(HookBridgeError::Io {
                operation: "atomic_write::open",
                path: path.to_path_buf(),
                kind: std::io::ErrorKind::PermissionDenied,
            }),
            AtomicWriteFailure::Write => Err(HookBridgeError::Io {
                operation: "atomic_write::write",
                path: path.to_path_buf(),
                kind: std::io::ErrorKind::PermissionDenied,
            }),
            AtomicWriteFailure::Commit => Err(HookBridgeError::Io {
                operation: "atomic_write::commit",
                path: path.to_path_buf(),
                kind: std::io::ErrorKind::PermissionDenied,
            }),
        }
    }
}

#[test]
fn atomic_write_preserves_original_when_open_fails() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };
    let target = temp.path().join("hooks.json");
    assert!(std::fs::write(&target, br#"{"old":true}"#).is_ok());
    let fs = FailingAtomicFileSystem::new(AtomicWriteFailure::Open);

    let result = atomic_write(&fs, &target, br#"{"new":true}"#);

    assert!(matches!(
        result,
        Err(HookBridgeError::Io {
            operation: "atomic_write::open",
            ..
        })
    ));
    assert!(std::fs::read_to_string(&target).is_ok_and(|content| content == r#"{"old":true}"#));
}

#[test]
fn atomic_write_preserves_original_when_write_fails() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };
    let target = temp.path().join("hooks.json");
    assert!(std::fs::write(&target, br#"{"old":true}"#).is_ok());
    let fs = FailingAtomicFileSystem::new(AtomicWriteFailure::Write);

    let result = atomic_write(&fs, &target, br#"{"new":true}"#);

    assert!(matches!(
        result,
        Err(HookBridgeError::Io {
            operation: "atomic_write::write",
            ..
        })
    ));
    assert!(std::fs::read_to_string(&target).is_ok_and(|content| content == r#"{"old":true}"#));
}

#[test]
fn atomic_write_preserves_original_when_commit_fails() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };
    let target = temp.path().join("hooks.json");
    assert!(std::fs::write(&target, br#"{"old":true}"#).is_ok());
    let fs = FailingAtomicFileSystem::new(AtomicWriteFailure::Commit);

    let result = atomic_write(&fs, &target, br#"{"new":true}"#);

    assert!(matches!(
        result,
        Err(HookBridgeError::Io {
            operation: "atomic_write::commit",
            ..
        })
    ));
    assert!(std::fs::read_to_string(&target).is_ok_and(|content| content == r#"{"old":true}"#));
}
