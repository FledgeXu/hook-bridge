use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;
use sha2::{Digest, Sha256};

fn parse_stdout_json(output: &[u8]) -> serde_json::Value {
    let parse_result = serde_json::from_slice(output);
    assert!(parse_result.is_ok(), "stdout should be valid json");
    parse_result.unwrap_or_else(|_| unreachable!())
}

fn retry_state_path(
    platform: &str,
    source_config_path: &Path,
    session_id: &str,
    rule_id: &str,
) -> PathBuf {
    let normalized_source_config =
        fs::canonicalize(source_config_path).unwrap_or_else(|_| source_config_path.to_path_buf());
    let mut hasher = Sha256::new();
    hasher.update(normalized_source_config.to_string_lossy().as_bytes());
    hasher.update(session_id.as_bytes());
    let hash = hex::encode(hasher.finalize());

    std::env::temp_dir()
        .join("hook_bridge")
        .join("retries")
        .join(platform)
        .join(hash)
        .join(format!("{rule_id}.json"))
}

#[path = "cli_run/basic.rs"]
mod basic;

#[path = "cli_run/retry_state.rs"]
mod retry_state;

#[path = "cli_run/retry_policy.rs"]
mod retry_policy;

#[path = "cli_run/platform_outputs.rs"]
mod platform_outputs;

#[path = "cli_run/stop_and_feedback.rs"]
mod stop_and_feedback;
