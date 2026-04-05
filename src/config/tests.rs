use std::path::PathBuf;

use crate::error::HookBridgeError;
use crate::platform::Platform;

use super::parse_and_normalize;

fn assert_validation_error_contains(yaml: &str, needle: &str) {
    let message =
        parse_and_normalize("cfg.yaml".into(), yaml)
            .err()
            .and_then(|error| match error {
                HookBridgeError::ConfigValidation { message } => Some(message),
                _ => None,
            });
    assert!(
        message
            .as_deref()
            .is_some_and(|message| message.contains(needle)),
        "config validation message should contain '{needle}'"
    );
}

#[test]
fn rejects_missing_required_field() {
    let yaml = r"
version: 1
hooks:
  - id: r1
    event: before_command
";

    assert_validation_error_contains(yaml, "missing field `command`");
}

#[test]
fn rejects_duplicate_id() {
    let yaml = r"
version: 1
hooks:
  - id: same
    event: before_command
    command: echo one
  - id: same
    event: after_command
    command: echo two
";

    assert_validation_error_contains(yaml, "duplicate rule id 'same'");
}

#[test]
fn rejects_invalid_version() {
    let yaml = r"
version: 2
hooks:
  - id: r1
    event: before_command
    command: echo one
";

    assert_validation_error_contains(yaml, "field 'version' must be 1");
}

#[test]
fn rejects_empty_hook_list() {
    let yaml = r"
version: 1
hooks: []
";

    assert_validation_error_contains(yaml, "field 'hooks' must not be empty");
}

#[test]
fn rejects_invalid_event_name() {
    let yaml = r"
version: 1
hooks:
  - id: r1
    event: not_a_real_event
    command: echo one
";

    assert_validation_error_contains(yaml, "field 'event' value 'not_a_real_event'");
}

#[test]
fn rejects_invalid_rule_id() {
    let yaml = r"
version: 1
hooks:
  - id: bad id
    event: before_command
    command: echo one
";

    assert_validation_error_contains(yaml, "has invalid id");
}

#[test]
fn rejects_empty_rule_id() {
    let yaml = r"
version: 1
hooks:
  - id: '   '
    event: before_command
    command: echo one
";

    assert_validation_error_contains(yaml, "field 'id' must not be empty");
}

#[test]
fn rejects_platform_specific_field_in_common_layer() {
    let yaml = r"
version: 1
hooks:
  - id: r1
    event: before_command
    command: echo one
    decision: block
";

    assert_validation_error_contains(yaml, "field 'decision' is platform-specific");
}

#[test]
fn rejects_enabled_in_common_layer() {
    let yaml = r"
version: 1
hooks:
  - id: r1
    event: before_command
    command: echo one
    enabled: false
";

    assert_validation_error_contains(yaml, "field 'enabled' is platform-specific");
}

#[test]
fn rejects_codex_stop_fields_in_common_layer() {
    let yaml = r"
version: 1
hooks:
  - id: r1
    event: before_command
    command: echo one
    stopReason: nope
";

    assert_validation_error_contains(
        yaml,
        "field 'stopReason' is platform-specific and must be set in 'platforms.codex.<field>'",
    );
}

#[test]
fn accepts_top_level_status_message() {
    let yaml = r"
version: 1
hooks:
  - id: r1
    event: before_command
    command: echo one
    status_message: Checking command policy
";

    let config_result = parse_and_normalize("cfg.yaml".into(), yaml);
    assert!(config_result.is_ok(), "config should parse");
    let Ok(config) = config_result else {
        return;
    };
    let maybe_rule = config.hooks.iter().find(|hook| hook.id == "r1");
    assert!(maybe_rule.is_some(), "rule must exist");
    let Some(rule) = maybe_rule else {
        return;
    };

    assert_eq!(
        rule.status_message.as_deref(),
        Some("Checking command policy")
    );
}

#[test]
fn rejects_empty_status_message() {
    let yaml = r"
version: 1
hooks:
  - id: r1
    event: before_command
    command: echo one
    status_message: '   '
";

    assert_validation_error_contains(yaml, "field 'status_message' must not be empty");
}

#[test]
fn platform_override_replaces_common_fields() {
    let yaml = r"
version: 1
defaults:
  shell: sh
  timeout_sec: 30
hooks:
  - id: r1
    event: before_command
    command: echo common
    matcher: .*common.*
    platforms:
      codex:
        event: after_command
        command: echo codex
        matcher: .*codex.*
        timeout_sec: 9
";

    let config_result = parse_and_normalize("cfg.yaml".into(), yaml);
    assert!(config_result.is_ok(), "config should parse");
    let Ok(config) = config_result else {
        return;
    };
    let maybe_rule = config.hooks.iter().find(|hook| hook.id == "r1");
    assert!(maybe_rule.is_some(), "rule must exist");
    let Some(rule) = maybe_rule else {
        return;
    };

    assert_eq!(
        rule.codex.as_ref().map(|value| value.event.as_str()),
        Some("PostToolUse")
    );
    assert_eq!(
        rule.codex.as_ref().map(|value| value.command.as_str()),
        Some("echo codex")
    );
    assert_eq!(
        rule.codex
            .as_ref()
            .and_then(|value| value.matcher.as_deref()),
        Some(".*codex.*")
    );
    assert_eq!(rule.codex.as_ref().map(|value| value.timeout_sec), Some(9));
    assert_eq!(
        rule.claude.as_ref().map(|value| value.command.as_str()),
        Some("echo common")
    );
}

#[test]
fn platform_specific_extra_field_is_allowed_in_platform_override() {
    let yaml = r"
version: 1
hooks:
  - id: r1
    event: before_command
    command: echo ok
    platforms:
      codex:
        continue: false
";

    let config_result = parse_and_normalize("cfg.yaml".into(), yaml);
    assert!(config_result.is_ok(), "config should parse");
    let Ok(config) = config_result else {
        return;
    };
    let maybe_rule = config.hooks.iter().find(|hook| hook.id == "r1");
    assert!(maybe_rule.is_some(), "rule must exist");
    let Some(rule) = maybe_rule else {
        return;
    };
    let codex = rule.codex.as_ref();
    assert!(codex.is_some(), "codex mapping must exist");
    let Some(codex_rule) = codex else {
        return;
    };
    assert_eq!(
        codex_rule.extra.get("continue"),
        Some(&serde_json::Value::Bool(false))
    );
}

#[test]
fn rejects_unsupported_claude_session_start_extra_field() {
    let yaml = r"
version: 1
hooks:
  - id: r1
    event: session_start
    command: echo ok
    platforms:
      claude:
        decision: block
";

    assert_validation_error_contains(
        yaml,
        "field 'platforms.claude.decision' is not supported for event 'SessionStart'",
    );
}

#[test]
fn allows_matcher_for_claude_elicitation_event() {
    let yaml = r"
version: 1
hooks:
  - id: r1
    event: Elicitation
    command: echo ok
    matcher: mcp-server-name
    platforms:
      codex:
        enabled: false
";

    let config_result = parse_and_normalize("cfg.yaml".into(), yaml);
    assert!(config_result.is_ok(), "config should parse");
    let Ok(config) = config_result else {
        return;
    };
    let maybe_rule = config.hooks.iter().find(|hook| hook.id == "r1");
    assert!(maybe_rule.is_some(), "rule must exist");
    let Some(rule) = maybe_rule else {
        return;
    };

    assert_eq!(
        rule.claude
            .as_ref()
            .and_then(|value| value.matcher.as_deref()),
        Some("mcp-server-name")
    );
}

#[test]
fn allows_matcher_for_claude_notification_and_subagent_events() {
    let yaml = r"
version: 1
hooks:
  - id: notify
    event: Notification
    command: echo notify
    matcher: token-refresh
    platforms:
      codex:
        enabled: false
  - id: sub_start
    event: SubagentStart
    command: echo start
    matcher: review-agent
    platforms:
      codex:
        enabled: false
  - id: sub_stop
    event: SubagentStop
    command: echo stop
    matcher: review-agent
    platforms:
      codex:
        enabled: false
";

    let config_result = parse_and_normalize("cfg.yaml".into(), yaml);
    assert!(config_result.is_ok(), "config should parse");
    let Ok(config) = config_result else {
        return;
    };

    let notify = config.hooks.iter().find(|hook| hook.id == "notify");
    assert!(notify.is_some(), "notification rule must exist");
    let Some(notify_rule) = notify else {
        return;
    };
    assert_eq!(
        notify_rule
            .claude
            .as_ref()
            .and_then(|value| value.matcher.as_deref()),
        Some("token-refresh")
    );

    let sub_start = config.hooks.iter().find(|hook| hook.id == "sub_start");
    assert!(sub_start.is_some(), "subagent-start rule must exist");
    let Some(sub_start_rule) = sub_start else {
        return;
    };
    assert_eq!(
        sub_start_rule
            .claude
            .as_ref()
            .and_then(|value| value.matcher.as_deref()),
        Some("review-agent")
    );

    let sub_stop = config.hooks.iter().find(|hook| hook.id == "sub_stop");
    assert!(sub_stop.is_some(), "subagent-stop rule must exist");
    let Some(sub_stop_rule) = sub_stop else {
        return;
    };
    assert_eq!(
        sub_stop_rule
            .claude
            .as_ref()
            .and_then(|value| value.matcher.as_deref()),
        Some("review-agent")
    );
}

#[test]
fn default_inheritance_order_is_platform_then_rule_then_defaults() {
    let yaml = r"
version: 1
defaults:
  shell: sh
  timeout_sec: 90
  max_retries: 7
  working_dir: /from-default
hooks:
  - id: r1
    event: before_command
    command: echo common
    shell: bash
    timeout_sec: 50
    max_retries: 4
    working_dir: /from-rule
    platforms:
      codex:
        command: echo codex
        shell: zsh
        timeout_sec: 10
        max_retries: 2
        working_dir: /from-platform
";

    let config_result = parse_and_normalize("cfg.yaml".into(), yaml);
    assert!(config_result.is_ok(), "config should parse");
    let Ok(config) = config_result else {
        return;
    };
    let maybe_rule = config.hooks.iter().find(|hook| hook.id == "r1");
    assert!(maybe_rule.is_some(), "rule must exist");
    let Some(rule) = maybe_rule else {
        return;
    };

    assert_eq!(
        rule.claude.as_ref().map(|value| value.shell.as_str()),
        Some("bash")
    );
    assert_eq!(
        rule.claude.as_ref().map(|value| value.timeout_sec),
        Some(50)
    );
    assert_eq!(rule.claude.as_ref().map(|value| value.max_retries), Some(4));
    assert_eq!(
        rule.claude
            .as_ref()
            .and_then(|value| value.working_dir.as_ref()),
        Some(&PathBuf::from("/from-rule"))
    );

    assert_eq!(
        rule.codex.as_ref().map(|value| value.shell.as_str()),
        Some("zsh")
    );
    assert_eq!(rule.codex.as_ref().map(|value| value.timeout_sec), Some(10));
    assert_eq!(rule.codex.as_ref().map(|value| value.max_retries), Some(2));
    assert_eq!(
        rule.codex
            .as_ref()
            .and_then(|value| value.working_dir.as_ref()),
        Some(&PathBuf::from("/from-platform"))
    );
}

#[test]
fn env_and_working_dir_support_null_and_missing_paths() {
    let yaml = r"
version: 1
defaults:
  working_dir: /base
hooks:
  - id: r1
    event: before_command
    command: echo one
    env: null
    platforms:
      codex:
        working_dir: null
        env:
          X: y
  - id: r2
    event: before_command
    command: echo two
";

    let config_result = parse_and_normalize("cfg.yaml".into(), yaml);
    assert!(config_result.is_ok(), "config should parse");
    let Ok(config) = config_result else {
        return;
    };
    let rule1 = config.hooks.iter().find(|hook| hook.id == "r1");
    assert!(rule1.is_some(), "r1 must exist");
    let Some(rule1_value) = rule1 else {
        return;
    };
    let rule1_claude = rule1_value.claude.as_ref();
    assert!(rule1_claude.is_some(), "r1 claude should exist");
    let Some(rule1_claude_value) = rule1_claude else {
        return;
    };
    assert!(rule1_claude_value.env.is_empty());
    assert_eq!(
        rule1_claude_value.working_dir.as_ref(),
        Some(&PathBuf::from("/base"))
    );

    let rule1_codex = rule1_value.codex.as_ref();
    assert!(rule1_codex.is_some(), "r1 codex should exist");
    let Some(rule1_codex_value) = rule1_codex else {
        return;
    };
    assert_eq!(rule1_codex_value.env.get("X"), Some(&"y".to_string()));
    assert_eq!(
        rule1_codex_value.working_dir.as_ref(),
        Some(&PathBuf::from("/base"))
    );

    let rule2 = config.hooks.iter().find(|hook| hook.id == "r2");
    assert!(rule2.is_some(), "r2 must exist");
    let Some(rule2_value) = rule2 else {
        return;
    };
    assert_eq!(
        rule2_value
            .claude
            .as_ref()
            .and_then(|value| value.working_dir.as_ref()),
        Some(&PathBuf::from("/base"))
    );
}

#[test]
fn find_platform_rule_reports_missing_cases() {
    let yaml = r"
version: 1
hooks:
  - id: only_claude
    event: before_command
    command: echo ok
    platforms:
      codex:
        enabled: false
";
    let config_result = parse_and_normalize("cfg.yaml".into(), yaml);
    assert!(config_result.is_ok(), "config should parse");
    let Ok(config) = config_result else {
        return;
    };

    assert_eq!(
        config.find_platform_rule(Platform::Codex, "only_claude"),
        Err(HookBridgeError::ConfigValidation {
            message: "rule 'only_claude' has no codex mapping".to_string(),
        })
    );
    let codex_only_result = parse_and_normalize(
        "cfg.yaml".into(),
        r"
version: 1
hooks:
  - id: only_codex
    event: before_command
    command: echo ok
    platforms:
      claude:
        enabled: false
",
    );
    assert!(codex_only_result.is_ok(), "config should parse");
    let Ok(codex_only) = codex_only_result else {
        return;
    };
    assert_eq!(
        codex_only.find_platform_rule(Platform::Claude, "only_codex"),
        Err(HookBridgeError::ConfigValidation {
            message: "rule 'only_codex' has no claude mapping".to_string(),
        })
    );
    assert_eq!(
        config.find_platform_rule(Platform::Claude, "missing"),
        Err(HookBridgeError::ConfigValidation {
            message: "rule 'missing' does not exist in config".to_string(),
        })
    );
}

#[test]
fn rejects_unknown_extra_and_invalid_platform_specific_fields() {
    let unknown = r"
version: 1
hooks:
  - id: r1
    event: before_command
    command: echo one
    mystery: true
";
    let no_platforms = r"
version: 1
hooks:
  - id: r1
    event: before_command
    command: echo one
    platforms:
      claude:
        enabled: false
      codex:
        enabled: false
";
    let empty_shell = r"
version: 1
hooks:
  - id: r1
    event: before_command
    command: echo one
    shell: '   '
";
    let empty_command = r"
version: 1
hooks:
  - id: r1
    event: before_command
    command: '   '
";
    let invalid_platform_field = r"
version: 1
hooks:
  - id: r1
    event: before_command
    command: echo one
    platforms:
      codex:
        decision: stop
";
    let invalid_platform_status_message = r"
version: 1
hooks:
  - id: r2
    event: before_command
    command: echo one
    platforms:
      codex:
        status_message: hidden
";

    assert_validation_error_contains(unknown, "is not recognized in hook schema");
    assert_validation_error_contains(no_platforms, "does not map to any platform");
    assert_validation_error_contains(empty_shell, "field 'shell' must not be empty");
    assert_validation_error_contains(empty_command, "field 'command' must not be empty");
    assert_validation_error_contains(
        invalid_platform_field,
        "field 'platforms.codex.decision' is not supported for event 'PreToolUse'",
    );
    assert_validation_error_contains(
        invalid_platform_status_message,
        "field 'platforms.codex.status_message' is not supported for event 'PreToolUse'",
    );
}
