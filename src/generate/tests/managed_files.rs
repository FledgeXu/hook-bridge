use super::*;

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
