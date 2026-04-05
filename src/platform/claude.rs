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
#[path = "claude_tests.rs"]
mod tests;
