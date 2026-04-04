use std::collections::BTreeSet;

use super::Platform;

pub const CLAUDE_EVENTS: &[&str] = &["before_command", "after_command", "session_start"];

pub const CODEX_EVENTS: &[&str] = &["before_command", "after_command", "session_start"];

#[must_use]
pub fn events(platform: Platform) -> &'static [&'static str] {
    match platform {
        Platform::Claude => CLAUDE_EVENTS,
        Platform::Codex => CODEX_EVENTS,
    }
}

#[must_use]
pub fn supports_event(platform: Platform, event: &str) -> bool {
    events(platform).contains(&event)
}

#[must_use]
pub fn event_supports_matcher(platform: Platform, event: &str) -> bool {
    match platform {
        Platform::Claude => matches!(event, "before_command" | "after_command"),
        Platform::Codex => matches!(event, "before_command" | "after_command"),
    }
}

#[must_use]
pub fn timeout_field_name(platform: Platform) -> &'static str {
    match platform {
        Platform::Claude | Platform::Codex => "timeout_sec",
    }
}

#[must_use]
pub fn allowed_extra_fields(platform: Platform, _event: &str) -> BTreeSet<&'static str> {
    match platform {
        Platform::Claude => ["decision", "reason"].into_iter().collect(),
        Platform::Codex => ["continue", "stopReason", "systemMessage"]
            .into_iter()
            .collect(),
    }
}
