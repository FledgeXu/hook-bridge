use std::path::PathBuf;

use serde_json::json;

use crate::error::HookBridgeError;
use crate::platform::{ParsedContextFields, Platform, PlatformOutput, normalize_event_name};
use crate::run::{BridgeOutput, ExecutionResult, InternalStatus, RuntimeContext};

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
/// Returns a platform-protocol error if the output cannot be expressed for the event.
pub fn translate_output(
    context: &RuntimeContext,
    result: &ExecutionResult,
) -> Result<PlatformOutput, HookBridgeError> {
    if let Some(output) = permission_request_exit_two_output(context, result)? {
        return Ok(output);
    }

    if let Some(output) = teammate_feedback_output(context, result) {
        return Ok(output);
    }

    if let Some(bridge_output) = &result.bridge_output {
        return translate_bridge_output(context, bridge_output);
    }

    let bridge_output = match result.status {
        InternalStatus::Success => return Ok(empty_output()),
        InternalStatus::Stop => BridgeOutput::Stop {
            reason: result.message.clone(),
            system_message: result.system_message.clone(),
        },
        InternalStatus::Block => BridgeOutput::Block {
            reason: result.message.clone(),
            system_message: result.system_message.clone(),
        },
        InternalStatus::Error => BridgeOutput::Error {
            message: result.message.clone(),
            system_message: result.system_message.clone(),
        },
    };

    translate_bridge_output(context, &bridge_output)
}

fn translate_bridge_output(
    context: &RuntimeContext,
    bridge_output: &BridgeOutput,
) -> Result<PlatformOutput, HookBridgeError> {
    match bridge_output {
        BridgeOutput::Success => Ok(empty_output()),
        BridgeOutput::Block { reason, .. }
        | BridgeOutput::Error {
            message: reason, ..
        } if supports_top_level_block(&context.event) => Ok(json_output(&json!({
            "decision": "block",
            "reason": reason,
        }))?),
        BridgeOutput::Stop { reason, .. } if supports_continue_false(&context.event) => {
            Ok(json_output(&json!({
                "continue": false,
                "stopReason": reason,
            }))?)
        }
        BridgeOutput::AdditionalContext { text } if supports_additional_context(&context.event) => {
            Ok(json_output(&json!({
                "hookSpecificOutput": {
                    "additionalContext": text,
                }
            }))?)
        }
        BridgeOutput::PermissionDecision {
            behavior,
            reason,
            updated_input,
            additional_context,
        } if context.event == "PermissionRequest" => permission_request_output(
            behavior,
            reason.as_ref(),
            updated_input.as_ref(),
            additional_context.as_deref(),
        ),
        BridgeOutput::PermissionRetry { .. } if context.event == "PermissionDenied" => {
            Ok(json_output(&json!({
                "hookSpecificOutput": {
                    "retry": true,
                }
            }))?)
        }
        BridgeOutput::WorktreePath { path } if context.event == "WorktreeCreate" => {
            Ok(PlatformOutput {
                stdout: format!("{path}\n").into_bytes(),
                stderr: Vec::new(),
                exit_code: 0,
            })
        }
        BridgeOutput::ElicitationResponse { action, content }
            if matches!(context.event.as_str(), "Elicitation" | "ElicitationResult") =>
        {
            let mut hook_specific = serde_json::Map::new();
            hook_specific.insert(
                "action".to_string(),
                serde_json::Value::String(action.clone()),
            );
            if let Some(content) = content {
                hook_specific.insert("content".to_string(), content.clone());
            }

            Ok(json_output(&serde_json::Value::Object(
                serde_json::Map::from_iter([(
                    "hookSpecificOutput".to_string(),
                    serde_json::Value::Object(hook_specific),
                )]),
            ))?)
        }
        BridgeOutput::Block { .. }
        | BridgeOutput::Stop { .. }
        | BridgeOutput::AdditionalContext { .. }
        | BridgeOutput::PermissionDecision { .. }
        | BridgeOutput::PermissionRetry { .. }
        | BridgeOutput::WorktreePath { .. }
        | BridgeOutput::ElicitationResponse { .. }
        | BridgeOutput::Error { .. } => Err(HookBridgeError::PlatformProtocol {
            message: format!(
                "claude event '{}' does not support bridge output '{bridge_output:?}'",
                context.raw_event
            ),
        }),
    }
}

fn supports_top_level_block(event: &str) -> bool {
    matches!(
        event,
        "PreToolUse"
            | "UserPromptSubmit"
            | "PostToolUse"
            | "PostToolUseFailure"
            | "Stop"
            | "SubagentStop"
            | "ConfigChange"
    )
}

fn supports_continue_false(event: &str) -> bool {
    matches!(
        event,
        "Stop" | "SubagentStop" | "TaskCreated" | "TaskCompleted" | "TeammateIdle"
    )
}

fn supports_additional_context(event: &str) -> bool {
    matches!(
        event,
        "SessionStart" | "UserPromptSubmit" | "PostToolUse" | "PostToolUseFailure" | "Notification"
    )
}

fn permission_request_output(
    behavior: &str,
    reason: Option<&String>,
    updated_input: Option<&serde_json::Value>,
    additional_context: Option<&str>,
) -> Result<PlatformOutput, HookBridgeError> {
    let mut decision = serde_json::Map::from_iter([(
        "behavior".to_string(),
        serde_json::Value::String(behavior.to_string()),
    )]);
    if let Some(updated_input) = updated_input {
        decision.insert("updatedInput".to_string(), updated_input.clone());
    }

    let mut hook_specific =
        serde_json::Map::from_iter([("decision".to_string(), serde_json::Value::Object(decision))]);
    if let Some(reason) = reason {
        hook_specific.insert(
            "reason".to_string(),
            serde_json::Value::String(reason.clone()),
        );
    }
    if let Some(additional_context) = additional_context {
        hook_specific.insert(
            "additionalContext".to_string(),
            serde_json::Value::String(additional_context.to_string()),
        );
    }

    json_output(&serde_json::Value::Object(serde_json::Map::from_iter([(
        "hookSpecificOutput".to_string(),
        serde_json::Value::Object(hook_specific),
    )])))
}

fn permission_request_exit_two_output(
    context: &RuntimeContext,
    result: &ExecutionResult,
) -> Result<Option<PlatformOutput>, HookBridgeError> {
    if context.event != "PermissionRequest"
        || result.exit_code != Some(2)
        || result.bridge_output.is_some()
    {
        return Ok(None);
    }

    let reason = std::str::from_utf8(&result.raw_stderr)
        .ok()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| result.message.clone());

    Ok(Some(permission_request_output(
        "deny",
        reason.as_ref(),
        None,
        None,
    )?))
}

fn empty_output() -> PlatformOutput {
    PlatformOutput {
        stdout: Vec::new(),
        stderr: Vec::new(),
        exit_code: 0,
    }
}

fn json_output(value: &serde_json::Value) -> Result<PlatformOutput, HookBridgeError> {
    Ok(PlatformOutput {
        stdout: serialize_output(value)?,
        stderr: Vec::new(),
        exit_code: 0,
    })
}

fn teammate_feedback_output(
    context: &RuntimeContext,
    result: &ExecutionResult,
) -> Option<PlatformOutput> {
    if !matches!(
        context.event.as_str(),
        "TaskCreated" | "TaskCompleted" | "TeammateIdle"
    ) {
        return None;
    }
    if result.exit_code != Some(2) || result.bridge_output.is_some() {
        return None;
    }

    Some(PlatformOutput {
        stdout: Vec::new(),
        stderr: result.raw_stderr.clone(),
        exit_code: 2,
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
    use crate::run::{BridgeOutput, ExecutionResult, InternalStatus, RuntimeContext};

    use super::{parse_context_fields, translate_output};

    fn context(event: &str) -> RuntimeContext {
        RuntimeContext {
            platform: Platform::Claude,
            raw_event: event.to_string(),
            event: event.to_string(),
            rule_id: "r1".to_string(),
            source_config_path: "/tmp/cfg.yaml".into(),
            session_or_thread_id: "s1".to_string(),
            cwd: None,
            transcript_path: None,
            raw_payload: "{}".to_string(),
        }
    }

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
                event: "PreToolUse".to_string(),
                session_or_thread_id: "s1".to_string(),
                cwd: Some(PathBuf::from("/tmp")),
                transcript_path: None,
            })
        );
    }

    #[test]
    fn preserves_optional_transcript_path() {
        let payload = serde_json::json!({
            "hook_event_name": "Notification",
            "session_id": "s1",
            "transcript_path": "/tmp/transcript.json",
        });

        assert_eq!(
            parse_context_fields(&payload),
            Ok(crate::platform::ParsedContextFields {
                raw_event: "Notification".to_string(),
                event: "Notification".to_string(),
                session_or_thread_id: "s1".to_string(),
                cwd: None,
                transcript_path: Some(PathBuf::from("/tmp/transcript.json")),
            })
        );
    }

    #[test]
    fn translates_success_to_empty_stdout() {
        let result = ExecutionResult {
            status: InternalStatus::Success,
            message: None,
            system_message: None,
            exit_code: Some(0),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
            bridge_output: None,
        };

        assert_eq!(
            translate_output(&context("Notification"), &result),
            Ok(crate::platform::PlatformOutput {
                stdout: Vec::new(),
                stderr: Vec::new(),
                exit_code: 0,
            })
        );
    }

    #[test]
    fn translates_additional_context_output_for_session_start() {
        let result = ExecutionResult {
            status: InternalStatus::Success,
            message: None,
            system_message: None,
            exit_code: Some(0),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
            bridge_output: Some(BridgeOutput::AdditionalContext {
                text: "Read the repo rules".to_string(),
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
                    "additionalContext": "Read the repo rules"
                }
            }))
        );
    }

    #[test]
    fn translates_worktree_create_to_plain_path() {
        let result = ExecutionResult {
            status: InternalStatus::Success,
            message: None,
            system_message: None,
            exit_code: Some(0),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
            bridge_output: Some(BridgeOutput::WorktreePath {
                path: "/tmp/worktree".to_string(),
            }),
        };

        assert_eq!(
            translate_output(&context("WorktreeCreate"), &result),
            Ok(crate::platform::PlatformOutput {
                stdout: b"/tmp/worktree\n".to_vec(),
                stderr: Vec::new(),
                exit_code: 0,
            })
        );
    }

    #[test]
    fn translates_exit_code_two_feedback_for_task_completed() {
        let result = ExecutionResult {
            status: InternalStatus::Block,
            message: Some("feedback".to_string()),
            system_message: None,
            exit_code: Some(2),
            raw_stdout: Vec::new(),
            raw_stderr: b"ask the user for clarification\n".to_vec(),
            bridge_output: None,
        };

        assert_eq!(
            translate_output(&context("TaskCompleted"), &result),
            Ok(crate::platform::PlatformOutput {
                stdout: Vec::new(),
                stderr: b"ask the user for clarification\n".to_vec(),
                exit_code: 2,
            })
        );
    }

    #[test]
    fn translates_exit_code_two_permission_request_to_deny_decision() {
        let result = ExecutionResult {
            status: InternalStatus::Block,
            message: Some("command failed with exit code 2: deny permission".to_string()),
            system_message: None,
            exit_code: Some(2),
            raw_stdout: Vec::new(),
            raw_stderr: b"permission denied by policy\n".to_vec(),
            bridge_output: None,
        };

        let translated = translate_output(&context("PermissionRequest"), &result);
        assert!(translated.is_ok());
        let output = translated.unwrap_or_else(|_| unreachable!());
        let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
        assert_eq!(
            parsed,
            Some(serde_json::json!({
                "hookSpecificOutput": {
                    "decision": {
                        "behavior": "deny"
                    },
                    "reason": "permission denied by policy"
                }
            }))
        );
    }

    #[test]
    fn translates_block_output_for_user_prompt_submit() {
        let result = ExecutionResult {
            status: InternalStatus::Block,
            message: None,
            system_message: None,
            exit_code: Some(0),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
            bridge_output: Some(BridgeOutput::Block {
                reason: Some("confirm first".to_string()),
                system_message: None,
            }),
        };

        let translated = translate_output(&context("UserPromptSubmit"), &result);
        assert!(translated.is_ok());
        let output = translated.unwrap_or_else(|_| unreachable!());
        let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
        assert_eq!(
            parsed,
            Some(serde_json::json!({
                "decision": "block",
                "reason": "confirm first"
            }))
        );
    }

    #[test]
    fn translates_non_structured_block_for_pre_tool_use() {
        let result = ExecutionResult {
            status: InternalStatus::Block,
            message: Some("deny tool".to_string()),
            system_message: Some("bridge blocked".to_string()),
            exit_code: Some(1),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
            bridge_output: None,
        };

        let translated = translate_output(&context("PreToolUse"), &result);
        assert!(translated.is_ok());
        let output = translated.unwrap_or_else(|_| unreachable!());
        let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
        assert_eq!(
            parsed,
            Some(serde_json::json!({
                "decision": "block",
                "reason": "deny tool"
            }))
        );
    }

    #[test]
    fn translates_continue_false_output_for_task_completed() {
        let result = ExecutionResult {
            status: InternalStatus::Stop,
            message: None,
            system_message: None,
            exit_code: Some(0),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
            bridge_output: Some(BridgeOutput::Stop {
                reason: Some("halt teammate".to_string()),
                system_message: None,
            }),
        };

        let translated = translate_output(&context("TaskCompleted"), &result);
        assert!(translated.is_ok());
        let output = translated.unwrap_or_else(|_| unreachable!());
        let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
        assert_eq!(
            parsed,
            Some(serde_json::json!({
                "continue": false,
                "stopReason": "halt teammate"
            }))
        );
    }

    #[test]
    fn translates_permission_request_decision_behavior() {
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
                additional_context: None,
            }),
        };

        let translated = translate_output(&context("PermissionRequest"), &result);
        assert!(translated.is_ok());
        let output = translated.unwrap_or_else(|_| unreachable!());
        let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
        assert_eq!(
            parsed,
            Some(serde_json::json!({
                "hookSpecificOutput": {
                    "decision": {
                        "behavior": "deny"
                    },
                    "reason": "blocked"
                }
            }))
        );
    }

    #[test]
    fn translates_permission_request_updated_input() {
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

        let translated = translate_output(&context("PermissionRequest"), &result);
        assert!(translated.is_ok());
        let output = translated.unwrap_or_else(|_| unreachable!());
        let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
        assert_eq!(
            parsed,
            Some(serde_json::json!({
                "hookSpecificOutput": {
                    "decision": {
                        "behavior": "allow",
                        "updatedInput": {
                            "command": "safe-command --flag"
                        }
                    },
                    "reason": "rewritten"
                }
            }))
        );
    }

    #[test]
    fn translates_permission_request_additional_context() {
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
                additional_context: Some("show the safe rewrite to the user".to_string()),
            }),
        };

        let translated = translate_output(&context("PermissionRequest"), &result);
        assert!(translated.is_ok());
        let output = translated.unwrap_or_else(|_| unreachable!());
        let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
        assert_eq!(
            parsed,
            Some(serde_json::json!({
                "hookSpecificOutput": {
                    "decision": {
                        "behavior": "allow",
                        "updatedInput": {
                            "command": "safe-command --flag"
                        }
                    },
                    "reason": "rewritten",
                    "additionalContext": "show the safe rewrite to the user"
                }
            }))
        );
    }

    #[test]
    fn translates_permission_retry_output() {
        let result = ExecutionResult {
            status: InternalStatus::Success,
            message: None,
            system_message: None,
            exit_code: Some(0),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
            bridge_output: Some(BridgeOutput::PermissionRetry { reason: None }),
        };

        let translated = translate_output(&context("PermissionDenied"), &result);
        assert!(translated.is_ok());
        let output = translated.unwrap_or_else(|_| unreachable!());
        let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
        assert_eq!(
            parsed,
            Some(serde_json::json!({
                "hookSpecificOutput": {
                    "retry": true
                }
            }))
        );
    }

    #[test]
    fn translates_additional_context_for_notification() {
        let result = ExecutionResult {
            status: InternalStatus::Success,
            message: None,
            system_message: None,
            exit_code: Some(0),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
            bridge_output: Some(BridgeOutput::AdditionalContext {
                text: "show in transcript context".to_string(),
            }),
        };

        let translated = translate_output(&context("Notification"), &result);
        assert!(translated.is_ok());
        let output = translated.unwrap_or_else(|_| unreachable!());
        let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
        assert_eq!(
            parsed,
            Some(serde_json::json!({
                "hookSpecificOutput": {
                    "additionalContext": "show in transcript context"
                }
            }))
        );
    }

    #[test]
    fn translates_elicitation_response_output_for_elicitation_result() {
        let result = ExecutionResult {
            status: InternalStatus::Success,
            message: None,
            system_message: None,
            exit_code: Some(0),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
            bridge_output: Some(BridgeOutput::ElicitationResponse {
                action: "accept".to_string(),
                content: Some(serde_json::json!({ "answer": "yes" })),
            }),
        };

        let translated = translate_output(&context("ElicitationResult"), &result);
        assert!(translated.is_ok());
        let output = translated.unwrap_or_else(|_| unreachable!());
        let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
        assert_eq!(
            parsed,
            Some(serde_json::json!({
                "hookSpecificOutput": {
                    "action": "accept",
                    "content": { "answer": "yes" }
                }
            }))
        );
    }

    #[test]
    fn translates_elicitation_response_output() {
        let result = ExecutionResult {
            status: InternalStatus::Success,
            message: None,
            system_message: None,
            exit_code: Some(0),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
            bridge_output: Some(BridgeOutput::ElicitationResponse {
                action: "accept".to_string(),
                content: Some(serde_json::json!({ "answer": "yes" })),
            }),
        };

        let translated = translate_output(&context("Elicitation"), &result);
        assert!(translated.is_ok());
        let output = translated.unwrap_or_else(|_| unreachable!());
        let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
        assert_eq!(
            parsed,
            Some(serde_json::json!({
                "hookSpecificOutput": {
                    "action": "accept",
                    "content": { "answer": "yes" }
                }
            }))
        );
    }

    #[test]
    fn rejects_additional_context_for_unsupported_event() {
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
            translate_output(&context("CwdChanged"), &result),
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
                reason: Some("ok".to_string()),
                updated_input: None,
                additional_context: None,
            }),
        };

        assert!(matches!(
            translate_output(&context("Notification"), &result),
            Err(crate::error::HookBridgeError::PlatformProtocol { .. })
        ));
    }

    #[test]
    fn rejects_permission_retry_for_unsupported_event() {
        let result = ExecutionResult {
            status: InternalStatus::Success,
            message: None,
            system_message: None,
            exit_code: Some(0),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
            bridge_output: Some(BridgeOutput::PermissionRetry { reason: None }),
        };

        assert!(matches!(
            translate_output(&context("PermissionRequest"), &result),
            Err(crate::error::HookBridgeError::PlatformProtocol { .. })
        ));
    }

    #[test]
    fn rejects_elicitation_response_for_unsupported_event() {
        let result = ExecutionResult {
            status: InternalStatus::Success,
            message: None,
            system_message: None,
            exit_code: Some(0),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
            bridge_output: Some(BridgeOutput::ElicitationResponse {
                action: "accept".to_string(),
                content: None,
            }),
        };

        assert!(matches!(
            translate_output(&context("Notification"), &result),
            Err(crate::error::HookBridgeError::PlatformProtocol { .. })
        ));
    }

    #[test]
    fn exit_code_two_permission_request_prefers_explicit_bridge_output() {
        let result = ExecutionResult {
            status: InternalStatus::Success,
            message: None,
            system_message: None,
            exit_code: Some(2),
            raw_stdout: Vec::new(),
            raw_stderr: b"should not override\n".to_vec(),
            bridge_output: Some(BridgeOutput::PermissionDecision {
                behavior: "allow".to_string(),
                reason: Some("rewritten".to_string()),
                updated_input: None,
                additional_context: None,
            }),
        };

        let translated = translate_output(&context("PermissionRequest"), &result);
        assert!(translated.is_ok());
        let output = translated.unwrap_or_else(|_| unreachable!());
        let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
        assert_eq!(
            parsed,
            Some(serde_json::json!({
                "hookSpecificOutput": {
                    "decision": {
                        "behavior": "allow"
                    },
                    "reason": "rewritten"
                }
            }))
        );
    }

    #[test]
    fn teammate_feedback_does_not_override_bridge_output() {
        let result = ExecutionResult {
            status: InternalStatus::Stop,
            message: None,
            system_message: None,
            exit_code: Some(2),
            raw_stdout: Vec::new(),
            raw_stderr: b"feedback\n".to_vec(),
            bridge_output: Some(BridgeOutput::Stop {
                reason: Some("halt teammate".to_string()),
                system_message: None,
            }),
        };

        let translated = translate_output(&context("TaskCompleted"), &result);
        assert!(translated.is_ok());
        let output = translated.unwrap_or_else(|_| unreachable!());
        let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
        assert_eq!(
            parsed,
            Some(serde_json::json!({
                "continue": false,
                "stopReason": "halt teammate"
            }))
        );
        assert_eq!(output.stderr, Vec::<u8>::new());
        assert_eq!(output.exit_code, 0);
    }

    #[test]
    fn rejects_unsupported_bridge_output_for_notification() {
        let result = ExecutionResult {
            status: InternalStatus::Success,
            message: None,
            system_message: None,
            exit_code: Some(0),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
            bridge_output: Some(BridgeOutput::WorktreePath {
                path: "/tmp/nope".to_string(),
            }),
        };

        assert!(matches!(
            translate_output(&context("Notification"), &result),
            Err(crate::error::HookBridgeError::PlatformProtocol { .. })
        ));
    }

    #[test]
    fn rejects_non_structured_block_for_side_effect_only_event() {
        let result = ExecutionResult {
            status: InternalStatus::Block,
            message: Some("failed".to_string()),
            system_message: Some("bridge failed".to_string()),
            exit_code: Some(1),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
            bridge_output: None,
        };

        assert!(matches!(
            translate_output(&context("Notification"), &result),
            Err(crate::error::HookBridgeError::PlatformProtocol { .. })
        ));
    }

    #[test]
    fn rejects_non_structured_stop_for_side_effect_only_event() {
        let result = ExecutionResult {
            status: InternalStatus::Stop,
            message: Some("stop".to_string()),
            system_message: None,
            exit_code: Some(0),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
            bridge_output: None,
        };

        assert!(matches!(
            translate_output(&context("CwdChanged"), &result),
            Err(crate::error::HookBridgeError::PlatformProtocol { .. })
        ));
    }

    #[test]
    fn translates_non_structured_block_for_subagent_stop() {
        let result = ExecutionResult {
            status: InternalStatus::Block,
            message: Some("blocked".to_string()),
            system_message: None,
            exit_code: Some(1),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
            bridge_output: None,
        };

        let translated = translate_output(&context("SubagentStop"), &result);
        assert!(translated.is_ok());
        let output = translated.unwrap_or_else(|_| unreachable!());
        let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
        assert_eq!(
            parsed,
            Some(serde_json::json!({
                "decision": "block",
                "reason": "blocked"
            }))
        );
    }
}
