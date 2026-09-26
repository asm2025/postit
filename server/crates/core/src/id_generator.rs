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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn system_id_generator_produces_unique_v7_uuids() {
        let generator = SystemIdGenerator;
        let a = generator.generate();
        let b = generator.generate();

        assert_ne!(a, b);
        assert_eq!(a.get_version_num(), 7);
        assert_eq!(b.get_version_num(), 7);
    }

    #[test]
    fn test_id_generator_is_deterministic_and_ordered() {
        let generator = TestIdGenerator::new();
        let first = generator.generate();
        let second = generator.generate();
        let third = generator.generate();

        assert!(first < second);
        assert!(second < third);
    }

    #[test]
    fn test_id_generator_never_repeats_across_many_calls() {
        let generator = TestIdGenerator::new();
        let mut seen = HashSet::new();

        for _ in 0..1000 {
            assert!(seen.insert(generator.generate()));
        }
    }
}
