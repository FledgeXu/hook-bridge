use std::path::PathBuf;

use crate::error::HookBridgeError;
use crate::platform::capability::{self, DecisionKind};
use crate::platform::{ParsedContextFields, Platform, PlatformOutput, normalize_event_name};
use crate::run::{BridgeOutput, ExecutionResult, InternalStatus, RuntimeContext};

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
/// Returns a platform-protocol error if the output cannot be expressed for the event.
pub fn translate_output(
    context: &RuntimeContext,
    result: &ExecutionResult,
) -> Result<PlatformOutput, HookBridgeError> {
    if let Some(bridge_output) = &result.bridge_output {
        return translate_bridge_output(context, bridge_output);
    }

    match result.status {
        InternalStatus::Success => Ok(PlatformOutput {
            stdout: Vec::new(),
            stderr: Vec::new(),
            exit_code: 0,
        }),
        InternalStatus::Stop => stop_output(
            context,
            result.message.clone(),
            result.system_message.clone(),
        ),
        InternalStatus::Block | InternalStatus::Error => block_output(
            context,
            result.message.clone(),
            result.system_message.clone(),
        ),
    }
}

fn translate_bridge_output(
    context: &RuntimeContext,
    bridge_output: &BridgeOutput,
) -> Result<PlatformOutput, HookBridgeError> {
    match bridge_output {
        BridgeOutput::Success => Ok(PlatformOutput {
            stdout: Vec::new(),
            stderr: Vec::new(),
            exit_code: 0,
        }),
        BridgeOutput::Block {
            reason,
            system_message,
        } => block_output(context, reason.clone(), system_message.clone()),
        BridgeOutput::Error {
            message,
            system_message,
        } => block_output(context, message.clone(), system_message.clone()),
        BridgeOutput::Stop {
            reason,
            system_message,
        } => stop_output(context, reason.clone(), system_message.clone()),
        BridgeOutput::AdditionalContext { text }
            if matches!(
                context.event.as_str(),
                "SessionStart" | "UserPromptSubmit" | "PostToolUse"
            ) =>
        {
            Ok(PlatformOutput {
                stdout: serialize_output(&serde_json::json!({
                    "hookSpecificOutput": {
                        "hookEventName": context.event,
                        "additionalContext": text,
                    }
                }))?,
                stderr: Vec::new(),
                exit_code: 0,
            })
        }
        BridgeOutput::PermissionDecision {
            behavior,
            reason,
            updated_input,
            additional_context,
            ..
        } if context.event == "PreToolUse" => {
            if updated_input.is_some() {
                return Err(HookBridgeError::PlatformProtocol {
                    message: format!(
                        "codex event '{}' does not support permission_decision.updated_input",
                        context.raw_event
                    ),
                });
            }

            let mut hook_specific = serde_json::Map::new();
            hook_specific.insert(
                "hookEventName".to_string(),
                serde_json::Value::String("PreToolUse".to_string()),
            );
            hook_specific.insert(
                "permissionDecision".to_string(),
                serde_json::Value::String(behavior.clone()),
            );
            if let Some(reason) = reason {
                hook_specific.insert(
                    "permissionDecisionReason".to_string(),
                    serde_json::Value::String(reason.clone()),
                );
            }
            if let Some(additional_context) = additional_context {
                hook_specific.insert(
                    "additionalContext".to_string(),
                    serde_json::Value::String(additional_context.clone()),
                );
            }

            Ok(PlatformOutput {
                stdout: serialize_output(&serde_json::json!({
                    "hookSpecificOutput": hook_specific,
                }))?,
                stderr: Vec::new(),
                exit_code: 0,
            })
        }
        other => Err(HookBridgeError::PlatformProtocol {
            message: format!(
                "codex event '{}' does not support bridge output '{other:?}'",
                context.raw_event
            ),
        }),
    }
}

fn stop_output(
    context: &RuntimeContext,
    reason: Option<String>,
    system_message: Option<String>,
) -> Result<PlatformOutput, HookBridgeError> {
    ensure_decision_supported(context, DecisionKind::Stop)?;

    let mut value = serde_json::Map::new();
    value.insert("continue".to_string(), serde_json::Value::Bool(false));
    if let Some(reason) = reason {
        value.insert("stopReason".to_string(), serde_json::Value::String(reason));
    }
    if let Some(system_message) = system_message {
        value.insert(
            "systemMessage".to_string(),
            serde_json::Value::String(system_message),
        );
    }

    Ok(PlatformOutput {
        stdout: serialize_output(&serde_json::Value::Object(value))?,
        stderr: Vec::new(),
        exit_code: 0,
    })
}

fn block_output(
    context: &RuntimeContext,
    reason: Option<String>,
    system_message: Option<String>,
) -> Result<PlatformOutput, HookBridgeError> {
    ensure_decision_supported(context, DecisionKind::Block)?;

    let mut value = serde_json::Map::new();
    value.insert(
        "decision".to_string(),
        serde_json::Value::String("block".to_string()),
    );
    if let Some(reason) = reason {
        value.insert("reason".to_string(), serde_json::Value::String(reason));
    }
    if let Some(system_message) = system_message {
        value.insert(
            "systemMessage".to_string(),
            serde_json::Value::String(system_message),
        );
    }

    Ok(PlatformOutput {
        stdout: serialize_output(&serde_json::Value::Object(value))?,
        stderr: Vec::new(),
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
    use crate::run::{BridgeOutput, ExecutionResult, InternalStatus, RuntimeContext};

    use super::{parse_context_fields, translate_output};

    fn context(event: &str) -> RuntimeContext {
        RuntimeContext {
            platform: Platform::Codex,
            raw_event: event.to_string(),
            event: event.to_string(),
            rule_id: "r1".to_string(),
            source_config_path: "/tmp/cfg.yaml".into(),
            session_or_thread_id: "t1".to_string(),
            cwd: None,
            transcript_path: None,
            raw_payload: "{}".to_string(),
        }
    }

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
                event: "PreToolUse".to_string(),
                session_or_thread_id: "t1".to_string(),
                cwd: Some(PathBuf::from("/tmp")),
                transcript_path: None,
            })
        );
    }

    #[test]
    fn translates_success_to_empty_output() {
        let success = ExecutionResult {
            status: InternalStatus::Success,
            message: None,
            system_message: None,
            exit_code: Some(0),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
            bridge_output: None,
        };

        assert_eq!(
            translate_output(&context("PreToolUse"), &success),
            Ok(crate::platform::PlatformOutput {
                stdout: Vec::new(),
                stderr: Vec::new(),
                exit_code: 0,
            })
        );
    }

    #[test]
    fn translates_additional_context_for_session_start() {
        let result = ExecutionResult {
            status: InternalStatus::Success,
            message: None,
            system_message: None,
            exit_code: Some(0),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
            bridge_output: Some(BridgeOutput::AdditionalContext {
                text: "Load conventions".to_string(),
            }),
        };

        let translated = translate_output(&context("SessionStart"), &result);
        assert!(translated.is_ok());
        let output = translated.unwrap_or_else(|_| unreachable!());
        let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
        assert_eq!(
            parsed,
            Some(serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "SessionStart",
                    "additionalContext": "Load conventions"
                }
            }))
        );
    }

    #[test]
    fn translates_stop_output_to_continue_false_shape() {
        let stop = ExecutionResult {
            status: InternalStatus::Stop,
            message: Some("stop now".to_string()),
            system_message: Some("bridge stopped".to_string()),
            exit_code: Some(0),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
            bridge_output: None,
        };

        let translated = translate_output(&context("Stop"), &stop);
        assert!(translated.is_ok());
        let output = translated.unwrap_or_else(|_| unreachable!());
        let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
        assert_eq!(
            parsed,
            Some(serde_json::json!({
                "continue": false,
                "stopReason": "stop now",
                "systemMessage": "bridge stopped",
            }))
        );
    }

    #[test]
    fn preserves_optional_transcript_path() {
        let payload = serde_json::json!({
            "hook_event_name": "PostToolUse",
            "session_id": "t1",
            "transcript_path": "/tmp/transcript.json",
        });

        assert_eq!(
            parse_context_fields(&payload),
            Ok(crate::platform::ParsedContextFields {
                raw_event: "PostToolUse".to_string(),
                event: "PostToolUse".to_string(),
                session_or_thread_id: "t1".to_string(),
                cwd: None,
                transcript_path: Some(PathBuf::from("/tmp/transcript.json")),
            })
        );
    }

    #[test]
    fn rejects_missing_required_fields_and_unsupported_event() {
        assert!(matches!(
            parse_context_fields(&serde_json::json!({ "session_id": "t1" })),
            Err(crate::error::HookBridgeError::PlatformProtocol { .. })
        ));
        assert!(matches!(
            parse_context_fields(&serde_json::json!({
                "hook_event_name": "Notification",
                "session_id": "t1",
            })),
            Err(crate::error::HookBridgeError::PlatformProtocol { .. })
        ));
    }

    #[test]
    fn translates_block_output_to_decision_shape() {
        let failure = ExecutionResult {
            status: InternalStatus::Block,
            message: Some("deny".to_string()),
            system_message: Some("bridge blocked".to_string()),
            exit_code: Some(1),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
            bridge_output: None,
        };

        let translated = translate_output(&context("PreToolUse"), &failure);
        assert!(translated.is_ok());
        let output = translated.unwrap_or_else(|_| unreachable!());
        let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
        assert_eq!(
            parsed,
            Some(serde_json::json!({
                "decision": "block",
                "reason": "deny",
                "systemMessage": "bridge blocked",
            }))
        );
    }

    #[test]
    fn translates_permission_decision_for_pre_tool_use() {
        let result = ExecutionResult {
            status: InternalStatus::Success,
            message: None,
            system_message: None,
            exit_code: Some(0),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
            bridge_output: Some(BridgeOutput::PermissionDecision {
                behavior: "deny".to_string(),
                reason: Some("blocked".to_string()),
                updated_input: None,
                additional_context: Some("extra".to_string()),
            }),
        };

        let translated = translate_output(&context("PreToolUse"), &result);
        assert!(translated.is_ok());
        let output = translated.unwrap_or_else(|_| unreachable!());
        let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
        assert_eq!(
            parsed,
            Some(serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "deny",
                    "permissionDecisionReason": "blocked",
                    "additionalContext": "extra"
                }
            }))
        );
    }

    #[test]
    fn rejects_permission_decision_updated_input_for_pre_tool_use() {
        let result = ExecutionResult {
            status: InternalStatus::Success,
            message: None,
            system_message: None,
            exit_code: Some(0),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
            bridge_output: Some(BridgeOutput::PermissionDecision {
                behavior: "allow".to_string(),
                reason: Some("rewritten".to_string()),
                updated_input: Some(serde_json::json!({
                    "command": "safe-command --flag"
                })),
                additional_context: None,
            }),
        };

        assert!(matches!(
            translate_output(&context("PreToolUse"), &result),
            Err(crate::error::HookBridgeError::PlatformProtocol { .. })
        ));
    }

    #[test]
    fn rejects_permission_decision_for_unsupported_event() {
        let result = ExecutionResult {
            status: InternalStatus::Success,
            message: None,
            system_message: None,
            exit_code: Some(0),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
            bridge_output: Some(BridgeOutput::PermissionDecision {
                behavior: "allow".to_string(),
                reason: Some("safe".to_string()),
                updated_input: None,
                additional_context: Some("note".to_string()),
            }),
        };

        assert!(matches!(
            translate_output(&context("Stop"), &result),
            Err(crate::error::HookBridgeError::PlatformProtocol { .. })
        ));
    }

    #[test]
    fn rejects_stop_for_pre_tool_use_and_additional_context_for_unsupported_event() {
        let stop = ExecutionResult {
            status: InternalStatus::Stop,
            message: Some("nope".to_string()),
            system_message: None,
            exit_code: Some(0),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
            bridge_output: None,
        };
        assert!(matches!(
            translate_output(&context("PreToolUse"), &stop),
            Err(crate::error::HookBridgeError::PlatformProtocol { .. })
        ));

        let result = ExecutionResult {
            status: InternalStatus::Success,
            message: None,
            system_message: None,
            exit_code: Some(0),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
            bridge_output: Some(BridgeOutput::AdditionalContext {
                text: "ignored".to_string(),
            }),
        };
        assert!(matches!(
            translate_output(&context("Stop"), &result),
            Err(crate::error::HookBridgeError::PlatformProtocol { .. })
        ));
    }
}
