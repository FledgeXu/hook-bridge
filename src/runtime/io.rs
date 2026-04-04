use std::io::{Read, Write};

use crate::error::HookBridgeError;

pub trait Io {
    /// Reads all bytes from stdin.
    ///
    /// # Errors
    ///
    /// Returns an error when stdin cannot be read.
    fn read_stdin(&self) -> Result<Vec<u8>, HookBridgeError>;
    /// Writes bytes to stdout.
    ///
    /// # Errors
    ///
    /// Returns an error when stdout cannot be written.
    fn write_stdout(&self, bytes: &[u8]) -> Result<(), HookBridgeError>;
    /// Writes bytes to stderr.
    ///
    /// # Errors
    ///
    /// Returns an error when stderr cannot be written.
    fn write_stderr(&self, bytes: &[u8]) -> Result<(), HookBridgeError>;
}

#[derive(Debug, Default)]
pub struct StdIo;

impl Io for StdIo {
    fn read_stdin(&self) -> Result<Vec<u8>, HookBridgeError> {
        read_all(std::io::stdin(), "<stdin>", "read_stdin")
    }

    fn write_stdout(&self, bytes: &[u8]) -> Result<(), HookBridgeError> {
        write_all(std::io::stdout().lock(), bytes, "<stdout>", "write_stdout")
    }

    fn write_stderr(&self, bytes: &[u8]) -> Result<(), HookBridgeError> {
        write_all(std::io::stderr().lock(), bytes, "<stderr>", "write_stderr")
    }
}

fn read_all<R>(
    mut reader: R,
    path: &str,
    operation: &'static str,
) -> Result<Vec<u8>, HookBridgeError>
where
    R: Read,
{
    let mut buffer = Vec::new();
    reader
        .read_to_end(&mut buffer)
        .map_err(|error| HookBridgeError::Io {
            operation,
            path: path.into(),
            kind: error.kind(),
        })?;
    Ok(buffer)
}

fn write_all<W>(
    mut writer: W,
    bytes: &[u8],
    path: &str,
    operation: &'static str,
) -> Result<(), HookBridgeError>
where
    W: Write,
{
    writer
        .write_all(bytes)
        .map_err(|error| HookBridgeError::Io {
            operation,
            path: path.into(),
            kind: error.kind(),
        })
}

#[derive(Debug, Default)]
pub struct FakeIo {
    pub stdin: Vec<u8>,
}

impl Io for FakeIo {
    fn read_stdin(&self) -> Result<Vec<u8>, HookBridgeError> {
        Ok(self.stdin.clone())
    }

    fn write_stdout(&self, _bytes: &[u8]) -> Result<(), HookBridgeError> {
        Ok(())
    }

    fn write_stderr(&self, _bytes: &[u8]) -> Result<(), HookBridgeError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::{FakeIo, Io, StdIo, read_all, write_all};
    use crate::error::HookBridgeError;

    struct BrokenReader;

    impl io::Read for BrokenReader {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("broken input"))
        }
    }

    struct BrokenWriter;

    impl io::Write for BrokenWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("broken output"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn fake_io_returns_configured_stdin() {
        let io = FakeIo {
            stdin: b"{}".to_vec(),
        };

        let result = io.read_stdin();

        assert_eq!(result, Ok(b"{}".to_vec()));
    }

    #[test]
    fn fake_io_accepts_stdout_and_stderr_writes() {
        let io = FakeIo::default();

        assert_eq!(io.write_stdout(b"ok"), Ok(()));
        assert_eq!(io.write_stderr(b"err"), Ok(()));
    }

    #[test]
    fn std_io_supports_empty_reads_and_writes() {
        let io = StdIo;

        assert_eq!(io.write_stdout(b""), Ok(()));
        assert_eq!(io.write_stderr(b""), Ok(()));
    }

    #[test]
    fn std_io_error_helpers_carry_stable_stdio_paths() {
        let stdin_result = read_all(BrokenReader, "<stdin>", "read_stdin");
        let stdout_result = write_all(BrokenWriter, b"out", "<stdout>", "write_stdout");
        let stderr_result = write_all(BrokenWriter, b"err", "<stderr>", "write_stderr");

        assert!(matches!(
            stdin_result,
            Err(HookBridgeError::Io {
                operation: "read_stdin",
                path,
                ..
            }) if path.to_string_lossy() == "<stdin>"
        ));
        assert!(matches!(
            stdout_result,
            Err(HookBridgeError::Io {
                operation: "write_stdout",
                path,
                ..
            }) if path.to_string_lossy() == "<stdout>"
        ));
        assert!(matches!(
            stderr_result,
            Err(HookBridgeError::Io {
                operation: "write_stderr",
                path,
                ..
            }) if path.to_string_lossy() == "<stderr>"
        ));
    }
}
