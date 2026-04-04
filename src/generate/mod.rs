use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::cli::GenerateArgs;
use crate::config::{NormalizedConfig, parse_and_normalize};
use crate::error::HookBridgeError;
use crate::platform::Platform;
use crate::platform::capability;
use crate::runtime::Runtime;
use crate::runtime::fs::atomic_write;

pub const MANAGED_BY: &str = "hook_bridge";
pub const MANAGED_VERSION: u8 = 1;
pub const CLAUDE_TARGET: &str = ".claude/settings.json";
pub const CODEX_TARGET: &str = ".codex/hooks.json";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ManagedMetadata {
    pub managed_by: String,
    pub managed_version: u8,
    pub source_config: String,
}

#[derive(Debug, Clone, Serialize)]
struct ManagedHooksFile {
    #[serde(rename = "_hook_bridge")]
    metadata: ManagedMetadata,
    hooks: BTreeMap<String, Vec<serde_json::Value>>,
}

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
    pub native_extra: std::collections::BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformGenerationInput {
    pub rules: Vec<PlatformGenerationRule>,
}

/// Executes the `generate` command.
///
/// # Errors
///
/// Returns validation, conflict, serialization, and filesystem errors.
pub fn execute(args: &GenerateArgs, runtime: &dyn Runtime) -> Result<(), HookBridgeError> {
    let yaml = runtime.fs().read_to_string(&args.config)?;
    let source_config = normalize_config_path(&args.config)?;
    let normalized = parse_and_normalize(source_config, &yaml)?;

    ensure_generation_targets_are_writable(runtime, [Platform::Claude, Platform::Codex])?;

    write_platform_file(runtime, &normalized, Platform::Claude)?;
    write_platform_file(runtime, &normalized, Platform::Codex)?;

    Ok(())
}

fn ensure_generation_targets_are_writable(
    runtime: &dyn Runtime,
    platforms: [Platform; 2],
) -> Result<(), HookBridgeError> {
    for platform in platforms {
        ensure_no_unmanaged_conflict(runtime, target_path(platform))?;
    }
    Ok(())
}

fn normalize_config_path(path: &Path) -> Result<PathBuf, HookBridgeError> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }

    let current_dir = std::env::current_dir().map_err(|error| HookBridgeError::Process {
        message: format!("failed to resolve current working directory: {error}"),
    })?;
    Ok(current_dir.join(path))
}

fn write_platform_file(
    runtime: &dyn Runtime,
    normalized: &NormalizedConfig,
    platform: Platform,
) -> Result<(), HookBridgeError> {
    let hooks = collect_platform_hooks(&build_generation_input(normalized), platform);
    let target = target_path(platform);

    ensure_no_unmanaged_conflict(runtime, target)?;

    let file = ManagedHooksFile {
        metadata: ManagedMetadata {
            managed_by: MANAGED_BY.to_string(),
            managed_version: MANAGED_VERSION,
            source_config: normalized.source_path.display().to_string(),
        },
        hooks,
    };

    let payload =
        serde_json::to_vec_pretty(&file).map_err(|error| HookBridgeError::PlatformProtocol {
            message: format!(
                "failed to serialize {} managed file: {error}",
                platform.as_str()
            ),
        })?;

    atomic_write(runtime.fs(), target, &payload)
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

fn collect_platform_hooks(
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

fn native_event_name(event: &str) -> &str {
    match event {
        "before_command" => "PreToolUse",
        "after_command" => "PostToolUse",
        "session_start" => "SessionStart",
        _ => event,
    }
}

fn ensure_no_unmanaged_conflict(
    runtime: &dyn Runtime,
    target: &Path,
) -> Result<(), HookBridgeError> {
    if !runtime.fs().exists(target)? {
        return Ok(());
    }

    let content = runtime.fs().read_to_string(target)?;
    if is_managed_content(&content) {
        return Ok(());
    }

    Err(HookBridgeError::FileConflict {
        path: target.to_path_buf(),
    })
}

#[must_use]
pub fn target_path(platform: Platform) -> &'static Path {
    match platform {
        Platform::Claude => Path::new(CLAUDE_TARGET),
        Platform::Codex => Path::new(CODEX_TARGET),
    }
}

#[must_use]
pub fn is_managed_content(content: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(content) else {
        return false;
    };
    value
        .get("_hook_bridge")
        .and_then(|meta| meta.get("managed_by"))
        .and_then(serde_json::Value::as_str)
        == Some(MANAGED_BY)
}

/// Loads managed metadata from a generated platform file.
///
/// # Errors
///
/// Returns errors for missing files, invalid JSON, or missing metadata fields.
pub fn load_metadata(
    runtime: &dyn Runtime,
    platform: Platform,
) -> Result<ManagedMetadata, HookBridgeError> {
    let path = target_path(platform);
    let content = runtime.fs().read_to_string(path)?;
    let value: serde_json::Value =
        serde_json::from_str(&content).map_err(|error| HookBridgeError::PlatformProtocol {
            message: format!("invalid managed {} file JSON: {error}", platform.as_str()),
        })?;

    let meta = value
        .get("_hook_bridge")
        .ok_or_else(|| HookBridgeError::PlatformProtocol {
            message: format!("missing _hook_bridge metadata in {}", path.display()),
        })?;

    let managed_by = meta
        .get("managed_by")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| HookBridgeError::PlatformProtocol {
            message: format!("missing managed_by in {}", path.display()),
        })?;

    if managed_by != MANAGED_BY {
        return Err(HookBridgeError::PlatformProtocol {
            message: format!("file {} is not managed by hook_bridge", path.display()),
        });
    }

    let managed_version_raw = meta
        .get("managed_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| HookBridgeError::PlatformProtocol {
            message: format!("missing managed_version in {}", path.display()),
        })?;
    let managed_version =
        u8::try_from(managed_version_raw).map_err(|_| HookBridgeError::PlatformProtocol {
            message: format!(
                "managed_version '{}' in {} is out of range",
                managed_version_raw,
                path.display()
            ),
        })?;

    let source_config = meta
        .get("source_config")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| HookBridgeError::PlatformProtocol {
            message: format!("missing source_config in {}", path.display()),
        })?
        .to_string();

    if managed_version != MANAGED_VERSION {
        return Err(HookBridgeError::PlatformProtocol {
            message: format!(
                "unsupported managed_version '{}' in {} (expected {})",
                managed_version,
                path.display(),
                MANAGED_VERSION
            ),
        });
    }

    Ok(ManagedMetadata {
        managed_by: managed_by.to_string(),
        managed_version,
        source_config,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::env;
    use std::path::{Path, PathBuf};

    use super::{
        CLAUDE_TARGET, CODEX_TARGET, GenerateArgs, MANAGED_BY, MANAGED_VERSION,
        build_generation_input, build_run_command, collect_platform_hooks,
        ensure_generation_targets_are_writable, ensure_no_unmanaged_conflict, execute,
        is_managed_content, load_metadata, normalize_config_path, target_path,
    };
    use crate::config::parse_and_normalize;
    use crate::platform::Platform;

    struct CurrentDirGuard {
        original: PathBuf,
    }

    impl CurrentDirGuard {
        fn enter(path: &Path) -> std::io::Result<Self> {
            let original = env::current_dir()?;
            env::set_current_dir(path)?;
            Ok(Self { original })
        }
    }

    impl Drop for CurrentDirGuard {
        fn drop(&mut self) {
            let _ = env::set_current_dir(&self.original);
        }
    }

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
        assert!(parsed.is_err(), "session_start should reject matcher");
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

        assert_eq!(normalize_config_path(absolute), Ok(absolute.to_path_buf()));
        assert_eq!(target_path(Platform::Claude), Path::new(CLAUDE_TARGET));
        assert_eq!(target_path(Platform::Codex), Path::new(CODEX_TARGET));
    }

    #[test]
    fn normalize_config_path_joins_relative_paths_from_current_directory() {
        let lock_result = crate::CWD_LOCK.lock();
        assert!(lock_result.is_ok(), "cwd lock should not be poisoned");
        let Ok(_lock) = lock_result else {
            return;
        };
        let temp_result = tempfile::tempdir();
        assert!(temp_result.is_ok(), "tempdir creation should succeed");
        let Ok(temp) = temp_result else {
            return;
        };
        let guard_result = CurrentDirGuard::enter(temp.path());
        assert!(guard_result.is_ok(), "cwd switch should succeed");
        let Ok(_guard) = guard_result else {
            return;
        };
        let current_dir_result = std::env::current_dir();
        assert!(
            current_dir_result.is_ok(),
            "current directory should resolve after cwd switch"
        );
        let Ok(current_dir) = current_dir_result else {
            return;
        };

        assert_eq!(
            normalize_config_path(Path::new("hook-bridge.yaml")),
            Ok(current_dir.join("hook-bridge.yaml"))
        );
    }

    #[test]
    fn native_event_name_maps_supported_and_passthrough_events() {
        assert_eq!(super::native_event_name("before_command"), "PreToolUse");
        assert_eq!(super::native_event_name("after_command"), "PostToolUse");
        assert_eq!(super::native_event_name("session_start"), "SessionStart");
        assert_eq!(super::native_event_name("custom"), "custom");
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
    fn ensure_no_unmanaged_conflict_rejects_manual_file() {
        let temp_result = tempfile::tempdir();
        assert!(temp_result.is_ok(), "tempdir creation should succeed");
        let Ok(temp) = temp_result else {
            return;
        };
        let target = temp.path().join("hooks.json");
        let write_result = std::fs::write(&target, "{}");
        assert!(write_result.is_ok(), "fixture file should be writable");

        assert!(matches!(
            ensure_no_unmanaged_conflict(&crate::runtime::RealRuntime::default(), &target),
            Err(crate::error::HookBridgeError::FileConflict { path }) if path == target
        ));
    }

    #[test]
    fn ensure_no_unmanaged_conflict_allows_missing_and_managed_files() {
        let temp_result = tempfile::tempdir();
        assert!(temp_result.is_ok(), "tempdir creation should succeed");
        let Ok(temp) = temp_result else {
            return;
        };
        let missing = temp.path().join("missing.json");
        let managed = temp.path().join("managed.json");
        let write_result = std::fs::write(
            &managed,
            serde_json::json!({
                "_hook_bridge": {
                    "managed_by": "hook_bridge"
                }
            })
            .to_string(),
        );
        assert!(write_result.is_ok(), "managed fixture should be writable");

        assert_eq!(
            ensure_no_unmanaged_conflict(&crate::runtime::RealRuntime::default(), &missing),
            Ok(())
        );
        assert_eq!(
            ensure_no_unmanaged_conflict(&crate::runtime::RealRuntime::default(), &managed),
            Ok(())
        );
    }

    #[test]
    fn preflight_generation_targets_rejects_any_unmanaged_target() {
        let lock_result = crate::CWD_LOCK.lock();
        assert!(lock_result.is_ok(), "cwd lock should not be poisoned");
        let Ok(_lock) = lock_result else {
            return;
        };
        let temp_result = tempfile::tempdir();
        assert!(temp_result.is_ok(), "tempdir creation should succeed");
        let Ok(temp) = temp_result else {
            return;
        };
        let guard_result = CurrentDirGuard::enter(temp.path());
        assert!(guard_result.is_ok(), "cwd switch should succeed");
        let Ok(_guard) = guard_result else {
            return;
        };
        let create_dir_result = std::fs::create_dir_all(".codex");
        assert!(create_dir_result.is_ok(), "codex dir should be creatable");
        let write_result = std::fs::write(".codex/hooks.json", "{}");
        assert!(write_result.is_ok(), "fixture file should be writable");

        assert!(matches!(
            ensure_generation_targets_are_writable(
                &crate::runtime::RealRuntime::default(),
                [Platform::Claude, Platform::Codex]
            ),
            Err(crate::error::HookBridgeError::FileConflict { path })
                if path == PathBuf::from(CODEX_TARGET)
        ));
    }

    #[test]
    fn preflight_generation_targets_allows_missing_targets() {
        let lock_result = crate::CWD_LOCK.lock();
        assert!(lock_result.is_ok(), "cwd lock should not be poisoned");
        let Ok(_lock) = lock_result else {
            return;
        };
        let temp_result = tempfile::tempdir();
        assert!(temp_result.is_ok(), "tempdir creation should succeed");
        let Ok(temp) = temp_result else {
            return;
        };
        let guard_result = CurrentDirGuard::enter(temp.path());
        assert!(guard_result.is_ok(), "cwd switch should succeed");
        let Ok(_guard) = guard_result else {
            return;
        };

        assert_eq!(
            ensure_generation_targets_are_writable(
                &crate::runtime::RealRuntime::default(),
                [Platform::Claude, Platform::Codex]
            ),
            Ok(())
        );
    }

    #[test]
    fn execute_and_load_metadata_round_trip() {
        let lock_result = crate::CWD_LOCK.lock();
        assert!(lock_result.is_ok(), "cwd lock should not be poisoned");
        let _lock = lock_result.expect("cwd lock should not be poisoned");
        let temp_result = tempfile::tempdir();
        assert!(temp_result.is_ok(), "tempdir creation should succeed");
        let temp = temp_result.expect("tempdir creation should succeed");
        let guard_result = CurrentDirGuard::enter(temp.path());
        assert!(guard_result.is_ok(), "cwd switch should succeed");
        let _guard = guard_result.expect("cwd switch should succeed");
        let config_path = temp.path().join("hook-bridge.yaml");
        let write_result = std::fs::write(
            &config_path,
            r"
version: 1
hooks:
  - id: r1
    event: before_command
    command: echo ok
",
        );
        assert!(write_result.is_ok(), "config file should be written");

        assert_eq!(
            execute(
                &GenerateArgs {
                    config: config_path.clone(),
                },
                &crate::runtime::RealRuntime::default(),
            ),
            Ok(())
        );

        let metadata_result =
            load_metadata(&crate::runtime::RealRuntime::default(), Platform::Codex);
        assert!(metadata_result.is_ok(), "metadata should load");
        let metadata = metadata_result.expect("metadata should load");

        assert_eq!(
            metadata,
            super::ManagedMetadata {
                managed_by: MANAGED_BY.to_string(),
                managed_version: MANAGED_VERSION,
                source_config: config_path.display().to_string(),
            }
        );
    }

    #[test]
    fn load_metadata_rejects_invalid_shapes() {
        let lock_result = crate::CWD_LOCK.lock();
        assert!(lock_result.is_ok(), "cwd lock should not be poisoned");
        let _lock = lock_result.expect("cwd lock should not be poisoned");
        let temp_result = tempfile::tempdir();
        assert!(temp_result.is_ok(), "tempdir creation should succeed");
        let temp = temp_result.expect("tempdir creation should succeed");
        let guard_result = CurrentDirGuard::enter(temp.path());
        assert!(guard_result.is_ok(), "cwd switch should succeed");
        let _guard = guard_result.expect("cwd switch should succeed");
        let create_result = std::fs::create_dir_all(".codex");
        assert!(create_result.is_ok(), "codex dir should be creatable");
        let write_result = std::fs::write(".codex/hooks.json", "{");
        assert!(
            write_result.is_ok(),
            "managed file fixture should be writable"
        );

        assert!(matches!(
            load_metadata(&crate::runtime::RealRuntime::default(), Platform::Codex),
            Err(crate::error::HookBridgeError::PlatformProtocol { message })
                if message.contains("invalid managed codex file JSON")
        ));

        let write_result = std::fs::write(
            ".codex/hooks.json",
            serde_json::json!({
                "_hook_bridge": {
                    "managed_by": "someone_else",
                    "managed_version": 1,
                    "source_config": "/tmp/cfg.yaml"
                }
            })
            .to_string(),
        );
        assert!(
            write_result.is_ok(),
            "managed file fixture should be writable"
        );

        assert_eq!(
            load_metadata(&crate::runtime::RealRuntime::default(), Platform::Codex),
            Err(crate::error::HookBridgeError::PlatformProtocol {
                message: format!(
                    "file {} is not managed by hook_bridge",
                    Path::new(CODEX_TARGET).display()
                ),
            })
        );
    }

    #[test]
    fn load_metadata_rejects_missing_metadata_fields() {
        let lock_result = crate::CWD_LOCK.lock();
        assert!(lock_result.is_ok(), "cwd lock should not be poisoned");
        let _lock = lock_result.expect("cwd lock should not be poisoned");
        let temp_result = tempfile::tempdir();
        assert!(temp_result.is_ok(), "tempdir creation should succeed");
        let temp = temp_result.expect("tempdir creation should succeed");
        let guard_result = CurrentDirGuard::enter(temp.path());
        assert!(guard_result.is_ok(), "cwd switch should succeed");
        let _guard = guard_result.expect("cwd switch should succeed");
        let create_result = std::fs::create_dir_all(".codex");
        assert!(create_result.is_ok(), "codex dir should be creatable");

        assert_metadata_error("{}", "missing _hook_bridge metadata in .codex/hooks.json");
        assert_metadata_error(
            &serde_json::json!({
                "_hook_bridge": {
                    "managed_version": 1,
                    "source_config": "/tmp/cfg.yaml"
                }
            })
            .to_string(),
            "missing managed_by in .codex/hooks.json",
        );
        assert_metadata_error(
            &serde_json::json!({
                "_hook_bridge": {
                    "managed_by": "hook_bridge",
                    "source_config": "/tmp/cfg.yaml"
                }
            })
            .to_string(),
            "missing managed_version in .codex/hooks.json",
        );
        assert_metadata_error(
            &serde_json::json!({
                "_hook_bridge": {
                    "managed_by": "hook_bridge",
                    "managed_version": 999,
                    "source_config": "/tmp/cfg.yaml"
                }
            })
            .to_string(),
            "managed_version '999' in .codex/hooks.json is out of range",
        );
        assert_metadata_error(
            &serde_json::json!({
                "_hook_bridge": {
                    "managed_by": "hook_bridge",
                    "managed_version": 1
                }
            })
            .to_string(),
            "missing source_config in .codex/hooks.json",
        );
    }

    fn assert_metadata_error(content: &str, expected_message: &str) {
        let write_result = std::fs::write(".codex/hooks.json", content);
        assert!(
            write_result.is_ok(),
            "managed file fixture should be writable"
        );
        assert_eq!(
            load_metadata(&crate::runtime::RealRuntime::default(), Platform::Codex),
            Err(crate::error::HookBridgeError::PlatformProtocol {
                message: expected_message.to_string(),
            })
        );
    }
}
