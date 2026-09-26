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

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn system_clock_reports_a_time_close_to_now() {
        let before = Utc::now();
        let observed = SystemClock.now();
        let after = Utc::now();

        assert!(observed >= before);
        assert!(observed <= after);
    }

    #[test]
    fn test_clock_always_returns_the_fixed_time() {
        let fixed = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let clock = TestClock::new(fixed);

        assert_eq!(clock.now(), fixed);
        assert_eq!(clock.now(), fixed);
    }
}
