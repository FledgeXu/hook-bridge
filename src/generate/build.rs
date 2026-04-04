use std::collections::BTreeMap;

use crate::config::NormalizedConfig;
use crate::platform::Platform;
use crate::platform::capability;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformGenerationRule {
    pub platform: Platform,
    pub rule_id: String,
    pub event: String,
    pub native_event: String,
    pub command: String,
    pub matcher: Option<String>,
    pub timeout_field: String,
    pub timeout_value: u64,
    pub native_extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformGenerationInput {
    pub rules: Vec<PlatformGenerationRule>,
}

#[must_use]
pub fn build_generation_input(normalized: &NormalizedConfig) -> PlatformGenerationInput {
    let rules = normalized
        .hooks
        .iter()
        .flat_map(|hook| {
            [Platform::Claude, Platform::Codex]
                .into_iter()
                .filter_map(move |platform| {
                    let rule = match platform {
                        Platform::Claude => hook.claude.as_ref(),
                        Platform::Codex => hook.codex.as_ref(),
                    }?;

                    Some(PlatformGenerationRule {
                        platform,
                        rule_id: hook.id.clone(),
                        event: rule.event.clone(),
                        native_event: native_event_name(&rule.event).to_string(),
                        command: build_run_command(platform, &hook.id),
                        matcher: rule.matcher.clone(),
                        timeout_field: capability::timeout_field_name(platform).to_string(),
                        timeout_value: rule.timeout_sec,
                        native_extra: rule.extra.clone(),
                    })
                })
        })
        .collect();

    PlatformGenerationInput { rules }
}

pub(crate) fn collect_platform_hooks(
    generation: &PlatformGenerationInput,
    platform: Platform,
) -> BTreeMap<String, Vec<serde_json::Value>> {
    let mut hooks = BTreeMap::new();

    for rule in generation
        .rules
        .iter()
        .filter(|rule| rule.platform == platform)
    {
        hooks
            .entry(rule.native_event.clone())
            .or_insert_with(Vec::new)
            .push(platform_rule_to_json(rule));
    }

    hooks
}

fn platform_rule_to_json(rule: &PlatformGenerationRule) -> serde_json::Value {
    let mut handler = serde_json::Map::new();
    handler.insert(
        "type".to_string(),
        serde_json::Value::String("command".to_string()),
    );
    handler.insert(
        "command".to_string(),
        serde_json::Value::String(rule.command.clone()),
    );
    handler.insert(
        "id".to_string(),
        serde_json::Value::String(rule.rule_id.clone()),
    );
    handler.insert(
        rule.timeout_field.clone(),
        serde_json::Value::Number(rule.timeout_value.into()),
    );

    for (key, value) in &rule.native_extra {
        handler.insert(key.clone(), value.clone());
    }

    let mut matcher_group = serde_json::Map::new();
    if let Some(matcher) = &rule.matcher {
        matcher_group.insert(
            "matcher".to_string(),
            serde_json::Value::String(matcher.clone()),
        );
    }
    matcher_group.insert(
        "hooks".to_string(),
        serde_json::Value::Array(vec![serde_json::Value::Object(handler)]),
    );

    serde_json::Value::Object(matcher_group)
}

#[must_use]
pub fn build_run_command(platform: Platform, rule_id: &str) -> String {
    format!(
        "hook_bridge run --platform {} --rule-id {}",
        platform.as_str(),
        rule_id
    )
}

pub(crate) fn native_event_name(event: &str) -> &str {
    match event {
        "before_command" => "PreToolUse",
        "after_command" => "PostToolUse",
        "session_start" => "SessionStart",
        _ => event,
    }
}
