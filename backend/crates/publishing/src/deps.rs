//! What the publishing engine needs from the rest of the server.

use std::sync::Arc;

use postit_config::PublishingSettings;
use postit_core::{Clock, IdGenerator, PlatformId};
use postit_jobs::JobQueue;
use postit_media::MediaService;
use postit_notify::Notifier;
use postit_oauth::AccountService;
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;

/// The engine's collaborators. The server builds one and hands it to
/// [`crate::PublishingService::new`].
pub struct PublishingDeps {
    /// The database.
    pub pool: PgPool,
    /// Binds platform clients to accounts and marks grants that need a reconnect.
    pub accounts: AccountService,
    /// Resolves a variant's media IDs to assets.
    pub media: MediaService,
    /// Records notifications in the engine's transactions.
    pub notifier: Notifier,
    /// Enqueues the engine's own jobs.
    pub queue: JobQueue,
    /// New row IDs.
    pub ids: Arc<dyn IdGenerator>,
    /// The time source.
    pub clock: Arc<dyn Clock>,
    /// `[publishing]`: attempts, lease, and grace periods.
    pub settings: PublishingSettings,
    /// Cancelled when the server shuts down; a running delivery stops at its next safe point.
    pub shutdown: CancellationToken,
    /// The platforms whose agents this build includes.
    pub compiled_platforms: Vec<PlatformId>,
}
