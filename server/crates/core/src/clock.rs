use chrono::{DateTime, Utc};

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TestClock {
    fixed: DateTime<Utc>,
}

impl TestClock {
    #[must_use]
    pub const fn new(fixed: DateTime<Utc>) -> Self {
        Self { fixed }
    }
}

impl Clock for TestClock {
    fn now(&self) -> DateTime<Utc> {
        self.fixed
    }
}
