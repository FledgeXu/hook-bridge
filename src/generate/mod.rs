mod build;
mod managed;

use std::io::ErrorKind;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use crate::cli::GenerateArgs;
use crate::config::parse_and_normalize;
use crate::error::HookBridgeError;
use crate::platform::Platform;
use crate::runtime::Runtime;
use crate::runtime::fs::FsEntryType;
use crate::runtime::fs::atomic_write;
use dialoguer::Confirm;

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
    let confirmer = DialoguerForceOverwriteConfirmer;
    execute_with_confirmer(args, runtime, &confirmer)
}

fn execute_with_confirmer(
    args: &GenerateArgs,
    runtime: &dyn Runtime,
    confirmer: &dyn ForceOverwriteConfirmer,
) -> Result<(), HookBridgeError> {
    execute_with_confirmer_and_interactivity(
        args,
        runtime,
        confirmer,
        is_force_overwrite_interactive(
            std::io::stdin().is_terminal(),
            std::io::stderr().is_terminal(),
        ),
    )
}

fn execute_with_confirmer_and_interactivity(
    args: &GenerateArgs,
    runtime: &dyn Runtime,
    confirmer: &dyn ForceOverwriteConfirmer,
    interactive: bool,
) -> Result<(), HookBridgeError> {
    let base_dir = runtime.fs().current_dir()?;
    let source_config = normalize_path(&args.config, &base_dir);
    let yaml = runtime.fs().read_to_string(&source_config)?;
    let normalized = parse_and_normalize(source_config, &yaml)?;
    let target_platforms = args.platform.map_or_else(
        || vec![Platform::Claude, Platform::Codex],
        |platform| vec![platform],
    );
    let targets = resolve_generation_targets(&target_platforms, &base_dir);

    maybe_confirm_force_overwrite(args, runtime, confirmer, &targets, interactive)?;
    if args.force {
        ensure_force_generation_targets_are_writable(runtime, &targets)?;
    } else {
        ensure_generation_targets_are_writable(runtime, &target_platforms, &base_dir)?;
    }

    for platform in target_platforms {
        write_platform_file(runtime, &normalized, platform, &base_dir, args.force)?;
    }

    Ok(())
}

trait ForceOverwriteConfirmer {
    fn confirm(&self, prompt: &str) -> Result<bool, HookBridgeError>;
}

#[derive(Debug, Default)]
struct DialoguerForceOverwriteConfirmer;

impl ForceOverwriteConfirmer for DialoguerForceOverwriteConfirmer {
    fn confirm(&self, prompt: &str) -> Result<bool, HookBridgeError> {
        Confirm::new()
            .with_prompt(prompt)
            .default(false)
            .show_default(false)
            .wait_for_newline(true)
            .interact()
            .map_err(|error| HookBridgeError::Parameter {
                message: format!("failed to read force overwrite confirmation: {error}"),
            })
    }
}

fn maybe_confirm_force_overwrite(
    args: &GenerateArgs,
    runtime: &dyn Runtime,
    confirmer: &dyn ForceOverwriteConfirmer,
    targets: &[PathBuf],
    interactive: bool,
) -> Result<(), HookBridgeError> {
    if !args.force || args.yes {
        return Ok(());
    }
    if !interactive {
        return Err(HookBridgeError::Parameter {
            message: "--force requires --yes in non-interactive environments".to_string(),
        });
    }

    runtime
        .io()
        .write_stderr(format_force_overwrite_targets(targets).as_bytes())?;
    let is_confirmed = confirmer.confirm("Proceed with force overwrite?")?;
    if is_confirmed {
        return Ok(());
    }

    Err(HookBridgeError::Parameter {
        message: "force overwrite canceled by user".to_string(),
    })
}

const fn is_force_overwrite_interactive(stdin_terminal: bool, stderr_terminal: bool) -> bool {
    stdin_terminal && stderr_terminal
}

fn resolve_generation_targets(target_platforms: &[Platform], base_dir: &Path) -> Vec<PathBuf> {
    target_platforms
        .iter()
        .map(|platform| normalize_path(target_path(*platform), base_dir))
        .collect()
}

fn format_force_overwrite_targets(targets: &[PathBuf]) -> String {
    let mut message = String::from("Force overwrite will replace these target files:\n");
    for target in targets {
        message.push_str("  - ");
        message.push_str(&target.display().to_string());
        message.push('\n');
    }
    message
}

fn ensure_force_generation_targets_are_writable(
    runtime: &dyn Runtime,
    targets: &[PathBuf],
) -> Result<(), HookBridgeError> {
    for target in targets {
        ensure_force_target_is_writable(runtime, target)?;
    }
    Ok(())
}

fn ensure_force_target_is_writable(
    runtime: &dyn Runtime,
    target: &Path,
) -> Result<(), HookBridgeError> {
    ensure_existing_force_target_is_replaceable(runtime, target)?;
    ensure_force_target_parent_is_writable(runtime, target)
}

fn ensure_force_target_parent_is_writable(
    runtime: &dyn Runtime,
    target: &Path,
) -> Result<(), HookBridgeError> {
    let parent = target
        .parent()
        .ok_or_else(|| HookBridgeError::ConfigValidation {
            message: format!("path '{}' has no parent directory", target.display()),
        })?;
    let mut cursor = Some(parent.to_path_buf());
    while let Some(path) = cursor {
        match runtime.fs().metadata(&path)? {
            Some(metadata) => match metadata.entry_type {
                FsEntryType::Directory => {
                    if metadata.readonly {
                        return Err(HookBridgeError::Io {
                            operation: "force_preflight_parent_writable",
                            path,
                            kind: ErrorKind::PermissionDenied,
                        });
                    }
                    return Ok(());
                }
                FsEntryType::File | FsEntryType::Other => {
                    return Err(HookBridgeError::FileConflict { path });
                }
            },
            None => {
                cursor = path.parent().map(Path::to_path_buf);
            }
        }
    }
    Ok(())
}

fn ensure_existing_force_target_is_replaceable(
    runtime: &dyn Runtime,
    target: &Path,
) -> Result<(), HookBridgeError> {
    match runtime.fs().metadata(target)? {
        None => Ok(()),
        Some(metadata) => match metadata.entry_type {
            FsEntryType::File => {
                if metadata.readonly {
                    return Err(HookBridgeError::Io {
                        operation: "force_preflight_target_writable",
                        path: target.to_path_buf(),
                        kind: ErrorKind::PermissionDenied,
                    });
                }
                Ok(())
            }
            FsEntryType::Directory | FsEntryType::Other => Err(HookBridgeError::FileConflict {
                path: target.to_path_buf(),
            }),
        },
    }
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
    allow_unmanaged_overwrite: bool,
) -> Result<(), HookBridgeError> {
    let hooks = build::collect_platform_hooks(&build_generation_input(normalized), platform);
    let target = normalize_path(target_path(platform), base_dir);

    if !allow_unmanaged_overwrite {
        managed::ensure_no_unmanaged_conflict(runtime, &target)?;
    }

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
