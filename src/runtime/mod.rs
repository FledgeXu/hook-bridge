pub mod clock;
pub mod fs;
pub mod io;
pub mod process;

use std::path::PathBuf;

use clock::{Clock, SystemClock};
use fs::{FileSystem, OsFileSystem};
use io::{Io, StdIo};
use process::{ProcessRunner, SystemProcessRunner};

pub trait Runtime {
    fn fs(&self) -> &dyn FileSystem;
    fn clock(&self) -> &dyn Clock;
    fn process_runner(&self) -> &dyn ProcessRunner;
    fn io(&self) -> &dyn Io;
    fn temp_dir(&self) -> PathBuf;
}

#[derive(Debug, Default)]
pub struct RealRuntime {
    filesystem: OsFileSystem,
    clock: SystemClock,
    process_runner: SystemProcessRunner,
    io: StdIo,
}

impl Runtime for RealRuntime {
    fn fs(&self) -> &dyn FileSystem {
        &self.filesystem
    }

    fn clock(&self) -> &dyn Clock {
        &self.clock
    }

    fn process_runner(&self) -> &dyn ProcessRunner {
        &self.process_runner
    }

    fn io(&self) -> &dyn Io {
        &self.io
    }

    fn temp_dir(&self) -> PathBuf {
        std::env::temp_dir()
    }
}
