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
    use std::path::PathBuf;

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

        assert_eq!(
            parsed,
            Ok((
                "before_command".to_string(),
                "s1".to_string(),
                Some(PathBuf::from("/tmp")),
                None,
            ))
        );
    }

    #[test]
    fn parses_fallback_event_and_thread_fields() {
        let payload = serde_json::json!({
            "event": "after_command",
            "thread_id": "t1",
            "transcript_path": "/tmp/transcript.json",
        });

        assert_eq!(
            parse_context_fields(&payload),
            Ok((
                "after_command".to_string(),
                "t1".to_string(),
                None,
                Some(PathBuf::from("/tmp/transcript.json")),
            ))
        );
    }

    #[test]
    fn rejects_missing_required_fields() {
        assert_eq!(
            parse_context_fields(&serde_json::json!({ "session_id": "s1" })),
            Err(crate::error::HookBridgeError::PlatformProtocol {
                message: "claude payload missing required field 'hook_event_name'".to_string(),
            })
        );
        assert_eq!(
            parse_context_fields(&serde_json::json!({ "hook_event_name": "before_command" })),
            Err(crate::error::HookBridgeError::PlatformProtocol {
                message: "claude payload missing required field 'session_id' or 'thread_id'"
                    .to_string(),
            })
        );
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

    #[test]
    fn failure_output_blocks_with_messages() {
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
            status: InternalStatus::Block,
            message: Some("denied".to_string()),
            system_message: Some("bridge blocked".to_string()),
            exit_code: Some(1),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
        };

        assert_eq!(
            translate_output(&context, &result),
            serde_json::json!({
                "decision": "block",
                "message": "denied",
                "systemMessage": "bridge blocked",
            })
        );
    }
}
