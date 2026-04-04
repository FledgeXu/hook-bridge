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
        let mut buffer = Vec::new();
        std::io::stdin()
            .read_to_end(&mut buffer)
            .map_err(|error| HookBridgeError::Io {
                operation: "read_stdin",
                path: "<stdin>".into(),
                kind: error.kind(),
            })?;
        Ok(buffer)
    }

    fn write_stdout(&self, bytes: &[u8]) -> Result<(), HookBridgeError> {
        std::io::stdout()
            .lock()
            .write_all(bytes)
            .map_err(|error| HookBridgeError::Io {
                operation: "write_stdout",
                path: "<stdout>".into(),
                kind: error.kind(),
            })
    }

    fn write_stderr(&self, bytes: &[u8]) -> Result<(), HookBridgeError> {
        std::io::stderr()
            .lock()
            .write_all(bytes)
            .map_err(|error| HookBridgeError::Io {
                operation: "write_stderr",
                path: "<stderr>".into(),
                kind: error.kind(),
            })
    }
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
    use super::{FakeIo, Io};

    #[test]
    fn fake_io_returns_configured_stdin() {
        let io = FakeIo {
            stdin: b"{}".to_vec(),
        };

        let result = io.read_stdin();

        assert_eq!(result, Ok(b"{}".to_vec()));
    }
}
