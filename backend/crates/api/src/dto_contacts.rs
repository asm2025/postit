//! Wire types of the contacts, lists, imports, and suppression routes. A suppression shows the
//! address only while a contact with it exists, never its hash.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use postit_contacts::{ContactPatch, ImportOptions, NewContactInput, SettableStatus};
use postit_data::contacts::{
    Contact, ContactImport, ContactList, ContactStatus, ImportStatus, Suppression,
    SuppressionReason,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{ApiError, ErrorCode};

/// Whether a contact may be emailed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContactStatusDto {
    /// May be emailed.
    Subscribed,
    /// Opted out.
    Unsubscribed,
    /// The address bounced. Set by the system only.
    Bounced,
    /// The recipient reported spam. Set by the system only.
    Complained,
}

impl From<ContactStatus> for ContactStatusDto {
    fn from(status: ContactStatus) -> Self {
        match status {
            ContactStatus::Subscribed => Self::Subscribed,
            ContactStatus::Unsubscribed => Self::Unsubscribed,
            ContactStatus::Bounced => Self::Bounced,
            ContactStatus::Complained => Self::Complained,
        }
    }
}

impl From<ContactStatusDto> for ContactStatus {
    fn from(status: ContactStatusDto) -> Self {
        match status {
            ContactStatusDto::Subscribed => Self::Subscribed,
            ContactStatusDto::Unsubscribed => Self::Unsubscribed,
            ContactStatusDto::Bounced => Self::Bounced,
            ContactStatusDto::Complained => Self::Complained,
        }
    }
}

/// An email contact.
#[derive(Debug, Serialize, ToSchema)]
pub struct ContactDto {
    /// Contact ID.
    pub id: Uuid,
    /// The address, normalized (trimmed, lowercase).
    pub email: String,
    /// Given name.
    pub first_name: Option<String>,
    /// Family name.
    pub last_name: Option<String>,
    /// Custom merge fields.
    #[schema(value_type = Object)]
    pub fields: Value,
    /// Tags.
    pub tags: Vec<String>,
    /// The contact's language.
    pub locale: Option<String>,
    /// Where the contact's consent came from.
    pub consent_source: Option<String>,
    /// When consent was given.
    pub consent_at: Option<DateTime<Utc>>,
    /// Whether the contact may be emailed.
    pub status: ContactStatusDto,
    /// When the status last changed.
    pub status_changed_at: DateTime<Utc>,
    /// When the contact was added.
    pub created_at: DateTime<Utc>,
    /// When the contact last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<Contact> for ContactDto {
    fn from(c: Contact) -> Self {
        Self {
            id: c.id.as_uuid(),
            email: c.email,
            first_name: c.first_name,
            last_name: c.last_name,
            fields: c.fields,
            tags: c.tags,
            locale: c.locale,
            consent_source: c.consent_source,
            consent_at: c.consent_at,
            status: c.status.into(),
            status_changed_at: c.status_changed_at,
            created_at: c.created_at,
            updated_at: c.updated_at,
        }
    }
}

/// A contact to add. An address already in the book updates that contact instead: fields
/// given here replace the old values, custom `fields` merge key by key, and the rest stays.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateContactRequest {
    /// The address.
    pub email: String,
    /// Given name.
    pub first_name: Option<String>,
    /// Family name.
    pub last_name: Option<String>,
    /// Custom merge fields.
    #[schema(value_type = Option<Object>)]
    pub fields: Option<Map<String, Value>>,
    /// Tags.
    pub tags: Option<Vec<String>>,
    /// The contact's language.
    pub locale: Option<String>,
    /// Where the contact's consent came from ("signup form", "customer", "event"). Required.
    pub consent_source: String,
    /// When consent was given; defaults to now.
    pub consent_at: Option<DateTime<Utc>>,
}

impl From<CreateContactRequest> for NewContactInput {
    fn from(r: CreateContactRequest) -> Self {
        Self {
            email: r.email,
            first_name: r.first_name,
            last_name: r.last_name,
            fields: r.fields,
            tags: r.tags,
            locale: r.locale,
            consent_source: r.consent_source,
            consent_at: r.consent_at,
        }
    }
}

#[allow(
    clippy::option_option,
    reason = "absent, null, and a value are three different requests"
)]
fn present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// Changes to a contact. Absent fields stay as they are; the address never changes.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateContactRequest {
    /// A new given name, or `null` to clear it.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    pub first_name: Option<Option<String>>,
    /// A new family name, or `null` to clear it.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    pub last_name: Option<Option<String>>,
    /// The custom merge fields, replacing the old ones.
    #[schema(value_type = Option<Object>)]
    pub fields: Option<Map<String, Value>>,
    /// The tags, replacing the old ones.
    pub tags: Option<Vec<String>>,
    /// A new language, or `null` to clear it.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    pub locale: Option<Option<String>>,
    /// Where consent came from; cannot be blank.
    pub consent_source: Option<String>,
    /// When consent was given.
    pub consent_at: Option<DateTime<Utc>>,
    /// `subscribed` or `unsubscribed`; `bounced` and `complained` answer
    /// `status_not_settable`. Subscribing a contact the owner unsubscribed is owner-only.
    pub status: Option<ContactStatusDto>,
}

impl UpdateContactRequest {
    /// The service's patch.
    ///
    /// # Errors
    ///
    /// `status_not_settable` for `bounced` or `complained`.
    pub fn into_patch(self) -> Result<ContactPatch, ApiError> {
        let status = match self.status {
            None => None,
            Some(ContactStatusDto::Subscribed) => Some(SettableStatus::Subscribed),
            Some(ContactStatusDto::Unsubscribed) => Some(SettableStatus::Unsubscribed),
            Some(ContactStatusDto::Bounced | ContactStatusDto::Complained) => {
                return Err(ApiError::new(ErrorCode::StatusNotSettable));
            }
        };
        Ok(ContactPatch {
            first_name: self.first_name,
            last_name: self.last_name,
            fields: self.fields.map(Value::Object),
            tags: self.tags,
            locale: self.locale,
            consent_source: self.consent_source,
            consent_at: self.consent_at,
            status,
        })
    }
}

/// A contact list.
#[derive(Debug, Serialize, ToSchema)]
pub struct ContactListDto {
    /// List ID.
    pub id: Uuid,
    /// The list's name.
    pub name: String,
    /// How many contacts are in it.
    pub members: i64,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<ContactList> for ContactListDto {
    fn from(l: ContactList) -> Self {
        Self {
            id: l.id.as_uuid(),
            name: l.name,
            members: l.members,
            created_at: l.created_at,
            updated_at: l.updated_at,
        }
    }
}

/// A list's name, to create or rename it.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ContactListRequest {
    /// The name; cannot be blank.
    pub name: String,
}

/// Why an address is on the suppression list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SuppressionReasonDto {
    /// The recipient opted out.
    Unsubscribed,
    /// The address bounced. The only kind the owner can remove.
    Bounced,
    /// The recipient reported spam.
    Complained,
    /// The owner unsubscribed the contact.
    Manual,
}

impl From<SuppressionReason> for SuppressionReasonDto {
    fn from(reason: SuppressionReason) -> Self {
        match reason {
            SuppressionReason::Unsubscribed => Self::Unsubscribed,
            SuppressionReason::Bounced => Self::Bounced,
            SuppressionReason::Complained => Self::Complained,
            SuppressionReason::Manual => Self::Manual,
        }
    }
}

/// An address that is never emailed.
#[derive(Debug, Serialize, ToSchema)]
pub struct SuppressionDto {
    /// Entry ID.
    pub id: Uuid,
    /// Why it is suppressed.
    pub reason: SuppressionReasonDto,
    /// When it was suppressed.
    pub at: DateTime<Utc>,
    /// The contact with this address, while one exists.
    pub contact_id: Option<Uuid>,
    /// That contact's address, while it exists.
    pub email: Option<String>,
}

impl From<Suppression> for SuppressionDto {
    fn from(s: Suppression) -> Self {
        Self {
            id: s.id.as_uuid(),
            reason: s.reason.into(),
            at: s.at,
            contact_id: s.contact_id.map(|id| id.as_uuid()),
            email: s.email,
        }
    }
}

/// Where a contact import is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContactImportStatusDto {
    /// Rows are still being applied.
    Processing,
    /// Finished.
    Completed,
    /// Stopped without finishing.
    Failed,
}

impl From<ImportStatus> for ContactImportStatusDto {
    fn from(status: ImportStatus) -> Self {
        match status {
            ImportStatus::Processing => Self::Processing,
            ImportStatus::Completed => Self::Completed,
            ImportStatus::Failed => Self::Failed,
        }
    }
}

/// A row an import did not turn into a contact.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ContactImportRejectionDto {
    /// The row's position in the file; the header is row 1.
    pub row: i64,
    /// Why: `email_invalid`, `consent_missing`, `consent_at_invalid`, `row_malformed`,
    /// `duplicate_in_file`, or `contact_limit_exceeded`.
    pub code: String,
}

/// Which rows were rejected and why. Row numbers and codes only, never an address.
#[derive(Debug, Default, Serialize, Deserialize, ToSchema)]
pub struct ContactImportReportDto {
    /// The first 1,000 rejected rows.
    #[serde(default)]
    pub entries: Vec<ContactImportRejectionDto>,
    /// Every reject, counted by code.
    #[serde(default)]
    pub totals: BTreeMap<String, i64>,
}

/// A contact import, with its live counts.
#[derive(Debug, Serialize, ToSchema)]
pub struct ContactImportDto {
    /// Import ID.
    pub id: Uuid,
    /// Where it is.
    pub status: ContactImportStatusDto,
    /// How many data rows the file had.
    pub rows_total: i32,
    /// How many rows became or updated contacts so far.
    pub rows_ok: i32,
    /// How many rows were rejected so far.
    pub rows_rejected: i32,
    /// The rejected rows.
    pub report: ContactImportReportDto,
    /// The list the import adds its contacts to, until that list is deleted.
    pub list_id: Option<Uuid>,
    /// When the owner attested consent for the addresses.
    pub attested_at: DateTime<Utc>,
    /// When the import started.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<ContactImport> for ContactImportDto {
    fn from(i: ContactImport) -> Self {
        Self {
            id: i.id.as_uuid(),
            status: i.status.into(),
            rows_total: i.rows_total,
            rows_ok: i.rows_ok,
            rows_rejected: i.rows_rejected,
            report: serde_json::from_value(i.report).unwrap_or_default(),
            list_id: i.list_id.map(|id| id.as_uuid()),
            attested_at: i.attested_at,
            created_at: i.created_at,
            updated_at: i.updated_at,
        }
    }
}

/// The parts of `POST /contacts/import`, in this order.
#[derive(ToSchema)]
#[allow(
    dead_code,
    reason = "documents the multipart body; the handler reads it as a stream"
)]
pub struct ContactImportForm {
    /// The import options, as JSON. Must come first.
    pub options: ImportOptions,
    /// The CSV file: UTF-8, comma or semicolon separated, with a header row.
    #[schema(value_type = String, format = Binary)]
    pub file: Vec<u8>,
}
