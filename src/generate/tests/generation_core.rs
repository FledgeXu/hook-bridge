use super::*;

#[test]
fn command_template_contains_platform_and_rule_id() {
    assert_eq!(
        build_run_command(Platform::Codex, "r1"),
        "hook_bridge run --platform codex --rule-id r1"
    );
}

#[test]
fn recognizes_managed_content() {
    let json = r#"{"_hook_bridge":{"managed_by":"hook_bridge"}}"#;
    assert!(is_managed_content(json));
}

#[test]
fn rejects_unmanaged_content() {
    let json = r#"{"hooks":[]}"#;
    assert!(!is_managed_content(json));
}

#[test]
fn rejects_invalid_json_as_unmanaged_content() {
    assert!(!is_managed_content("{"));
}

#[test]
fn single_rule_expands_to_two_platform_rules() {
    let yaml = r"
version: 1
hooks:
  - id: r1
    event: before_command
    command: echo ok
";
    let parsed = parse_and_normalize("/tmp/cfg.yaml".into(), yaml);
    assert!(parsed.is_ok(), "config should parse");
    let Ok(config) = parsed else {
        return;
    };
    let generation = build_generation_input(&config);

    assert_eq!(generation.rules.len(), 2);
    assert!(generation.rules.iter().any(|rule| {
        rule.platform == Platform::Claude
            && rule.rule_id == "r1"
            && rule.native_event == "PreToolUse"
    }));
    assert!(generation.rules.iter().any(|rule| {
        rule.platform == Platform::Codex
            && rule.rule_id == "r1"
            && rule.native_event == "PreToolUse"
    }));
}

#[test]
fn disabled_platform_does_not_generate_rule() {
    let yaml = r"
version: 1
hooks:
  - id: r1
    event: before_command
    command: echo ok
    platforms:
      codex:
        enabled: false
";
    let parsed = parse_and_normalize("/tmp/cfg.yaml".into(), yaml);
    assert!(parsed.is_ok(), "config should parse");
    let Ok(config) = parsed else {
        return;
    };
    let generation = build_generation_input(&config);

    assert_eq!(generation.rules.len(), 1);
    assert_eq!(
        generation.rules.first().map(|rule| rule.platform),
        Some(Platform::Claude)
    );
}

#[test]
fn matcher_support_is_distinguished_by_event() {
    let yaml = r"
version: 1
hooks:
  - id: r1
    event: session_start
    command: echo ok
    matcher: never
";
    let parsed = parse_and_normalize("/tmp/cfg.yaml".into(), yaml);
    assert!(parsed.is_ok(), "session_start should allow matcher");

    let yaml = r"
version: 1
hooks:
  - id: r2
    event: Elicitation
    command: echo ok
    matcher: mcp-server-name
    platforms:
      codex:
        enabled: false
";
    let parsed = parse_and_normalize("/tmp/cfg.yaml".into(), yaml);
    assert!(parsed.is_ok(), "elicitation should allow matcher");

    let yaml = r"
version: 1
hooks:
  - id: r3
    event: Notification
    command: echo ok
    matcher: token-refresh
    platforms:
      codex:
        enabled: false
";
    let parsed = parse_and_normalize("/tmp/cfg.yaml".into(), yaml);
    assert!(parsed.is_ok(), "notification should allow matcher");

    let yaml = r"
version: 1
hooks:
  - id: r4
    event: SubagentStop
    command: echo ok
    matcher: review-agent
    platforms:
      codex:
        enabled: false
";
    let parsed = parse_and_normalize("/tmp/cfg.yaml".into(), yaml);
    assert!(parsed.is_ok(), "subagent stop should allow matcher");
}

#[test]
fn native_extra_fields_only_exist_on_target_platform() {
    let yaml = r"
version: 1
hooks:
  - id: r1
    event: before_command
    command: echo ok
    platforms:
      codex:
        stopReason: denied
";
    let parsed = parse_and_normalize("/tmp/cfg.yaml".into(), yaml);
    assert!(parsed.is_ok(), "config should parse");
    let Ok(config) = parsed else {
        return;
    };
    let generation = build_generation_input(&config);

    let claude_rule = generation
        .rules
        .iter()
        .find(|rule| rule.platform == Platform::Claude);
    assert!(claude_rule.is_some(), "claude rule should exist");
    let Some(claude_value) = claude_rule else {
        return;
    };
    let codex_rule = generation
        .rules
        .iter()
        .find(|rule| rule.platform == Platform::Codex);
    assert!(codex_rule.is_some(), "codex rule should exist");
    let Some(codex_value) = codex_rule else {
        return;
    };
    assert!(!claude_value.native_extra.contains_key("stopReason"));
    assert_eq!(
        codex_value.native_extra.get("stopReason"),
        Some(&serde_json::Value::String("denied".to_string()))
    );
}

#[test]
fn top_level_status_message_is_emitted_for_both_platforms() {
    let yaml = r"
version: 1
hooks:
  - id: r1
    event: before_command
    command: echo ok
    status_message: Checking command policy
";
    let parsed = parse_and_normalize("/tmp/cfg.yaml".into(), yaml);
    assert!(parsed.is_ok(), "config should parse");
    let Ok(config) = parsed else {
        return;
    };
    let generation = build_generation_input(&config);

    for platform in [Platform::Claude, Platform::Codex] {
        let rule = generation
            .rules
            .iter()
            .find(|rule| rule.platform == platform);
        assert!(rule.is_some(), "platform rule should exist");
        let Some(rule) = rule else {
            return;
        };
        assert_eq!(
            rule.status_message.as_deref(),
            Some("Checking command policy")
        );
    }
}

#[test]
fn execute_writes_status_message_to_claude_and_codex_targets() {
    let temp_result = tempfile::tempdir();
    assert!(temp_result.is_ok(), "tempdir creation should succeed");
    let Ok(temp) = temp_result else {
        return;
    };

    let Ok(_lock) = crate::CWD_LOCK.lock() else {
        return;
    };
    let guard_result = CurrentDirGuard::enter(temp.path());
    assert!(guard_result.is_ok(), "cwd switch should succeed");
    let Ok(_guard) = guard_result else {
        return;
    };

    let config_path = temp.path().join("hook-bridge.yaml");
    let write_result = std::fs::write(
        &config_path,
        r"
version: 1
hooks:
  - id: r1
    event: before_command
    command: echo ok
    status_message: Checking command policy
",
    );
    assert!(write_result.is_ok(), "config should be written");

    let args = GenerateArgs {
        config: PathBuf::from("hook-bridge.yaml"),
        platform: None,
        force: false,
        yes: false,
    };
    let runtime = crate::runtime::RealRuntime::default();

    let execute_result = execute(&args, &runtime);
    assert!(execute_result.is_ok(), "generate should succeed");

    let claude_content = std::fs::read_to_string(temp.path().join(CLAUDE_TARGET));
    assert!(claude_content.is_ok(), "claude target should exist");
    let codex_content = std::fs::read_to_string(temp.path().join(CODEX_TARGET));
    assert!(codex_content.is_ok(), "codex target should exist");

    assert!(
        claude_content
            .as_deref()
            .is_ok_and(|content| content.contains(r#""statusMessage": "Checking command policy""#))
    );
    assert!(
        codex_content
            .as_deref()
            .is_ok_and(|content| content.contains(r#""statusMessage": "Checking command policy""#))
    );
}

#[test]
fn enabled_flag_is_not_emitted_into_platform_native_fields() {
    let yaml = r"
version: 1
hooks:
  - id: r1
    event: before_command
    command: echo ok
    platforms:
      codex:
        enabled: true
        stopReason: denied
";
    let parsed = parse_and_normalize("/tmp/cfg.yaml".into(), yaml);
    assert!(parsed.is_ok(), "config should parse");
    let Ok(config) = parsed else {
        return;
    };
    let generation = build_generation_input(&config);
    let codex_rule = generation
        .rules
        .iter()
        .find(|rule| rule.platform == Platform::Codex);
    assert!(codex_rule.is_some(), "codex rule should exist");
    let Some(codex_value) = codex_rule else {
        return;
    };
    assert!(!codex_value.native_extra.contains_key("enabled"));
    assert_eq!(
        codex_value.native_extra.get("stopReason"),
        Some(&serde_json::Value::String("denied".to_string()))
    );
}

#[test]
fn timeout_sec_maps_to_platform_timeout_field() {
    let yaml = r"
version: 1
hooks:
  - id: r1
    event: before_command
    command: echo ok
    timeout_sec: 12
";
    let parsed = parse_and_normalize("/tmp/cfg.yaml".into(), yaml);
    assert!(parsed.is_ok(), "config should parse");
    let Ok(config) = parsed else {
        return;
    };
    let generation = build_generation_input(&config);

    for rule in generation.rules {
        assert_eq!(rule.timeout_field, "timeout_sec");
        assert_eq!(rule.timeout_value, 12);
    }
}

#[test]
fn normalize_and_target_paths_are_stable() {
    let absolute = Path::new("/tmp/hook-bridge.yaml");
    let runtime_cwd = PathBuf::from("/tmp/runtime-cwd");

    assert_eq!(
        normalize_path(absolute, &runtime_cwd),
        absolute.to_path_buf()
    );
    assert_eq!(target_path(Platform::Claude), Path::new(CLAUDE_TARGET));
    assert_eq!(target_path(Platform::Codex), Path::new(CODEX_TARGET));
}

#[test]
fn normalize_config_path_joins_relative_paths_from_runtime_current_directory() {
    let runtime_cwd = PathBuf::from("/tmp/runtime-cwd");
    assert_eq!(
        normalize_path(Path::new("hook-bridge.yaml"), &runtime_cwd),
        PathBuf::from("/tmp/runtime-cwd").join("hook-bridge.yaml")
    );
}

#[test]
fn force_overwrite_interactive_requires_both_stdin_and_stderr_tty() {
    assert!(!is_force_overwrite_interactive(false, false));
    assert!(!is_force_overwrite_interactive(true, false));
    assert!(!is_force_overwrite_interactive(false, true));
    assert!(is_force_overwrite_interactive(true, true));
}

#[test]
fn force_overwrite_target_message_lists_all_targets() {
    let message = format_force_overwrite_targets(&[
        PathBuf::from("/tmp/force-a.json"),
        PathBuf::from("/tmp/force-b.json"),
    ]);

    assert!(message.starts_with("Force overwrite will replace these target files:\n"));
    assert!(message.contains("  - /tmp/force-a.json\n"));
    assert!(message.contains("  - /tmp/force-b.json\n"));
}

#[test]
fn native_event_name_maps_supported_and_passthrough_events() {
    assert_eq!(native_event_name("before_command"), "before_command");
    assert_eq!(native_event_name("after_command"), "after_command");
    assert_eq!(native_event_name("session_start"), "session_start");
    assert_eq!(native_event_name("custom"), "custom");
}

#[test]
fn collect_platform_hooks_emits_matcher_and_native_fields() {
    let yaml = r"
version: 1
hooks:
  - id: r1
    event: before_command
    command: echo ok
    matcher: .*
    platforms:
      codex:
        stopReason: denied
";
    let config_result = parse_and_normalize("/tmp/cfg.yaml".into(), yaml);
    assert!(config_result.is_ok(), "config should parse");
    let Ok(config) = config_result else {
        return;
    };
    let generation = build_generation_input(&config);
    let codex_hooks = collect_platform_hooks(&generation, Platform::Codex);

    assert_eq!(
        codex_hooks,
        BTreeMap::from([(
            "PreToolUse".to_string(),
            vec![serde_json::json!({
                "matcher": ".*",
                "hooks": [{
                    "type": "command",
                    "id": "r1",
                    "command": "hook_bridge run --platform codex --rule-id r1",
                    "timeout_sec": 30,
                    "stopReason": "denied",
                }]
            })]
        )])
    );
}

#[test]
fn collect_platform_hooks_emits_status_message() {
    let yaml = r"
version: 1
hooks:
  - id: r1
    event: before_command
    command: echo ok
    status_message: Checking command policy
";
    let config_result = parse_and_normalize("/tmp/cfg.yaml".into(), yaml);
    assert!(config_result.is_ok(), "config should parse");
    let Ok(config) = config_result else {
        return;
    };
    let generation = build_generation_input(&config);
    let codex_hooks = collect_platform_hooks(&generation, Platform::Codex);

    assert_eq!(
        codex_hooks,
        BTreeMap::from([(
            "PreToolUse".to_string(),
            vec![serde_json::json!({
                "hooks": [{
                    "type": "command",
                    "command": "hook_bridge run --platform codex --rule-id r1",
                    "statusMessage": "Checking command policy",
                    "id": "r1",
                    "timeout_sec": 30,
                }]
            })]
        )])
    );
}

#[test]
fn collect_platform_hooks_emits_matcher_for_claude_elicitation() {
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
    let parsed = parse_and_normalize("/tmp/cfg.yaml".into(), yaml);
    assert!(parsed.is_ok(), "config should parse");
    let Ok(config) = parsed else {
        return;
    };
    let generation = build_generation_input(&config);

    let maybe_rule = generation
        .rules
        .iter()
        .find(|rule| rule.platform == Platform::Claude);
    assert!(maybe_rule.is_some(), "claude rule should exist");
    let Some(rule) = maybe_rule else {
        return;
    };

    assert_eq!(rule.event, "Elicitation");
    assert_eq!(rule.matcher.as_deref(), Some("mcp-server-name"));
}

#[test]
fn collect_platform_hooks_emits_matcher_for_claude_notification_and_subagent_events() {
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
    let parsed = parse_and_normalize("/tmp/cfg.yaml".into(), yaml);
    assert!(parsed.is_ok(), "config should parse");
    let Ok(config) = parsed else {
        return;
    };
    let generation = build_generation_input(&config);

    let notify = generation
        .rules
        .iter()
        .find(|rule| rule.platform == Platform::Claude && rule.event == "Notification");
    assert!(notify.is_some(), "claude notification rule should exist");
    let Some(notify_rule) = notify else {
        return;
    };
    assert_eq!(notify_rule.matcher.as_deref(), Some("token-refresh"));

    let sub_start = generation
        .rules
        .iter()
        .find(|rule| rule.platform == Platform::Claude && rule.event == "SubagentStart");
    assert!(
        sub_start.is_some(),
        "claude subagent-start rule should exist"
    );
    let Some(sub_start_rule) = sub_start else {
        return;
    };
    assert_eq!(sub_start_rule.matcher.as_deref(), Some("review-agent"));

    let sub_stop = generation
        .rules
        .iter()
        .find(|rule| rule.platform == Platform::Claude && rule.event == "SubagentStop");
    assert!(sub_stop.is_some(), "claude subagent-stop rule should exist");
    let Some(sub_stop_rule) = sub_stop else {
        return;
    };
    assert_eq!(sub_stop_rule.matcher.as_deref(), Some("review-agent"));
}
