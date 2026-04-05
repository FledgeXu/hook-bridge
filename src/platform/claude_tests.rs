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
