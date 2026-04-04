use std::path::{Path, PathBuf};

use crate::cli::RunArgs;
use crate::error::HookBridgeError;
use crate::platform::Platform;
use crate::platform::{claude, codex};

#[derive(Debug, Clone)]
pub struct RuntimeContext {
    pub platform: Platform,
    pub raw_event: String,
    pub event: String,
    pub rule_id: String,
    pub source_config_path: PathBuf,
    pub session_or_thread_id: String,
    pub cwd: Option<PathBuf>,
    pub transcript_path: Option<PathBuf>,
    pub raw_payload: String,
}

/// Parses runtime stdin into a normalized bridge context.
///
/// # Errors
///
/// Returns JSON parse errors and platform protocol errors for invalid runtime payloads.
pub fn parse_runtime_context(
    args: &RunArgs,
    raw_payload: &str,
    source_config_path: &Path,
) -> Result<RuntimeContext, HookBridgeError> {
    let value: serde_json::Value =
        serde_json::from_str(raw_payload).map_err(|error| HookBridgeError::JsonParse {
            message: format!("invalid runtime JSON input: {error}"),
        })?;

    let parsed = match args.platform {
        Platform::Claude => claude::parse_context_fields(&value)?,
        Platform::Codex => codex::parse_context_fields(&value)?,
    };

    Ok(RuntimeContext {
        platform: args.platform,
        raw_event: parsed.raw_event,
        event: parsed.event,
        rule_id: args.rule_id.clone(),
        source_config_path: source_config_path.to_path_buf(),
        session_or_thread_id: parsed.session_or_thread_id,
        cwd: parsed.cwd,
        transcript_path: parsed.transcript_path,
        raw_payload: raw_payload.to_string(),
    })
}
