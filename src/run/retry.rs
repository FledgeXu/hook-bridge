use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::PlatformRule;
use crate::error::HookBridgeError;
use crate::runtime::Runtime;
use crate::runtime::fs::atomic_write;

use super::{ExecutionResult, InternalStatus, RuntimeContext};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RetryState {
    pub(crate) consecutive_failures: u32,
    pub(crate) last_error: String,
    pub(crate) last_failure_epoch_sec: u64,
}

pub(crate) fn retry_guard_engaged(rule: &PlatformRule, state: &RetryState) -> bool {
    rule.max_retries > 0 && state.consecutive_failures >= rule.max_retries
}

pub(crate) fn retry_guard_result() -> ExecutionResult {
    ExecutionResult {
        status: InternalStatus::Stop,
        message: Some("max retries reached, skipping command execution".to_string()),
        system_message: Some("hook_bridge retry guard engaged".to_string()),
        exit_code: Some(0),
        raw_stdout: Vec::new(),
        raw_stderr: Vec::new(),
    }
}

pub(crate) fn update_retry_state(
    runtime: &dyn Runtime,
    path: &Path,
    state: &RetryState,
    result: &ExecutionResult,
) -> Result<(), HookBridgeError> {
    match result.status {
        InternalStatus::Success => runtime.fs().remove_file_if_exists(path),
        InternalStatus::Block | InternalStatus::Error => persist_failure_state(
            runtime,
            path,
            state,
            result
                .message
                .clone()
                .unwrap_or_else(|| "execution failed".to_string()),
        ),
        InternalStatus::Stop => Ok(()),
    }
}

pub(crate) fn now_epoch_sec(runtime: &dyn Runtime) -> Result<u64, HookBridgeError> {
    runtime
        .clock()
        .now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| HookBridgeError::Process {
            message: format!("clock error: {error}"),
        })
}

fn retry_state_root(runtime: &dyn Runtime) -> PathBuf {
    runtime.temp_dir().join("hook_bridge").join("retries")
}

pub(crate) fn retry_state_path(runtime: &dyn Runtime, context: &RuntimeContext) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(context.source_config_path.to_string_lossy().as_bytes());
    hasher.update(context.session_or_thread_id.as_bytes());
    let hash = hex::encode(hasher.finalize());

    retry_state_root(runtime)
        .join(context.platform.as_str())
        .join(hash)
        .join(format!("{}.json", context.rule_id))
}

pub(crate) fn load_retry_state(
    runtime: &dyn Runtime,
    path: &Path,
) -> Result<RetryState, HookBridgeError> {
    if !runtime.fs().exists(path)? {
        return Ok(RetryState {
            consecutive_failures: 0,
            last_error: String::new(),
            last_failure_epoch_sec: 0,
        });
    }

    let content = runtime.fs().read_to_string(path)?;
    serde_json::from_str(&content).map_err(|error| HookBridgeError::JsonParse {
        message: format!("invalid retry state JSON at '{}': {error}", path.display()),
    })
}

pub(crate) fn persist_retry_state(
    runtime: &dyn Runtime,
    path: &Path,
    state: &RetryState,
) -> Result<(), HookBridgeError> {
    let payload = serde_json::to_vec_pretty(state).map_err(|error| HookBridgeError::Process {
        message: format!("failed to serialize retry state: {error}"),
    })?;
    atomic_write(runtime.fs(), path, &payload)
}

fn persist_failure_state(
    runtime: &dyn Runtime,
    path: &Path,
    current: &RetryState,
    last_error: String,
) -> Result<(), HookBridgeError> {
    let updated = RetryState {
        consecutive_failures: current.consecutive_failures.saturating_add(1),
        last_error,
        last_failure_epoch_sec: now_epoch_sec(runtime)?,
    };
    persist_retry_state(runtime, path, &updated)
}
