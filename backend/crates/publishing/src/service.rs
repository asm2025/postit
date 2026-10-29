//! The publishing service.

use std::sync::Arc;

use crate::PublishingDeps;

/// The publishing engine. Cheap to clone.
#[derive(Clone)]
pub struct PublishingService {
    #[allow(
        dead_code,
        reason = "the job handlers that read it arrive in later tasks"
    )]
    pub(crate) deps: Arc<PublishingDeps>,
}

impl PublishingService {
    /// A service over `deps`.
    #[must_use]
    pub fn new(deps: PublishingDeps) -> Self {
        Self {
            deps: Arc::new(deps),
        }
    }
}
