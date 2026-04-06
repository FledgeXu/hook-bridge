use crate::cli::{Cli, Command};
use crate::error::HookBridgeError;
use crate::generate;
use crate::run;
use crate::runtime::Runtime;

pub struct App<R: Runtime> {
    runtime: R,
}

impl<R: Runtime> App<R> {
    #[must_use]
    pub const fn new(runtime: R) -> Self {
        Self { runtime }
    }

    /// Executes the selected CLI command against the provided runtime.
    ///
    /// # Errors
    ///
    /// Returns any command execution error from the `generate` or `run` flow.
    pub fn execute(&self, cli: Cli) -> Result<u8, HookBridgeError> {
        match cli.command {
            Command::Generate(args) => generate::execute(&args, &self.runtime).map(|()| 0),
            Command::Run(args) => run::execute(&args, &self.runtime),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::cli::{Cli, Command, GenerateArgs, RunArgs};
    use crate::runtime::Runtime;
    use crate::runtime::clock::{Clock, FixedClock};
    use crate::runtime::fs::{FakeFileSystem, FileSystem};
    use crate::runtime::io::{FakeIo, Io};
    use crate::runtime::process::{FakeProcessRunner, ProcessRunner};

    use super::App;

    struct TestRuntime {
        fs: FakeFileSystem,
        clock: FixedClock,
        process: FakeProcessRunner,
        io: FakeIo,
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
            std::env::temp_dir()
        }
    }

    fn test_app() -> App<TestRuntime> {
        App::new(TestRuntime {
            fs: FakeFileSystem::default(),
            clock: FixedClock::new(std::time::SystemTime::UNIX_EPOCH),
            process: FakeProcessRunner::success(0),
            io: FakeIo::default(),
        })
    }

    #[test]
    fn app_routes_generate_command() {
        let app = test_app();
        let cli = Cli {
            command: Command::Generate(GenerateArgs {
                config: "hook-bridge.yaml".into(),
                platform: None,
                force: false,
                yes: false,
            }),
        };

        let result = app.execute(cli);

        assert!(result.is_err());
    }

    #[test]
    fn app_routes_run_command() {
        let app = test_app();
        let cli = Cli {
            command: Command::Run(RunArgs {
                platform: crate::platform::Platform::Claude,
                rule_id: "rule_1".to_string(),
            }),
        };

        let result = app.execute(cli);

        assert!(result.is_err());
    }

    #[test]
    fn test_runtime_exposes_all_dependencies() {
        let runtime = TestRuntime {
            fs: FakeFileSystem::default(),
            clock: FixedClock::new(std::time::SystemTime::UNIX_EPOCH),
            process: FakeProcessRunner::success(0),
            io: FakeIo::default(),
        };

        assert!(matches!(runtime.fs().exists(Path::new(".")), Ok(false)));
        assert_eq!(runtime.clock().now(), std::time::SystemTime::UNIX_EPOCH);
        assert_eq!(runtime.temp_dir(), std::env::temp_dir());
        assert!(
            runtime
                .process_runner()
                .run(&crate::runtime::process::ProcessRequest {
                    program: "echo".to_string(),
                    args: vec!["ok".to_string()],
                    stdin: Vec::new(),
                    timeout: std::time::Duration::from_secs(1),
                    cwd: None,
                    env: std::collections::BTreeMap::new(),
                })
                .is_ok()
        );
        assert_eq!(runtime.io().read_stdin(), Ok(Vec::new()));
    }
}
