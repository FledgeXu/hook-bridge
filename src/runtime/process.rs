use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use wait_timeout::ChildExt;

use crate::error::HookBridgeError;

#[derive(Debug, Clone)]
pub struct ProcessRequest {
    pub program: String,
    pub args: Vec<String>,
    pub stdin: Vec<u8>,
    pub timeout: Duration,
    pub cwd: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    pub status_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub trait ProcessRunner {
    /// Executes a process request and captures its output.
    ///
    /// # Errors
    ///
    /// Returns an error when the process fails to spawn, times out, or output cannot be read.
    fn run(&self, request: &ProcessRequest) -> Result<ProcessOutput, HookBridgeError>;
}

#[derive(Debug, Default)]
pub struct SystemProcessRunner;

impl ProcessRunner for SystemProcessRunner {
    fn run(&self, request: &ProcessRequest) -> Result<ProcessOutput, HookBridgeError> {
        let mut command = Command::new(&request.program);
        command
            .args(&request.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(cwd) = &request.cwd {
            command.current_dir(cwd);
        }

        if !request.env.is_empty() {
            command.envs(
                request
                    .env
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str())),
            );
        }

        let mut child = command.spawn().map_err(|error| HookBridgeError::Process {
            message: format!("failed to spawn process: {error}"),
        })?;

        let mut stdout_reader = child
            .stdout
            .take()
            .map(|reader| spawn_pipe_reader(reader, "stdout"));
        let mut stderr_reader = child
            .stderr
            .take()
            .map(|reader| spawn_pipe_reader(reader, "stderr"));

        if let Some(mut stdin) = child.stdin.take() {
            if !request.stdin.is_empty()
                && let Err(error) = stdin.write_all(&request.stdin)
            {
                cleanup_child(&mut child, stdout_reader.take(), stderr_reader.take(), true);
                return Err(HookBridgeError::Process {
                    message: format!("failed to write child stdin: {error}"),
                });
            }
            // Drop stdin handle explicitly to signal EOF to child process.
            drop(stdin);
        } else {
            cleanup_child(&mut child, stdout_reader.take(), stderr_reader.take(), true);
            return Err(HookBridgeError::Process {
                message: "child stdin unavailable".to_string(),
            });
        }

        let status = match child.wait_timeout(request.timeout) {
            Ok(status) => status,
            Err(error) => {
                cleanup_child(&mut child, stdout_reader.take(), stderr_reader.take(), true);
                return Err(HookBridgeError::Process {
                    message: format!("failed while waiting child process: {error}"),
                });
            }
        };

        let Some(status) = status else {
            cleanup_child(&mut child, stdout_reader.take(), stderr_reader.take(), true);
            return Err(HookBridgeError::Timeout {
                timeout_sec: request.timeout.as_secs(),
            });
        };

        let stdout = collect_pipe_reader(stdout_reader.take(), "stdout")?;
        let stderr = collect_pipe_reader(stderr_reader.take(), "stderr")?;

        Ok(ProcessOutput {
            status_code: status.code().unwrap_or(-1),
            stdout,
            stderr,
        })
    }
}

fn spawn_pipe_reader<R>(
    mut reader: R,
    stream_name: &'static str,
) -> JoinHandle<Result<Vec<u8>, HookBridgeError>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut buffer = Vec::new();
        reader
            .read_to_end(&mut buffer)
            .map_err(|error| HookBridgeError::Process {
                message: format!("failed to read child {stream_name}: {error}"),
            })?;
        Ok(buffer)
    })
}

fn collect_pipe_reader(
    reader: Option<JoinHandle<Result<Vec<u8>, HookBridgeError>>>,
    stream_name: &'static str,
) -> Result<Vec<u8>, HookBridgeError> {
    match reader {
        Some(handle) => match handle.join() {
            Ok(result) => result,
            Err(_) => Err(HookBridgeError::Process {
                message: format!("child {stream_name} reader thread panicked"),
            }),
        },
        None => Ok(Vec::new()),
    }
}

fn cleanup_child(
    child: &mut Child,
    stdout_reader: Option<JoinHandle<Result<Vec<u8>, HookBridgeError>>>,
    stderr_reader: Option<JoinHandle<Result<Vec<u8>, HookBridgeError>>>,
    terminate: bool,
) {
    if terminate {
        let _ = child.kill();
    }
    let _ = child.wait();
    let _ = collect_pipe_reader(stdout_reader, "stdout");
    let _ = collect_pipe_reader(stderr_reader, "stderr");
}

#[derive(Debug)]
pub struct FakeProcessRunner {
    status_code: i32,
}

impl FakeProcessRunner {
    #[must_use]
    pub fn success(status_code: i32) -> Self {
        Self { status_code }
    }
}

impl ProcessRunner for FakeProcessRunner {
    fn run(&self, _request: &ProcessRequest) -> Result<ProcessOutput, HookBridgeError> {
        Ok(ProcessOutput {
            status_code: self.status_code,
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io;
    use std::time::Duration;

    use super::{
        FakeProcessRunner, ProcessRequest, ProcessRunner, SystemProcessRunner, collect_pipe_reader,
        spawn_pipe_reader,
    };
    use crate::error::HookBridgeError;

    #[test]
    fn fake_process_runner_returns_injected_result() {
        let runner = FakeProcessRunner::success(0);
        let request = ProcessRequest {
            program: "echo".to_string(),
            args: vec!["ok".to_string()],
            stdin: Vec::new(),
            timeout: Duration::from_secs(1),
            cwd: None,
            env: BTreeMap::new(),
        };

        let result = runner.run(&request);

        assert!(matches!(result, Ok(output) if output.status_code == 0));
    }

    #[test]
    fn system_runner_captures_stdout_stderr_and_exit_code() {
        let temp_result = tempfile::tempdir();
        assert!(temp_result.is_ok(), "tempdir creation should succeed");
        let Ok(temp) = temp_result else {
            return;
        };
        let runner = SystemProcessRunner;
        let request = ProcessRequest {
            program: "sh".to_string(),
            args: vec![
                "-lc".to_string(),
                "printf out; printf err 1>&2; exit 7".to_string(),
            ],
            stdin: Vec::new(),
            timeout: Duration::from_secs(1),
            cwd: Some(temp.path().to_path_buf()),
            env: BTreeMap::new(),
        };

        assert_eq!(
            runner.run(&request),
            Ok(super::ProcessOutput {
                status_code: 7,
                stdout: b"out".to_vec(),
                stderr: b"err".to_vec(),
            })
        );
    }

    #[test]
    fn system_runner_passes_cwd_env_and_stdin() {
        let temp_result = tempfile::tempdir();
        assert!(temp_result.is_ok(), "tempdir creation should succeed");
        let Ok(temp) = temp_result else {
            return;
        };
        let runner = SystemProcessRunner;
        let cwd_result = temp.path().canonicalize();
        assert!(cwd_result.is_ok(), "canonical path should resolve");
        let Ok(cwd) = cwd_result else {
            return;
        };
        let mut env = BTreeMap::new();
        env.insert("HOOK_BRIDGE_TEST".to_string(), "set".to_string());
        let request = ProcessRequest {
            program: "sh".to_string(),
            args: vec![
                "-lc".to_string(),
                "printf '%s|%s' \"$PWD\" \"$HOOK_BRIDGE_TEST\"; cat 1>&2".to_string(),
            ],
            stdin: b"stdin-payload".to_vec(),
            timeout: Duration::from_secs(1),
            cwd: Some(temp.path().to_path_buf()),
            env,
        };

        let output = runner.run(&request);

        assert_eq!(
            output,
            Ok(super::ProcessOutput {
                status_code: 0,
                stdout: format!("{}|set", cwd.display()).into_bytes(),
                stderr: b"stdin-payload".to_vec(),
            })
        );
    }

    #[test]
    fn system_runner_reports_spawn_failure() {
        let runner = SystemProcessRunner;
        let request = ProcessRequest {
            program: "command_that_does_not_exist_123".to_string(),
            args: Vec::new(),
            stdin: Vec::new(),
            timeout: Duration::from_secs(1),
            cwd: None,
            env: BTreeMap::new(),
        };

        assert!(matches!(
            runner.run(&request),
            Err(HookBridgeError::Process { message })
                if message.contains("failed to spawn process")
        ));
    }

    #[test]
    fn system_runner_reports_timeout() {
        let runner = SystemProcessRunner;
        let request = ProcessRequest {
            program: "sh".to_string(),
            args: vec!["-lc".to_string(), "sleep 2".to_string()],
            stdin: Vec::new(),
            timeout: Duration::from_secs(1),
            cwd: None,
            env: BTreeMap::new(),
        };

        assert_eq!(
            runner.run(&request),
            Err(HookBridgeError::Timeout { timeout_sec: 1 })
        );
    }

    #[test]
    fn system_runner_reports_child_stdin_write_failures() {
        let runner = SystemProcessRunner;
        let request = ProcessRequest {
            program: "sh".to_string(),
            args: vec!["-lc".to_string(), "exec 0<&-; exit 0".to_string()],
            stdin: vec![b'x'; 1024 * 1024],
            timeout: Duration::from_secs(1),
            cwd: None,
            env: BTreeMap::new(),
        };

        assert!(matches!(
            runner.run(&request),
            Err(HookBridgeError::Process { message })
                if message.contains("failed to write child stdin")
        ));
    }

    #[test]
    fn collect_pipe_reader_returns_empty_for_missing_handle() {
        assert_eq!(collect_pipe_reader(None, "stdout"), Ok(Vec::new()));
    }

    struct BrokenReader;

    impl io::Read for BrokenReader {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("broken pipe"))
        }
    }

    #[test]
    fn spawn_pipe_reader_surfaces_read_errors() {
        let handle = spawn_pipe_reader(BrokenReader, "stdout");

        assert!(matches!(
            collect_pipe_reader(Some(handle), "stdout"),
            Err(HookBridgeError::Process { message })
                if message.contains("failed to read child stdout")
        ));
    }

    #[test]
    fn collect_pipe_reader_surfaces_panics() {
        let handle = std::thread::spawn(|| -> Result<Vec<u8>, HookBridgeError> {
            std::panic::resume_unwind(Box::new("boom".to_string()));
        });

        assert_eq!(
            collect_pipe_reader(Some(handle), "stderr"),
            Err(HookBridgeError::Process {
                message: "child stderr reader thread panicked".to_string(),
            })
        );
    }
}
