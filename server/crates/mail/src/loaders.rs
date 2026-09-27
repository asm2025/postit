use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use postit_core::UserId;
use postit_data::users::UserRecord;
use sqlx::PgConnection;

use crate::error::MailError;
use crate::outbox::{MailKind, MailParams};
use crate::templates::MailContent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadOutcome {
    Send(MailContent),
    /// Nothing to say (or already covered by an earlier mail). No mail, no audit event.
    Skip,
    /// There is content, but it may not be sent before this time; the handler re-enqueues.
    Defer(DateTime<Utc>),
}

/// Loads a mail kind's content from current data, by ID, when `send_email` runs.
/// Implemented by the crate that owns the data (`postit-identity`; plan 03's `notify`).
#[async_trait]
pub trait MailContextLoader: Send + Sync {
    /// Called inside the handler's transaction, with `recipient`'s `users` row locked.
    async fn load(
        &self,
        conn: &mut PgConnection,
        recipient: &UserRecord,
        params: &MailParams,
        now: DateTime<Utc>,
    ) -> Result<LoadOutcome, MailError>;

    /// Records that this kind was sent to `recipient` at `at`, in the same transaction,
    /// after the SMTP server accepted the message.
    async fn mark_sent(
        &self,
        conn: &mut PgConnection,
        recipient: UserId,
        at: DateTime<Utc>,
    ) -> Result<(), MailError>;
}

#[derive(Clone, Default)]
pub struct MailLoaders {
    loaders: HashMap<MailKind, Arc<dyn MailContextLoader>>,
}

impl MailLoaders {
    /// # Errors
    ///
    /// Returns [`MailError::DuplicateLoader`] if `kind` already has a loader.
    pub fn register(
        &mut self,
        kind: MailKind,
        loader: Arc<dyn MailContextLoader>,
    ) -> Result<(), MailError> {
        if self.loaders.contains_key(&kind) {
            return Err(MailError::DuplicateLoader(kind));
        }
        self.loaders.insert(kind, loader);
        Ok(())
    }

    #[must_use]
    pub fn kinds(&self) -> Vec<MailKind> {
        let mut kinds: Vec<MailKind> = self.loaders.keys().copied().collect();
        kinds.sort_by_key(|k| k.as_str());
        kinds
    }

    pub(crate) fn get(&self, kind: MailKind) -> Option<Arc<dyn MailContextLoader>> {
        self.loaders.get(&kind).cloned()
    }
}
