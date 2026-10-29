//! The media step of user deletion.

use std::sync::Arc;

use async_trait::async_trait;
use postit_core::UserId;
use postit_identity::deletion::UserDeletionStep;
use postit_jobs::JobError;

use crate::storage::FsMediaStore;

/// Removes a deleted user's stored files by owner, not by listing rows: their whole key tree
/// (`{owner}/{yyyy}/{mm}/{asset}` and the `.thumb.jpg` beside each), which also holds the
/// bytes of their unfinished uploads. Upload chunks in the parts directory are named by a
/// fresh ID per request, not by owner; the request removes its own, and `media_gc` sweeps
/// what a crashed request left. Runs after the publishing step, so no delivery still reads a
/// file. A missing directory means done, so repeating it is harmless.
pub struct MediaDeletionStep {
    store: Arc<FsMediaStore>,
}

impl MediaDeletionStep {
    /// The step over `store`.
    #[must_use]
    pub fn new(store: Arc<FsMediaStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl UserDeletionStep for MediaDeletionStep {
    fn name(&self) -> &'static str {
        "media"
    }

    async fn run(&self, user: UserId) -> Result<(), JobError> {
        let removed = self
            .store
            .remove_owner_files(user)
            .await
            .map_err(|err| JobError::Retry(err.to_string()))?;
        tracing::info!(%user, removed, "media files of a deleted user removed");
        Ok(())
    }
}
