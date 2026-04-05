use std::path::Path;

use crate::error::HookBridgeError;
use crate::platform::Platform;
use crate::runtime::Runtime;

pub const MANAGED_BY: &str = "hook_bridge";
pub const MANAGED_VERSION: u8 = 1;
pub const CLAUDE_TARGET: &str = ".claude/settings.json";
pub const CODEX_TARGET: &str = ".codex/hooks.json";

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct ManagedMetadata {
    pub managed_by: String,
    pub managed_version: u8,
    pub source_config: String,
}

/// Validates that all generation targets are either missing or already managed by `hook_bridge`.
///
/// # Errors
///
/// Returns filesystem and file-conflict errors while checking target files.
pub fn ensure_generation_targets_are_writable(
    runtime: &dyn Runtime,
    platforms: &[Platform],
) -> Result<(), HookBridgeError> {
    for &platform in platforms {
        ensure_no_unmanaged_conflict(runtime, target_path(platform))?;
    }
    Ok(())
}

pub(crate) fn ensure_no_unmanaged_conflict(
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
