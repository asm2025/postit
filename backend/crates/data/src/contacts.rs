//! Email contacts, their lists, the suppression list, and import records.
//!
//! Every method takes the connection first and the caller's [`OwnerScope`] second. Reads need
//! [`Capability::View`] and writes need [`Capability::Edit`], except removing a suppression,
//! which needs [`Capability::OwnerOnly`]. None of these rows is bound to an account, so a
//! delegate's account limit does not hide them.
//!
//! Addresses are compared by [`normalize_email`] and matched across tables by [`email_hash`].
//! The suppression list keeps hashes only, never addresses.

use chrono::{DateTime, Utc};
use emixdb::dto::{Pagination, ResultSet};
use postit_core::{ContactId, ContactImportId, ContactListId, SuppressionId, UserId};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use crate::advisory_lock::{CONTACTS_OWNER_LOCK_NAMESPACE, xact_lock_pair};
use crate::constraint::translate;
use crate::email_hash::{email_hash, normalize_email};
use crate::error::DataError;
use crate::paging;
use crate::text::{parse, text_enum};
use crate::{Capability, OwnerScope};

text_enum! {
    /// Whether a contact may be emailed.
    ContactStatus {
        /// May be emailed.
        Subscribed => "subscribed",
        /// Opted out.
        Unsubscribed => "unsubscribed",
        /// The address bounced.
        Bounced => "bounced",
        /// The recipient reported spam.
        Complained => "complained",
    }
}

text_enum! {
    /// Why an address is on the suppression list.
    SuppressionReason {
        /// The recipient opted out.
        Unsubscribed => "unsubscribed",
        /// The address bounced.
        Bounced => "bounced",
        /// The recipient reported spam.
        Complained => "complained",
        /// The owner added it by hand.
        Manual => "manual",
    }
}

text_enum! {
    /// Where a contact import is.
    ImportStatus {
        /// Rows are still being read.
        Processing => "processing",
        /// Finished.
        Completed => "completed",
        /// Stopped without finishing.
        Failed => "failed",
    }
}

/// An email contact.
#[derive(Debug, Clone, PartialEq)]
pub struct Contact {
    /// The contact.
    pub id: ContactId,
    /// The normalized address.
    pub email: String,
    /// Given name.
    pub first_name: Option<String>,
    /// Family name.
    pub last_name: Option<String>,
    /// Free-form fields for mail merge.
    pub fields: Value,
    /// Tags the owner filters by.
    pub tags: Vec<String>,
    /// The contact's language.
    pub locale: Option<String>,
    /// Where the consent came from.
    pub consent_source: Option<String>,
    /// When the contact consented.
    pub consent_at: Option<DateTime<Utc>>,
    /// Whether the contact may be emailed.
    pub status: ContactStatus,
    /// When the status last changed.
    pub status_changed_at: DateTime<Utc>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

struct ContactRow {
    id: Uuid,
    email: String,
    first_name: Option<String>,
    last_name: Option<String>,
    fields: Value,
    tags: Vec<String>,
    locale: Option<String>,
    consent_source: Option<String>,
    consent_at: Option<DateTime<Utc>>,
    status: String,
    status_changed_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    inserted: bool,
}

impl TryFrom<ContactRow> for Contact {
    type Error = DataError;

    fn try_from(row: ContactRow) -> Result<Self, DataError> {
        Ok(Self {
            id: ContactId::from_uuid(row.id),
            email: row.email,
            first_name: row.first_name,
            last_name: row.last_name,
            fields: row.fields,
            tags: row.tags,
            locale: row.locale,
            consent_source: row.consent_source,
            consent_at: row.consent_at,
            status: parse(&row.status)?,
            status_changed_at: row.status_changed_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

/// A contact to create, or the details an existing one takes.
#[derive(Debug, Clone)]
pub struct NewContact {
    /// The ID a new contact gets. An existing contact keeps its own.
    pub id: ContactId,
    /// The address, in any case and with any surrounding whitespace.
    pub email: String,
    /// Given name. `None` keeps what an existing contact has.
    pub first_name: Option<String>,
    /// Family name. `None` keeps what an existing contact has.
    pub last_name: Option<String>,
    /// Free-form fields for mail merge. `None` keeps what an existing contact has; a new
    /// contact gets an empty object.
    pub fields: Option<Value>,
    /// Tags the owner filters by. `None` keeps what an existing contact has; a new contact
    /// gets none.
    pub tags: Option<Vec<String>>,
    /// The contact's language. `None` keeps what an existing contact has.
    pub locale: Option<String>,
    /// Where the consent came from. `None` keeps what the contact already has.
    pub consent_source: Option<String>,
    /// When the contact consented. `None` keeps what the contact already has.
    pub consent_at: Option<DateTime<Utc>>,
}

/// Changes to one contact. The outer `None` leaves a column alone; for the nullable columns
/// `Some(None)` clears it.
#[derive(Debug, Clone, Default)]
pub struct ContactChanges {
    /// Given name.
    pub first_name: Option<Option<String>>,
    /// Family name.
    pub last_name: Option<Option<String>>,
    /// Free-form fields for mail merge.
    pub fields: Option<Value>,
    /// Tags the owner filters by.
    pub tags: Option<Vec<String>>,
    /// The contact's language.
    pub locale: Option<Option<String>>,
    /// Where the consent came from.
    pub consent_source: Option<String>,
    /// When the contact consented.
    pub consent_at: Option<DateTime<Utc>>,
}

/// What an upsert did.
#[derive(Debug, Clone, PartialEq)]
pub enum ContactUpsert {
    /// A new contact.
    Created(Contact),
    /// An existing contact matched by address and updated.
    Updated(Contact),
}

impl ContactUpsert {
    /// The contact either way.
    #[must_use]
    pub fn contact(&self) -> &Contact {
        match self {
            Self::Created(contact) | Self::Updated(contact) => contact,
        }
    }
}

/// What [`ContactRepo::list`] narrows by.
#[derive(Debug, Clone, Default)]
pub struct ContactFilter {
    /// Only members of this list.
    pub list_id: Option<ContactListId>,
    /// Only contacts with this tag.
    pub tag: Option<String>,
    /// Only contacts with this status.
    pub status: Option<ContactStatus>,
}

/// Contacts.
pub struct ContactRepo;

impl ContactRepo {
    /// Adds a contact, or updates the one with the same normalized address. An update keeps the
    /// contact's ID, address, status, and every value given as `None`, the consent record
    /// included.
    ///
    /// A new contact whose address is on the suppression list starts as unsubscribed, bounced,
    /// or complained (a `manual` entry counts as unsubscribed), so a re-import cannot undo an
    /// opt-out.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn upsert(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        key: &[u8],
        new: NewContact,
    ) -> Result<ContactUpsert, DataError> {
        scope.require(Capability::Edit)?;
        let email = normalize_email(&new.email);
        let hash = email_hash(key, &email);
        let row = sqlx::query_as!(
            ContactRow,
            r#"INSERT INTO contacts
                   (id, owner_id, email, email_hash, first_name, last_name, fields, tags, locale,
                    consent_source, consent_at, status, created_by, updated_by)
               VALUES ($1, $2, $3, $4, $5, $6, COALESCE($7, '{}'::jsonb),
                       COALESCE($8::text[], '{}'), $9, $10, $11,
                       COALESCE((SELECT CASE reason WHEN 'manual' THEN 'unsubscribed'
                                                    ELSE reason END
                                 FROM suppressions
                                 WHERE owner_id = $2 AND email_hash = $4),
                                'subscribed'),
                       $12, $12)
               ON CONFLICT (owner_id, email) DO UPDATE
               SET first_name = COALESCE(EXCLUDED.first_name, contacts.first_name),
                   last_name = COALESCE(EXCLUDED.last_name, contacts.last_name),
                   fields = COALESCE($7, contacts.fields),
                   tags = COALESCE($8::text[], contacts.tags),
                   locale = COALESCE(EXCLUDED.locale, contacts.locale),
                   consent_source = COALESCE(EXCLUDED.consent_source, contacts.consent_source),
                   consent_at = COALESCE(EXCLUDED.consent_at, contacts.consent_at),
                   updated_at = now(), updated_by = EXCLUDED.updated_by
               RETURNING id, email::text AS "email!", first_name, last_name, fields, tags, locale,
                         consent_source, consent_at, status, status_changed_at, created_at,
                         updated_at, (xmax = 0) AS "inserted!""#,
            new.id.as_uuid(),
            scope.owner.as_uuid(),
            email,
            &hash[..],
            new.first_name,
            new.last_name,
            new.fields,
            new.tags.as_deref(),
            new.locale,
            new.consent_source,
            new.consent_at,
            scope.actor.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await
        .map_err(translate)?;
        let created = row.inserted;
        let contact = Contact::try_from(row)?;
        Ok(if created {
            ContactUpsert::Created(contact)
        } else {
            ContactUpsert::Updated(contact)
        })
    }

    /// One contact.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such contact.
    pub async fn get(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: ContactId,
    ) -> Result<Contact, DataError> {
        scope.require(Capability::View)?;
        sqlx::query_as!(
            ContactRow,
            r#"SELECT id, email::text AS "email!", first_name, last_name, fields, tags, locale,
                      consent_source, consent_at, status, status_changed_at, created_at,
                      updated_at, FALSE AS "inserted!"
               FROM contacts WHERE owner_id = $1 AND id = $2"#,
            scope.owner.as_uuid(),
            id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(DataError::NotFound)?
        .try_into()
    }

    /// The owner's contacts, oldest first.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn list(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        pagination: Pagination,
        filter: ContactFilter,
    ) -> Result<ResultSet<Contact>, DataError> {
        scope.require(Capability::View)?;
        let (limit, offset) = paging::bounds(pagination);
        let list = filter.list_id.map(|id| id.as_uuid());
        let status = filter.status.map(ContactStatus::as_str);
        let rows = sqlx::query_as!(
            ContactRow,
            r#"SELECT id, email::text AS "email!", first_name, last_name, fields, tags, locale,
                      consent_source, consent_at, status, status_changed_at, created_at,
                      updated_at, FALSE AS "inserted!"
               FROM contacts c
               WHERE owner_id = $1
                 AND ($2::uuid IS NULL OR EXISTS (
                         SELECT 1 FROM contact_list_members m
                         WHERE m.owner_id = c.owner_id AND m.list_id = $2 AND m.contact_id = c.id))
                 AND ($3::text IS NULL OR $3 = ANY(c.tags))
                 AND ($4::text IS NULL OR c.status = $4)
               ORDER BY created_at, id
               LIMIT $5 OFFSET $6"#,
            scope.owner.as_uuid(),
            list,
            filter.tag,
            status,
            limit,
            offset,
        )
        .fetch_all(&mut *conn)
        .await?;
        let total = sqlx::query_scalar!(
            r#"SELECT COUNT(*) FROM contacts c
               WHERE owner_id = $1
                 AND ($2::uuid IS NULL OR EXISTS (
                         SELECT 1 FROM contact_list_members m
                         WHERE m.owner_id = c.owner_id AND m.list_id = $2 AND m.contact_id = c.id))
                 AND ($3::text IS NULL OR $3 = ANY(c.tags))
                 AND ($4::text IS NULL OR c.status = $4)"#,
            scope.owner.as_uuid(),
            list,
            filter.tag,
            status,
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(ResultSet {
            data: rows
                .into_iter()
                .map(Contact::try_from)
                .collect::<Result<_, _>>()?,
            total: paging::total(total),
            pagination: Some(pagination),
        })
    }

    /// Changes the fields of one contact. Only the columns given in `changes` change.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such contact.
    pub async fn update(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: ContactId,
        changes: ContactChanges,
    ) -> Result<Contact, DataError> {
        scope.require(Capability::Edit)?;
        let mut tx = conn.begin().await?;
        let mut row = sqlx::query_as!(
            ContactRow,
            r#"SELECT id, email::text AS "email!", first_name, last_name, fields, tags, locale,
                      consent_source, consent_at, status, status_changed_at, created_at,
                      updated_at, FALSE AS "inserted!"
               FROM contacts WHERE owner_id = $1 AND id = $2 FOR UPDATE"#,
            scope.owner.as_uuid(),
            id.as_uuid(),
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(DataError::NotFound)?;
        if let Some(first_name) = changes.first_name {
            row.first_name = first_name;
        }
        if let Some(last_name) = changes.last_name {
            row.last_name = last_name;
        }
        if let Some(fields) = changes.fields {
            row.fields = fields;
        }
        if let Some(tags) = changes.tags {
            row.tags = tags;
        }
        if let Some(locale) = changes.locale {
            row.locale = locale;
        }
        if let Some(source) = changes.consent_source {
            row.consent_source = Some(source);
        }
        if let Some(at) = changes.consent_at {
            row.consent_at = Some(at);
        }
        sqlx::query!(
            "UPDATE contacts
             SET first_name = $3, last_name = $4, fields = $5, tags = $6, locale = $7,
                 consent_source = $8, consent_at = $9, updated_at = now(), updated_by = $10
             WHERE owner_id = $1 AND id = $2",
            scope.owner.as_uuid(),
            id.as_uuid(),
            row.first_name,
            row.last_name,
            row.fields,
            &row.tags,
            row.locale,
            row.consent_source,
            row.consent_at,
            scope.actor.as_uuid(),
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Self::get(conn, scope, id).await
    }

    /// Sets a contact's status by hand. Only `subscribed` and `unsubscribed` can be set: a
    /// bounce or a complaint is something the recipient's mail system reports.
    ///
    /// Unsubscribing writes a `manual` suppression so the opt-out survives deleting and
    /// re-adding the contact. Subscribing again deletes that entry, which only the owner may
    /// do.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such contact.
    /// [`DataError::Conflict`] (`status_not_settable`) when the target is `bounced` or
    /// `complained`, or the contact is. [`DataError::Conflict`] (`suppressed`) when subscribing
    /// a contact whose address is on the suppression list for a reason other than `manual`.
    /// Remove a bounce entry first. Subscribing past a `manual` entry as a delegate fails with
    /// the owner-only scope error.
    pub async fn set_status(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: ContactId,
        status: ContactStatus,
    ) -> Result<Contact, DataError> {
        scope.require(Capability::Edit)?;
        if matches!(status, ContactStatus::Bounced | ContactStatus::Complained) {
            return Err(DataError::Conflict("status_not_settable".to_string()));
        }
        let mut tx = conn.begin().await?;
        let row = sqlx::query!(
            "SELECT status, email_hash FROM contacts WHERE owner_id = $1 AND id = $2 FOR UPDATE",
            scope.owner.as_uuid(),
            id.as_uuid(),
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(DataError::NotFound)?;
        let current: ContactStatus = parse(&row.status)?;
        let hash: [u8; 32] = row.email_hash.try_into().map_err(|_| {
            DataError::Sql(sqlx::Error::Decode(
                "contact email_hash is not 32 bytes".into(),
            ))
        })?;
        if status == ContactStatus::Unsubscribed {
            match current {
                ContactStatus::Unsubscribed => {
                    drop(tx);
                    return Self::get(conn, scope, id).await;
                }
                ContactStatus::Bounced | ContactStatus::Complained => {
                    return Err(DataError::Conflict("status_not_settable".to_string()));
                }
                ContactStatus::Subscribed => {
                    SuppressionRepo::add_hash(&mut tx, scope, hash, SuppressionReason::Manual)
                        .await?;
                }
            }
        } else {
            let reason = sqlx::query_scalar!(
                "SELECT reason FROM suppressions WHERE owner_id = $1 AND email_hash = $2 FOR UPDATE",
                scope.owner.as_uuid(),
                &hash[..],
            )
            .fetch_optional(&mut *tx)
            .await?;
            match reason.as_deref() {
                None => {}
                Some("manual") => {
                    scope.require(Capability::OwnerOnly)?;
                    sqlx::query!(
                        "DELETE FROM suppressions WHERE owner_id = $1 AND email_hash = $2",
                        scope.owner.as_uuid(),
                        &hash[..],
                    )
                    .execute(&mut *tx)
                    .await?;
                }
                Some(_) => return Err(DataError::Conflict("suppressed".to_string())),
            }
        }
        sqlx::query!(
            "UPDATE contacts
             SET status = $3, status_changed_at = now(), updated_at = now(), updated_by = $4
             WHERE owner_id = $1 AND id = $2",
            scope.owner.as_uuid(),
            id.as_uuid(),
            status.as_str(),
            scope.actor.as_uuid(),
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Self::get(conn, scope, id).await
    }

    /// Deletes a contact. Its suppression, if any, stays.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such contact.
    pub async fn delete(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: ContactId,
    ) -> Result<(), DataError> {
        scope.require(Capability::Edit)?;
        let deleted = sqlx::query!(
            "DELETE FROM contacts WHERE owner_id = $1 AND id = $2",
            scope.owner.as_uuid(),
            id.as_uuid(),
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if deleted == 0 {
            return Err(DataError::NotFound);
        }
        Ok(())
    }

    /// How many contacts `owner` has. This is a system read for the contacts cap, not an access
    /// check.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn count(conn: &mut PgConnection, owner: UserId) -> Result<i64, DataError> {
        Ok(sqlx::query_scalar!(
            r#"SELECT COUNT(*) AS "count!" FROM contacts WHERE owner_id = $1"#,
            owner.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await?)
    }

    /// Whether `owner` has a contact with this address hash. This is a system read for the
    /// contacts cap, not an access check.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn exists(
        conn: &mut PgConnection,
        owner: UserId,
        hash: [u8; 32],
    ) -> Result<bool, DataError> {
        Ok(sqlx::query_scalar!(
            r#"SELECT EXISTS (
                   SELECT 1 FROM contacts WHERE owner_id = $1 AND email_hash = $2
               ) AS "present!""#,
            owner.as_uuid(),
            &hash[..],
        )
        .fetch_one(&mut *conn)
        .await?)
    }

    /// Takes the owner's contacts lock for the rest of the open transaction, so a count check
    /// and the inserts it guards cannot interleave with another writer's. `tx` must be inside
    /// a transaction.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn lock_owner(tx: &mut PgConnection, owner: UserId) -> Result<(), DataError> {
        let digest = Sha256::digest(owner.as_uuid().as_bytes());
        let key = i32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]);
        xact_lock_pair(tx, CONTACTS_OWNER_LOCK_NAMESPACE, key).await
    }
}

/// A contact list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactList {
    /// The list.
    pub id: ContactListId,
    /// Its name.
    pub name: String,
    /// How many contacts it holds.
    pub members: i64,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

struct ListRow {
    id: Uuid,
    name: String,
    members: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<ListRow> for ContactList {
    fn from(row: ListRow) -> Self {
        Self {
            id: ContactListId::from_uuid(row.id),
            name: row.name,
            members: row.members,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// Contact lists.
pub struct ContactListRepo;

impl ContactListRepo {
    /// Creates an empty list.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn create(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: ContactListId,
        name: &str,
    ) -> Result<ContactList, DataError> {
        scope.require(Capability::Edit)?;
        sqlx::query!(
            "INSERT INTO contact_lists (id, owner_id, name, created_by, updated_by)
             VALUES ($1, $2, $3, $4, $4)",
            id.as_uuid(),
            scope.owner.as_uuid(),
            name,
            scope.actor.as_uuid(),
        )
        .execute(&mut *conn)
        .await?;
        Self::get(conn, scope, id).await
    }

    /// One list.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such list.
    pub async fn get(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: ContactListId,
    ) -> Result<ContactList, DataError> {
        scope.require(Capability::View)?;
        sqlx::query_as!(
            ListRow,
            r#"SELECT l.id, l.name, l.created_at, l.updated_at,
                      (SELECT COUNT(*) FROM contact_list_members m
                       WHERE m.owner_id = l.owner_id AND m.list_id = l.id) AS "members!"
               FROM contact_lists l WHERE l.owner_id = $1 AND l.id = $2"#,
            scope.owner.as_uuid(),
            id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .map(ContactList::from)
        .ok_or(DataError::NotFound)
    }

    /// The owner's lists by name.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn list(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        pagination: Pagination,
    ) -> Result<ResultSet<ContactList>, DataError> {
        scope.require(Capability::View)?;
        let (limit, offset) = paging::bounds(pagination);
        let rows = sqlx::query_as!(
            ListRow,
            r#"SELECT l.id, l.name, l.created_at, l.updated_at,
                      (SELECT COUNT(*) FROM contact_list_members m
                       WHERE m.owner_id = l.owner_id AND m.list_id = l.id) AS "members!"
               FROM contact_lists l WHERE l.owner_id = $1
               ORDER BY l.name, l.id
               LIMIT $2 OFFSET $3"#,
            scope.owner.as_uuid(),
            limit,
            offset,
        )
        .fetch_all(&mut *conn)
        .await?;
        let total = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM contact_lists WHERE owner_id = $1",
            scope.owner.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(ResultSet {
            data: rows.into_iter().map(ContactList::from).collect(),
            total: paging::total(total),
            pagination: Some(pagination),
        })
    }

    /// Renames a list.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such list.
    pub async fn rename(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: ContactListId,
        name: &str,
    ) -> Result<ContactList, DataError> {
        scope.require(Capability::Edit)?;
        let renamed = sqlx::query!(
            "UPDATE contact_lists SET name = $3, updated_at = now(), updated_by = $4
             WHERE owner_id = $1 AND id = $2",
            scope.owner.as_uuid(),
            id.as_uuid(),
            name,
            scope.actor.as_uuid(),
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if renamed == 0 {
            return Err(DataError::NotFound);
        }
        Self::get(conn, scope, id).await
    }

    /// Deletes a list. Its contacts stay.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such list.
    pub async fn delete(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: ContactListId,
    ) -> Result<(), DataError> {
        scope.require(Capability::Edit)?;
        let deleted = sqlx::query!(
            "DELETE FROM contact_lists WHERE owner_id = $1 AND id = $2",
            scope.owner.as_uuid(),
            id.as_uuid(),
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if deleted == 0 {
            return Err(DataError::NotFound);
        }
        Ok(())
    }

    /// Adds contacts to a list. Contacts already in it are skipped.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such list, or a contact is not the
    /// owner's. Nothing is added then.
    pub async fn add_members(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        list: ContactListId,
        contacts: &[ContactId],
    ) -> Result<(), DataError> {
        scope.require(Capability::Edit)?;
        let ids = distinct(contacts);
        let mut tx = conn.begin().await?;
        ensure_list(&mut tx, scope, list).await?;
        let owned = sqlx::query_scalar!(
            r#"SELECT COUNT(*) AS "owned!" FROM contacts WHERE owner_id = $1 AND id = ANY($2)"#,
            scope.owner.as_uuid(),
            &ids,
        )
        .fetch_one(&mut *tx)
        .await?;
        if usize::try_from(owned).ok() != Some(ids.len()) {
            return Err(DataError::NotFound);
        }
        sqlx::query!(
            "INSERT INTO contact_list_members (list_id, contact_id, owner_id)
             SELECT $2, id, owner_id FROM contacts WHERE owner_id = $1 AND id = ANY($3)
             ON CONFLICT DO NOTHING",
            scope.owner.as_uuid(),
            list.as_uuid(),
            &ids,
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "UPDATE contact_lists SET updated_at = now(), updated_by = $3
             WHERE owner_id = $1 AND id = $2",
            scope.owner.as_uuid(),
            list.as_uuid(),
            scope.actor.as_uuid(),
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Removes contacts from a list. Contacts that are not in it are ignored.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such list.
    pub async fn remove_members(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        list: ContactListId,
        contacts: &[ContactId],
    ) -> Result<(), DataError> {
        scope.require(Capability::Edit)?;
        let ids = distinct(contacts);
        let mut tx = conn.begin().await?;
        ensure_list(&mut tx, scope, list).await?;
        sqlx::query!(
            "DELETE FROM contact_list_members
             WHERE owner_id = $1 AND list_id = $2 AND contact_id = ANY($3)",
            scope.owner.as_uuid(),
            list.as_uuid(),
            &ids,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

fn distinct(contacts: &[ContactId]) -> Vec<Uuid> {
    let mut ids: Vec<Uuid> = contacts.iter().map(ContactId::as_uuid).collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

async fn ensure_list(
    conn: &mut PgConnection,
    scope: &OwnerScope,
    list: ContactListId,
) -> Result<(), DataError> {
    let present = sqlx::query_scalar!(
        r#"SELECT EXISTS (
               SELECT 1 FROM contact_lists WHERE owner_id = $1 AND id = $2
           ) AS "present!""#,
        scope.owner.as_uuid(),
        list.as_uuid(),
    )
    .fetch_one(&mut *conn)
    .await?;
    if present {
        Ok(())
    } else {
        Err(DataError::NotFound)
    }
}

/// An address on the suppression list. The table holds the hash only; a listing shows the
/// contact's address while the contact still exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suppression {
    /// The entry.
    pub id: SuppressionId,
    /// Why it is suppressed.
    pub reason: SuppressionReason,
    /// When it was suppressed.
    pub at: DateTime<Utc>,
    /// The live contact with this address, if any.
    pub contact_id: Option<ContactId>,
    /// That contact's address, if any.
    pub email: Option<String>,
}

fn reason_rank(reason: SuppressionReason) -> u8 {
    match reason {
        SuppressionReason::Manual => 0,
        SuppressionReason::Bounced => 1,
        SuppressionReason::Unsubscribed | SuppressionReason::Complained => 2,
    }
}

/// The suppression list.
pub struct SuppressionRepo;

impl SuppressionRepo {
    /// Suppresses an address. See [`SuppressionRepo::add_hash`].
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn add(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        key: &[u8],
        email: &str,
        reason: SuppressionReason,
    ) -> Result<(), DataError> {
        Self::add_hash(conn, scope, email_hash(key, email), reason).await
    }

    /// Suppresses a hashed address. An address has one entry. A later reason replaces the
    /// entry's only when it ranks higher: `manual` is below `bounced`, which is below
    /// `unsubscribed` and `complained`, and those two rank equal, so the first of them stays.
    ///
    /// `Unsubscribed` and `Complained` move matching contacts that are `subscribed` or
    /// `bounced`. `Bounced` moves `subscribed` ones only. `Manual` moves none.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn add_hash(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        hash: [u8; 32],
        reason: SuppressionReason,
    ) -> Result<(), DataError> {
        scope.require(Capability::Edit)?;
        let mut tx = conn.begin().await?;
        sqlx::query!(
            "INSERT INTO suppressions (id, owner_id, email_hash, reason, created_by, updated_by)
             VALUES ($1, $2, $3, $4, $5, $5)
             ON CONFLICT (owner_id, email_hash) DO UPDATE
             SET reason = EXCLUDED.reason, at = now(), updated_at = now(),
                 updated_by = EXCLUDED.updated_by
             WHERE (CASE EXCLUDED.reason WHEN 'manual' THEN 0 WHEN 'bounced' THEN 1 ELSE 2 END)
                 > (CASE suppressions.reason WHEN 'manual' THEN 0 WHEN 'bounced' THEN 1 ELSE 2 END)",
            Uuid::now_v7(),
            scope.owner.as_uuid(),
            &hash[..],
            reason.as_str(),
            scope.actor.as_uuid(),
        )
        .execute(&mut *tx)
        .await?;
        if reason_rank(reason) > 0 {
            let movable: Vec<String> = if reason == SuppressionReason::Bounced {
                vec!["subscribed".to_string()]
            } else {
                vec!["subscribed".to_string(), "bounced".to_string()]
            };
            sqlx::query!(
                "UPDATE contacts
                 SET status = $3, status_changed_at = now(), updated_at = now(), updated_by = $4
                 WHERE owner_id = $1 AND email_hash = $2 AND status = ANY($5)",
                scope.owner.as_uuid(),
                &hash[..],
                reason.as_str(),
                scope.actor.as_uuid(),
                &movable,
            )
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Whether the hashed address is suppressed.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn contains(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        hash: [u8; 32],
    ) -> Result<bool, DataError> {
        scope.require(Capability::View)?;
        Ok(sqlx::query_scalar!(
            r#"SELECT EXISTS (
                   SELECT 1 FROM suppressions WHERE owner_id = $1 AND email_hash = $2
               ) AS "present!""#,
            scope.owner.as_uuid(),
            &hash[..],
        )
        .fetch_one(&mut *conn)
        .await?)
    }

    /// The owner's suppressions, newest first, each with the live contact of that address if
    /// there is one. The table itself keeps no address.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn list(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        pagination: Pagination,
    ) -> Result<ResultSet<Suppression>, DataError> {
        scope.require(Capability::View)?;
        let (limit, offset) = paging::bounds(pagination);
        let rows = sqlx::query!(
            r#"SELECT s.id, s.reason, s.at, c.id AS "contact_id?", c.email::text AS "email?"
               FROM suppressions s
               LEFT JOIN contacts c ON c.owner_id = s.owner_id AND c.email_hash = s.email_hash
               WHERE s.owner_id = $1
               ORDER BY s.at DESC, s.id DESC
               LIMIT $2 OFFSET $3"#,
            scope.owner.as_uuid(),
            limit,
            offset,
        )
        .fetch_all(&mut *conn)
        .await?;
        let total = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM suppressions WHERE owner_id = $1",
            scope.owner.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await?;
        let data = rows
            .into_iter()
            .map(|row| {
                Ok(Suppression {
                    id: SuppressionId::from_uuid(row.id),
                    reason: parse(&row.reason)?,
                    at: row.at,
                    contact_id: row.contact_id.map(ContactId::from_uuid),
                    email: row.email,
                })
            })
            .collect::<Result<_, DataError>>()?;
        Ok(ResultSet {
            data,
            total: paging::total(total),
            pagination: Some(pagination),
        })
    }

    /// Takes a bounce entry off the list and moves the contact with that address, if it is
    /// `bounced`, back to `subscribed`. Only the owner can: it is what lets mail reach an
    /// address that bounced. When the contact is `unsubscribed`, the entry becomes `manual`
    /// instead, so the owner's opt-out survives.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such entry, and [`DataError::Conflict`]
    /// (`suppression_not_removable`) unless its reason is `bounced`.
    pub async fn remove(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: SuppressionId,
    ) -> Result<(), DataError> {
        scope.require(Capability::OwnerOnly)?;
        let mut tx = conn.begin().await?;
        let entry = sqlx::query!(
            "SELECT email_hash, reason FROM suppressions
             WHERE owner_id = $1 AND id = $2 FOR UPDATE",
            scope.owner.as_uuid(),
            id.as_uuid(),
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(DataError::NotFound)?;
        if parse::<SuppressionReason>(&entry.reason)? != SuppressionReason::Bounced {
            return Err(DataError::Conflict("suppression_not_removable".to_string()));
        }
        let unsubscribed = sqlx::query_scalar!(
            r#"SELECT EXISTS (
                   SELECT 1 FROM contacts
                   WHERE owner_id = $1 AND email_hash = $2 AND status = 'unsubscribed'
               ) AS "present!""#,
            scope.owner.as_uuid(),
            entry.email_hash,
        )
        .fetch_one(&mut *tx)
        .await?;
        if unsubscribed {
            sqlx::query!(
                "UPDATE suppressions
                 SET reason = 'manual', updated_at = now(), updated_by = $3
                 WHERE owner_id = $1 AND id = $2",
                scope.owner.as_uuid(),
                id.as_uuid(),
                scope.actor.as_uuid(),
            )
            .execute(&mut *tx)
            .await?;
        } else {
            sqlx::query!(
                "DELETE FROM suppressions WHERE owner_id = $1 AND id = $2",
                scope.owner.as_uuid(),
                id.as_uuid(),
            )
            .execute(&mut *tx)
            .await?;
            sqlx::query!(
                "UPDATE contacts
                 SET status = 'subscribed', status_changed_at = now(), updated_at = now(),
                     updated_by = $3
                 WHERE owner_id = $1 AND email_hash = $2 AND status = 'bounced'",
                scope.owner.as_uuid(),
                entry.email_hash,
                scope.actor.as_uuid(),
            )
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

/// A contact import.
#[derive(Debug, Clone, PartialEq)]
pub struct ContactImport {
    /// The import.
    pub id: ContactImportId,
    /// Where it is.
    pub status: ImportStatus,
    /// How many rows the file had.
    pub rows_total: i32,
    /// How many rows became contacts.
    pub rows_ok: i32,
    /// How many rows were rejected.
    pub rows_rejected: i32,
    /// `{"entries": [{"row", "code"}], "totals": {code: count}}`: row numbers and reason codes,
    /// never addresses. `entries` stops at 1,000; `totals` counts every reject.
    pub report: Value,
    /// The list the import adds its contacts to, until that list is deleted.
    pub list_id: Option<ContactListId>,
    /// Who attested the owner has consent for these addresses.
    pub attested_by: Option<UserId>,
    /// When they attested.
    pub attested_at: DateTime<Utc>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

struct ImportRow {
    id: Uuid,
    status: String,
    rows_total: i32,
    rows_ok: i32,
    rows_rejected: i32,
    report: Value,
    list_id: Option<Uuid>,
    attested_by: Option<Uuid>,
    attested_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<ImportRow> for ContactImport {
    type Error = DataError;

    fn try_from(row: ImportRow) -> Result<Self, DataError> {
        Ok(Self {
            id: ContactImportId::from_uuid(row.id),
            status: parse(&row.status)?,
            rows_total: row.rows_total,
            rows_ok: row.rows_ok,
            rows_rejected: row.rows_rejected,
            report: row.report,
            list_id: row.list_id.map(ContactListId::from_uuid),
            attested_by: row.attested_by.map(UserId::from_uuid),
            attested_at: row.attested_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

/// One file row waiting for the import job: its 1-based position in the file and its parsed
/// columns.
#[derive(Debug, Clone, PartialEq)]
pub struct StagedRow {
    /// The row's position in the file.
    pub row_no: i32,
    /// The parsed columns.
    pub data: Value,
}

/// A row an import did not turn into a contact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportRejection {
    /// The row's position in the file.
    pub row_no: i32,
    /// Why, as a stable code. Never an address.
    pub code: &'static str,
}

/// How many rejected rows a report lists. Every reject still counts in `totals`.
const REPORT_ENTRIES_MAX: usize = 1000;

/// Contact imports. Importing contacts is owner-only (plan 01), and so is reading an
/// import's progress and report, so the scoped methods refuse a delegated scope. The methods
/// taking an `owner` are system calls for the import job and the retention sweep.
pub struct ContactImportRepo;

impl ContactImportRepo {
    /// Records the start of an import. The caller is recorded as the one who attested consent.
    /// `list` is the list the import adds its contacts to.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such list.
    pub async fn create(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: ContactImportId,
        rows_total: i32,
        list: Option<ContactListId>,
    ) -> Result<ContactImport, DataError> {
        scope.require(Capability::OwnerOnly)?;
        sqlx::query_as!(
            ImportRow,
            "INSERT INTO contact_imports
                 (id, owner_id, rows_total, list_id, attested_by, created_by, updated_by)
             VALUES ($1, $2, $3, $4, $5, $5, $5)
             RETURNING id, status, rows_total, rows_ok, rows_rejected, report, list_id,
                       attested_by, attested_at, created_at, updated_at",
            id.as_uuid(),
            scope.owner.as_uuid(),
            rows_total,
            list.map(|list| list.as_uuid()),
            scope.actor.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await
        .map_err(|err| {
            if crate::constraint::is_foreign_key_violation(&err) {
                DataError::NotFound
            } else {
                DataError::Sql(err)
            }
        })?
        .try_into()
    }

    /// Stages parsed file rows for the import job.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such import, and [`DataError::Conflict`]
    /// (`import_finished`) when it is no longer processing. Nothing is staged then.
    pub async fn stage_rows(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: ContactImportId,
        rows: &[StagedRow],
    ) -> Result<(), DataError> {
        scope.require(Capability::OwnerOnly)?;
        if rows.is_empty() {
            return Self::get(conn, scope, id).await.map(|_| ());
        }
        let numbers: Vec<i32> = rows.iter().map(|row| row.row_no).collect();
        let data: Vec<Value> = rows.iter().map(|row| row.data.clone()).collect();
        let staged = sqlx::query!(
            "INSERT INTO contact_import_rows (import_id, owner_id, row_no, data)
             SELECT $2, $1, r.row_no, r.data FROM unnest($3::int[], $4::jsonb[]) AS r(row_no, data)
             WHERE EXISTS (SELECT 1 FROM contact_imports
                           WHERE owner_id = $1 AND id = $2 AND status = 'processing')",
            scope.owner.as_uuid(),
            id.as_uuid(),
            &numbers,
            &data,
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if staged == 0 {
            Self::get(conn, scope, id).await?;
            return Err(DataError::Conflict("import_finished".to_string()));
        }
        Ok(())
    }

    /// Sets how many rows the file had, once the whole file is read.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such import.
    pub async fn set_total(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: ContactImportId,
        rows_total: i32,
    ) -> Result<(), DataError> {
        scope.require(Capability::OwnerOnly)?;
        let updated = sqlx::query!(
            "UPDATE contact_imports
             SET rows_total = $3, updated_at = now(), updated_by = $4
             WHERE owner_id = $1 AND id = $2",
            scope.owner.as_uuid(),
            id.as_uuid(),
            rows_total,
            scope.actor.as_uuid(),
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if updated == 0 {
            return Err(DataError::NotFound);
        }
        Ok(())
    }

    /// Takes up to `limit` staged rows, lowest row number first, and deletes them in the same
    /// statement, skipping rows another worker holds. A system call for the import job: `tx`
    /// must be inside the transaction that processes the rows, so a failure puts them back.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn take_chunk(
        tx: &mut PgConnection,
        owner: UserId,
        id: ContactImportId,
        limit: i64,
    ) -> Result<Vec<StagedRow>, DataError> {
        let mut rows: Vec<StagedRow> = sqlx::query!(
            "DELETE FROM contact_import_rows
             WHERE (import_id, row_no) IN (
                 SELECT import_id, row_no FROM contact_import_rows
                 WHERE owner_id = $1 AND import_id = $2
                 ORDER BY row_no LIMIT $3
                 FOR UPDATE SKIP LOCKED)
             RETURNING row_no, data",
            owner.as_uuid(),
            id.as_uuid(),
            limit,
        )
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|row| StagedRow {
            row_no: row.row_no,
            data: row.data,
        })
        .collect();
        rows.sort_by_key(|row| row.row_no);
        Ok(rows)
    }

    /// Adds to an import's counts and report: `ok_delta` to `rows_ok`, the rejects to
    /// `rows_rejected`, every reject to the report's `totals`, and the rejects to its `entries`
    /// until those hold 1,000. It also counts as a sign of life for the stale sweep. A system
    /// call; staging and the job both make it.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such import.
    pub async fn progress(
        conn: &mut PgConnection,
        owner: UserId,
        id: ContactImportId,
        ok_delta: i32,
        rejects: &[ImportRejection],
    ) -> Result<(), DataError> {
        let mut tx = conn.begin().await?;
        let mut report = sqlx::query_scalar!(
            "SELECT report FROM contact_imports WHERE owner_id = $1 AND id = $2 FOR UPDATE",
            owner.as_uuid(),
            id.as_uuid(),
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(DataError::NotFound)?;
        let mut entries = match report.get_mut("entries").map(Value::take) {
            Some(Value::Array(entries)) => entries,
            _ => Vec::new(),
        };
        let mut totals = match report.get_mut("totals").map(Value::take) {
            Some(Value::Object(totals)) => totals,
            _ => serde_json::Map::new(),
        };
        for reject in rejects {
            if entries.len() < REPORT_ENTRIES_MAX {
                entries.push(serde_json::json!({"row": reject.row_no, "code": reject.code}));
            }
            let count = totals.get(reject.code).and_then(Value::as_i64).unwrap_or(0);
            totals.insert(reject.code.to_string(), Value::from(count + 1));
        }
        let report = serde_json::json!({"entries": entries, "totals": totals});
        sqlx::query!(
            "UPDATE contact_imports
             SET rows_ok = rows_ok + $3, rows_rejected = rows_rejected + $4, report = $5,
                 updated_at = now()
             WHERE owner_id = $1 AND id = $2",
            owner.as_uuid(),
            id.as_uuid(),
            ok_delta,
            i32::try_from(rejects.len()).unwrap_or(i32::MAX),
            report,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Closes an import as completed. A system call for the import job.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such import, and [`DataError::Conflict`]
    /// (`import_finished`) when it is already closed.
    pub async fn complete(
        conn: &mut PgConnection,
        owner: UserId,
        id: ContactImportId,
    ) -> Result<ContactImport, DataError> {
        let updated = sqlx::query_as!(
            ImportRow,
            "UPDATE contact_imports
             SET status = 'completed', updated_at = now()
             WHERE owner_id = $1 AND id = $2 AND status = 'processing'
             RETURNING id, status, rows_total, rows_ok, rows_rejected, report, list_id,
                       attested_by, attested_at, created_at, updated_at",
            owner.as_uuid(),
            id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?;
        if let Some(row) = updated {
            return row.try_into();
        }
        Self::fetch(conn, owner, id).await?;
        Err(DataError::Conflict("import_finished".to_string()))
    }

    /// Marks a processing import `failed` and deletes its staged rows, in one statement. An
    /// import that is not processing is left alone. A system call.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn abandon(
        conn: &mut PgConnection,
        owner: UserId,
        id: ContactImportId,
    ) -> Result<(), DataError> {
        sqlx::query_scalar!(
            r#"WITH failed AS (
                   UPDATE contact_imports SET status = 'failed', updated_at = now()
                   WHERE owner_id = $1 AND id = $2 AND status = 'processing'
                   RETURNING id),
               gone AS (
                   DELETE FROM contact_import_rows
                   WHERE owner_id = $1 AND import_id IN (SELECT id FROM failed))
               SELECT COUNT(*) AS "failed!" FROM failed"#,
            owner.as_uuid(),
            id.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(())
    }

    /// One import.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such import.
    pub async fn get(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: ContactImportId,
    ) -> Result<ContactImport, DataError> {
        scope.require(Capability::OwnerOnly)?;
        Self::fetch(conn, scope.owner, id).await
    }

    async fn fetch(
        conn: &mut PgConnection,
        owner: UserId,
        id: ContactImportId,
    ) -> Result<ContactImport, DataError> {
        sqlx::query_as!(
            ImportRow,
            "SELECT id, status, rows_total, rows_ok, rows_rejected, report, list_id,
                    attested_by, attested_at, created_at, updated_at
             FROM contact_imports WHERE owner_id = $1 AND id = $2",
            owner.as_uuid(),
            id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(DataError::NotFound)?
        .try_into()
    }

    /// The owner's imports, newest first.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn list(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        pagination: Pagination,
    ) -> Result<ResultSet<ContactImport>, DataError> {
        scope.require(Capability::OwnerOnly)?;
        let (limit, offset) = paging::bounds(pagination);
        let rows = sqlx::query_as!(
            ImportRow,
            "SELECT id, status, rows_total, rows_ok, rows_rejected, report, list_id,
                    attested_by, attested_at, created_at, updated_at
             FROM contact_imports WHERE owner_id = $1
             ORDER BY created_at DESC, id DESC
             LIMIT $2 OFFSET $3",
            scope.owner.as_uuid(),
            limit,
            offset,
        )
        .fetch_all(&mut *conn)
        .await?;
        let total = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM contact_imports WHERE owner_id = $1",
            scope.owner.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(ResultSet {
            data: rows
                .into_iter()
                .map(ContactImport::try_from)
                .collect::<Result<_, _>>()?,
            total: paging::total(total),
            pagination: Some(pagination),
        })
    }
}
