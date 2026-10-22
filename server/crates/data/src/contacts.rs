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
use postit_core::{ContactId, ContactImportId, ContactListId, UserId};
use serde_json::Value;
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

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
    /// Where the consent came from. `None` keeps what the contact already has.
    pub consent_source: Option<String>,
    /// When the contact consented. `None` keeps what the contact already has.
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
    /// contact's ID, address, status, and earlier consent record when none is given.
    ///
    /// A new contact whose address is on the suppression list starts as unsubscribed, bounced,
    /// or complained, so a re-import cannot undo an opt-out.
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
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11,
                       COALESCE((SELECT reason FROM suppressions
                                 WHERE owner_id = $2 AND email_hash = $4 AND reason <> 'manual'),
                                'subscribed'),
                       $12, $12)
               ON CONFLICT (owner_id, email) DO UPDATE
               SET first_name = EXCLUDED.first_name, last_name = EXCLUDED.last_name,
                   fields = EXCLUDED.fields, tags = EXCLUDED.tags, locale = EXCLUDED.locale,
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
            &new.tags,
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

    /// Sets a contact's status by hand.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such contact, and
    /// [`DataError::Conflict`] (`suppressed`) when subscribing a contact whose address is on
    /// the suppression list for a reason other than `manual`. Remove the suppression first.
    pub async fn set_status(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: ContactId,
        status: ContactStatus,
    ) -> Result<Contact, DataError> {
        scope.require(Capability::Edit)?;
        let mut tx = conn.begin().await?;
        let blocked = status == ContactStatus::Subscribed
            && sqlx::query_scalar!(
                r#"SELECT EXISTS (
                       SELECT 1 FROM contacts c
                       JOIN suppressions s
                         ON s.owner_id = c.owner_id AND s.email_hash = c.email_hash
                       WHERE c.owner_id = $1 AND c.id = $2 AND s.reason <> 'manual'
                   ) AS "blocked!""#,
                scope.owner.as_uuid(),
                id.as_uuid(),
            )
            .fetch_one(&mut *tx)
            .await?;
        if blocked {
            return Err(DataError::Conflict("suppressed".to_string()));
        }
        let updated = sqlx::query!(
            "UPDATE contacts
             SET status = $3, status_changed_at = now(), updated_at = now(), updated_by = $4
             WHERE owner_id = $1 AND id = $2",
            scope.owner.as_uuid(),
            id.as_uuid(),
            status.as_str(),
            scope.actor.as_uuid(),
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if updated == 0 {
            return Err(DataError::NotFound);
        }
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

/// An address on the suppression list. It holds the hash only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suppression {
    /// The address's [`email_hash`].
    pub email_hash: Vec<u8>,
    /// Why it is suppressed.
    pub reason: SuppressionReason,
    /// When it was suppressed.
    pub at: DateTime<Utc>,
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

    /// Suppresses a hashed address and moves any matching contact still subscribed to
    /// `reason`. Adding the same hash again changes nothing, except that a `manual` entry
    /// takes the reason of a later, more specific one. A `manual` reason leaves contacts alone.
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
             WHERE suppressions.reason = 'manual' AND EXCLUDED.reason <> 'manual'",
            Uuid::now_v7(),
            scope.owner.as_uuid(),
            &hash[..],
            reason.as_str(),
            scope.actor.as_uuid(),
        )
        .execute(&mut *tx)
        .await?;
        if reason != SuppressionReason::Manual {
            sqlx::query!(
                "UPDATE contacts
                 SET status = $3, status_changed_at = now(), updated_at = now(), updated_by = $4
                 WHERE owner_id = $1 AND email_hash = $2 AND status = 'subscribed'",
                scope.owner.as_uuid(),
                &hash[..],
                reason.as_str(),
                scope.actor.as_uuid(),
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

    /// The owner's suppressions, newest first. Rows carry hashes and reasons, never addresses.
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
            "SELECT email_hash, reason, at FROM suppressions
             WHERE owner_id = $1
             ORDER BY at DESC, id DESC
             LIMIT $2 OFFSET $3",
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
                    email_hash: row.email_hash,
                    reason: parse(&row.reason)?,
                    at: row.at,
                })
            })
            .collect::<Result<_, DataError>>()?;
        Ok(ResultSet {
            data,
            total: paging::total(total),
            pagination: Some(pagination),
        })
    }

    /// Takes an address off the list. Only the owner can: it is what lets mail reach someone
    /// who opted out.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the hash is not suppressed.
    pub async fn remove(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        hash: [u8; 32],
    ) -> Result<(), DataError> {
        scope.require(Capability::OwnerOnly)?;
        let removed = sqlx::query!(
            "DELETE FROM suppressions WHERE owner_id = $1 AND email_hash = $2",
            scope.owner.as_uuid(),
            &hash[..],
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if removed == 0 {
            return Err(DataError::NotFound);
        }
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
    /// Row numbers and reason codes, never addresses.
    pub report: Value,
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
            attested_by: row.attested_by.map(UserId::from_uuid),
            attested_at: row.attested_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

/// Contact imports.
pub struct ContactImportRepo;

impl ContactImportRepo {
    /// Records the start of an import. The caller is recorded as the one who attested consent.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn create(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: ContactImportId,
        rows_total: i32,
    ) -> Result<ContactImport, DataError> {
        scope.require(Capability::Edit)?;
        sqlx::query_as!(
            ImportRow,
            "INSERT INTO contact_imports (id, owner_id, rows_total, attested_by, created_by, updated_by)
             VALUES ($1, $2, $3, $4, $4, $4)
             RETURNING id, status, rows_total, rows_ok, rows_rejected, report, attested_by,
                       attested_at, created_at, updated_at",
            id.as_uuid(),
            scope.owner.as_uuid(),
            rows_total,
            scope.actor.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await?
        .try_into()
    }

    /// Closes an import with its counts and report. `report` holds row numbers and reason
    /// codes only.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such import, and [`DataError::Conflict`]
    /// (`import_finished`) when it is already closed.
    pub async fn finish(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: ContactImportId,
        ok: i32,
        rejected: i32,
        report: Value,
    ) -> Result<ContactImport, DataError> {
        scope.require(Capability::Edit)?;
        let updated = sqlx::query_as!(
            ImportRow,
            "UPDATE contact_imports
             SET status = 'completed', rows_ok = $3, rows_rejected = $4, report = $5,
                 updated_at = now(), updated_by = $6
             WHERE owner_id = $1 AND id = $2 AND status = 'processing'
             RETURNING id, status, rows_total, rows_ok, rows_rejected, report, attested_by,
                       attested_at, created_at, updated_at",
            scope.owner.as_uuid(),
            id.as_uuid(),
            ok,
            rejected,
            report,
            scope.actor.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?;
        if let Some(row) = updated {
            return row.try_into();
        }
        Self::get(conn, scope, id).await?;
        Err(DataError::Conflict("import_finished".to_string()))
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
        scope.require(Capability::View)?;
        sqlx::query_as!(
            ImportRow,
            "SELECT id, status, rows_total, rows_ok, rows_rejected, report, attested_by,
                    attested_at, created_at, updated_at
             FROM contact_imports WHERE owner_id = $1 AND id = $2",
            scope.owner.as_uuid(),
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
        scope.require(Capability::View)?;
        let (limit, offset) = paging::bounds(pagination);
        let rows = sqlx::query_as!(
            ImportRow,
            "SELECT id, status, rows_total, rows_ok, rows_rejected, report, attested_by,
                    attested_at, created_at, updated_at
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
