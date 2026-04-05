mod build;
mod managed;

use std::path::{Path, PathBuf};

use crate::cli::GenerateArgs;
use crate::config::parse_and_normalize;
use crate::error::HookBridgeError;
use crate::platform::Platform;
use crate::runtime::Runtime;
use crate::runtime::fs::atomic_write;

pub use build::{
    PlatformGenerationInput, PlatformGenerationRule, build_generation_input, build_run_command,
};
pub use managed::{
    CLAUDE_TARGET, CODEX_TARGET, MANAGED_BY, MANAGED_VERSION, ManagedMetadata,
    ensure_generation_targets_are_writable, is_managed_content, load_metadata, target_path,
};

#[derive(Debug, Clone, serde::Serialize)]
struct ManagedHooksFile {
    #[serde(rename = "_hook_bridge")]
    metadata: ManagedMetadata,
    hooks: std::collections::BTreeMap<String, Vec<serde_json::Value>>,
}

/// Executes the `generate` command.
///
/// # Errors
///
/// Returns validation, conflict, serialization, and filesystem errors.
pub fn execute(args: &GenerateArgs, runtime: &dyn Runtime) -> Result<(), HookBridgeError> {
    let base_dir = runtime.fs().current_dir()?;
    let source_config = normalize_path(&args.config, &base_dir);
    let yaml = runtime.fs().read_to_string(&source_config)?;
    let normalized = parse_and_normalize(source_config, &yaml)?;
    let target_platforms = args.platform.map_or_else(
        || vec![Platform::Claude, Platform::Codex],
        |platform| vec![platform],
    );

    ensure_generation_targets_are_writable(runtime, &target_platforms, &base_dir)?;

    for platform in target_platforms {
        write_platform_file(runtime, &normalized, platform, &base_dir)?;
    }

    Ok(())
}

fn normalize_path(path: &Path, base_dir: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    base_dir.join(path)
}

fn write_platform_file(
    runtime: &dyn Runtime,
    normalized: &crate::config::NormalizedConfig,
    platform: Platform,
    base_dir: &Path,
) -> Result<(), HookBridgeError> {
    let hooks = build::collect_platform_hooks(&build_generation_input(normalized), platform);
    let target = normalize_path(target_path(platform), base_dir);

    managed::ensure_no_unmanaged_conflict(runtime, &target)?;

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

    atomic_write(runtime.fs(), &target, &payload)
}

#[cfg(test)]
mod tests;
