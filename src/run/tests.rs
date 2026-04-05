use std::cell::RefCell;
use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cli::RunArgs;
use crate::config::PlatformRule;
use crate::error::HookBridgeError;
use crate::platform::Platform;
use crate::runtime::Runtime;
use crate::runtime::clock::{Clock, FixedClock};
use crate::runtime::fs::{FakeFileSystem, FileSystem, OsFileSystem};
use crate::runtime::io::{FakeIo, Io};
use crate::runtime::process::{FakeProcessRunner, ProcessOutput, ProcessRequest, ProcessRunner};

use super::{
    BridgeOutput, ExecutionResult, InternalStatus, RetryState, RuntimeContext, command_env,
    execute, execute_rule, format_non_zero_exit_summary, load_retry_state, now_epoch_sec,
    parse_runtime_context, persist_retry_state, retry_guard_engaged, retry_guard_result,
    retry_state_path, run_user_command, summarize_output_stream, translate_output,
    update_retry_state,
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
        event: "PreToolUse".to_string(),
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
        event: "PreToolUse".to_string(),
        rule_id: "r1".to_string(),
        source_config_path: "/tmp/cfg.yaml".into(),
        session_or_thread_id: "t1".to_string(),
        cwd: Some(PathBuf::from("/context-cwd")),
        transcript_path: None,
        raw_payload: r#"{"hook_event_name":"PreToolUse","session_id":"t1"}"#.to_string(),
    }
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

#[path = "tests/context_execute.rs"]
mod context_execute;

#[path = "tests/command_output.rs"]
mod command_output;

#[path = "tests/retry_state.rs"]
mod retry_state;
