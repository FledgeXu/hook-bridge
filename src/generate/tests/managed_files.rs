use super::*;
use std::path::{Path, PathBuf};

use crate::runtime::Runtime;
use crate::runtime::clock::{Clock, FixedClock};
use crate::runtime::fs::{FakeFileSystem, FileSystem};
use crate::runtime::io::{FakeIo, Io};
use crate::runtime::process::{FakeProcessRunner, ProcessRunner};

struct FakeForceOverwriteConfirmer {
    answer: bool,
}

impl ForceOverwriteConfirmer for FakeForceOverwriteConfirmer {
    fn confirm(&self, _prompt: &str) -> Result<bool, HookBridgeError> {
        Ok(self.answer)
    }
}

struct FakeFsRuntime {
    fs: FakeFileSystem,
    clock: FixedClock,
    process: FakeProcessRunner,
    io: FakeIo,
}

impl FakeFsRuntime {
    fn new(fs: FakeFileSystem) -> Self {
        Self {
            fs,
            clock: FixedClock::new(std::time::SystemTime::UNIX_EPOCH),
            process: FakeProcessRunner::success(0),
            io: FakeIo::default(),
        }
    }
}

impl Runtime for FakeFsRuntime {
    fn fs(&self) -> &dyn FileSystem {
        &self.fs
    }

    fn clock(&self) -> &dyn Clock {
        &self.clock
    }

    fn process_runner(&self) -> &dyn ProcessRunner {
        &self.process
    }

    fn io(&self) -> &dyn Io {
        &self.io
    }

    fn temp_dir(&self) -> std::path::PathBuf {
        std::path::PathBuf::from("/tmp/hook-bridge-fake-runtime")
    }
}

#[test]
fn dialoguer_confirmer_maps_non_interactive_confirm_error_to_parameter() {
    use std::io::IsTerminal;

    let confirmer = DialoguerForceOverwriteConfirmer;
    if std::io::stdin().is_terminal() || std::io::stderr().is_terminal() {
        return;
    }

    assert!(matches!(
        confirmer.confirm("Proceed with force overwrite?"),
        Err(HookBridgeError::Parameter { message })
            if message.contains("failed to read force overwrite confirmation")
    ));
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
            &[Platform::Claude, Platform::Codex],
            temp.path(),
        ),
        Err(crate::error::HookBridgeError::FileConflict { path })
            if path == temp.path().join(CODEX_TARGET)
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
            &[Platform::Claude, Platform::Codex],
            temp.path(),
        ),
        Ok(())
    );
}

#[test]
fn force_preflight_parent_rejects_target_without_parent() {
    let runtime = TestRuntime::new(PathBuf::from("/tmp"));

    assert_eq!(
        ensure_force_target_parent_is_writable(&runtime, Path::new("/")),
        Err(HookBridgeError::ConfigValidation {
            message: "path '/' has no parent directory".to_string(),
        })
    );
}

#[test]
fn force_preflight_parent_rejects_readonly_directory() {
    let temp = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
    let runtime = TestRuntime::new(temp.path().to_path_buf());
    let readonly_dir = temp.path().join("readonly");
    assert!(std::fs::create_dir_all(&readonly_dir).is_ok());

    let original_permissions = std::fs::metadata(&readonly_dir)
        .unwrap_or_else(|_| unreachable!())
        .permissions();
    let mut readonly_permissions = original_permissions.clone();
    readonly_permissions.set_readonly(true);
    assert!(std::fs::set_permissions(&readonly_dir, readonly_permissions).is_ok());

    let target = readonly_dir.join("hooks.json");
    assert!(matches!(
        ensure_force_target_parent_is_writable(&runtime, &target),
        Err(HookBridgeError::Io {
            operation: "force_preflight_parent_writable",
            kind: std::io::ErrorKind::PermissionDenied,
            ..
        })
    ));

    let _ = std::fs::set_permissions(&readonly_dir, original_permissions);
}

#[test]
fn force_preflight_parent_rejects_file_path() {
    let temp = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
    let runtime = TestRuntime::new(temp.path().to_path_buf());
    let parent_file = temp.path().join("not-a-dir");
    assert!(std::fs::write(&parent_file, "fixture").is_ok());

    let target = parent_file.join("hooks.json");
    assert_eq!(
        ensure_force_target_parent_is_writable(&runtime, &target),
        Err(HookBridgeError::FileConflict { path: parent_file })
    );
}

#[test]
fn force_preflight_target_rejects_readonly_file() {
    let temp = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
    let runtime = TestRuntime::new(temp.path().to_path_buf());
    let target = temp.path().join("hooks.json");
    assert!(std::fs::write(&target, "{}").is_ok());

    let original_permissions = std::fs::metadata(&target)
        .unwrap_or_else(|_| unreachable!())
        .permissions();
    let mut readonly_permissions = original_permissions.clone();
    readonly_permissions.set_readonly(true);
    assert!(std::fs::set_permissions(&target, readonly_permissions).is_ok());

    assert!(matches!(
        ensure_existing_force_target_is_replaceable(&runtime, &target),
        Err(HookBridgeError::Io {
            operation: "force_preflight_target_writable",
            kind: std::io::ErrorKind::PermissionDenied,
            ..
        })
    ));

    let _ = std::fs::set_permissions(&target, original_permissions);
}

#[test]
fn force_preflight_parent_allows_missing_ancestors_in_fake_fs() {
    let runtime = FakeFsRuntime::new(FakeFileSystem::default());
    let target = Path::new("/virtual/missing/tree/hooks.json");

    assert_eq!(
        ensure_force_target_parent_is_writable(&runtime, target),
        Ok(())
    );
}

#[test]
fn execute_force_rejects_non_interactive_without_yes() {
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
    let create_dir_result = std::fs::create_dir_all(".codex");
    assert!(create_dir_result.is_ok(), "codex dir should be creatable");
    let write_result = std::fs::write(".codex/hooks.json", "{}");
    assert!(write_result.is_ok(), "fixture file should be writable");
    let confirmer = FakeForceOverwriteConfirmer { answer: true };

    assert_eq!(
        execute_with_confirmer_and_interactivity(
            &GenerateArgs {
                config: config_path.clone(),
                platform: Some(Platform::Codex),
                force: true,
                yes: false,
            },
            &TestRuntime::new(temp.path().to_path_buf()),
            &confirmer,
            false,
        ),
        Err(HookBridgeError::Parameter {
            message: "--force requires --yes in non-interactive environments".to_string(),
        })
    );
}

#[test]
fn execute_force_overwrites_unmanaged_file_after_confirmation() {
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
    let create_dir_result = std::fs::create_dir_all(".codex");
    assert!(create_dir_result.is_ok(), "codex dir should be creatable");
    let write_result = std::fs::write(".codex/hooks.json", "{}");
    assert!(write_result.is_ok(), "fixture file should be writable");
    let confirmer = FakeForceOverwriteConfirmer { answer: true };

    assert_eq!(
        execute_with_confirmer_and_interactivity(
            &GenerateArgs {
                config: config_path.clone(),
                platform: Some(Platform::Codex),
                force: true,
                yes: false,
            },
            &TestRuntime::new(temp.path().to_path_buf()),
            &confirmer,
            true,
        ),
        Ok(())
    );

    let content_result = std::fs::read_to_string(".codex/hooks.json");
    assert!(content_result.is_ok(), "codex file should be rewritten");
    let Ok(content) = content_result else {
        return;
    };
    assert!(content.contains("\"managed_by\": \"hook_bridge\""));
}

#[test]
fn execute_force_cancels_when_confirmation_declined() {
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
    let create_dir_result = std::fs::create_dir_all(".codex");
    assert!(create_dir_result.is_ok(), "codex dir should be creatable");
    let write_result = std::fs::write(".codex/hooks.json", "{}");
    assert!(write_result.is_ok(), "fixture file should be writable");
    let confirmer = FakeForceOverwriteConfirmer { answer: false };

    assert_eq!(
        execute_with_confirmer_and_interactivity(
            &GenerateArgs {
                config: config_path.clone(),
                platform: Some(Platform::Codex),
                force: true,
                yes: false,
            },
            &TestRuntime::new(temp.path().to_path_buf()),
            &confirmer,
            true,
        ),
        Err(HookBridgeError::Parameter {
            message: "force overwrite canceled by user".to_string(),
        })
    );
}

#[test]
fn execute_and_load_metadata_round_trip() {
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
                platform: None,
                force: false,
                yes: false,
            },
            &crate::runtime::RealRuntime::default(),
        ),
        Ok(())
    );

    let metadata_result = load_metadata(&crate::runtime::RealRuntime::default(), Platform::Codex);
    assert!(metadata_result.is_ok(), "metadata should load");
    let Ok(metadata) = metadata_result else {
        return;
    };

    assert_eq!(
        metadata,
        ManagedMetadata {
            managed_by: MANAGED_BY.to_string(),
            managed_version: MANAGED_VERSION,
            source_config: config_path.display().to_string(),
        }
    );
}

#[test]
fn execute_resolves_relative_config_path_through_runtime_before_reading() {
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
    let runtime_cwd = temp.path().join("runtime-cwd");
    let create_result = std::fs::create_dir_all(&runtime_cwd);
    assert!(create_result.is_ok(), "runtime cwd should be creatable");
    let config_path = runtime_cwd.join("hook-bridge.yaml");
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
                config: PathBuf::from("hook-bridge.yaml"),
                platform: Some(Platform::Codex),
                force: false,
                yes: false,
            },
            &TestRuntime::new(runtime_cwd.clone()),
        ),
        Ok(())
    );

    let metadata_path = runtime_cwd.join(CODEX_TARGET);
    let metadata_content = std::fs::read_to_string(&metadata_path);
    assert!(
        metadata_content.is_ok(),
        "managed codex file should be written in runtime cwd"
    );
    let Ok(content) = metadata_content else {
        return;
    };
    let parsed_result = serde_json::from_str::<serde_json::Value>(&content);
    assert!(parsed_result.is_ok(), "managed file should be valid json");
    let Ok(parsed) = parsed_result else {
        return;
    };
    assert_eq!(
        parsed
            .get("_hook_bridge")
            .and_then(|value| value.get("source_config"))
            .and_then(serde_json::Value::as_str),
        Some(config_path.to_string_lossy().as_ref())
    );
}

#[test]
fn load_metadata_rejects_invalid_shapes() {
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
    let create_result = std::fs::create_dir_all(".codex");
    assert!(create_result.is_ok(), "codex dir should be creatable");
    let target = current_target_path(Platform::Codex);
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
            message: format!("file {} is not managed by hook_bridge", target.display()),
        })
    );
}

#[test]
fn load_metadata_rejects_missing_metadata_fields() {
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
    let create_result = std::fs::create_dir_all(".codex");
    assert!(create_result.is_ok(), "codex dir should be creatable");
    let target = current_target_path(Platform::Codex);

    assert_metadata_error(
        "{}",
        format!("missing _hook_bridge metadata in {}", target.display()),
    );
    assert_metadata_error(
        &serde_json::json!({
            "_hook_bridge": {
                "managed_version": 1,
                "source_config": "/tmp/cfg.yaml"
            }
        })
        .to_string(),
        format!("missing managed_by in {}", target.display()),
    );
    assert_metadata_error(
        &serde_json::json!({
            "_hook_bridge": {
                "managed_by": "hook_bridge",
                "source_config": "/tmp/cfg.yaml"
            }
        })
        .to_string(),
        format!("missing managed_version in {}", target.display()),
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
        format!(
            "managed_version '999' in {} is out of range",
            target.display()
        ),
    );
    assert_metadata_error(
        &serde_json::json!({
            "_hook_bridge": {
                "managed_by": "hook_bridge",
                "managed_version": 1
            }
        })
        .to_string(),
        format!("missing source_config in {}", target.display()),
    );
}
