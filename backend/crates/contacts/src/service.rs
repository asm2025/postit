//! The contact service.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use emixdb::dto::{Pagination, ResultSet};
use lettre::Address;
use postit_config::ContactsSettings;
use postit_core::{AuditEventId, Clock, ContactId, ContactListId, IdGenerator, SuppressionId};
use postit_data::audit::{AuditEvent, AuditEventKind, AuditLog};
use postit_data::contacts::{
    Contact, ContactChanges, ContactFilter, ContactList, ContactListRepo, ContactRepo,
    ContactStatus, ContactUpsert, NewContact, Suppression, SuppressionRepo,
};
use postit_data::email_hash::{email_hash, normalize_email};
use postit_data::limits::UserLimitsRepo;
use postit_data::{Capability, DataError, OwnerScope};
use postit_jobs::JobQueue;
use serde_json::{Map, Value};
use sqlx::{PgConnection, PgPool};

use crate::ContactError;

/// The longest address RFC 5321 allows.
const MAX_ADDRESS_LEN: usize = 254;

/// Parses an email address and returns it normalized (trimmed, ASCII lowercase), or `None` when
/// it is not a plain `local@domain` address.
#[must_use]
pub fn parse_address(raw: &str) -> Option<String> {
    let normalized = normalize_email(raw);
    if normalized.is_empty() || normalized.len() > MAX_ADDRESS_LEN {
        return None;
    }
    normalized.parse::<Address>().ok()?;
    Some(normalized)
}

/// What [`ContactService`] needs.
pub struct ContactsDeps {
    /// The database.
    pub pool: PgPool,
    /// Time source.
    pub clock: Arc<dyn Clock>,
    /// ID source.
    pub ids: Arc<dyn IdGenerator>,
    /// Where the import job is queued.
    pub queue: JobQueue,
    /// The HMAC key behind address hashes.
    pub signing_key: Arc<[u8]>,
    /// Import and export limits.
    pub settings: ContactsSettings,
}

/// A contact to add.
#[derive(Debug, Clone)]
pub struct NewContactInput {
    /// The address; parsed and normalized by the service.
    pub email: String,
    /// Given name.
    pub first_name: Option<String>,
    /// Family name.
    pub last_name: Option<String>,
    /// Custom merge fields, always an object. When the address is already in the book these
    /// merge into the contact's fields: a key given here replaces the old value, other keys
    /// stay.
    pub fields: Option<Map<String, Value>>,
    /// Tags.
    pub tags: Option<Vec<String>>,
    /// Language of the contact.
    pub locale: Option<String>,
    /// Where the contact's consent came from. Required.
    pub consent_source: String,
    /// When consent was given; defaults to now.
    pub consent_at: Option<DateTime<Utc>>,
}

/// A status the owner may set by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettableStatus {
    /// May be emailed.
    Subscribed,
    /// Opted out.
    Unsubscribed,
}

/// Changes to a contact. The address cannot change. A double option sets (`Some(Some)`),
/// clears (`Some(None)`), or keeps (`None`) the value.
#[derive(Debug, Clone, Default)]
pub struct ContactPatch {
    /// Given name.
    pub first_name: Option<Option<String>>,
    /// Family name.
    pub last_name: Option<Option<String>>,
    /// Custom merge fields.
    pub fields: Option<Value>,
    /// Tags.
    pub tags: Option<Vec<String>>,
    /// Language.
    pub locale: Option<Option<String>>,
    /// Where consent came from.
    pub consent_source: Option<String>,
    /// When consent was given.
    pub consent_at: Option<DateTime<Utc>>,
    /// Subscribe or unsubscribe.
    pub status: Option<SettableStatus>,
}

/// Contacts, lists, and the suppression list.
pub struct ContactService {
    pub(crate) pool: PgPool,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) ids: Arc<dyn IdGenerator>,
    pub(crate) queue: JobQueue,
    pub(crate) signing_key: Arc<[u8]>,
    pub(crate) settings: ContactsSettings,
}

pub(crate) fn require(scope: &OwnerScope, capability: Capability) -> Result<(), ContactError> {
    scope
        .require(capability)
        .map_err(|_| ContactError::Forbidden(capability))
}

/// An audit event by `scope`'s actor in its owner's workspace, with `details`. A key ending in
/// `_user_id` is refused by the builder, which answers [`ContactError::Data`].
pub(crate) fn audit_event(
    kind: AuditEventKind,
    scope: &OwnerScope,
    details: impl IntoIterator<Item = (&'static str, Value)>,
) -> Result<AuditEvent, ContactError> {
    let event = AuditEvent::new(kind).actor(scope.actor).owner(scope.owner);
    details
        .into_iter()
        .try_fold(event, |event, (key, value)| event.detail(key, value))
        .map_err(|_| ContactError::Data(DataError::Conflict("audit_detail".to_string())))
}

impl ContactService {
    /// Builds the service.
    #[must_use]
    pub fn new(deps: ContactsDeps) -> Self {
        Self {
            pool: deps.pool,
            clock: deps.clock,
            ids: deps.ids,
            queue: deps.queue,
            signing_key: deps.signing_key,
            settings: deps.settings,
        }
    }

    /// Adds a contact, or updates the one with the same address. An update keeps what the input
    /// leaves out, and custom `fields` merge into the existing ones (a key given replaces the
    /// old value, other keys stay). A new contact is refused when it would pass the owner's
    /// contacts cap (0 means no cap); an address already in the book is never refused by the cap.
    ///
    /// # Errors
    ///
    /// [`ContactError::InvalidEmail`], [`ContactError::ConsentRequired`],
    /// [`ContactError::Limit`], [`ContactError::Forbidden`], or [`ContactError::Data`].
    pub async fn create(
        &self,
        scope: &OwnerScope,
        input: NewContactInput,
    ) -> Result<Contact, ContactError> {
        require(scope, Capability::Edit)?;
        let email = parse_address(&input.email).ok_or(ContactError::InvalidEmail)?;
        let consent_source = input.consent_source.trim();
        if consent_source.is_empty() {
            return Err(ContactError::ConsentRequired);
        }
        let consent_at = input.consent_at.unwrap_or_else(|| self.clock.now());
        let hash = email_hash(&self.signing_key, &email);

        let mut tx = self.pool.begin().await?;
        ContactRepo::lock_owner(&mut tx, scope.owner).await?;
        if !ContactRepo::exists(&mut tx, scope.owner, hash).await? {
            let cap = UserLimitsRepo::get(&mut tx, scope.owner)
                .await?
                .contacts_cap;
            if cap > 0 && ContactRepo::count(&mut tx, scope.owner).await? >= i64::from(cap) {
                return Err(ContactError::Limit);
            }
        }
        let upsert = ContactRepo::upsert(
            &mut tx,
            scope,
            &self.signing_key,
            NewContact {
                id: ContactId::from_uuid(self.ids.generate()),
                email,
                first_name: input.first_name,
                last_name: input.last_name,
                fields: input.fields,
                tags: input.tags,
                locale: input.locale,
                consent_source: Some(consent_source.to_string()),
                consent_at: Some(consent_at),
            },
        )
        .await?;
        tx.commit().await?;
        Ok(match upsert {
            ContactUpsert::Created(contact) | ContactUpsert::Updated(contact) => contact,
        })
    }

    /// Changes a contact. The address stays; `status` may only move between subscribed and
    /// unsubscribed.
    ///
    /// # Errors
    ///
    /// [`ContactError::NotFound`], [`ContactError::ConsentRequired`] for a blank consent
    /// source, [`ContactError::Suppressed`] when subscribing a bounced or complained address,
    /// [`ContactError::Forbidden`], or [`ContactError::Data`].
    pub async fn update(
        &self,
        scope: &OwnerScope,
        id: ContactId,
        patch: ContactPatch,
    ) -> Result<Contact, ContactError> {
        require(scope, Capability::Edit)?;
        let consent_source = match patch.consent_source {
            Some(source) if source.trim().is_empty() => {
                return Err(ContactError::ConsentRequired);
            }
            Some(source) => Some(source.trim().to_string()),
            None => None,
        };
        let changes = ContactChanges {
            first_name: patch.first_name,
            last_name: patch.last_name,
            fields: patch.fields,
            tags: patch.tags,
            locale: patch.locale,
            consent_source,
            consent_at: patch.consent_at,
        };
        let mut tx = self.pool.begin().await?;
        let mut contact = ContactRepo::update(&mut tx, scope, id, changes).await?;
        if let Some(status) = patch.status {
            let status = match status {
                SettableStatus::Subscribed => ContactStatus::Subscribed,
                SettableStatus::Unsubscribed => ContactStatus::Unsubscribed,
            };
            contact = ContactRepo::set_status(&mut tx, scope, id, status).await?;
        }
        tx.commit().await?;
        Ok(contact)
    }

    /// One contact.
    ///
    /// # Errors
    ///
    /// [`ContactError::NotFound`] when the owner has no such contact.
    pub async fn get(&self, scope: &OwnerScope, id: ContactId) -> Result<Contact, ContactError> {
        require(scope, Capability::View)?;
        let mut conn = self.pool.acquire().await?;
        Ok(ContactRepo::get(&mut conn, scope, id).await?)
    }

    /// The owner's contacts, narrowed by `filter`.
    ///
    /// # Errors
    ///
    /// [`ContactError::Data`] on a database failure.
    pub async fn list(
        &self,
        scope: &OwnerScope,
        pagination: Pagination,
        filter: ContactFilter,
    ) -> Result<ResultSet<Contact>, ContactError> {
        require(scope, Capability::View)?;
        let mut conn = self.pool.acquire().await?;
        Ok(ContactRepo::list(&mut conn, scope, pagination, filter).await?)
    }

    /// Deletes a contact. Its suppression, if any, stays.
    ///
    /// # Errors
    ///
    /// [`ContactError::NotFound`] when the owner has no such contact.
    pub async fn delete(&self, scope: &OwnerScope, id: ContactId) -> Result<(), ContactError> {
        require(scope, Capability::Edit)?;
        let mut conn = self.pool.acquire().await?;
        Ok(ContactRepo::delete(&mut conn, scope, id).await?)
    }

    /// Creates an empty list.
    ///
    /// # Errors
    ///
    /// [`ContactError::InvalidName`] for a blank name.
    pub async fn create_list(
        &self,
        scope: &OwnerScope,
        name: &str,
    ) -> Result<ContactList, ContactError> {
        require(scope, Capability::Edit)?;
        let name = name.trim();
        if name.is_empty() {
            return Err(ContactError::InvalidName);
        }
        let mut conn = self.pool.acquire().await?;
        let id = ContactListId::from_uuid(self.ids.generate());
        Ok(ContactListRepo::create(&mut conn, scope, id, name).await?)
    }

    /// Renames a list.
    ///
    /// # Errors
    ///
    /// [`ContactError::InvalidName`] for a blank name, [`ContactError::NotFound`] for an
    /// unknown list.
    pub async fn rename_list(
        &self,
        scope: &OwnerScope,
        id: ContactListId,
        name: &str,
    ) -> Result<ContactList, ContactError> {
        require(scope, Capability::Edit)?;
        let name = name.trim();
        if name.is_empty() {
            return Err(ContactError::InvalidName);
        }
        let mut conn = self.pool.acquire().await?;
        Ok(ContactListRepo::rename(&mut conn, scope, id, name).await?)
    }

    /// Deletes a list and records `contact_list_deleted` with the list ID and member count.
    /// Its contacts stay.
    ///
    /// # Errors
    ///
    /// [`ContactError::NotFound`] for an unknown list.
    pub async fn delete_list(
        &self,
        scope: &OwnerScope,
        id: ContactListId,
    ) -> Result<(), ContactError> {
        require(scope, Capability::Edit)?;
        let mut tx = self.pool.begin().await?;
        let list = ContactListRepo::get(&mut tx, scope, id).await?;
        ContactListRepo::delete(&mut tx, scope, id).await?;
        let event = audit_event(
            AuditEventKind::ContactListDeleted,
            scope,
            [
                ("list_id", Value::from(id.to_string())),
                ("members", Value::from(list.members)),
            ],
        )?;
        self.record(&mut tx, event).await?;
        tx.commit().await?;
        Ok(())
    }

    /// The owner's lists.
    ///
    /// # Errors
    ///
    /// [`ContactError::Data`] on a database failure.
    pub async fn lists(
        &self,
        scope: &OwnerScope,
        pagination: Pagination,
    ) -> Result<ResultSet<ContactList>, ContactError> {
        require(scope, Capability::View)?;
        let mut conn = self.pool.acquire().await?;
        Ok(ContactListRepo::list(&mut conn, scope, pagination).await?)
    }

    /// Adds contacts to a list; those already in it are skipped.
    ///
    /// # Errors
    ///
    /// [`ContactError::NotFound`] when the list or a contact is not the owner's.
    pub async fn add_member(
        &self,
        scope: &OwnerScope,
        list: ContactListId,
        contacts: &[ContactId],
    ) -> Result<(), ContactError> {
        require(scope, Capability::Edit)?;
        let mut conn = self.pool.acquire().await?;
        Ok(ContactListRepo::add_members(&mut conn, scope, list, contacts).await?)
    }

    /// Removes contacts from a list.
    ///
    /// # Errors
    ///
    /// [`ContactError::NotFound`] for an unknown list.
    pub async fn remove_member(
        &self,
        scope: &OwnerScope,
        list: ContactListId,
        contacts: &[ContactId],
    ) -> Result<(), ContactError> {
        require(scope, Capability::Edit)?;
        let mut conn = self.pool.acquire().await?;
        Ok(ContactListRepo::remove_members(&mut conn, scope, list, contacts).await?)
    }

    /// The owner's suppression list.
    ///
    /// # Errors
    ///
    /// [`ContactError::Data`] on a database failure.
    pub async fn suppressions(
        &self,
        scope: &OwnerScope,
        pagination: Pagination,
    ) -> Result<ResultSet<Suppression>, ContactError> {
        require(scope, Capability::View)?;
        let mut conn = self.pool.acquire().await?;
        Ok(SuppressionRepo::list(&mut conn, scope, pagination).await?)
    }

    /// Removes a bounce from the suppression list and records `suppression_removed` with the
    /// entry ID. Owner only.
    ///
    /// # Errors
    ///
    /// [`ContactError::NotRemovable`] unless the entry is a bounce,
    /// [`ContactError::NotFound`], [`ContactError::Forbidden`] for a delegate.
    pub async fn remove_suppression(
        &self,
        scope: &OwnerScope,
        id: SuppressionId,
    ) -> Result<(), ContactError> {
        require(scope, Capability::OwnerOnly)?;
        let mut tx = self.pool.begin().await?;
        SuppressionRepo::remove(&mut tx, scope, id).await?;
        let event = audit_event(
            AuditEventKind::SuppressionRemoved,
            scope,
            [("suppression_id", Value::from(id.to_string()))],
        )?;
        self.record(&mut tx, event).await?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn record(
        &self,
        conn: &mut PgConnection,
        event: AuditEvent,
    ) -> Result<(), ContactError> {
        let id = AuditEventId::from_uuid(self.ids.generate());
        AuditLog::record(conn, id, event).await?;
        Ok(())
    }
}
