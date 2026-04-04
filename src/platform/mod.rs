use clap::ValueEnum;
use std::path::PathBuf;

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

#[must_use]
pub fn normalize_event_name(platform: Platform, event: &str) -> Option<&'static str> {
    let normalized = match event {
        "PreToolUse" | "before_command" => "before_command",
        "PostToolUse" | "after_command" => "after_command",
        "SessionStart" | "session_start" => "session_start",
        _ => return None,
    };

    if capability::supports_event(platform, normalized) {
        Some(normalized)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{Platform, normalize_event_name};

    #[test]
    fn platform_as_str_returns_stable_values() {
        assert_eq!(Platform::Claude.as_str(), "claude");
        assert_eq!(Platform::Codex.as_str(), "codex");
    }

    #[test]
    fn normalize_event_name_accepts_native_and_unified_values() {
        assert_eq!(
            normalize_event_name(Platform::Codex, "PreToolUse"),
            Some("before_command")
        );
        assert_eq!(
            normalize_event_name(Platform::Claude, "PostToolUse"),
            Some("after_command")
        );
        assert_eq!(
            normalize_event_name(Platform::Codex, "SessionStart"),
            Some("session_start")
        );
        assert_eq!(
            normalize_event_name(Platform::Claude, "before_command"),
            Some("before_command")
        );
        assert_eq!(normalize_event_name(Platform::Codex, "Notification"), None);
    }
}
