use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::cli::GenerateArgs;
use crate::config::{NormalizedConfig, parse_and_normalize};
use crate::error::HookBridgeError;
use crate::platform::Platform;
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
    hooks: Vec<PlatformHookOutput>,
}

#[derive(Debug, Clone, Serialize)]
struct PlatformHookOutput {
    id: String,
    event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    matcher: Option<String>,
    command: String,
    timeout_sec: u64,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    extra: std::collections::BTreeMap<String, serde_json::Value>,
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

    write_platform_file(runtime, &normalized, Platform::Claude)?;
    write_platform_file(runtime, &normalized, Platform::Codex)?;

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
    let hooks = collect_platform_hooks(normalized, platform);
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

fn collect_platform_hooks(
    normalized: &NormalizedConfig,
    platform: Platform,
) -> Vec<PlatformHookOutput> {
    normalized
        .hooks
        .iter()
        .filter_map(|hook| {
            let rule = match platform {
                Platform::Claude => hook.claude.as_ref(),
                Platform::Codex => hook.codex.as_ref(),
            }?;

            Some(PlatformHookOutput {
                id: hook.id.clone(),
                event: rule.event.clone(),
                matcher: rule.matcher.clone(),
                command: build_run_command(platform, &hook.id),
                timeout_sec: rule.timeout_sec,
                extra: rule.extra.clone(),
            })
        })
        .collect()
}

#[must_use]
pub fn build_run_command(platform: Platform, rule_id: &str) -> String {
    format!(
        "hook_bridge run --platform {} --rule-id {}",
        platform.as_str(),
        rule_id
    )
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
    use crate::platform::Platform;

    use super::{build_run_command, is_managed_content};

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
}
