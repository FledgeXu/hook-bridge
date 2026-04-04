use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::cli::RunArgs;
use crate::config::{PlatformRule, parse_and_normalize};
use crate::error::HookBridgeError;
use crate::generate;
use crate::platform::Platform;
use crate::platform::{claude, codex};
use crate::runtime::Runtime;
use crate::runtime::fs::atomic_write;
use crate::runtime::process::ProcessRequest;

#[derive(Debug, Clone)]
pub struct RuntimeContext {
    pub platform: Platform,
    pub event: String,
    pub rule_id: String,
    pub source_config_path: PathBuf,
    pub session_or_thread_id: String,
    pub cwd: Option<PathBuf>,
    pub transcript_path: Option<PathBuf>,
    pub raw_payload: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InternalStatus {
    Success,
    Stop,
    Block,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionResult {
    pub status: InternalStatus,
    pub message: Option<String>,
    pub system_message: Option<String>,
    pub exit_code: Option<i32>,
    pub raw_stdout: Vec<u8>,
    pub raw_stderr: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct RetryState {
    consecutive_failures: u32,
    last_error: String,
    last_failure_epoch_sec: u64,
}

/// Executes the `run` command.
///
/// # Errors
///
/// Returns errors for managed-file lookup, runtime input parsing, rule lookup, command execution,
/// retry-state persistence, and output write failures.
pub fn execute(args: &RunArgs, runtime: &dyn Runtime) -> Result<(), HookBridgeError> {
    let metadata = generate::load_metadata(runtime, args.platform)?;
    let source_config_path = PathBuf::from(&metadata.source_config);
    if !source_config_path.is_absolute() {
        return Err(HookBridgeError::ConfigValidation {
            message: format!(
                "managed source_config must be absolute, got '{}'",
                source_config_path.display()
            ),
        });
    }
    let config_yaml = runtime.fs().read_to_string(source_config_path.as_path())?;
    let config = parse_and_normalize(source_config_path.clone(), &config_yaml)?;

    let stdin = runtime.io().read_stdin()?;
    let raw_payload = String::from_utf8(stdin).map_err(|error| HookBridgeError::JsonParse {
        message: format!("stdin payload is not valid UTF-8 JSON: {error}"),
    })?;

    let context = parse_runtime_context(args, &raw_payload, &source_config_path)?;
    let rule = config.find_platform_rule(args.platform, &args.rule_id)?;

    if context.event != rule.event {
        return Err(HookBridgeError::PlatformProtocol {
            message: format!(
                "event mismatch for rule '{}': stdin event '{}' but configured event '{}'",
                args.rule_id, context.event, rule.event
            ),
        });
    }

    let state_path = retry_state_path(runtime, &context);
    let state = load_retry_state(runtime, &state_path)?;
    if state.consecutive_failures >= rule.max_retries && rule.max_retries > 0 {
        let output = translate_output(
            args.platform,
            &context,
            &ExecutionResult {
                status: InternalStatus::Stop,
                message: Some("max retries reached, skipping command execution".to_string()),
                system_message: Some("hook_bridge retry guard engaged".to_string()),
                exit_code: Some(0),
                raw_stdout: Vec::new(),
                raw_stderr: Vec::new(),
            },
        )?;
        runtime.io().write_stdout(&output.stdout)?;
        return Ok(());
    }

    let process_result = run_user_command(runtime, rule, &context);
    let execution_result = match process_result {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            status: InternalStatus::Error,
            message: Some(error.to_string()),
            system_message: Some("hook_bridge command execution failed".to_string()),
            exit_code: Some(1),
            raw_stdout: Vec::new(),
            raw_stderr: Vec::new(),
        },
    };

    match execution_result.status {
        InternalStatus::Success => {
            runtime.fs().remove_file_if_exists(&state_path)?;
        }
        InternalStatus::Error | InternalStatus::Block | InternalStatus::Stop => {
            persist_failure_state(
                runtime,
                &state_path,
                &state,
                execution_result
                    .message
                    .clone()
                    .unwrap_or_else(|| "execution failed".to_string()),
            )?;
        }
    }

    let output = translate_output(args.platform, &context, &execution_result)?;
    runtime.io().write_stdout(&output.stdout)?;

    Ok(())
}

fn run_user_command(
    runtime: &dyn Runtime,
    rule: &PlatformRule,
    context: &RuntimeContext,
) -> Result<ExecutionResult, HookBridgeError> {
    let mut env = rule.env.clone();
    env.insert(
        "HOOK_BRIDGE_PLATFORM".to_string(),
        context.platform.as_str().to_string(),
    );
    env.insert("HOOK_BRIDGE_RULE_ID".to_string(), context.rule_id.clone());
    env.insert("HOOK_BRIDGE_EVENT".to_string(), context.event.clone());

    let request = ProcessRequest {
        program: rule.shell.clone(),
        args: vec!["-lc".to_string(), rule.command.clone()],
        stdin: context.raw_payload.as_bytes().to_vec(),
        timeout: Duration::from_secs(rule.timeout_sec),
        cwd: rule.working_dir.clone().or_else(|| context.cwd.clone()),
        env,
    };

    let output = runtime.process_runner().run(&request)?;

    if output.status_code == 0 {
        Ok(ExecutionResult {
            status: InternalStatus::Success,
            message: None,
            system_message: None,
            exit_code: Some(0),
            raw_stdout: output.stdout,
            raw_stderr: output.stderr,
        })
    } else {
        Ok(ExecutionResult {
            status: InternalStatus::Error,
            message: Some(format!(
                "command exited with non-zero status {}",
                output.status_code
            )),
            system_message: Some("hook_bridge command returned non-zero exit code".to_string()),
            exit_code: Some(output.status_code),
            raw_stdout: output.stdout,
            raw_stderr: output.stderr,
        })
    }
}

fn now_epoch_sec(runtime: &dyn Runtime) -> Result<u64, HookBridgeError> {
    runtime
        .clock()
        .now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| HookBridgeError::Process {
            message: format!("clock error: {error}"),
        })
}

fn parse_runtime_context(
    args: &RunArgs,
    raw_payload: &str,
    source_config_path: &Path,
) -> Result<RuntimeContext, HookBridgeError> {
    let value: serde_json::Value =
        serde_json::from_str(raw_payload).map_err(|error| HookBridgeError::JsonParse {
            message: format!("invalid runtime JSON input: {error}"),
        })?;

    let (event, session_or_thread_id, cwd, transcript_path) = match args.platform {
        Platform::Claude => claude::parse_context_fields(&value)?,
        Platform::Codex => codex::parse_context_fields(&value)?,
    };

    Ok(RuntimeContext {
        platform: args.platform,
        event,
        rule_id: args.rule_id.clone(),
        source_config_path: source_config_path.to_path_buf(),
        session_or_thread_id,
        cwd,
        transcript_path,
        raw_payload: raw_payload.to_string(),
    })
}

fn retry_state_root(runtime: &dyn Runtime) -> PathBuf {
    runtime.temp_dir().join("hook_bridge").join("retries")
}

fn retry_state_path(runtime: &dyn Runtime, context: &RuntimeContext) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(context.source_config_path.to_string_lossy().as_bytes());
    hasher.update(context.session_or_thread_id.as_bytes());
    let hash = hex::encode(hasher.finalize());

    retry_state_root(runtime)
        .join(context.platform.as_str())
        .join(hash)
        .join(format!("{}.json", context.rule_id))
}

fn load_retry_state(runtime: &dyn Runtime, path: &Path) -> Result<RetryState, HookBridgeError> {
    if !runtime.fs().exists(path)? {
        return Ok(RetryState {
            consecutive_failures: 0,
            last_error: String::new(),
            last_failure_epoch_sec: 0,
        });
    }

    let content = runtime.fs().read_to_string(path)?;
    serde_json::from_str(&content).map_err(|error| HookBridgeError::JsonParse {
        message: format!("invalid retry state JSON at '{}': {error}", path.display()),
    })
}

fn persist_retry_state(
    runtime: &dyn Runtime,
    path: &Path,
    state: &RetryState,
) -> Result<(), HookBridgeError> {
    let payload = serde_json::to_vec_pretty(state).map_err(|error| HookBridgeError::Process {
        message: format!("failed to serialize retry state: {error}"),
    })?;
    atomic_write(runtime.fs(), path, &payload)
}

fn persist_failure_state(
    runtime: &dyn Runtime,
    path: &Path,
    current: &RetryState,
    last_error: String,
) -> Result<(), HookBridgeError> {
    let updated = RetryState {
        consecutive_failures: current.consecutive_failures.saturating_add(1),
        last_error,
        last_failure_epoch_sec: now_epoch_sec(runtime)?,
    };
    persist_retry_state(runtime, path, &updated)
}

struct TranslatedOutput {
    stdout: Vec<u8>,
}

fn translate_output(
    platform: Platform,
    context: &RuntimeContext,
    result: &ExecutionResult,
) -> Result<TranslatedOutput, HookBridgeError> {
    let json = match platform {
        Platform::Claude => claude::translate_output(context, result),
        Platform::Codex => codex::translate_output(context, result),
    };

    let mut stdout =
        serde_json::to_vec(&json).map_err(|error| HookBridgeError::PlatformProtocol {
            message: format!("failed to serialize platform output JSON: {error}"),
        })?;
    stdout.push(b'\n');

    Ok(TranslatedOutput { stdout })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::cli::RunArgs;
    use crate::platform::Platform;

    use crate::runtime::Runtime;
    use crate::runtime::clock::{Clock, FixedClock};
    use crate::runtime::fs::{FakeFileSystem, FileSystem};
    use crate::runtime::io::{FakeIo, Io};
    use crate::runtime::process::{FakeProcessRunner, ProcessRunner};

    use super::{RuntimeContext, parse_runtime_context, retry_state_path};

    struct TestRuntime {
        fs: FakeFileSystem,
        clock: FixedClock,
        process: FakeProcessRunner,
        io: FakeIo,
        tmp: std::path::PathBuf,
    }

    impl Runtime for TestRuntime {
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
            self.tmp.clone()
        }
    }

    #[test]
    fn parse_context_works_for_codex_shape() {
        let args = RunArgs {
            platform: Platform::Codex,
            rule_id: "r1".to_string(),
        };
        let payload = r#"{"hook_event_name":"before_command","thread_id":"t1","cwd":"/tmp"}"#;
        let context_result = parse_runtime_context(&args, payload, Path::new("/tmp/cfg.yaml"));
        assert!(context_result.is_ok(), "payload should parse");
        let Ok(context) = context_result else {
            return;
        };
        assert_eq!(context.event, "before_command");
        assert_eq!(context.session_or_thread_id, "t1");
    }

    #[test]
    fn retry_key_is_stable_for_platform_session_and_rule() {
        let context = RuntimeContext {
            platform: Platform::Claude,
            event: "before_command".to_string(),
            rule_id: "rule_1".to_string(),
            source_config_path: "/tmp/custom/cfg.yaml".into(),
            session_or_thread_id: "session_1".to_string(),
            cwd: None,
            transcript_path: None,
            raw_payload: "{}".to_string(),
        };
        let runtime = TestRuntime {
            fs: FakeFileSystem::default(),
            clock: FixedClock::new(std::time::SystemTime::UNIX_EPOCH),
            process: FakeProcessRunner::success(0),
            io: FakeIo::default(),
            tmp: "/tmp/custom".into(),
        };
        let path = retry_state_path(&runtime, &context);
        let as_string = path.display().to_string();
        assert!(as_string.contains("/tmp/custom/hook_bridge/retries/claude/"));
        assert!(as_string.ends_with("/rule_1.json"));
    }

    #[test]
    fn retry_key_is_isolated_by_source_config_path() {
        let runtime = TestRuntime {
            fs: FakeFileSystem::default(),
            clock: FixedClock::new(std::time::SystemTime::UNIX_EPOCH),
            process: FakeProcessRunner::success(0),
            io: FakeIo::default(),
            tmp: "/tmp/custom".into(),
        };
        let context_a = RuntimeContext {
            platform: Platform::Codex,
            event: "before_command".to_string(),
            rule_id: "rule_same".to_string(),
            source_config_path: "/repo_a/hook-bridge.yaml".into(),
            session_or_thread_id: "thread_same".to_string(),
            cwd: None,
            transcript_path: None,
            raw_payload: "{}".to_string(),
        };
        let context_b = RuntimeContext {
            platform: Platform::Codex,
            event: "before_command".to_string(),
            rule_id: "rule_same".to_string(),
            source_config_path: "/repo_b/hook-bridge.yaml".into(),
            session_or_thread_id: "thread_same".to_string(),
            cwd: None,
            transcript_path: None,
            raw_payload: "{}".to_string(),
        };

        let path_a = retry_state_path(&runtime, &context_a);
        let path_b = retry_state_path(&runtime, &context_b);

        assert_ne!(path_a, path_b);
    }
}
