use std::path::PathBuf;

use serde_json::json;

use crate::error::HookBridgeError;
use crate::run::{ExecutionResult, InternalStatus, RuntimeContext};

pub const PLATFORM_NAME: &str = "claude";

/// Parses normalized runtime fields from a Claude hook payload.
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
            message: "claude payload missing required field 'hook_event_name'".to_string(),
        })?;

    let session = payload
        .get("session_id")
        .or_else(|| payload.get("thread_id"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| HookBridgeError::PlatformProtocol {
            message: "claude payload missing required field 'session_id' or 'thread_id'"
                .to_string(),
        })?;

    let cwd = payload
        .get("cwd")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from);
    let transcript = payload
        .get("transcript_path")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from);

    Ok((event.to_string(), session.to_string(), cwd, transcript))
}

#[must_use]
pub fn translate_output(_context: &RuntimeContext, result: &ExecutionResult) -> serde_json::Value {
    match result.status {
        InternalStatus::Success => json!({}),
        InternalStatus::Stop | InternalStatus::Block | InternalStatus::Error => json!({
            "decision": "block",
            "message": result.message,
            "systemMessage": result.system_message,
        }),
    }
}

#[cfg(test)]
mod tests {
    use crate::platform::Platform;
    use crate::run::{ExecutionResult, InternalStatus, RuntimeContext};

    use super::{parse_context_fields, translate_output};

    #[test]
    fn parses_hook_event_name_field() {
        let payload = serde_json::json!({
            "hook_event_name": "before_command",
            "session_id": "s1",
            "cwd": "/tmp",
        });

        let parsed = parse_context_fields(&payload);
        assert!(parsed.is_ok(), "payload should parse");
        let Ok((event, session, cwd, _)) = parsed else {
            return;
        };
        assert_eq!(event, "before_command");
        assert_eq!(session, "s1");
        assert_eq!(cwd, Some(std::path::PathBuf::from("/tmp")));
    }

    #[test]
    fn success_output_omits_decision_field() {
        let context = RuntimeContext {
            platform: Platform::Claude,
            event: "before_command".to_string(),
            rule_id: "r1".to_string(),
            source_config_path: "/tmp/cfg.yaml".into(),
            session_or_thread_id: "s1".to_string(),
            cwd: None,
            transcript_path: None,
            raw_payload: "{}".to_string(),
        };
        let result = ExecutionResult {
            status: InternalStatus::Success,
            message: None,
            system_message: None,
            exit_code: Some(0),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
        };
        let out = translate_output(&context, &result);
        assert!(
            out.get("decision").is_none(),
            "success path should omit decision"
        );
    }
}
