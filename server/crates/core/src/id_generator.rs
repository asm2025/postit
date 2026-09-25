use std::sync::atomic::{AtomicU64, Ordering};

use uuid::Uuid;

pub trait IdGenerator: Send + Sync {
    fn generate(&self) -> Uuid;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemIdGenerator;

impl IdGenerator for SystemIdGenerator {
    fn generate(&self) -> Uuid {
        Uuid::now_v7()
    }
}

/// Deterministic generator for tests: increments a counter into the UUID's random field
/// so successive ids are stable and ordered without relying on wall-clock time.
#[derive(Debug, Default)]
pub struct TestIdGenerator {
    counter: AtomicU64,
}

impl TestIdGenerator {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            counter: AtomicU64::new(0),
        }
    }
}

impl IdGenerator for TestIdGenerator {
    fn generate(&self) -> Uuid {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        Uuid::from_u128(u128::from(n))
    }
}
