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
    use std::path::PathBuf;

    use crate::platform::Platform;
    use crate::run::{ExecutionResult, InternalStatus, RuntimeContext};

    use super::{parse_context_fields, translate_output};

    #[test]
    fn parses_hook_event_name_field() {
        let payload = serde_json::json!({
            "hook_event_name": "before_command",
            "thread_id": "t1",
            "cwd": "/tmp",
        });

        assert_eq!(
            parse_context_fields(&payload),
            Ok((
                "before_command".to_string(),
                "t1".to_string(),
                Some(PathBuf::from("/tmp")),
                None,
            ))
        );
    }

    #[test]
    fn parses_fallback_event_and_session_fields() {
        let payload = serde_json::json!({
            "event": "after_command",
            "session_id": "s2",
            "transcript_path": "/tmp/transcript.json",
        });

        assert_eq!(
            parse_context_fields(&payload),
            Ok((
                "after_command".to_string(),
                "s2".to_string(),
                None,
                Some(PathBuf::from("/tmp/transcript.json")),
            ))
        );
    }

    #[test]
    fn rejects_missing_required_fields() {
        assert_eq!(
            parse_context_fields(&serde_json::json!({ "thread_id": "t1" })),
            Err(crate::error::HookBridgeError::PlatformProtocol {
                message: "codex payload missing required field 'hook_event_name'".to_string(),
            })
        );
        assert_eq!(
            parse_context_fields(&serde_json::json!({ "hook_event_name": "before_command" })),
            Err(crate::error::HookBridgeError::PlatformProtocol {
                message: "codex payload missing required field 'thread_id' or 'session_id'"
                    .to_string(),
            })
        );
    }

    #[test]
    fn translates_success_and_failure_outputs() {
        let context = RuntimeContext {
            platform: Platform::Codex,
            event: "before_command".to_string(),
            rule_id: "r1".to_string(),
            source_config_path: "/tmp/cfg.yaml".into(),
            session_or_thread_id: "t1".to_string(),
            cwd: None,
            transcript_path: None,
            raw_payload: "{}".to_string(),
        };
        let success = ExecutionResult {
            status: InternalStatus::Success,
            message: None,
            system_message: None,
            exit_code: Some(0),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
        };
        let failure = ExecutionResult {
            status: InternalStatus::Error,
            message: Some("denied".to_string()),
            system_message: Some("bridge blocked".to_string()),
            exit_code: Some(1),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
        };

        assert_eq!(
            translate_output(&context, &success),
            serde_json::json!({
                "event": "before_command",
                "continue": true
            })
        );
        assert_eq!(
            translate_output(&context, &failure),
            serde_json::json!({
                "event": "before_command",
                "continue": false,
                "stopReason": "denied",
                "systemMessage": "bridge blocked",
            })
        );
    }
}
