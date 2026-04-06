use std::path::PathBuf;

use hook_bridge::error::HookBridgeError;
use hook_bridge::runtime::fs::{FakeFileSystem, FileSystem, FsEntryType, FsMetadata, OsFileSystem};

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
