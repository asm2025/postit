//! Wire types of the target group and campaign routes.

use chrono::{DateTime, NaiveDate, Utc};
use postit_data::campaigns::{Campaign, CampaignFields, CampaignPatch};
use postit_data::target_groups::TargetGroup;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::dto_contacts::present;

/// A named set of accounts a post can be sent to in one pick. A delegate with a limited
/// account list sees only the members inside the limit.
#[derive(Debug, Serialize, ToSchema)]
pub struct TargetGroupDto {
    /// The group.
    pub id: Uuid,
    /// Its name.
    pub name: String,
    /// Its member accounts the caller can see.
    pub account_ids: Vec<Uuid>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<TargetGroup> for TargetGroupDto {
    fn from(group: TargetGroup) -> Self {
        Self {
            id: group.id.as_uuid(),
            name: group.name,
            account_ids: group.accounts.into_iter().map(|a| a.as_uuid()).collect(),
            updated_at: group.updated_at,
        }
    }
}

/// A target group to create, or the whole group a `PUT` stores.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TargetGroupRequest {
    /// Its name.
    pub name: String,
    /// Its member accounts. A delegate with a limited account list keeps the members they
    /// cannot see; naming one they cannot see is `account_not_in_scope`.
    #[serde(default)]
    pub account_ids: Vec<Uuid>,
}

/// A campaign.
#[derive(Debug, Serialize, ToSchema)]
pub struct CampaignDto {
    /// The campaign.
    pub id: Uuid,
    /// Its name.
    pub name: String,
    /// The first day it runs.
    pub starts_on: Option<NaiveDate>,
    /// The last day it runs.
    pub ends_on: Option<NaiveDate>,
    /// A display color, as the client chose it.
    pub color: Option<String>,
    /// Whether it is hidden from the default list.
    pub archived: bool,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<Campaign> for CampaignDto {
    fn from(c: Campaign) -> Self {
        Self {
            id: c.id.as_uuid(),
            name: c.name,
            starts_on: c.starts_on,
            ends_on: c.ends_on,
            color: c.color,
            archived: c.archived,
            created_at: c.created_at,
            updated_at: c.updated_at,
        }
    }
}

/// A campaign to create.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateCampaignRequest {
    /// Its name.
    pub name: String,
    /// The first day it runs.
    #[serde(default)]
    pub starts_on: Option<NaiveDate>,
    /// The last day it runs; not before `starts_on` (`validation_failed`, `field: "ends_on"`).
    #[serde(default)]
    pub ends_on: Option<NaiveDate>,
    /// A display color.
    #[serde(default)]
    pub color: Option<String>,
}

impl From<CreateCampaignRequest> for CampaignFields {
    fn from(body: CreateCampaignRequest) -> Self {
        Self {
            name: body.name,
            starts_on: body.starts_on,
            ends_on: body.ends_on,
            color: body.color,
        }
    }
}

/// Changes to a campaign. Absent fields stay as they are; `null` clears a date or the color.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchCampaignRequest {
    /// A new name.
    pub name: Option<String>,
    /// A new first day, or `null` to clear it.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<NaiveDate>)]
    pub starts_on: Option<Option<NaiveDate>>,
    /// A new last day, or `null` to clear it.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<NaiveDate>)]
    pub ends_on: Option<Option<NaiveDate>>,
    /// A new display color, or `null` to clear it.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    pub color: Option<Option<String>>,
    /// Archive or restore the campaign.
    pub archived: Option<bool>,
}

impl PatchCampaignRequest {
    /// The field changes, without `archived`.
    #[must_use]
    pub fn patch(&self) -> CampaignPatch {
        CampaignPatch {
            name: self.name.clone(),
            starts_on: self.starts_on,
            ends_on: self.ends_on,
            color: self.color.clone(),
        }
    }
}

/// Flat on purpose (see `UserListQuery`).
#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct CampaignListQuery {
    /// Include archived campaigns (default false).
    pub include_archived: Option<bool>,
    /// 1-based page number (default 1).
    pub page: Option<u64>,
    /// Items per page, 1-100 (default 20).
    pub page_size: Option<u64>,
}
