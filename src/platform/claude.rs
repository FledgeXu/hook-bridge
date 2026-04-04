use std::path::PathBuf;

use serde_json::json;

use crate::error::HookBridgeError;
use crate::platform::{ParsedContextFields, Platform, PlatformOutput, normalize_event_name};
use crate::run::{ExecutionResult, InternalStatus, RuntimeContext};

pub const PLATFORM_NAME: &str = "claude";

/// Parses normalized runtime fields from a Claude hook payload.
///
/// # Errors
///
/// Returns an error when required fields are missing or invalid.
pub fn parse_context_fields(
    payload: &serde_json::Value,
) -> Result<ParsedContextFields, HookBridgeError> {
    let event = payload
        .get("hook_event_name")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| HookBridgeError::PlatformProtocol {
            message: "claude payload missing required field 'hook_event_name'".to_string(),
        })?;
    let normalized_event = normalize_event_name(Platform::Claude, event).ok_or_else(|| {
        HookBridgeError::PlatformProtocol {
            message: format!(
                "claude payload event '{event}' is not supported for platform 'claude'"
            ),
        }
    })?;

    let session = payload
        .get("session_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| HookBridgeError::PlatformProtocol {
            message: "claude payload missing required field 'session_id'".to_string(),
        })?;

    let cwd = payload
        .get("cwd")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from);
    let transcript = payload
        .get("transcript_path")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from);

    Ok(ParsedContextFields {
        raw_event: event.to_string(),
        event: normalized_event.to_string(),
        session_or_thread_id: session.to_string(),
        cwd,
        transcript_path: transcript,
    })
}

/// Translates an internal execution result into Claude's native hook output.
///
/// # Errors
///
/// Returns a platform-protocol error if the output JSON cannot be serialized.
pub fn translate_output(
    _context: &RuntimeContext,
    result: &ExecutionResult,
) -> Result<PlatformOutput, HookBridgeError> {
    let payload = match result.status {
        InternalStatus::Success => {
            return Ok(PlatformOutput {
                stdout: Vec::new(),
                exit_code: 0,
            });
        }
        InternalStatus::Stop | InternalStatus::Block | InternalStatus::Error => json!({
            "decision": "block",
            "message": result.message,
            "systemMessage": result.system_message,
        }),
    };

    Ok(PlatformOutput {
        stdout: serialize_output(&payload)?,
        exit_code: 0,
    })
}

fn serialize_output(value: &serde_json::Value) -> Result<Vec<u8>, HookBridgeError> {
    let mut stdout =
        serde_json::to_vec(value).map_err(|error| HookBridgeError::PlatformProtocol {
            message: format!("failed to serialize claude output JSON: {error}"),
        })?;
    stdout.push(b'\n');
    Ok(stdout)
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
            "hook_event_name": "PreToolUse",
            "session_id": "s1",
            "cwd": "/tmp",
        });

        let parsed = parse_context_fields(&payload);

        assert_eq!(
            parsed,
            Ok(crate::platform::ParsedContextFields {
                raw_event: "PreToolUse".to_string(),
                event: "before_command".to_string(),
                session_or_thread_id: "s1".to_string(),
                cwd: Some(PathBuf::from("/tmp")),
                transcript_path: None,
            })
        );
    }

    #[test]
    fn preserves_optional_transcript_path() {
        let payload = serde_json::json!({
            "hook_event_name": "after_command",
            "session_id": "s1",
            "transcript_path": "/tmp/transcript.json",
        });

        assert_eq!(
            parse_context_fields(&payload),
            Ok(crate::platform::ParsedContextFields {
                raw_event: "after_command".to_string(),
                event: "after_command".to_string(),
                session_or_thread_id: "s1".to_string(),
                cwd: None,
                transcript_path: Some(PathBuf::from("/tmp/transcript.json")),
            })
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
                message: "claude payload missing required field 'session_id'".to_string(),
            })
        );
    }

    #[test]
    fn rejects_unsupported_event_names() {
        assert_eq!(
            parse_context_fields(&serde_json::json!({
                "hook_event_name": "Notification",
                "session_id": "s1",
            })),
            Err(crate::error::HookBridgeError::PlatformProtocol {
                message:
                    "claude payload event 'Notification' is not supported for platform 'claude'"
                        .to_string(),
            })
        );
    }

    #[test]
    fn success_output_emits_empty_stdout() {
        let context = RuntimeContext {
            platform: Platform::Claude,
            raw_event: "before_command".to_string(),
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
        assert_eq!(
            translate_output(&context, &result),
            Ok(crate::platform::PlatformOutput {
                stdout: Vec::new(),
                exit_code: 0,
            })
        );
    }

    #[test]
    fn failure_output_blocks_with_messages() {
        let context = RuntimeContext {
            platform: Platform::Claude,
            raw_event: "before_command".to_string(),
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

        let translated = translate_output(&context, &result);
        assert!(translated.is_ok(), "claude block output should serialize");
        let Ok(output) = translated else {
            return;
        };
        assert_eq!(output.exit_code, 0);
        let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout);
        assert!(parsed.is_ok(), "claude block output should be JSON");
        assert_eq!(
            parsed.ok(),
            Some(serde_json::json!({
                "decision": "block",
                "message": "denied",
                "systemMessage": "bridge blocked",
            }))
        );
    }
}
