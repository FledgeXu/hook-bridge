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
    pub raw_event: String,
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

    let execution_result = execute_rule(runtime, rule, &context)?;

    let output = translate_output(args.platform, &context, &execution_result)?;
    runtime.io().write_stdout(&output.stdout)?;

    Ok(())
}

fn execute_rule(
    runtime: &dyn Runtime,
    rule: &PlatformRule,
    context: &RuntimeContext,
) -> Result<ExecutionResult, HookBridgeError> {
    let state_path = retry_state_path(runtime, context);
    let state = load_retry_state(runtime, &state_path)?;
    if retry_guard_engaged(rule, &state) {
        return Ok(retry_guard_result());
    }

    let execution_result = run_user_command(runtime, rule, context);

    update_retry_state(runtime, &state_path, &state, &execution_result)?;

    Ok(execution_result)
}

fn run_user_command(
    runtime: &dyn Runtime,
    rule: &PlatformRule,
    context: &RuntimeContext,
) -> ExecutionResult {
    let request = ProcessRequest {
        program: rule.shell.clone(),
        args: vec!["-lc".to_string(), rule.command.clone()],
        stdin: context.raw_payload.as_bytes().to_vec(),
        timeout: Duration::from_secs(rule.timeout_sec),
        cwd: rule.working_dir.clone().or_else(|| context.cwd.clone()),
        env: command_env(rule, context),
    };

    let output = match runtime.process_runner().run(&request) {
        Ok(output) => output,
        Err(error) => {
            return ExecutionResult {
                status: InternalStatus::Error,
                message: Some(error.to_string()),
                system_message: Some("hook_bridge command execution failed".to_string()),
                exit_code: Some(1),
                raw_stdout: Vec::new(),
                raw_stderr: Vec::new(),
            };
        }
    };

    if output.status_code == 0 {
        ExecutionResult {
            status: InternalStatus::Success,
            message: None,
            system_message: None,
            exit_code: Some(0),
            raw_stdout: output.stdout,
            raw_stderr: output.stderr,
        }
    } else {
        ExecutionResult {
            status: InternalStatus::Block,
            message: Some(format!(
                "command exited with non-zero status {}",
                output.status_code
            )),
            system_message: Some("hook_bridge command returned non-zero exit code".to_string()),
            exit_code: Some(output.status_code),
            raw_stdout: output.stdout,
            raw_stderr: output.stderr,
        }
    }
}

fn command_env(
    rule: &PlatformRule,
    context: &RuntimeContext,
) -> std::collections::BTreeMap<String, String> {
    let mut env = rule.env.clone();
    env.insert(
        "HOOK_BRIDGE_PLATFORM".to_string(),
        context.platform.as_str().to_string(),
    );
    env.insert("HOOK_BRIDGE_RULE_ID".to_string(), context.rule_id.clone());
    env.insert("HOOK_BRIDGE_EVENT".to_string(), context.event.clone());
    env
}

fn retry_guard_engaged(rule: &PlatformRule, state: &RetryState) -> bool {
    rule.max_retries > 0 && state.consecutive_failures >= rule.max_retries
}

fn retry_guard_result() -> ExecutionResult {
    ExecutionResult {
        status: InternalStatus::Stop,
        message: Some("max retries reached, skipping command execution".to_string()),
        system_message: Some("hook_bridge retry guard engaged".to_string()),
        exit_code: Some(0),
        raw_stdout: Vec::new(),
        raw_stderr: Vec::new(),
    }
}

fn update_retry_state(
    runtime: &dyn Runtime,
    path: &Path,
    state: &RetryState,
    result: &ExecutionResult,
) -> Result<(), HookBridgeError> {
    match result.status {
        InternalStatus::Success => runtime.fs().remove_file_if_exists(path),
        InternalStatus::Block | InternalStatus::Error => persist_failure_state(
            runtime,
            path,
            state,
            result
                .message
                .clone()
                .unwrap_or_else(|| "execution failed".to_string()),
        ),
        InternalStatus::Stop => Ok(()),
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

    let parsed = match args.platform {
        Platform::Claude => claude::parse_context_fields(&value)?,
        Platform::Codex => codex::parse_context_fields(&value)?,
    };

    Ok(RuntimeContext {
        platform: args.platform,
        raw_event: parsed.raw_event,
        event: parsed.event,
        rule_id: args.rule_id.clone(),
        source_config_path: source_config_path.to_path_buf(),
        session_or_thread_id: parsed.session_or_thread_id,
        cwd: parsed.cwd,
        transcript_path: parsed.transcript_path,
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
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::env;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use crate::cli::RunArgs;
    use crate::config::PlatformRule;
    use crate::error::HookBridgeError;
    use crate::platform::{Platform, normalize_event_name};

    use crate::runtime::Runtime;
    use crate::runtime::clock::{Clock, FixedClock};
    use crate::runtime::fs::{FakeFileSystem, FileSystem, OsFileSystem};
    use crate::runtime::io::{FakeIo, Io};
    use crate::runtime::process::{
        FakeProcessRunner, ProcessOutput, ProcessRequest, ProcessRunner,
    };

    use super::{
        ExecutionResult, InternalStatus, RetryState, RuntimeContext, command_env, execute,
        execute_rule, load_retry_state, now_epoch_sec, parse_runtime_context, persist_retry_state,
        retry_guard_engaged, retry_guard_result, retry_state_path, run_user_command,
        translate_output, update_retry_state,
    };

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

    #[derive(Default)]
    struct CapturingIo {
        stdin: Vec<u8>,
        stdout: RefCell<Vec<u8>>,
    }

    impl Io for CapturingIo {
        fn read_stdin(&self) -> Result<Vec<u8>, crate::error::HookBridgeError> {
            Ok(self.stdin.clone())
        }

        fn write_stdout(&self, bytes: &[u8]) -> Result<(), crate::error::HookBridgeError> {
            self.stdout.borrow_mut().extend_from_slice(bytes);
            Ok(())
        }

        fn write_stderr(&self, _bytes: &[u8]) -> Result<(), crate::error::HookBridgeError> {
            Ok(())
        }
    }

    struct ExecuteRuntime {
        fs: OsFileSystem,
        clock: FixedClock,
        process: FakeProcessRunner,
        io: CapturingIo,
        tmp: PathBuf,
    }

    impl Runtime for ExecuteRuntime {
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

        fn temp_dir(&self) -> PathBuf {
            self.tmp.clone()
        }
    }

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

    enum RecordedProcessResult {
        Success(ProcessOutput),
        Failure(HookBridgeError),
    }

    struct RecordingProcessRunner {
        requests: RefCell<Vec<ProcessRequest>>,
        result: RecordedProcessResult,
    }

    impl RecordingProcessRunner {
        fn success(status_code: i32, stdout: &[u8], stderr: &[u8]) -> Self {
            Self {
                requests: RefCell::new(Vec::new()),
                result: RecordedProcessResult::Success(ProcessOutput {
                    status_code,
                    stdout: stdout.to_vec(),
                    stderr: stderr.to_vec(),
                }),
            }
        }

        fn failure(error: HookBridgeError) -> Self {
            Self {
                requests: RefCell::new(Vec::new()),
                result: RecordedProcessResult::Failure(error),
            }
        }
    }

    impl ProcessRunner for RecordingProcessRunner {
        fn run(&self, request: &ProcessRequest) -> Result<ProcessOutput, HookBridgeError> {
            self.requests.borrow_mut().push(request.clone());
            match &self.result {
                RecordedProcessResult::Success(output) => Ok(output.clone()),
                RecordedProcessResult::Failure(error) => match error {
                    HookBridgeError::Parameter { message } => Err(HookBridgeError::Parameter {
                        message: message.clone(),
                    }),
                    HookBridgeError::ConfigValidation { message } => {
                        Err(HookBridgeError::ConfigValidation {
                            message: message.clone(),
                        })
                    }
                    HookBridgeError::FileConflict { path } => {
                        Err(HookBridgeError::FileConflict { path: path.clone() })
                    }
                    HookBridgeError::JsonParse { message } => Err(HookBridgeError::JsonParse {
                        message: message.clone(),
                    }),
                    HookBridgeError::Process { message } => Err(HookBridgeError::Process {
                        message: message.clone(),
                    }),
                    HookBridgeError::Timeout { timeout_sec } => Err(HookBridgeError::Timeout {
                        timeout_sec: *timeout_sec,
                    }),
                    HookBridgeError::PlatformProtocol { message } => {
                        Err(HookBridgeError::PlatformProtocol {
                            message: message.clone(),
                        })
                    }
                    HookBridgeError::Io {
                        operation,
                        path,
                        kind,
                    } => Err(HookBridgeError::Io {
                        operation,
                        path: path.clone(),
                        kind: *kind,
                    }),
                    HookBridgeError::NotImplemented { feature } => {
                        Err(HookBridgeError::NotImplemented { feature })
                    }
                },
            }
        }
    }

    struct RecordingRuntime {
        fs: OsFileSystem,
        clock: FixedClock,
        process: RecordingProcessRunner,
        io: CapturingIo,
        tmp: PathBuf,
    }

    impl Runtime for RecordingRuntime {
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

        fn temp_dir(&self) -> PathBuf {
            self.tmp.clone()
        }
    }

    fn sample_rule() -> PlatformRule {
        PlatformRule {
            event: "before_command".to_string(),
            command: "echo ok".to_string(),
            matcher: None,
            shell: "sh".to_string(),
            timeout_sec: 30,
            max_retries: 2,
            working_dir: Some(PathBuf::from("/rule-cwd")),
            env: BTreeMap::from([
                ("USER_DEFINED".to_string(), "1".to_string()),
                (
                    "HOOK_BRIDGE_EVENT".to_string(),
                    "user-overridden".to_string(),
                ),
            ]),
            extra: BTreeMap::new(),
        }
    }

    fn sample_context() -> RuntimeContext {
        RuntimeContext {
            platform: Platform::Codex,
            raw_event: "PreToolUse".to_string(),
            event: "before_command".to_string(),
            rule_id: "r1".to_string(),
            source_config_path: "/tmp/cfg.yaml".into(),
            session_or_thread_id: "t1".to_string(),
            cwd: Some(PathBuf::from("/context-cwd")),
            transcript_path: None,
            raw_payload: r#"{"hook_event_name":"PreToolUse","session_id":"t1"}"#.to_string(),
        }
    }

    #[test]
    fn parse_context_works_for_codex_shape() {
        let args = RunArgs {
            platform: Platform::Codex,
            rule_id: "r1".to_string(),
        };
        let payload = r#"{"hook_event_name":"before_command","session_id":"t1","cwd":"/tmp"}"#;
        let context = parse_runtime_context(&args, payload, Path::new("/tmp/cfg.yaml"));
        assert_eq!(
            context.as_ref().map(|value| value.event.as_str()),
            Ok("before_command")
        );
        assert_eq!(
            context.as_ref().map(|value| value.raw_event.as_str()),
            Ok("before_command")
        );
        assert_eq!(
            context
                .as_ref()
                .map(|value| value.session_or_thread_id.as_str()),
            Ok("t1")
        );
    }

    #[test]
    fn parse_context_preserves_raw_native_event_for_platform_output() {
        let args = RunArgs {
            platform: Platform::Codex,
            rule_id: "r1".to_string(),
        };
        let payload = r#"{"hook_event_name":"PreToolUse","session_id":"t1","cwd":"/tmp"}"#;
        let context = parse_runtime_context(&args, payload, Path::new("/tmp/cfg.yaml"));

        assert_eq!(
            context.as_ref().map(|value| value.event.as_str()),
            Ok("before_command")
        );
        assert_eq!(
            context.as_ref().map(|value| value.raw_event.as_str()),
            Ok("PreToolUse")
        );
    }

    #[test]
    fn parse_context_preserves_full_raw_payload() {
        let args = RunArgs {
            platform: Platform::Claude,
            rule_id: "r1".to_string(),
        };
        let payload = r#"{"hook_event_name":"before_command","session_id":"s1","cwd":"/tmp","extra":{"nested":true}}"#;
        let context = parse_runtime_context(&args, payload, Path::new("/tmp/cfg.yaml"));

        assert_eq!(
            context.as_ref().map(|value| value.raw_payload.as_str()),
            Ok(payload)
        );
    }

    #[test]
    fn parse_context_rejects_payload_event_not_supported_by_selected_platform() {
        let args = RunArgs {
            platform: Platform::Codex,
            rule_id: "r1".to_string(),
        };

        assert!(matches!(
            parse_runtime_context(
                &args,
                r#"{"hook_event_name":"Notification","session_id":"t1"}"#,
                Path::new("/tmp/cfg.yaml"),
            ),
            Err(crate::error::HookBridgeError::PlatformProtocol { message })
                if message
                    == "codex payload event 'Notification' is not supported for platform 'codex'"
        ));
    }

    #[test]
    fn retry_key_is_stable_for_platform_session_and_rule() {
        let context = RuntimeContext {
            platform: Platform::Claude,
            raw_event: "before_command".to_string(),
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
            raw_event: "before_command".to_string(),
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
            raw_event: "before_command".to_string(),
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

    #[test]
    fn parse_context_rejects_invalid_json() {
        let args = RunArgs {
            platform: Platform::Codex,
            rule_id: "r1".to_string(),
        };

        assert!(matches!(
            parse_runtime_context(&args, "{", Path::new("/tmp/cfg.yaml")),
            Err(crate::error::HookBridgeError::JsonParse { message })
                if message.contains("invalid runtime JSON input")
        ));
    }

    #[test]
    fn test_runtime_exposes_all_dependencies() {
        let runtime = TestRuntime {
            fs: FakeFileSystem::default(),
            clock: FixedClock::new(std::time::SystemTime::UNIX_EPOCH),
            process: FakeProcessRunner::success(0),
            io: FakeIo::default(),
            tmp: "/tmp/run-tests".into(),
        };

        assert!(matches!(runtime.fs().exists(Path::new(".")), Ok(false)));
        assert_eq!(runtime.clock().now(), std::time::SystemTime::UNIX_EPOCH);
        assert_eq!(runtime.io().read_stdin(), Ok(Vec::new()));
        assert_eq!(runtime.temp_dir(), PathBuf::from("/tmp/run-tests"));
        assert!(matches!(
            runtime.process_runner().run(&crate::runtime::process::ProcessRequest {
                program: "echo".to_string(),
                args: vec!["ok".to_string()],
                stdin: Vec::new(),
                timeout: std::time::Duration::from_secs(1),
                cwd: None,
                env: std::collections::BTreeMap::new(),
            }),
            Ok(output) if output.status_code == 0
        ));
    }

    #[test]
    fn normalize_event_name_accepts_native_and_unified_values() {
        assert_eq!(
            normalize_event_name(Platform::Codex, "PreToolUse"),
            Some("before_command")
        );
        assert_eq!(
            normalize_event_name(Platform::Claude, "PostToolUse"),
            Some("after_command")
        );
        assert_eq!(
            normalize_event_name(Platform::Codex, "SessionStart"),
            Some("session_start")
        );
        assert_eq!(
            normalize_event_name(Platform::Claude, "before_command"),
            Some("before_command")
        );
        assert_eq!(normalize_event_name(Platform::Codex, "Notification"), None);
    }

    fn write_managed_hooks_file(root: &Path, source_config: &str) {
        let create_result = std::fs::create_dir_all(root.join(".codex"));
        assert!(create_result.is_ok(), "codex dir should be creatable");
        let write_result = std::fs::write(
            root.join(".codex/hooks.json"),
            serde_json::json!({
                "_hook_bridge": {
                    "managed_by": "hook_bridge",
                    "managed_version": 1,
                    "source_config": source_config,
                }
            })
            .to_string(),
        );
        assert!(
            write_result.is_ok(),
            "managed hooks fixture should be writable"
        );
    }

    fn write_config(root: &Path) -> PathBuf {
        let config = root.join("hook-bridge.yaml");
        let write_result = std::fs::write(
            &config,
            r"
version: 1
hooks:
  - id: r1
    event: before_command
    command: echo ok
    max_retries: 1
",
        );
        assert!(write_result.is_ok(), "config file should be writable");
        config
    }

    #[test]
    fn execute_rejects_relative_managed_source_config() {
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
        write_managed_hooks_file(temp.path(), "hook-bridge.yaml");
        let runtime = ExecuteRuntime {
            fs: OsFileSystem,
            clock: FixedClock::new(std::time::SystemTime::UNIX_EPOCH),
            process: FakeProcessRunner::success(0),
            io: CapturingIo::default(),
            tmp: temp.path().to_path_buf(),
        };

        assert_eq!(
            execute(
                &RunArgs {
                    platform: Platform::Codex,
                    rule_id: "r1".to_string(),
                },
                &runtime,
            ),
            Err(crate::error::HookBridgeError::ConfigValidation {
                message: "managed source_config must be absolute, got 'hook-bridge.yaml'"
                    .to_string(),
            })
        );
    }

    #[test]
    fn execute_rejects_non_utf8_stdin() {
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
        let config_path = write_config(temp.path());
        write_managed_hooks_file(temp.path(), &config_path.display().to_string());
        let runtime = ExecuteRuntime {
            fs: OsFileSystem,
            clock: FixedClock::new(std::time::SystemTime::UNIX_EPOCH),
            process: FakeProcessRunner::success(0),
            io: CapturingIo {
                stdin: vec![0xff],
                stdout: RefCell::new(Vec::new()),
            },
            tmp: temp.path().to_path_buf(),
        };

        assert!(matches!(
            execute(
                &RunArgs {
                    platform: Platform::Codex,
                    rule_id: "r1".to_string(),
                },
                &runtime,
            ),
            Err(crate::error::HookBridgeError::JsonParse { message })
                if message.contains("stdin payload is not valid UTF-8 JSON")
        ));
    }

    #[test]
    fn execute_rejects_event_mismatch() {
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
        let config_path = write_config(temp.path());
        write_managed_hooks_file(temp.path(), &config_path.display().to_string());
        let runtime = ExecuteRuntime {
            fs: OsFileSystem,
            clock: FixedClock::new(std::time::SystemTime::UNIX_EPOCH),
            process: FakeProcessRunner::success(0),
            io: CapturingIo {
                stdin: br#"{"hook_event_name":"after_command","session_id":"t1"}"#.to_vec(),
                stdout: RefCell::new(Vec::new()),
            },
            tmp: temp.path().to_path_buf(),
        };

        assert_eq!(
            execute(
                &RunArgs {
                    platform: Platform::Codex,
                    rule_id: "r1".to_string(),
                },
                &runtime,
            ),
            Err(crate::error::HookBridgeError::PlatformProtocol {
                message: "event mismatch for rule 'r1': stdin event 'after_command' but configured event 'before_command'".to_string(),
            })
        );
    }

    #[test]
    fn execute_short_circuits_when_retry_guard_is_engaged() {
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
        let config_path = write_config(temp.path());
        write_managed_hooks_file(temp.path(), &config_path.display().to_string());
        let runtime = ExecuteRuntime {
            fs: OsFileSystem,
            clock: FixedClock::new(std::time::UNIX_EPOCH + std::time::Duration::from_secs(10)),
            process: FakeProcessRunner::success(0),
            io: CapturingIo {
                stdin: br#"{"hook_event_name":"before_command","session_id":"t1"}"#.to_vec(),
                stdout: RefCell::new(Vec::new()),
            },
            tmp: temp.path().to_path_buf(),
        };
        let context = RuntimeContext {
            platform: Platform::Codex,
            raw_event: "before_command".to_string(),
            event: "before_command".to_string(),
            rule_id: "r1".to_string(),
            source_config_path: config_path.clone(),
            session_or_thread_id: "t1".to_string(),
            cwd: None,
            transcript_path: None,
            raw_payload: "{}".to_string(),
        };
        let state_path = retry_state_path(&runtime, &context);
        assert_eq!(
            persist_retry_state(
                &runtime,
                &state_path,
                &RetryState {
                    consecutive_failures: 1,
                    last_error: "boom".to_string(),
                    last_failure_epoch_sec: 9,
                },
            ),
            Ok(())
        );

        assert_eq!(
            execute(
                &RunArgs {
                    platform: Platform::Codex,
                    rule_id: "r1".to_string(),
                },
                &runtime,
            ),
            Ok(())
        );
        assert!(
            String::from_utf8(runtime.io.stdout.borrow().clone())
                .is_ok_and(|payload| payload.contains("max retries reached")),
            "retry guard should write protocol output"
        );
    }

    #[test]
    fn execute_runtime_exposes_all_dependencies() {
        let runtime = ExecuteRuntime {
            fs: OsFileSystem,
            clock: FixedClock::new(std::time::UNIX_EPOCH),
            process: FakeProcessRunner::success(0),
            io: CapturingIo::default(),
            tmp: "/tmp/exec-tests".into(),
        };

        assert!(matches!(
            runtime.fs().exists(Path::new("/definitely/missing")),
            Ok(false)
        ));
        assert_eq!(runtime.clock().now(), std::time::UNIX_EPOCH);
        assert_eq!(
            runtime
                .process_runner()
                .run(&crate::runtime::process::ProcessRequest {
                    program: "echo".to_string(),
                    args: vec!["ok".to_string()],
                    stdin: Vec::new(),
                    timeout: std::time::Duration::from_secs(1),
                    cwd: None,
                    env: std::collections::BTreeMap::new(),
                }),
            Ok(crate::runtime::process::ProcessOutput {
                status_code: 0,
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
        );
        assert_eq!(runtime.io().read_stdin(), Ok(Vec::new()));
        assert_eq!(runtime.temp_dir(), PathBuf::from("/tmp/exec-tests"));
    }

    #[test]
    fn helper_functions_cover_error_and_output_paths() {
        let retry_path = PathBuf::from("/tmp/retry.json");
        let runtime = TestRuntime {
            fs: FakeFileSystem::with_existing(vec![retry_path.clone()]),
            clock: FixedClock::new(std::time::UNIX_EPOCH - std::time::Duration::from_secs(1)),
            process: FakeProcessRunner::success(0),
            io: FakeIo::default(),
            tmp: "/tmp/custom".into(),
        };
        let context = RuntimeContext {
            platform: Platform::Codex,
            raw_event: "before_command".to_string(),
            event: "before_command".to_string(),
            rule_id: "r1".to_string(),
            source_config_path: "/tmp/cfg.yaml".into(),
            session_or_thread_id: "t1".to_string(),
            cwd: None,
            transcript_path: None,
            raw_payload: "{}".to_string(),
        };

        assert!(matches!(
            now_epoch_sec(&runtime),
            Err(crate::error::HookBridgeError::Process { message })
                if message.contains("clock error")
        ));
        assert!(matches!(
            load_retry_state(&runtime, retry_path.as_path()),
            Err(crate::error::HookBridgeError::JsonParse { message })
                if message.contains("invalid retry state JSON")
        ));
        assert_eq!(
            translate_output(
                Platform::Codex,
                &context,
                &super::ExecutionResult {
                    status: super::InternalStatus::Error,
                    message: Some("boom".to_string()),
                    system_message: Some("bridge failed".to_string()),
                    exit_code: Some(1),
                    raw_stdout: Vec::new(),
                    raw_stderr: Vec::new(),
                },
            )
            .map(|output| {
                let payload_result = String::from_utf8(output.stdout);
                assert!(
                    payload_result.is_ok(),
                    "protocol output should be valid utf-8"
                );
                let Ok(payload) = payload_result else {
                    return serde_json::Value::Null;
                };
                let value_result = serde_json::from_str::<serde_json::Value>(payload.trim_end());
                assert!(value_result.is_ok(), "protocol output should be valid json");
                let Ok(value) = value_result else {
                    return serde_json::Value::Null;
                };
                value
            }),
            Ok(serde_json::json!({
                "decision": "block",
                "reason": "boom",
            }))
        );
    }

    #[test]
    fn command_env_injects_bridge_metadata_over_user_values() {
        let env = command_env(&sample_rule(), &sample_context());

        assert_eq!(env.get("USER_DEFINED"), Some(&"1".to_string()));
        assert_eq!(env.get("HOOK_BRIDGE_PLATFORM"), Some(&"codex".to_string()));
        assert_eq!(env.get("HOOK_BRIDGE_RULE_ID"), Some(&"r1".to_string()));
        assert_eq!(
            env.get("HOOK_BRIDGE_EVENT"),
            Some(&"before_command".to_string())
        );
    }

    #[test]
    fn run_user_command_passes_stdin_cwd_timeout_and_env_to_process_runner() {
        let temp_result = tempfile::tempdir();
        assert!(temp_result.is_ok(), "tempdir creation should succeed");
        let Ok(temp) = temp_result else {
            return;
        };
        let runtime = RecordingRuntime {
            fs: OsFileSystem,
            clock: FixedClock::new(std::time::UNIX_EPOCH),
            process: RecordingProcessRunner::success(0, b"out", b"err"),
            io: CapturingIo::default(),
            tmp: temp.path().to_path_buf(),
        };

        let result = run_user_command(&runtime, &sample_rule(), &sample_context());
        let requests = runtime.process.requests.borrow();
        let request = requests.first();

        assert_eq!(result.status, InternalStatus::Success);
        assert_eq!(result.raw_stdout, b"out".to_vec());
        assert_eq!(result.raw_stderr, b"err".to_vec());
        assert!(request.is_some(), "process request should be recorded");
        let Some(request) = request else {
            return;
        };
        assert_eq!(request.program, "sh");
        assert_eq!(request.args, vec!["-lc".to_string(), "echo ok".to_string()]);
        assert_eq!(
            request.stdin,
            br#"{"hook_event_name":"PreToolUse","session_id":"t1"}"#.to_vec()
        );
        assert_eq!(request.timeout, Duration::from_secs(30));
        assert_eq!(request.cwd, Some(PathBuf::from("/rule-cwd")));
        assert_eq!(
            request.env.get("HOOK_BRIDGE_PLATFORM"),
            Some(&"codex".to_string())
        );
        assert_eq!(request.env.get("USER_DEFINED"), Some(&"1".to_string()));
    }

    #[test]
    fn run_user_command_maps_non_zero_exit_to_block_result() {
        let temp_result = tempfile::tempdir();
        assert!(temp_result.is_ok(), "tempdir creation should succeed");
        let Ok(temp) = temp_result else {
            return;
        };
        let runtime = RecordingRuntime {
            fs: OsFileSystem,
            clock: FixedClock::new(std::time::UNIX_EPOCH),
            process: RecordingProcessRunner::success(23, b"out", b"err"),
            io: CapturingIo::default(),
            tmp: temp.path().to_path_buf(),
        };

        let result = run_user_command(&runtime, &sample_rule(), &sample_context());

        assert_eq!(result.status, InternalStatus::Block);
        assert_eq!(result.exit_code, Some(23));
        assert_eq!(
            result.message,
            Some("command exited with non-zero status 23".to_string())
        );
        assert_eq!(
            result.system_message,
            Some("hook_bridge command returned non-zero exit code".to_string())
        );
    }

    #[test]
    fn run_user_command_maps_spawn_and_timeout_errors_to_error_result() {
        let temp_result = tempfile::tempdir();
        assert!(temp_result.is_ok(), "tempdir creation should succeed");
        let Ok(temp) = temp_result else {
            return;
        };
        let runtime = RecordingRuntime {
            fs: OsFileSystem,
            clock: FixedClock::new(std::time::UNIX_EPOCH),
            process: RecordingProcessRunner::failure(HookBridgeError::Timeout { timeout_sec: 3 }),
            io: CapturingIo::default(),
            tmp: temp.path().to_path_buf(),
        };

        let result = run_user_command(&runtime, &sample_rule(), &sample_context());

        assert_eq!(result.status, InternalStatus::Error);
        assert_eq!(result.exit_code, Some(1));
        assert_eq!(result.message, Some("timeout after 3s".to_string()));
    }

    #[test]
    fn retry_guard_helpers_preserve_stop_as_non_failure() {
        let rule = sample_rule();
        let state = RetryState {
            consecutive_failures: 2,
            last_error: "boom".to_string(),
            last_failure_epoch_sec: 1,
        };

        assert!(retry_guard_engaged(&rule, &state));
        assert_eq!(retry_guard_result().status, InternalStatus::Stop);
    }

    #[test]
    fn update_retry_state_clears_success_persists_failures_and_ignores_stop() {
        let temp_result = tempfile::tempdir();
        assert!(temp_result.is_ok(), "tempdir creation should succeed");
        let Ok(temp) = temp_result else {
            return;
        };
        let runtime = ExecuteRuntime {
            fs: OsFileSystem,
            clock: FixedClock::new(std::time::UNIX_EPOCH + Duration::from_secs(77)),
            process: FakeProcessRunner::success(0),
            io: CapturingIo::default(),
            tmp: temp.path().to_path_buf(),
        };
        let path = temp.path().join("retry/state.json");
        let initial = RetryState {
            consecutive_failures: 1,
            last_error: "old".to_string(),
            last_failure_epoch_sec: 5,
        };

        assert_eq!(persist_retry_state(&runtime, &path, &initial), Ok(()));
        assert_eq!(
            update_retry_state(
                &runtime,
                &path,
                &initial,
                &ExecutionResult {
                    status: InternalStatus::Success,
                    message: None,
                    system_message: None,
                    exit_code: Some(0),
                    raw_stdout: Vec::new(),
                    raw_stderr: Vec::new(),
                }
            ),
            Ok(())
        );
        assert_eq!(runtime.fs().exists(&path), Ok(false));

        assert_eq!(
            update_retry_state(
                &runtime,
                &path,
                &initial,
                &ExecutionResult {
                    status: InternalStatus::Block,
                    message: Some("blocked".to_string()),
                    system_message: None,
                    exit_code: Some(2),
                    raw_stdout: Vec::new(),
                    raw_stderr: Vec::new(),
                }
            ),
            Ok(())
        );
        assert_eq!(
            load_retry_state(&runtime, &path),
            Ok(RetryState {
                consecutive_failures: 2,
                last_error: "blocked".to_string(),
                last_failure_epoch_sec: 77,
            })
        );

        assert_eq!(
            update_retry_state(&runtime, &path, &initial, &retry_guard_result()),
            Ok(())
        );
        assert_eq!(
            load_retry_state(&runtime, &path),
            Ok(RetryState {
                consecutive_failures: 2,
                last_error: "blocked".to_string(),
                last_failure_epoch_sec: 77,
            })
        );
    }

    #[test]
    fn execute_rule_creates_retry_state_and_clears_it_after_later_success() {
        let temp_result = tempfile::tempdir();
        assert!(temp_result.is_ok(), "tempdir creation should succeed");
        let Ok(temp) = temp_result else {
            return;
        };
        let context = RuntimeContext {
            source_config_path: temp.path().join("hook-bridge.yaml"),
            ..sample_context()
        };
        let state_path = retry_state_path(
            &ExecuteRuntime {
                fs: OsFileSystem,
                clock: FixedClock::new(std::time::UNIX_EPOCH),
                process: FakeProcessRunner::success(0),
                io: CapturingIo::default(),
                tmp: temp.path().to_path_buf(),
            },
            &context,
        );

        let failing_runtime = RecordingRuntime {
            fs: OsFileSystem,
            clock: FixedClock::new(std::time::UNIX_EPOCH + Duration::from_secs(11)),
            process: RecordingProcessRunner::failure(HookBridgeError::Process {
                message: "failed to spawn process: boom".to_string(),
            }),
            io: CapturingIo::default(),
            tmp: temp.path().to_path_buf(),
        };
        let failure_result = execute_rule(&failing_runtime, &sample_rule(), &context);

        assert!(matches!(
            failure_result,
            Ok(ExecutionResult {
                status: InternalStatus::Error,
                ..
            })
        ));
        assert_eq!(
            load_retry_state(&failing_runtime, &state_path),
            Ok(RetryState {
                consecutive_failures: 1,
                last_error: "process error: failed to spawn process: boom".to_string(),
                last_failure_epoch_sec: 11,
            })
        );

        let success_runtime = RecordingRuntime {
            fs: OsFileSystem,
            clock: FixedClock::new(std::time::UNIX_EPOCH + Duration::from_secs(12)),
            process: RecordingProcessRunner::success(0, b"", b""),
            io: CapturingIo::default(),
            tmp: temp.path().to_path_buf(),
        };

        assert!(matches!(
            execute_rule(&success_runtime, &sample_rule(), &context),
            Ok(ExecutionResult {
                status: InternalStatus::Success,
                ..
            })
        ));
        assert_eq!(success_runtime.fs().exists(&state_path), Ok(false));
    }
}
