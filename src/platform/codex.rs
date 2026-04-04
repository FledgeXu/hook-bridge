use std::path::PathBuf;

use serde_json::json;

use crate::error::HookBridgeError;
use crate::run::{ExecutionResult, InternalStatus, RuntimeContext};

pub const PLATFORM_NAME: &str = "codex";

/// Parses normalized runtime fields from a Codex hook payload.
///
/// # Errors
///
/// Returns an error when required fields are missing or invalid.
pub fn parse_context_fields(
    payload: &serde_json::Value,
) -> Result<(String, String, Option<PathBuf>, Option<PathBuf>), HookBridgeError> {
    let event = payload
        .get("hook_event_name")
        .and_then(serde_json::Value::as_str)
        .or_else(|| payload.get("event").and_then(serde_json::Value::as_str))
        .ok_or_else(|| HookBridgeError::PlatformProtocol {
            message: "codex payload missing required field 'hook_event_name'".to_string(),
        })?;
    let thread = payload
        .get("thread_id")
        .or_else(|| payload.get("session_id"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| HookBridgeError::PlatformProtocol {
            message: "codex payload missing required field 'thread_id' or 'session_id'".to_string(),
        })?;

    let cwd = payload
        .get("cwd")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from);
    let transcript = payload
        .get("transcript_path")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from);

    Ok((event.to_string(), thread.to_string(), cwd, transcript))
}

#[must_use]
pub fn translate_output(context: &RuntimeContext, result: &ExecutionResult) -> serde_json::Value {
    match result.status {
        InternalStatus::Success => json!({
            "event": context.event,
            "continue": true
        }),
        InternalStatus::Stop | InternalStatus::Block | InternalStatus::Error => json!({
            "event": context.event,
            "continue": false,
            "stopReason": result.message,
            "systemMessage": result.system_message,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_context_fields;

    #[test]
    fn parses_hook_event_name_field() {
        let payload = serde_json::json!({
            "hook_event_name": "before_command",
            "thread_id": "t1",
            "cwd": "/tmp",
        });

        let parsed = parse_context_fields(&payload);
        assert!(parsed.is_ok(), "payload should parse");
        let Ok((event, thread, cwd, _)) = parsed else {
            return;
        };
        assert_eq!(event, "before_command");
        assert_eq!(thread, "t1");
        assert_eq!(cwd, Some(std::path::PathBuf::from("/tmp")));
    }
}
