use std::io;
use std::process::{Command, ExitCode};

use hook_bridge::{STATUS_MESSAGE, entrypoint, run};

struct FailingWriter;

impl io::Write for FailingWriter {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("injected write failure"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn run_writes_expected_status_message() {
    let mut output = Vec::new();

    let result = run(&mut output);

    assert!(result.is_ok(), "run should write to the provided buffer");
    assert_eq!(output, format!("{STATUS_MESSAGE}\n").into_bytes());
}

#[test]
fn entrypoint_delegates_to_run_and_reports_success() {
    let mut output = Vec::new();

    let status = entrypoint(&mut output);

    assert_eq!(status, ExitCode::SUCCESS);
    assert_eq!(output, format!("{STATUS_MESSAGE}\n").into_bytes());
}

#[test]
fn entrypoint_reports_failure_when_writer_errors() {
    let mut writer = FailingWriter;

    let status = entrypoint(&mut writer);

    assert_eq!(status, ExitCode::FAILURE);
}

#[test]
fn binary_prints_expected_status_message() {
    let output_result = Command::new(env!("CARGO_BIN_EXE_hook_bridge")).output();

    let output = match output_result {
        Ok(output) => output,
        Err(error) => {
            let execution_error = Some(error);
            assert!(
                execution_error.is_none(),
                "binary should execute successfully during integration testing: {execution_error:?}"
            );
            return;
        }
    };

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, format!("{STATUS_MESSAGE}\n").into_bytes());
    assert!(output.stderr.is_empty(), "binary should not write to stderr");
}
