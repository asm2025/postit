//! Exporting contacts to CSV.
//!
//! The owner's rows are counted and the export audited before the first byte, then streamed a
//! page at a time by keyset, holding no connection between pages. A cell that a spreadsheet
//! could run as a formula is neutralized with a leading `'`, which the importer removes, so an
//! export still reads back as it was.

use bytes::Bytes;
use chrono::SecondsFormat;
use csv_async::AsyncWriterBuilder;
use futures::{Stream, stream};
use postit_core::{ContactId, ContactListId};
use postit_data::audit::AuditEventKind;
use postit_data::contacts::{Contact, ContactListRepo, ContactRepo};
use postit_data::{Capability, OwnerScope};
use serde_json::Value;
use sqlx::PgPool;

use crate::service::{audit_event, require};
use crate::{ContactError, ContactService};

/// Contacts read per page.
const PAGE: i64 = 500;

/// The columns every export starts with, in order.
const STANDARD: [&str; 8] = [
    "email",
    "first_name",
    "last_name",
    "tags",
    "locale",
    "consent_source",
    "consent_at",
    "status",
];

/// What starts a cell a spreadsheet may run as a formula.
const FORMULA_STARTS: [char; 6] = ['=', '+', '-', '@', '\t', '\r'];

/// Whether a cell must be neutralized. Leading `'` characters are skipped, so a value that
/// already begins with `'=` is escaped too, and the importer, which skips them the same way,
/// gives it back unchanged.
pub(crate) fn needs_neutralizing(cell: &str) -> bool {
    cell.trim_start_matches('\'').starts_with(FORMULA_STARTS)
}

fn neutralize(cell: &str) -> String {
    if needs_neutralizing(cell) {
        format!("'{cell}")
    } else {
        cell.to_string()
    }
}

/// A custom field's cell: strings as they are, `null` empty, anything else as JSON text.
fn field_cell(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

/// The header of a custom field. A key that would repeat a standard column gets a `field_`
/// prefix, so every header stays unique and the file can be mapped back.
fn field_header(key: &str) -> String {
    if STANDARD.contains(&key) {
        format!("field_{key}")
    } else {
        key.to_string()
    }
}

fn row(contact: &Contact, keys: &[String]) -> Vec<String> {
    let mut cells = vec![
        contact.email.clone(),
        contact.first_name.clone().unwrap_or_default(),
        contact.last_name.clone().unwrap_or_default(),
        contact.tags.join("|"),
        contact.locale.clone().unwrap_or_default(),
        contact.consent_source.clone().unwrap_or_default(),
        contact
            .consent_at
            .map(|at| at.to_rfc3339_opts(SecondsFormat::AutoSi, true))
            .unwrap_or_default(),
        contact.status.as_str().to_string(),
    ];
    cells.extend(keys.iter().map(|key| field_cell(contact.fields.get(key))));
    cells.iter().map(|cell| neutralize(cell)).collect()
}

/// Encodes `records` as CSV.
async fn encode(records: Vec<Vec<String>>) -> Result<Bytes, ContactError> {
    let mut writer = AsyncWriterBuilder::new().create_writer(Vec::new());
    for record in records {
        writer
            .write_record(record)
            .await
            .map_err(|_| ContactError::CsvInvalid)?;
    }
    let buffer = writer
        .into_inner()
        .await
        .map_err(|_| ContactError::CsvInvalid)?;
    Ok(Bytes::from(buffer))
}

/// Where the stream is.
struct Cursor {
    pool: PgPool,
    owner: postit_core::UserId,
    list: Option<ContactListId>,
    keys: Vec<String>,
    header: bool,
    after: Option<ContactId>,
    done: bool,
}

async fn next_chunk(mut cursor: Cursor) -> Result<Option<(Bytes, Cursor)>, ContactError> {
    if !cursor.header {
        cursor.header = true;
        let header = STANDARD
            .iter()
            .map(ToString::to_string)
            .chain(cursor.keys.iter().map(|key| field_header(key)))
            .map(|name| neutralize(&name))
            .collect();
        let bytes = encode(vec![header]).await?;
        return Ok(Some((bytes, cursor)));
    }
    if cursor.done {
        return Ok(None);
    }
    let mut conn = cursor.pool.acquire().await?;
    let page =
        ContactRepo::page_after(&mut conn, cursor.owner, cursor.list, cursor.after, PAGE).await?;
    drop(conn);
    let Some(last) = page.last() else {
        return Ok(None);
    };
    cursor.after = Some(last.id);
    cursor.done = i64::try_from(page.len()).is_ok_and(|len| len < PAGE);
    let bytes = encode(page.iter().map(|c| row(c, &cursor.keys)).collect()).await?;
    Ok(Some((bytes, cursor)))
}

impl ContactService {
    /// Exports the owner's contacts, optionally only the members of `list`, as a CSV stream.
    /// Owner only.
    ///
    /// The rows are counted and `contacts_exported` recorded (with the count and the list ID,
    /// never an address) before the stream is returned, so a download that is dropped is still
    /// audited. The columns are `email`, `first_name`, `last_name`, `tags` (joined with `|`),
    /// `locale`, `consent_source`, `consent_at`, `status`, then one per custom field key in
    /// sorted order. Every cell, header cells included, is formula-neutralized. A custom key
    /// that repeats a standard column is headed `field_<key>`. Custom values that are not
    /// strings come out as text (`5` as `5`, `-1` as `'-1`, `null` as empty) and read back as
    /// strings.
    ///
    /// The stream holds no connection between pages.
    ///
    /// # Errors
    ///
    /// [`ContactError::Forbidden`] for a delegate, [`ContactError::NotFound`] for a list the
    /// owner does not have, [`ContactError::Data`] on a database failure.
    pub async fn export(
        &self,
        scope: &OwnerScope,
        list: Option<ContactListId>,
    ) -> Result<impl Stream<Item = Result<Bytes, ContactError>> + Send + use<>, ContactError> {
        require(scope, Capability::OwnerOnly)?;
        let mut tx = self.pool.begin().await?;
        let count = match list {
            Some(id) => ContactListRepo::get(&mut tx, scope, id).await?.members,
            None => ContactRepo::count(&mut tx, scope.owner).await?,
        };
        let keys = ContactRepo::field_keys(&mut tx, scope.owner, list).await?;
        let mut details = vec![("count", Value::from(count))];
        if let Some(id) = list {
            details.push(("list_id", Value::from(id.to_string())));
        }
        let event = audit_event(AuditEventKind::ContactsExported, scope, details)?;
        self.record(&mut tx, event).await?;
        tx.commit().await?;

        let cursor = Cursor {
            pool: self.pool.clone(),
            owner: scope.owner,
            list,
            keys,
            header: false,
            after: None,
            done: false,
        };
        Ok(stream::try_unfold(cursor, next_chunk))
    }
}

#[cfg(test)]
mod tests {
    use super::{field_header, needs_neutralizing, neutralize};

    #[test]
    fn formula_starts_are_neutralized() {
        for cell in ["=1", "+1", "-1", "@a", "\ta", "\ra", "'=1", "''@x"] {
            assert!(needs_neutralizing(cell), "{cell:?}");
            assert_eq!(neutralize(cell), format!("'{cell}"));
        }
        for cell in ["", "plain", "'plain", "a=b", " =1"] {
            assert_eq!(neutralize(cell), cell);
        }
    }

    #[test]
    fn a_field_named_like_a_column_is_prefixed() {
        assert_eq!(field_header("email"), "field_email");
        assert_eq!(field_header("plan"), "plan");
    }
}
