use std::collections::BTreeSet;

use super::Platform;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    BeforeCommand,
    AfterCommand,
    SessionStart,
}

impl HookEvent {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BeforeCommand => "before_command",
            Self::AfterCommand => "after_command",
            Self::SessionStart => "session_start",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionKind {
    Continue,
    Stop,
    Block,
}

#[derive(Debug, Clone, Copy)]
pub struct EventCapability {
    pub event: HookEvent,
    pub supports_matcher: bool,
    pub allowed_extra_fields: &'static [&'static str],
    pub allowed_decisions: &'static [DecisionKind],
}

#[derive(Debug, Clone, Copy)]
pub struct PlatformCapability {
    pub platform: Platform,
    pub timeout_field: &'static str,
    pub events: &'static [EventCapability],
}

const CLAUDE_EVENT_CAPS: &[EventCapability] = &[
    EventCapability {
        event: HookEvent::BeforeCommand,
        supports_matcher: true,
        allowed_extra_fields: &["decision", "reason"],
        allowed_decisions: &[
            DecisionKind::Continue,
            DecisionKind::Stop,
            DecisionKind::Block,
        ],
    },
    EventCapability {
        event: HookEvent::AfterCommand,
        supports_matcher: true,
        allowed_extra_fields: &["decision", "reason"],
        allowed_decisions: &[
            DecisionKind::Continue,
            DecisionKind::Stop,
            DecisionKind::Block,
        ],
    },
    EventCapability {
        event: HookEvent::SessionStart,
        supports_matcher: false,
        allowed_extra_fields: &["decision", "reason"],
        allowed_decisions: &[
            DecisionKind::Continue,
            DecisionKind::Stop,
            DecisionKind::Block,
        ],
    },
];

const CODEX_EVENT_CAPS: &[EventCapability] = &[
    EventCapability {
        event: HookEvent::BeforeCommand,
        supports_matcher: true,
        allowed_extra_fields: &["continue", "stopReason", "systemMessage"],
        allowed_decisions: &[DecisionKind::Continue, DecisionKind::Stop],
    },
    EventCapability {
        event: HookEvent::AfterCommand,
        supports_matcher: true,
        allowed_extra_fields: &["continue", "stopReason", "systemMessage"],
        allowed_decisions: &[DecisionKind::Continue, DecisionKind::Stop],
    },
    EventCapability {
        event: HookEvent::SessionStart,
        supports_matcher: false,
        allowed_extra_fields: &["continue", "stopReason", "systemMessage"],
        allowed_decisions: &[DecisionKind::Continue, DecisionKind::Stop],
    },
];

const CLAUDE_CAPABILITY: PlatformCapability = PlatformCapability {
    platform: Platform::Claude,
    timeout_field: "timeout_sec",
    events: CLAUDE_EVENT_CAPS,
};

const CODEX_CAPABILITY: PlatformCapability = PlatformCapability {
    platform: Platform::Codex,
    timeout_field: "timeout_sec",
    events: CODEX_EVENT_CAPS,
};

pub const CLAUDE_EVENTS: &[&str] = &["before_command", "after_command", "session_start"];
pub const CODEX_EVENTS: &[&str] = &["before_command", "after_command", "session_start"];

#[must_use]
pub const fn matrix(platform: Platform) -> &'static PlatformCapability {
    match platform {
        Platform::Claude => &CLAUDE_CAPABILITY,
        Platform::Codex => &CODEX_CAPABILITY,
    }
}

#[must_use]
pub fn event_capability(platform: Platform, event: &str) -> Option<&'static EventCapability> {
    matrix(platform)
        .events
        .iter()
        .find(|entry| entry.event.as_str() == event)
}

#[must_use]
pub fn events(platform: Platform) -> &'static [&'static str] {
    match platform {
        Platform::Claude => CLAUDE_EVENTS,
        Platform::Codex => CODEX_EVENTS,
    }
}

#[must_use]
pub fn supports_event(platform: Platform, event: &str) -> bool {
    event_capability(platform, event).is_some()
}

#[must_use]
pub fn event_supports_matcher(platform: Platform, event: &str) -> bool {
    event_capability(platform, event).is_some_and(|entry| entry.supports_matcher)
}

#[must_use]
pub fn timeout_field_name(platform: Platform) -> &'static str {
    matrix(platform).timeout_field
}

#[must_use]
pub fn allowed_extra_fields(platform: Platform, event: &str) -> BTreeSet<&'static str> {
    event_capability(platform, event)
        .map(|entry| entry.allowed_extra_fields.iter().copied().collect())
        .unwrap_or_default()
}

#[must_use]
pub fn allowed_decisions(platform: Platform, event: &str) -> &'static [DecisionKind] {
    event_capability(platform, event).map_or(&[], |entry| entry.allowed_decisions)
}

#[cfg(test)]
mod tests {
    use crate::platform::Platform;

    use super::{
        DecisionKind, allowed_decisions, allowed_extra_fields, event_capability,
        event_supports_matcher, events, supports_event, timeout_field_name,
    };

    #[test]
    fn returns_event_capability_for_known_event() {
        assert_eq!(
            event_capability(Platform::Codex, "before_command").map(|value| value.supports_matcher),
            Some(true)
        );
    }

    #[test]
    fn timeout_field_is_exposed_from_matrix() {
        assert_eq!(timeout_field_name(Platform::Claude), "timeout_sec");
        assert_eq!(timeout_field_name(Platform::Codex), "timeout_sec");
    }

    #[test]
    fn exposes_decision_capability_per_platform_event() {
        assert!(allowed_decisions(Platform::Codex, "before_command").contains(&DecisionKind::Stop));
        assert!(
            !allowed_decisions(Platform::Codex, "before_command").contains(&DecisionKind::Block)
        );
    }

    #[test]
    fn exposes_events_matcher_support_and_extra_fields() {
        assert_eq!(
            events(Platform::Claude),
            &["before_command", "after_command", "session_start"]
        );
        assert_eq!(
            events(Platform::Codex),
            &["before_command", "after_command", "session_start"]
        );
        assert!(supports_event(Platform::Claude, "session_start"));
        assert!(!supports_event(Platform::Claude, "unknown"));
        assert!(event_supports_matcher(Platform::Claude, "before_command"));
        assert!(!event_supports_matcher(Platform::Claude, "session_start"));
        assert!(allowed_extra_fields(Platform::Codex, "before_command").contains("stopReason"));
        assert!(allowed_extra_fields(Platform::Codex, "unknown").is_empty());
    }
}
