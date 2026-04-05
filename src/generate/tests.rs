use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};

use crate::config::parse_and_normalize;
use crate::platform::Platform;

use super::build::{collect_platform_hooks, native_event_name};
use super::managed::ensure_no_unmanaged_conflict;
use super::{
    CLAUDE_TARGET, CODEX_TARGET, GenerateArgs, MANAGED_BY, MANAGED_VERSION, ManagedMetadata,
    build_generation_input, build_run_command, ensure_generation_targets_are_writable, execute,
    is_managed_content, load_metadata, normalize_config_path, target_path,
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

fn assert_metadata_error(content: &str, expected_message: &str) {
    let write_result = std::fs::write(".codex/hooks.json", content);
    assert!(
        write_result.is_ok(),
        "managed file fixture should be writable"
    );
    assert_eq!(
        load_metadata(&crate::runtime::RealRuntime::default(), Platform::Codex),
        Err(crate::error::HookBridgeError::PlatformProtocol {
            message: expected_message.to_string(),
        })
    );
}

#[path = "tests/generation_core.rs"]
mod generation_core;

#[path = "tests/managed_files.rs"]
mod managed_files;
