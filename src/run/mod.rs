mod context;
mod retry;

use std::time::Duration;

use crate::cli::RunArgs;
use crate::config::{PlatformRule, parse_and_normalize};
use crate::error::HookBridgeError;
use crate::generate;
use crate::platform::capability::{self, DecisionKind};
use crate::platform::{self, Platform};
use crate::runtime::Runtime;
use crate::runtime::process::ProcessRequest;

pub use context::{RuntimeContext, parse_runtime_context};
#[cfg(test)]
pub(crate) use retry::{RetryState, now_epoch_sec, persist_retry_state};
pub(crate) use retry::{
    load_retry_state, retry_guard_engaged, retry_guard_result, retry_state_path, update_retry_state,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InternalStatus {
    Success,
    Stop,
    Block,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeOutput {
    Success,
    Block {
        reason: Option<String>,
        system_message: Option<String>,
    },
    Stop {
        reason: Option<String>,
        system_message: Option<String>,
    },
    AdditionalContext {
        text: String,
    },
    PermissionDecision {
        behavior: String,
        reason: Option<String>,
        updated_input: Option<serde_json::Value>,
        additional_context: Option<String>,
    },
    PermissionRetry {
        reason: Option<String>,
    },
    WorktreePath {
        path: String,
    },
    ElicitationResponse {
        action: String,
        content: Option<serde_json::Value>,
    },
    Error {
        message: Option<String>,
        system_message: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionResult {
    pub status: InternalStatus,
    pub message: Option<String>,
    pub system_message: Option<String>,
    pub exit_code: Option<i32>,
    pub raw_stdout: Vec<u8>,
    pub raw_stderr: Vec<u8>,
    pub bridge_output: Option<BridgeOutput>,
}

/// Executes the `run` command.
///
/// # Errors
///
/// Returns errors for managed-file lookup, runtime input parsing, rule lookup, command execution,
/// retry-state persistence, and output write failures.
pub fn execute(args: &RunArgs, runtime: &dyn Runtime) -> Result<u8, HookBridgeError> {
    let metadata = generate::load_metadata(runtime, args.platform)?;
    let source_config_path = std::path::PathBuf::from(&metadata.source_config);
    if !source_config_path.is_absolute() {
        return Err(HookBridgeError::ConfigValidation {
            message: format!(
                "managed source_config must be absolute, got '{}'",
                source_config_path.display()
            ),
        });
    }
    let config_yaml = runtime.fs().read_to_string(source_config_path.as_path())?;
    let config = parse_and_normalize(source_config_path.clone(), &config_yaml)?;

    let stdin = runtime.io().read_stdin()?;
    let raw_payload = String::from_utf8(stdin).map_err(|error| HookBridgeError::JsonParse {
        message: format!("stdin payload is not valid UTF-8 JSON: {error}"),
    })?;

    let context = parse_runtime_context(args, &raw_payload, &source_config_path)?;
    let rule = config.find_platform_rule(args.platform, &args.rule_id)?;

    if context.event != rule.event {
        return Err(HookBridgeError::PlatformProtocol {
            message: format!(
                "event mismatch for rule '{}': stdin event '{}' but configured event '{}'",
                args.rule_id, context.event, rule.event
            ),
        });
    }

    let execution_result = execute_rule(runtime, rule, &context)?;

    let output = translate_output(args.platform, &context, &execution_result)?;
    runtime.io().write_stdout(&output.stdout)?;
    runtime.io().write_stderr(&output.stderr)?;

    Ok(output.exit_code)
}

fn execute_rule(
    runtime: &dyn Runtime,
    rule: &PlatformRule,
    context: &RuntimeContext,
) -> Result<ExecutionResult, HookBridgeError> {
    let state_path = retry_state_path(runtime, context);
    let state = load_retry_state(runtime, &state_path)?;
    if retry_guard_engaged(rule, &state) {
        let mut result = retry_guard_result();
        if !capability::allowed_decisions(context.platform, &context.event)
            .contains(&DecisionKind::Stop)
        {
            result.status = InternalStatus::Block;
        }
        return Ok(result);
    }

    let execution_result = run_user_command(runtime, rule, context)?;

    if let Err(error) = translate_output(context.platform, context, &execution_result) {
        update_retry_state(
            runtime,
            &state_path,
            &state,
            &ExecutionResult {
                status: InternalStatus::Error,
                message: Some(error.to_string()),
                system_message: None,
                exit_code: execution_result.exit_code,
                raw_stdout: execution_result.raw_stdout.clone(),
                raw_stderr: execution_result.raw_stderr.clone(),
                bridge_output: None,
            },
        )?;
        return Err(error);
    }

    update_retry_state(runtime, &state_path, &state, &execution_result)?;

    Ok(execution_result)
}

fn run_user_command(
    runtime: &dyn Runtime,
    rule: &PlatformRule,
    context: &RuntimeContext,
) -> Result<ExecutionResult, HookBridgeError> {
    let request = ProcessRequest {
        program: rule.shell.clone(),
        args: vec!["-lc".to_string(), rule.command.clone()],
        stdin: context.raw_payload.as_bytes().to_vec(),
        timeout: Duration::from_secs(rule.timeout_sec),
        cwd: rule.working_dir.clone().or_else(|| context.cwd.clone()),
        env: command_env(rule, context),
    };

    let output = match runtime.process_runner().run(&request) {
        Ok(output) => output,
        Err(error) => {
            return Ok(ExecutionResult {
                status: InternalStatus::Error,
                message: Some(error.to_string()),
                system_message: Some("hook_bridge command execution failed".to_string()),
                exit_code: Some(1),
                raw_stdout: Vec::new(),
                raw_stderr: Vec::new(),
                bridge_output: None,
            });
        }
    };

    if output.status_code == 0 {
        if let Some(result) = parse_bridge_output(&output.stdout)? {
            return Ok(ExecutionResult {
                raw_stdout: output.stdout,
                raw_stderr: output.stderr,
                exit_code: Some(output.status_code),
                ..result
            });
        }

        if let Some(result) = codex_plaintext_success_result(context, &output.stdout)? {
            return Ok(ExecutionResult {
                raw_stdout: output.stdout,
                raw_stderr: output.stderr,
                exit_code: Some(output.status_code),
                ..result
            });
        }

        Ok(ExecutionResult {
            status: InternalStatus::Success,
            message: None,
            system_message: None,
            exit_code: Some(0),
            raw_stdout: output.stdout,
            raw_stderr: output.stderr,
            bridge_output: None,
        })
    } else {
        Ok(ExecutionResult {
            status: InternalStatus::Block,
            message: Some(format!(
                "command exited with non-zero status {}",
                output.status_code
            )),
            system_message: Some("hook_bridge command returned non-zero exit code".to_string()),
            exit_code: Some(output.status_code),
            raw_stdout: output.stdout,
            raw_stderr: output.stderr,
            bridge_output: None,
        })
    }
}

fn codex_plaintext_success_result(
    context: &RuntimeContext,
    stdout: &[u8],
) -> Result<Option<ExecutionResult>, HookBridgeError> {
    if context.platform != Platform::Codex {
        return Ok(None);
    }

    let text = match std::str::from_utf8(stdout) {
        Ok(text) => text.trim_end_matches(['\r', '\n']),
        Err(_) => return Ok(None),
    };

    if text.is_empty() {
        return Ok(None);
    }

    match context.event.as_str() {
        "SessionStart" | "UserPromptSubmit" => Ok(Some(execution_result_from_bridge_output(
            BridgeOutput::AdditionalContext {
                text: text.to_string(),
            },
        ))),
        "Stop" => Err(HookBridgeError::PlatformProtocol {
            message: format!(
                "codex event '{}' does not support plain-text success stdout; use structured bridge JSON or an empty response",
                context.raw_event
            ),
        }),
        _ => Ok(None),
    }
}

fn command_env(
    rule: &PlatformRule,
    context: &RuntimeContext,
) -> std::collections::BTreeMap<String, String> {
    let mut env = rule.env.clone();
    env.insert(
        "HOOK_BRIDGE_PLATFORM".to_string(),
        context.platform.as_str().to_string(),
    );
    env.insert("HOOK_BRIDGE_RULE_ID".to_string(), context.rule_id.clone());
    env.insert("HOOK_BRIDGE_EVENT".to_string(), context.event.clone());
    env
}

struct TranslatedOutput {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    exit_code: u8,
}

fn translate_output(
    platform: Platform,
    context: &RuntimeContext,
    result: &ExecutionResult,
) -> Result<TranslatedOutput, HookBridgeError> {
    let output = platform::translate_output(platform, context, result)?;

    Ok(TranslatedOutput {
        stdout: output.stdout,
        stderr: output.stderr,
        exit_code: u8::try_from(output.exit_code).map_err(|_| {
            HookBridgeError::PlatformProtocol {
                message: format!(
                    "platform returned unsupported exit code {}",
                    output.exit_code
                ),
            }
        })?,
    })
}

fn parse_bridge_output(stdout: &[u8]) -> Result<Option<ExecutionResult>, HookBridgeError> {
    let trimmed = std::str::from_utf8(stdout).map(str::trim).unwrap_or("");
    if trimmed.is_empty() {
        return Ok(None);
    }

    let value: serde_json::Value = match serde_json::from_str(trimmed) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };

    let Some(bridge) = value.get("hook_bridge") else {
        return Ok(None);
    };

    let kind = bridge
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| HookBridgeError::PlatformProtocol {
            message: "bridge stdout JSON must include string field 'hook_bridge.kind'".to_string(),
        })?;

    let bridge_output = parse_bridge_output_kind(bridge, kind)?;

    Ok(Some(execution_result_from_bridge_output(bridge_output)))
}

fn parse_bridge_output_kind(
    bridge: &serde_json::Value,
    kind: &str,
) -> Result<BridgeOutput, HookBridgeError> {
    match kind {
        "success" => Ok(BridgeOutput::Success),
        "block" => Ok(BridgeOutput::Block {
            reason: bridge_string_field(bridge, "reason"),
            system_message: bridge_string_field(bridge, "system_message"),
        }),
        "stop" => Ok(BridgeOutput::Stop {
            reason: bridge_string_field(bridge, "reason"),
            system_message: bridge_string_field(bridge, "system_message"),
        }),
        "additional_context" => Ok(BridgeOutput::AdditionalContext {
            text: required_bridge_string_field(bridge, kind, "text")?,
        }),
        "permission_decision" => Ok(BridgeOutput::PermissionDecision {
            behavior: required_bridge_string_field(bridge, kind, "behavior")?,
            reason: bridge_string_field(bridge, "reason"),
            updated_input: bridge.get("updated_input").cloned(),
            additional_context: bridge_string_field(bridge, "additional_context"),
        }),
        "permission_retry" => Ok(BridgeOutput::PermissionRetry {
            reason: bridge_string_field(bridge, "reason"),
        }),
        "worktree_path" => Ok(BridgeOutput::WorktreePath {
            path: required_bridge_string_field(bridge, kind, "path")?,
        }),
        "elicitation_response" => Ok(BridgeOutput::ElicitationResponse {
            action: required_bridge_string_field(bridge, kind, "action")?,
            content: bridge.get("content").cloned(),
        }),
        "error" => Ok(BridgeOutput::Error {
            message: bridge_string_field(bridge, "message"),
            system_message: bridge_string_field(bridge, "system_message"),
        }),
        other => Err(HookBridgeError::PlatformProtocol {
            message: format!("bridge stdout JSON has unsupported result kind '{other}'"),
        }),
    }
}

fn execution_result_from_bridge_output(bridge_output: BridgeOutput) -> ExecutionResult {
    let (status, message, system_message) = match &bridge_output {
        BridgeOutput::Success
        | BridgeOutput::AdditionalContext { .. }
        | BridgeOutput::PermissionDecision { .. }
        | BridgeOutput::PermissionRetry { .. }
        | BridgeOutput::WorktreePath { .. }
        | BridgeOutput::ElicitationResponse { .. } => (InternalStatus::Success, None, None),
        BridgeOutput::Block {
            reason,
            system_message,
        } => (
            InternalStatus::Block,
            reason.clone(),
            system_message.clone(),
        ),
        BridgeOutput::Stop {
            reason,
            system_message,
        } => (InternalStatus::Stop, reason.clone(), system_message.clone()),
        BridgeOutput::Error {
            message,
            system_message,
        } => (
            InternalStatus::Error,
            message.clone(),
            system_message.clone(),
        ),
    };

    ExecutionResult {
        status,
        message,
        system_message,
        exit_code: None,
        raw_stdout: Vec::new(),
        raw_stderr: Vec::new(),
        bridge_output: Some(bridge_output),
    }
}

fn bridge_string_field(bridge: &serde_json::Value, name: &str) -> Option<String> {
    bridge
        .get(name)
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string)
}

fn required_bridge_string_field(
    bridge: &serde_json::Value,
    kind: &str,
    field: &str,
) -> Result<String, HookBridgeError> {
    bridge_string_field(bridge, field).ok_or_else(|| HookBridgeError::PlatformProtocol {
        message: format!("bridge result '{kind}' requires string field '{field}'"),
    })
}

#[cfg(test)]
mod tests;
