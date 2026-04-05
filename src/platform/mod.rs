use clap::ValueEnum;
use std::path::PathBuf;

use crate::error::HookBridgeError;
use crate::run::{ExecutionResult, RuntimeContext};

pub mod capability;
pub mod claude;
pub mod codex;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum Platform {
    Claude,
    Codex,
}

impl Platform {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedContextFields {
    pub raw_event: String,
    pub event: String,
    pub session_or_thread_id: String,
    pub cwd: Option<PathBuf>,
    pub transcript_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
}

#[must_use]
pub fn normalize_event_name(platform: Platform, event: &str) -> Option<&'static str> {
    let normalized = match event {
        "before_command" | "PreToolUse" => "PreToolUse",
        "after_command" | "PostToolUse" => "PostToolUse",
        "session_start" | "SessionStart" => "SessionStart",
        "UserPromptSubmit" => "UserPromptSubmit",
        "Stop" => "Stop",
        "PermissionRequest" => "PermissionRequest",
        "PermissionDenied" => "PermissionDenied",
        "PostToolUseFailure" => "PostToolUseFailure",
        "Notification" => "Notification",
        "SubagentStart" => "SubagentStart",
        "SubagentStop" => "SubagentStop",
        "TaskCreated" => "TaskCreated",
        "TaskCompleted" => "TaskCompleted",
        "StopFailure" => "StopFailure",
        "TeammateIdle" => "TeammateIdle",
        "InstructionsLoaded" => "InstructionsLoaded",
        "ConfigChange" => "ConfigChange",
        "CwdChanged" => "CwdChanged",
        "FileChanged" => "FileChanged",
        "WorktreeCreate" => "WorktreeCreate",
        "WorktreeRemove" => "WorktreeRemove",
        "PreCompact" => "PreCompact",
        "PostCompact" => "PostCompact",
        "Elicitation" => "Elicitation",
        "ElicitationResult" => "ElicitationResult",
        "SessionEnd" => "SessionEnd",
        _ => return None,
    };

    if capability::supports_event(platform, normalized) {
        Some(normalized)
    } else {
        None
    }
}

/// Translates a normalized execution result into the selected platform's native hook output.
///
/// # Errors
///
/// Returns a platform-protocol error when the internal result cannot be expressed for the
/// selected platform event without silently degrading behavior.
pub fn translate_output(
    platform: Platform,
    context: &RuntimeContext,
    result: &ExecutionResult,
) -> Result<PlatformOutput, HookBridgeError> {
    match platform {
        Platform::Claude => claude::translate_output(context, result),
        Platform::Codex => codex::translate_output(context, result),
    }
}

#[cfg(test)]
mod tests {
    use super::{Platform, PlatformOutput, normalize_event_name};

    #[test]
    fn platform_as_str_returns_stable_values() {
        assert_eq!(Platform::Claude.as_str(), "claude");
        assert_eq!(Platform::Codex.as_str(), "codex");
    }

    #[test]
    fn normalize_event_name_accepts_native_and_unified_values() {
        assert_eq!(
            normalize_event_name(Platform::Codex, "PreToolUse"),
            Some("PreToolUse")
        );
        assert_eq!(
            normalize_event_name(Platform::Claude, "PostToolUse"),
            Some("PostToolUse")
        );
        assert_eq!(
            normalize_event_name(Platform::Codex, "SessionStart"),
            Some("SessionStart")
        );
        assert_eq!(
            normalize_event_name(Platform::Claude, "before_command"),
            Some("PreToolUse")
        );
        assert_eq!(
            normalize_event_name(Platform::Claude, "Notification"),
            Some("Notification")
        );
        assert_eq!(normalize_event_name(Platform::Codex, "Notification"), None);
    }

    #[test]
    fn platform_output_preserves_stdout_and_exit_code() {
        assert_eq!(
            PlatformOutput {
                stdout: b"{}".to_vec(),
                stderr: b"warn".to_vec(),
                exit_code: 0,
            },
            PlatformOutput {
                stdout: b"{}".to_vec(),
                stderr: b"warn".to_vec(),
                exit_code: 0,
            }
        );
    }
}
