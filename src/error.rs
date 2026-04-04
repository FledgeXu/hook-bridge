use std::io::ErrorKind;
use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HookBridgeError {
    #[error("parameter error: {message}")]
    Parameter { message: String },

    #[error("config validation error: {message}")]
    ConfigValidation { message: String },

    #[error("file conflict: {path}")]
    FileConflict { path: PathBuf },

    #[error("json parse error: {message}")]
    JsonParse { message: String },

    #[error("process error: {message}")]
    Process { message: String },

    #[error("timeout after {timeout_sec}s")]
    Timeout { timeout_sec: u64 },

    #[error("platform protocol error: {message}")]
    PlatformProtocol { message: String },

    #[error("io error during {operation} ({kind:?}) on {path}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        kind: ErrorKind,
    },

    #[error("feature not implemented yet: {feature}")]
    NotImplemented { feature: &'static str },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ExitCodeKind {
    Success = 0,
    Parameter = 2,
    ConfigValidation = 3,
    FileConflict = 4,
    JsonParse = 5,
    Process = 6,
    Timeout = 7,
    PlatformProtocol = 8,
    NotImplemented = 9,
    Io = 10,
}

#[must_use]
pub const fn exit_code_for_error(error: &HookBridgeError) -> u8 {
    match error {
        HookBridgeError::Parameter { .. } => ExitCodeKind::Parameter as u8,
        HookBridgeError::ConfigValidation { .. } => ExitCodeKind::ConfigValidation as u8,
        HookBridgeError::FileConflict { .. } => ExitCodeKind::FileConflict as u8,
        HookBridgeError::JsonParse { .. } => ExitCodeKind::JsonParse as u8,
        HookBridgeError::Process { .. } => ExitCodeKind::Process as u8,
        HookBridgeError::Timeout { .. } => ExitCodeKind::Timeout as u8,
        HookBridgeError::PlatformProtocol { .. } => ExitCodeKind::PlatformProtocol as u8,
        HookBridgeError::NotImplemented { .. } => ExitCodeKind::NotImplemented as u8,
        HookBridgeError::Io { .. } => ExitCodeKind::Io as u8,
    }
}

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;
    use std::path::PathBuf;

    use super::{ExitCodeKind, HookBridgeError, exit_code_for_error};

    #[test]
    fn maps_not_implemented_to_stable_exit_code() {
        let error = HookBridgeError::NotImplemented { feature: "run" };
        assert_eq!(
            exit_code_for_error(&error),
            ExitCodeKind::NotImplemented as u8
        );
    }

    #[test]
    fn maps_every_error_variant_to_stable_exit_code() {
        let cases = [
            (
                HookBridgeError::Parameter {
                    message: "bad flag".to_string(),
                },
                ExitCodeKind::Parameter as u8,
            ),
            (
                HookBridgeError::ConfigValidation {
                    message: "invalid yaml".to_string(),
                },
                ExitCodeKind::ConfigValidation as u8,
            ),
            (
                HookBridgeError::FileConflict {
                    path: PathBuf::from("a.json"),
                },
                ExitCodeKind::FileConflict as u8,
            ),
            (
                HookBridgeError::JsonParse {
                    message: "invalid json".to_string(),
                },
                ExitCodeKind::JsonParse as u8,
            ),
            (
                HookBridgeError::Process {
                    message: "spawn failed".to_string(),
                },
                ExitCodeKind::Process as u8,
            ),
            (
                HookBridgeError::Timeout { timeout_sec: 1 },
                ExitCodeKind::Timeout as u8,
            ),
            (
                HookBridgeError::PlatformProtocol {
                    message: "unknown event".to_string(),
                },
                ExitCodeKind::PlatformProtocol as u8,
            ),
            (
                HookBridgeError::Io {
                    operation: "read",
                    path: PathBuf::from("<stdin>"),
                    kind: ErrorKind::UnexpectedEof,
                },
                ExitCodeKind::Io as u8,
            ),
        ];

        for (error, expected_code) in cases {
            assert_eq!(exit_code_for_error(&error), expected_code);
        }
    }
}
