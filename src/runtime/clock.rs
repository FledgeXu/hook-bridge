use std::time::SystemTime;

pub trait Clock {
    fn now(&self) -> SystemTime;
}

#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

#[derive(Debug)]
pub struct FixedClock {
    now: SystemTime,
}

impl FixedClock {
    #[must_use]
    pub const fn new(now: SystemTime) -> Self {
        Self { now }
    }
}

impl Clock for FixedClock {
    fn now(&self) -> SystemTime {
        self.now
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::{Clock, FixedClock};

    #[test]
    fn fixed_clock_returns_stable_time() {
        let expected = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
        let clock = FixedClock::new(expected);
        assert_eq!(clock.now(), expected);
    }
}
