use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! id_newtype {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
        )]
        #[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Wraps a raw `Uuid`.
            #[must_use]
            pub const fn from_uuid(id: Uuid) -> Self {
                Self(id)
            }

            /// The underlying `Uuid`.
            #[must_use]
            pub const fn as_uuid(&self) -> Uuid {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl From<Uuid> for $name {
            fn from(id: Uuid) -> Self {
                Self(id)
            }
        }

        impl From<$name> for Uuid {
            fn from(id: $name) -> Self {
                id.0
            }
        }
    };
}

id_newtype!(UserId, "Identifier of a user.");
id_newtype!(AccountId, "Identifier of a connected social account.");
id_newtype!(PostId, "Identifier of a post.");
id_newtype!(AuditEventId, "Identifier of an audit event.");
id_newtype!(MediaAssetId, "Identifier of a stored media asset.");
id_newtype!(
    PublishJobId,
    "Identifier of a publish job (one post sent to several targets)."
);
id_newtype!(
    DeliveryId,
    "Identifier of one delivery of a publish job to one target account."
);
id_newtype!(
    DirectSendId,
    "Identifier of one direct send to one recipient."
);
id_newtype!(CampaignId, "Identifier of an email campaign.");
id_newtype!(GrantId, "Identifier of a credential grant.");
id_newtype!(TargetGroupId, "Identifier of a target group.");
id_newtype!(RecurrenceId, "Identifier of a recurrence.");
id_newtype!(ContactId, "Identifier of an email contact.");
id_newtype!(ContactListId, "Identifier of a contact list.");
id_newtype!(ContactImportId, "Identifier of a contact import.");
id_newtype!(DeliveryAttemptId, "Identifier of one delivery attempt.");
id_newtype!(InboxItemId, "Identifier of an inbox item.");
id_newtype!(AiProviderId, "Identifier of a configured AI provider.");
id_newtype!(AiGenerationId, "Identifier of one AI generation.");
id_newtype!(NotificationId, "Identifier of a notification.");
id_newtype!(PushEndpointId, "Identifier of a push endpoint.");
id_newtype!(DelegationId, "Identifier of a delegation.");
id_newtype!(WritingStyleId, "Identifier of a writing style.");
id_newtype!(BrandProfileId, "Identifier of a brand profile.");
id_newtype!(OAuthStateId, "Identifier of a pending OAuth authorization.");
id_newtype!(
    PendingConnectionId,
    "Identifier of a connection waiting for the user to pick accounts."
);
