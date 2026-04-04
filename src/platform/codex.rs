use std::path::PathBuf;

use crate::error::HookBridgeError;
use crate::platform::capability::{self, DecisionKind};
use crate::platform::{ParsedContextFields, Platform, PlatformOutput, normalize_event_name};
use crate::run::{ExecutionResult, InternalStatus, RuntimeContext};

pub const PLATFORM_NAME: &str = "codex";

/// Parses normalized runtime fields from a Codex hook payload.
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
            message: "codex payload missing required field 'hook_event_name'".to_string(),
        })?;
    let normalized_event = normalize_event_name(Platform::Codex, event).ok_or_else(|| {
        HookBridgeError::PlatformProtocol {
            message: format!("codex payload event '{event}' is not supported for platform 'codex'"),
        }
    })?;
    let thread = payload
        .get("session_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| HookBridgeError::PlatformProtocol {
            message: "codex payload missing required field 'session_id'".to_string(),
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
        session_or_thread_id: thread.to_string(),
        cwd,
        transcript_path: transcript,
    })
}

/// Translates an internal execution result into Codex's native hook output.
///
/// # Errors
///
/// Returns a platform-protocol error when the event cannot express the internal decision without
/// degrading semantics, or when the output JSON cannot be serialized.
pub fn translate_output(
    context: &RuntimeContext,
    result: &ExecutionResult,
) -> Result<PlatformOutput, HookBridgeError> {
    match result.status {
        InternalStatus::Success => Ok(PlatformOutput {
            stdout: Vec::new(),
            exit_code: 0,
        }),
        InternalStatus::Stop => stop_output(context, result),
        InternalStatus::Block | InternalStatus::Error => block_output(context, result),
    }
}

fn stop_output(
    context: &RuntimeContext,
    result: &ExecutionResult,
) -> Result<PlatformOutput, HookBridgeError> {
    ensure_decision_supported(context, DecisionKind::Stop)?;

    let mut value = serde_json::Map::new();
    value.insert("continue".to_string(), serde_json::Value::Bool(false));
    if let Some(reason) = stop_reason(result) {
        value.insert("stopReason".to_string(), serde_json::Value::String(reason));
    }
    if let Some(system_message) = result.system_message.clone() {
        value.insert(
            "systemMessage".to_string(),
            serde_json::Value::String(system_message),
        );
    }

    Ok(PlatformOutput {
        stdout: serialize_output(&serde_json::Value::Object(value))?,
        exit_code: 0,
    })
}

fn block_output(
    context: &RuntimeContext,
    result: &ExecutionResult,
) -> Result<PlatformOutput, HookBridgeError> {
    ensure_decision_supported(context, DecisionKind::Block)?;

    let mut value = serde_json::Map::new();
    value.insert(
        "decision".to_string(),
        serde_json::Value::String("block".to_string()),
    );
    if let Some(reason) = block_reason(result) {
        value.insert("reason".to_string(), serde_json::Value::String(reason));
    }
    if let Some(system_message) = result.system_message.clone() {
        value.insert(
            "systemMessage".to_string(),
            serde_json::Value::String(system_message),
        );
    }

    Ok(PlatformOutput {
        stdout: serialize_output(&serde_json::Value::Object(value))?,
        exit_code: 0,
    })
}

fn ensure_decision_supported(
    context: &RuntimeContext,
    decision: DecisionKind,
) -> Result<(), HookBridgeError> {
    if capability::allowed_decisions(Platform::Codex, &context.event).contains(&decision) {
        return Ok(());
    }

    Err(HookBridgeError::PlatformProtocol {
        message: format!(
            "codex event '{}' does not support runtime decision '{decision:?}'",
            context.raw_event
        ),
    })
}

fn block_reason(result: &ExecutionResult) -> Option<String> {
    result
        .message
        .clone()
        .or_else(|| result.system_message.clone())
}

fn stop_reason(result: &ExecutionResult) -> Option<String> {
    result
        .message
        .clone()
        .or_else(|| result.system_message.clone())
}

fn serialize_output(value: &serde_json::Value) -> Result<Vec<u8>, HookBridgeError> {
    let mut stdout =
        serde_json::to_vec(value).map_err(|error| HookBridgeError::PlatformProtocol {
            message: format!("failed to serialize codex output JSON: {error}"),
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
            "session_id": "t1",
            "cwd": "/tmp",
        });

        assert_eq!(
            parse_context_fields(&payload),
            Ok(crate::platform::ParsedContextFields {
                raw_event: "PreToolUse".to_string(),
                event: "before_command".to_string(),
                session_or_thread_id: "t1".to_string(),
                cwd: Some(PathBuf::from("/tmp")),
                transcript_path: None,
            })
        );
    }

    #[test]
    fn preserves_optional_transcript_path() {
        let payload = serde_json::json!({
            "hook_event_name": "after_command",
            "session_id": "s2",
            "transcript_path": "/tmp/transcript.json",
        });

        assert_eq!(
            parse_context_fields(&payload),
            Ok(crate::platform::ParsedContextFields {
                raw_event: "after_command".to_string(),
                event: "after_command".to_string(),
                session_or_thread_id: "s2".to_string(),
                cwd: None,
                transcript_path: Some(PathBuf::from("/tmp/transcript.json")),
            })
        );
    }

    #[test]
    fn rejects_missing_required_fields() {
        assert_eq!(
            parse_context_fields(&serde_json::json!({ "session_id": "t1" })),
            Err(crate::error::HookBridgeError::PlatformProtocol {
                message: "codex payload missing required field 'hook_event_name'".to_string(),
            })
        );
        assert_eq!(
            parse_context_fields(&serde_json::json!({ "hook_event_name": "before_command" })),
            Err(crate::error::HookBridgeError::PlatformProtocol {
                message: "codex payload missing required field 'session_id'".to_string(),
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
                message: "codex payload event 'Notification' is not supported for platform 'codex'"
                    .to_string(),
            })
        );
    }

    #[test]
    fn translates_success_to_empty_output() {
        let context = RuntimeContext {
            platform: Platform::Codex,
            raw_event: "before_command".to_string(),
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

        assert_eq!(
            translate_output(&context, &success),
            Ok(crate::platform::PlatformOutput {
                stdout: Vec::new(),
                exit_code: 0,
            })
        );
    }

    #[test]
    fn translates_before_command_error_to_block_shape() {
        let context = RuntimeContext {
            platform: Platform::Codex,
            raw_event: "PreToolUse".to_string(),
            event: "before_command".to_string(),
            rule_id: "r1".to_string(),
            source_config_path: "/tmp/cfg.yaml".into(),
            session_or_thread_id: "t1".to_string(),
            cwd: None,
            transcript_path: None,
            raw_payload: "{}".to_string(),
        };
        let failure = ExecutionResult {
            status: InternalStatus::Error,
            message: Some("denied".to_string()),
            system_message: Some("bridge blocked".to_string()),
            exit_code: Some(1),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
        };

        let translated = translate_output(&context, &failure);
        assert!(
            translated.is_ok(),
            "before_command block output should serialize"
        );
        let Ok(output) = translated else {
            return;
        };
        let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout);
        assert_eq!(
            parsed.ok(),
            Some(serde_json::json!({
                "decision": "block",
                "reason": "denied",
                "systemMessage": "bridge blocked",
            }))
        );
    }

    #[test]
    fn translates_after_command_stop_to_continue_false_shape() {
        let context = RuntimeContext {
            platform: Platform::Codex,
            raw_event: "PostToolUse".to_string(),
            event: "after_command".to_string(),
            rule_id: "r1".to_string(),
            source_config_path: "/tmp/cfg.yaml".into(),
            session_or_thread_id: "t1".to_string(),
            cwd: None,
            transcript_path: None,
            raw_payload: "{}".to_string(),
        };
        let stop = ExecutionResult {
            status: InternalStatus::Stop,
            message: Some("stop now".to_string()),
            system_message: Some("bridge stopped".to_string()),
            exit_code: Some(0),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
        };

        let translated = translate_output(&context, &stop);
        assert!(
            translated.is_ok(),
            "after_command stop output should serialize"
        );
        let Ok(output) = translated else {
            return;
        };
        let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout);
        assert_eq!(
            parsed.ok(),
            Some(serde_json::json!({
                "continue": false,
                "stopReason": "stop now",
                "systemMessage": "bridge stopped",
            }))
        );
    }

    #[test]
    fn rejects_stop_for_before_command_event() {
        let context = RuntimeContext {
            platform: Platform::Codex,
            raw_event: "PreToolUse".to_string(),
            event: "before_command".to_string(),
            rule_id: "r1".to_string(),
            source_config_path: "/tmp/cfg.yaml".into(),
            session_or_thread_id: "t1".to_string(),
            cwd: None,
            transcript_path: None,
            raw_payload: "{}".to_string(),
        };
        let stop = ExecutionResult {
            status: InternalStatus::Stop,
            message: Some("denied".to_string()),
            system_message: None,
            exit_code: Some(0),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
        };

        assert_eq!(
            translate_output(&context, &stop),
            Err(crate::error::HookBridgeError::PlatformProtocol {
                message: "codex event 'PreToolUse' does not support runtime decision 'Stop'"
                    .to_string(),
            })
        );
    }
}
