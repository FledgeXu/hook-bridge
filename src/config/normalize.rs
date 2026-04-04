use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use crate::error::HookBridgeError;
use crate::platform::Platform;
use crate::platform::capability;

use super::schema::{RawConfig, RawDefaults, RawHookRule};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedConfig {
    pub source_path: PathBuf,
    pub hooks: Vec<NormalizedHook>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedHook {
    pub id: String,
    pub description: Option<String>,
    pub claude: Option<PlatformRule>,
    pub codex: Option<PlatformRule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformRule {
    pub event: String,
    pub command: String,
    pub matcher: Option<String>,
    pub shell: String,
    pub timeout_sec: u64,
    pub max_retries: u32,
    pub working_dir: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl NormalizedConfig {
    /// Finds a normalized rule by id and platform.
    ///
    /// # Errors
    ///
    /// Returns an error if the rule id is unknown or the platform rule is not configured.
    pub fn find_platform_rule(
        &self,
        platform: Platform,
        rule_id: &str,
    ) -> Result<&PlatformRule, HookBridgeError> {
        let Some(hook) = self.hooks.iter().find(|hook| hook.id == rule_id) else {
            return Err(HookBridgeError::ConfigValidation {
                message: format!("rule '{rule_id}' does not exist in config"),
            });
        };

        match platform {
            Platform::Claude => {
                hook.claude
                    .as_ref()
                    .ok_or_else(|| HookBridgeError::ConfigValidation {
                        message: format!("rule '{rule_id}' has no claude mapping"),
                    })
            }
            Platform::Codex => {
                hook.codex
                    .as_ref()
                    .ok_or_else(|| HookBridgeError::ConfigValidation {
                        message: format!("rule '{rule_id}' has no codex mapping"),
                    })
            }
        }
    }
}

/// Parses and validates a YAML config file into normalized domain config.
///
/// # Errors
///
/// Returns configuration validation errors when the YAML shape or semantics are invalid.
pub fn parse_and_normalize(
    source_path: PathBuf,
    yaml: &str,
) -> Result<NormalizedConfig, HookBridgeError> {
    let raw: RawConfig =
        serde_yaml::from_str(yaml).map_err(|error| HookBridgeError::ConfigValidation {
            message: format!("failed to parse YAML: {error}"),
        })?;
    validate_and_normalize(source_path, raw)
}

fn validate_and_normalize(
    source_path: PathBuf,
    raw: RawConfig,
) -> Result<NormalizedConfig, HookBridgeError> {
    if raw.version != 1 {
        return Err(HookBridgeError::ConfigValidation {
            message: format!("field 'version' must be 1, got {}", raw.version),
        });
    }

    if raw.hooks.is_empty() {
        return Err(HookBridgeError::ConfigValidation {
            message: "field 'hooks' must not be empty".to_string(),
        });
    }

    let mut seen = HashSet::new();
    let mut hooks = Vec::with_capacity(raw.hooks.len());

    for raw_rule in raw.hooks {
        if raw_rule.id.trim().is_empty() {
            return Err(HookBridgeError::ConfigValidation {
                message: "field 'id' must not be empty".to_string(),
            });
        }
        if !is_valid_rule_id(&raw_rule.id) {
            return Err(HookBridgeError::ConfigValidation {
                message: format!(
                    "rule '{}' has invalid id: only [A-Za-z0-9._-] are allowed",
                    raw_rule.id
                ),
            });
        }
        if !seen.insert(raw_rule.id.clone()) {
            return Err(HookBridgeError::ConfigValidation {
                message: format!("duplicate rule id '{}'", raw_rule.id),
            });
        }

        validate_rule_extra_fields(&raw_rule)?;

        let claude = build_platform_rule(Platform::Claude, &raw.defaults, &raw_rule)?;
        let codex = build_platform_rule(Platform::Codex, &raw.defaults, &raw_rule)?;

        if claude.is_none() && codex.is_none() {
            return Err(HookBridgeError::ConfigValidation {
                message: format!("rule '{}' does not map to any platform", raw_rule.id),
            });
        }

        hooks.push(NormalizedHook {
            id: raw_rule.id,
            description: raw_rule.description,
            claude,
            codex,
        });
    }

    Ok(NormalizedConfig { source_path, hooks })
}

fn validate_rule_extra_fields(raw_rule: &RawHookRule) -> Result<(), HookBridgeError> {
    if let Some(key) = raw_rule.extra.keys().next() {
        if let Some(path_hint) = platform_specific_field_path_hint(key) {
            return Err(HookBridgeError::ConfigValidation {
                message: format!(
                    "rule '{}' field '{}' is platform-specific and must be set in {}",
                    raw_rule.id, key, path_hint
                ),
            });
        }

        return Err(HookBridgeError::ConfigValidation {
            message: format!(
                "rule '{}' field '{}' is not recognized in hook schema",
                raw_rule.id, key
            ),
        });
    }

    Ok(())
}

fn platform_specific_field_path_hint(key: &str) -> Option<&'static str> {
    match key {
        "decision" | "reason" => Some("'platforms.claude.<field>'"),
        "continue" | "stopReason" | "systemMessage" => Some("'platforms.codex.<field>'"),
        "enabled" => Some("'platforms.claude.enabled' or 'platforms.codex.enabled'"),
        _ => None,
    }
}

fn build_platform_rule(
    platform: Platform,
    defaults: &RawDefaults,
    raw_rule: &RawHookRule,
) -> Result<Option<PlatformRule>, HookBridgeError> {
    let override_block = match (&raw_rule.platforms, platform) {
        (Some(platforms), Platform::Claude) => platforms.claude.clone(),
        (Some(platforms), Platform::Codex) => platforms.codex.clone(),
        _ => None,
    };

    let platform_enabled = override_block.as_ref().is_none_or(|block| {
        !matches!(
            block.extra.get("enabled"),
            Some(serde_json::Value::Bool(false))
        )
    });

    if !platform_enabled {
        return Ok(None);
    }

    let event = override_block
        .as_ref()
        .and_then(|block| block.event.clone())
        .unwrap_or_else(|| raw_rule.event.clone());

    if !capability::supports_event(platform, &event) {
        return Err(HookBridgeError::ConfigValidation {
            message: format!(
                "rule '{}' field 'event' value '{}' is not supported by platform '{}': supported={:?}",
                raw_rule.id,
                event,
                platform.as_str(),
                capability::events(platform)
            ),
        });
    }

    let matcher = override_block
        .as_ref()
        .and_then(|block| block.matcher.clone())
        .or_else(|| raw_rule.matcher.clone());

    let shell = override_block
        .as_ref()
        .and_then(|block| block.shell.clone())
        .or_else(|| raw_rule.shell.clone())
        .or_else(|| defaults.shell.clone())
        .unwrap_or_else(|| "sh".to_string());

    if shell.trim().is_empty() {
        return Err(HookBridgeError::ConfigValidation {
            message: format!("rule '{}' field 'shell' must not be empty", raw_rule.id),
        });
    }

    if matcher.is_some() && !capability::event_supports_matcher(platform, &event) {
        return Err(HookBridgeError::ConfigValidation {
            message: format!(
                "rule '{}' field 'matcher' is not supported for event '{}' on platform '{}'",
                raw_rule.id,
                event,
                platform.as_str()
            ),
        });
    }

    let timeout_sec = override_block
        .as_ref()
        .and_then(|block| block.timeout_sec)
        .or(raw_rule.timeout_sec)
        .or(defaults.timeout_sec)
        .unwrap_or(30);

    let max_retries = override_block
        .as_ref()
        .and_then(|block| block.max_retries)
        .or(raw_rule.max_retries)
        .or(defaults.max_retries)
        .unwrap_or(0);

    let working_dir = override_block
        .as_ref()
        .and_then(|block| block.working_dir.clone())
        .or_else(|| raw_rule.working_dir.clone())
        .or_else(|| defaults.working_dir.clone());

    let mut env = raw_rule.env.clone();
    if let Some(block) = &override_block {
        env.extend(block.env.clone());
    }

    let command = override_block
        .as_ref()
        .and_then(|block| block.command.clone())
        .unwrap_or_else(|| raw_rule.command.clone());

    if command.trim().is_empty() {
        return Err(HookBridgeError::ConfigValidation {
            message: format!("rule '{}' field 'command' must not be empty", raw_rule.id),
        });
    }

    let mut extra = override_block
        .as_ref()
        .map_or_else(BTreeMap::new, |block| block.extra.clone());
    extra.remove("enabled");

    validate_extra_fields(platform, &event, &extra, &raw_rule.id)?;

    Ok(Some(PlatformRule {
        event,
        command,
        matcher,
        shell,
        timeout_sec,
        max_retries,
        working_dir,
        env,
        extra,
    }))
}

fn is_valid_rule_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

fn validate_extra_fields(
    platform: Platform,
    event: &str,
    extra: &BTreeMap<String, serde_json::Value>,
    rule_id: &str,
) -> Result<(), HookBridgeError> {
    let mut allowed = capability::allowed_extra_fields(platform, event);
    allowed.insert("enabled");

    for key in extra.keys() {
        if !allowed.contains(key.as_str()) {
            return Err(HookBridgeError::ConfigValidation {
                message: format!(
                    "rule '{}' field 'platforms.{}.{}' is not supported for event '{}'",
                    rule_id,
                    platform.as_str(),
                    key,
                    event
                ),
            });
        }
    }

    Ok(())
}
