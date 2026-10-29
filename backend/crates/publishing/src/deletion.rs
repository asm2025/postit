//! The publishing step of user deletion.

use async_trait::async_trait;
use postit_core::UserId;
use postit_data::publishing::{CancelOutcome, DeliveryRepo};
use postit_identity::deletion::UserDeletionStep;
use postit_jobs::JobError;

use crate::PublishingService;

/// Stops a deleted user's publishing before their grants are revoked and their rows go: it
/// cancels their pending, claimed, and parked deliveries and running direct campaigns,
/// requests a stop on in-flight ones, and deletes their natively scheduled uploads through
/// the agents (as a disconnect does). It asks for a retry while any of their deliveries is
/// claimed or in flight, so no worker still uses a credential or a media file when the next
/// steps run. Repeating it changes nothing more.
pub struct PublishingDeletionStep {
    service: PublishingService,
}

impl PublishingDeletionStep {
    /// The step over `service`.
    #[must_use]
    pub fn new(service: PublishingService) -> Self {
        Self { service }
    }
}

fn retry(err: impl std::fmt::Display) -> JobError {
    JobError::Retry(err.to_string())
}

#[async_trait]
impl UserDeletionStep for PublishingDeletionStep {
    fn name(&self) -> &'static str {
        "publishing"
    }

    async fn run(&self, user: UserId) -> Result<(), JobError> {
        let deps = &self.service.deps;
        let mut tx = deps.pool.begin().await.map_err(retry)?;
        let results = DeliveryRepo::cancel_for_owner_with_status(&mut tx, user, deps.clock.now())
            .await
            .map_err(retry)?;
        self.service
            .record_cancel_partials(&mut tx, user, &results)
            .await
            .map_err(retry)?;
        tx.commit().await.map_err(retry)?;
        let count =
            |outcome: CancelOutcome| results.iter().filter(|r| r.outcome == outcome).count();

        let natives = self
            .service
            .delete_natives(user, None)
            .await
            .map_err(retry)?;
        tracing::info!(
            cancelled = count(CancelOutcome::Cancelled),
            requested = count(CancelOutcome::Requested),
            natives_deleted = natives.deleted,
            natives_kept = natives.kept,
            natives_transient = natives.transient,
            "publishing of a deleted user stopped"
        );
        if natives.kept > 0 {
            tracing::warn!(
                remaining = natives.kept,
                "natively scheduled uploads of a deleted user could not be deleted on the platform"
            );
        }
        if natives.transient > 0 {
            // The grants are revoked and the user's rows deleted after this step, so an upload
            // left now would go live with no record: try again while the credentials work.
            return Err(JobError::Retry(format!(
                "{} natively scheduled upload(s) of the user could not be deleted yet",
                natives.transient
            )));
        }

        let mut conn = deps.pool.acquire().await.map_err(retry)?;
        if DeliveryRepo::owner_has_in_flight(&mut conn, user)
            .await
            .map_err(retry)?
        {
            return Err(JobError::Retry(
                "a delivery of the user is still being published".to_owned(),
            ));
        }
        Ok(())
    }
}
