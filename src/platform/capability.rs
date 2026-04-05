use std::collections::BTreeSet;

use super::Platform;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionKind {
    Continue,
    Stop,
    Block,
}

#[derive(Debug, Clone, Copy)]
pub struct EventCapability {
    pub event: &'static str,
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

const CLAUDE_DECISION_FIELDS: &[&str] = &["decision", "reason"];
const CODEX_DECISION_FIELDS: &[&str] = &["continue", "stopReason", "systemMessage"];
const NO_EXTRA_FIELDS: &[&str] = &[];
const SIDE_EFFECT_ONLY: &[DecisionKind] = &[];
const BLOCK_ONLY: &[DecisionKind] = &[DecisionKind::Block];
const STOP_ONLY: &[DecisionKind] = &[DecisionKind::Stop];
const STOP_OR_BLOCK: &[DecisionKind] = &[DecisionKind::Stop, DecisionKind::Block];
const CONTINUE_OR_BLOCK: &[DecisionKind] = &[DecisionKind::Continue, DecisionKind::Block];
const CONTINUE_STOP_OR_BLOCK: &[DecisionKind] = &[
    DecisionKind::Continue,
    DecisionKind::Stop,
    DecisionKind::Block,
];

const CLAUDE_EVENT_CAPS: &[EventCapability] = &[
    EventCapability {
        event: "SessionStart",
        supports_matcher: true,
        allowed_extra_fields: NO_EXTRA_FIELDS,
        allowed_decisions: SIDE_EFFECT_ONLY,
    },
    EventCapability {
        event: "InstructionsLoaded",
        supports_matcher: false,
        allowed_extra_fields: NO_EXTRA_FIELDS,
        allowed_decisions: SIDE_EFFECT_ONLY,
    },
    EventCapability {
        event: "UserPromptSubmit",
        supports_matcher: false,
        allowed_extra_fields: CLAUDE_DECISION_FIELDS,
        allowed_decisions: BLOCK_ONLY,
    },
    EventCapability {
        event: "PreToolUse",
        supports_matcher: true,
        allowed_extra_fields: &["decision", "reason"],
        allowed_decisions: CONTINUE_STOP_OR_BLOCK,
    },
    EventCapability {
        event: "PermissionRequest",
        supports_matcher: true,
        allowed_extra_fields: NO_EXTRA_FIELDS,
        allowed_decisions: SIDE_EFFECT_ONLY,
    },
    EventCapability {
        event: "PermissionDenied",
        supports_matcher: false,
        allowed_extra_fields: NO_EXTRA_FIELDS,
        allowed_decisions: SIDE_EFFECT_ONLY,
    },
    EventCapability {
        event: "PostToolUse",
        supports_matcher: true,
        allowed_extra_fields: CLAUDE_DECISION_FIELDS,
        allowed_decisions: BLOCK_ONLY,
    },
    EventCapability {
        event: "PostToolUseFailure",
        supports_matcher: true,
        allowed_extra_fields: CLAUDE_DECISION_FIELDS,
        allowed_decisions: BLOCK_ONLY,
    },
    EventCapability {
        event: "Notification",
        supports_matcher: true,
        allowed_extra_fields: NO_EXTRA_FIELDS,
        allowed_decisions: SIDE_EFFECT_ONLY,
    },
    EventCapability {
        event: "SubagentStart",
        supports_matcher: true,
        allowed_extra_fields: NO_EXTRA_FIELDS,
        allowed_decisions: SIDE_EFFECT_ONLY,
    },
    EventCapability {
        event: "SubagentStop",
        supports_matcher: true,
        allowed_extra_fields: NO_EXTRA_FIELDS,
        allowed_decisions: STOP_OR_BLOCK,
    },
    EventCapability {
        event: "TaskCreated",
        supports_matcher: false,
        allowed_extra_fields: NO_EXTRA_FIELDS,
        allowed_decisions: STOP_ONLY,
    },
    EventCapability {
        event: "TaskCompleted",
        supports_matcher: false,
        allowed_extra_fields: NO_EXTRA_FIELDS,
        allowed_decisions: STOP_ONLY,
    },
    EventCapability {
        event: "Stop",
        supports_matcher: false,
        allowed_extra_fields: NO_EXTRA_FIELDS,
        allowed_decisions: STOP_OR_BLOCK,
    },
    EventCapability {
        event: "StopFailure",
        supports_matcher: false,
        allowed_extra_fields: NO_EXTRA_FIELDS,
        allowed_decisions: SIDE_EFFECT_ONLY,
    },
    EventCapability {
        event: "TeammateIdle",
        supports_matcher: false,
        allowed_extra_fields: NO_EXTRA_FIELDS,
        allowed_decisions: STOP_ONLY,
    },
    EventCapability {
        event: "ConfigChange",
        supports_matcher: false,
        allowed_extra_fields: CLAUDE_DECISION_FIELDS,
        allowed_decisions: BLOCK_ONLY,
    },
    EventCapability {
        event: "CwdChanged",
        supports_matcher: false,
        allowed_extra_fields: NO_EXTRA_FIELDS,
        allowed_decisions: SIDE_EFFECT_ONLY,
    },
    EventCapability {
        event: "FileChanged",
        supports_matcher: false,
        allowed_extra_fields: NO_EXTRA_FIELDS,
        allowed_decisions: SIDE_EFFECT_ONLY,
    },
    EventCapability {
        event: "WorktreeCreate",
        supports_matcher: false,
        allowed_extra_fields: NO_EXTRA_FIELDS,
        allowed_decisions: SIDE_EFFECT_ONLY,
    },
    EventCapability {
        event: "WorktreeRemove",
        supports_matcher: false,
        allowed_extra_fields: NO_EXTRA_FIELDS,
        allowed_decisions: SIDE_EFFECT_ONLY,
    },
    EventCapability {
        event: "PreCompact",
        supports_matcher: false,
        allowed_extra_fields: NO_EXTRA_FIELDS,
        allowed_decisions: SIDE_EFFECT_ONLY,
    },
    EventCapability {
        event: "PostCompact",
        supports_matcher: false,
        allowed_extra_fields: NO_EXTRA_FIELDS,
        allowed_decisions: SIDE_EFFECT_ONLY,
    },
    EventCapability {
        event: "SessionEnd",
        supports_matcher: false,
        allowed_extra_fields: NO_EXTRA_FIELDS,
        allowed_decisions: SIDE_EFFECT_ONLY,
    },
    EventCapability {
        event: "Elicitation",
        supports_matcher: true,
        allowed_extra_fields: NO_EXTRA_FIELDS,
        allowed_decisions: SIDE_EFFECT_ONLY,
    },
    EventCapability {
        event: "ElicitationResult",
        supports_matcher: true,
        allowed_extra_fields: NO_EXTRA_FIELDS,
        allowed_decisions: SIDE_EFFECT_ONLY,
    },
];

const CODEX_EVENT_CAPS: &[EventCapability] = &[
    EventCapability {
        event: "SessionStart",
        supports_matcher: true,
        allowed_extra_fields: CODEX_DECISION_FIELDS,
        allowed_decisions: CONTINUE_OR_BLOCK,
    },
    EventCapability {
        event: "PreToolUse",
        supports_matcher: true,
        allowed_extra_fields: CODEX_DECISION_FIELDS,
        allowed_decisions: CONTINUE_OR_BLOCK,
    },
    EventCapability {
        event: "PostToolUse",
        supports_matcher: true,
        allowed_extra_fields: CODEX_DECISION_FIELDS,
        allowed_decisions: CONTINUE_STOP_OR_BLOCK,
    },
    EventCapability {
        event: "UserPromptSubmit",
        supports_matcher: false,
        allowed_extra_fields: CODEX_DECISION_FIELDS,
        allowed_decisions: CONTINUE_OR_BLOCK,
    },
    EventCapability {
        event: "Stop",
        supports_matcher: false,
        allowed_extra_fields: CODEX_DECISION_FIELDS,
        allowed_decisions: CONTINUE_STOP_OR_BLOCK,
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
        .find(|entry| entry.event == event)
}

#[must_use]
pub fn events(platform: Platform) -> &'static [&'static str] {
    match platform {
        Platform::Claude => &[
            "SessionStart",
            "InstructionsLoaded",
            "UserPromptSubmit",
            "PreToolUse",
            "PermissionRequest",
            "PermissionDenied",
            "PostToolUse",
            "PostToolUseFailure",
            "Notification",
            "SubagentStart",
            "SubagentStop",
            "TaskCreated",
            "TaskCompleted",
            "Stop",
            "StopFailure",
            "TeammateIdle",
            "ConfigChange",
            "CwdChanged",
            "FileChanged",
            "WorktreeCreate",
            "WorktreeRemove",
            "PreCompact",
            "PostCompact",
            "SessionEnd",
            "Elicitation",
            "ElicitationResult",
        ],
        Platform::Codex => &[
            "SessionStart",
            "PreToolUse",
            "PostToolUse",
            "UserPromptSubmit",
            "Stop",
        ],
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
            event_capability(Platform::Codex, "PreToolUse").map(|value| value.supports_matcher),
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
        assert!(allowed_decisions(Platform::Codex, "PreToolUse").contains(&DecisionKind::Block));
        assert!(!allowed_decisions(Platform::Codex, "PreToolUse").contains(&DecisionKind::Stop));
        assert!(allowed_decisions(Platform::Claude, "Stop").contains(&DecisionKind::Stop));
    }

    #[test]
    fn exposes_events_matcher_support_and_extra_fields() {
        assert!(events(Platform::Claude).contains(&"Notification"));
        assert!(events(Platform::Codex).contains(&"UserPromptSubmit"));
        assert!(supports_event(Platform::Claude, "SessionStart"));
        assert!(!supports_event(Platform::Claude, "unknown"));
        assert!(event_supports_matcher(Platform::Claude, "PreToolUse"));
        assert!(event_supports_matcher(Platform::Claude, "SessionStart"));
        assert!(event_supports_matcher(Platform::Claude, "Elicitation"));
        assert!(event_supports_matcher(Platform::Claude, "Notification"));
        assert!(event_supports_matcher(Platform::Claude, "SubagentStart"));
        assert!(event_supports_matcher(
            Platform::Claude,
            "ElicitationResult"
        ));
        assert!(event_supports_matcher(Platform::Claude, "SubagentStop"));
        assert!(!event_supports_matcher(Platform::Codex, "Stop"));
        assert!(allowed_extra_fields(Platform::Codex, "PreToolUse").contains("stopReason"));
        assert!(allowed_extra_fields(Platform::Claude, "SessionStart").is_empty());
        assert!(allowed_extra_fields(Platform::Codex, "unknown").is_empty());
    }
}
