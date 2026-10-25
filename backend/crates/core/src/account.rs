//! A connected account addressed together with its platform.

use serde::{Deserialize, Serialize};

use crate::{AccountId, PlatformId};

/// A connected account and the platform it lives on.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AccountRef {
    /// The platform the account belongs to.
    pub platform: PlatformId,
    /// The account.
    pub account: AccountId,
    /// The account's ID on the platform (`connected_accounts.remote_account_id`).
    pub remote_id: String,
}
