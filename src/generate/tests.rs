use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};

use crate::config::parse_and_normalize;
use crate::error::HookBridgeError;
use crate::platform::Platform;
use crate::runtime::Runtime;
use crate::runtime::clock::{Clock, FixedClock};
use crate::runtime::fs::{FileSystem, FsMetadata, OsFileSystem};
use crate::runtime::io::{FakeIo, Io};
use crate::runtime::process::{FakeProcessRunner, ProcessRunner};

use super::build::{collect_platform_hooks, native_event_name};
use super::managed::ensure_no_unmanaged_conflict;
use super::{
    CLAUDE_TARGET, CODEX_TARGET, DialoguerForceOverwriteConfirmer, ForceOverwriteConfirmer,
    GenerateArgs, MANAGED_BY, MANAGED_VERSION, ManagedMetadata, build_generation_input,
    build_run_command, ensure_existing_force_target_is_replaceable,
    ensure_force_target_parent_is_writable, ensure_generation_targets_are_writable, execute,
    execute_with_confirmer_and_interactivity, format_force_overwrite_targets,
    is_force_overwrite_interactive, is_managed_content, load_metadata, normalize_path, target_path,
};

struct CurrentDirGuard {
    original: PathBuf,
}

impl CurrentDirGuard {
    fn enter(path: &Path) -> std::io::Result<Self> {
        let original = env::current_dir()?;
        env::set_current_dir(path)?;
        Ok(Self { original })
    }
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        let _ = env::set_current_dir(&self.original);
    }
}

struct TestFileSystem {
    os: OsFileSystem,
    cwd: PathBuf,
}

impl TestFileSystem {
    fn new(cwd: PathBuf) -> Self {
        Self {
            os: OsFileSystem,
            cwd,
        }
    }
}

impl FileSystem for TestFileSystem {
    fn current_dir(&self) -> Result<PathBuf, HookBridgeError> {
        Ok(self.cwd.clone())
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
        self.os.atomic_write_all(path, content)
    }
}

struct TestRuntime {
    fs: TestFileSystem,
    clock: FixedClock,
    process: FakeProcessRunner,
    io: FakeIo,
}

impl TestRuntime {
    fn new(cwd: PathBuf) -> Self {
        Self {
            fs: TestFileSystem::new(cwd),
            clock: FixedClock::new(std::time::SystemTime::UNIX_EPOCH),
            process: FakeProcessRunner::success(0),
            io: FakeIo::default(),
        }
    }
}

impl Runtime for TestRuntime {
    fn fs(&self) -> &dyn FileSystem {
        &self.fs
    }

    fn clock(&self) -> &dyn Clock {
        &self.clock
    }

    fn process_runner(&self) -> &dyn ProcessRunner {
        &self.process
    }

    fn io(&self) -> &dyn Io {
        &self.io
    }

    fn temp_dir(&self) -> PathBuf {
        self.fs.cwd.clone()
    }
}

fn assert_metadata_error(content: &str, expected_message: String) {
    let write_result = std::fs::write(".codex/hooks.json", content);
    assert!(
        write_result.is_ok(),
        "managed file fixture should be writable"
    );
    assert_eq!(
        load_metadata(&crate::runtime::RealRuntime::default(), Platform::Codex),
        Err(crate::error::HookBridgeError::PlatformProtocol {
            message: expected_message,
        })
    );
}

fn current_target_path(platform: Platform) -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| unreachable!())
        .join(target_path(platform))
}

#[path = "tests/generation_core.rs"]
mod generation_core;

#[path = "tests/managed_files.rs"]
mod managed_files;
