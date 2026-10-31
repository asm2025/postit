//! Importing contacts from a CSV file.
//!
//! The upload is read as a stream and mapped row by row into `contact_import_rows`, a chunk at
//! a time, with the rows it can already refuse counted in the import's report. The
//! `contact_import` job then applies the staged rows a chunk per transaction, deleting each
//! chunk as it commits, so a crash resumes at the first row not yet applied and no row is
//! applied twice. Reports and errors carry row numbers and reason codes, never a cell value.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use chrono::{DateTime, NaiveDate, Utc};
use csv_async::{AsyncReader, AsyncReaderBuilder, ErrorKind, StringRecord, Trim};
use postit_core::{ContactId, ContactImportId, ContactListId, UserId};
use postit_data::audit::{AuditEvent, AuditEventKind};
use postit_data::contacts::{
    ContactImport, ContactImportRepo, ContactListRepo, ContactRepo, ContactUpsert, ImportRejection,
    ImportStatus, NewContact, StagedRow,
};
use postit_data::email_hash::email_hash;
use postit_data::limits::UserLimitsRepo;
use postit_data::{Capability, DataError, OwnerScope};
use postit_jobs::JobError;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::PgConnection;
use tokio::io::{AsyncBufReadExt as _, AsyncRead, AsyncReadExt as _, BufReader, ReadBuf};

use crate::jobs::ContactImportJob;
use crate::service::require;
use crate::{ContactError, ContactService, parse_address};

/// How many file rows are staged, and how many staged rows the job applies, per transaction.
const CHUNK: usize = 1000;
/// [`CHUNK`] as the repository takes it.
const CHUNK_ROWS: i64 = 1000;
/// The longest header line read before the delimiter is sniffed.
const HEADER_MAX: u64 = 64 * 1024;

/// Which CSV header holds each contact field. Header names are matched after trimming.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct ColumnMap {
    /// The address column. Required.
    pub email: String,
    /// The column noting where consent came from. Required.
    pub consent_source: String,
    /// When consent was given: RFC 3339, or a `YYYY-MM-DD` date taken as midnight UTC.
    #[serde(default)]
    pub consent_at: Option<String>,
    /// Given name.
    #[serde(default)]
    pub first_name: Option<String>,
    /// Family name.
    #[serde(default)]
    pub last_name: Option<String>,
    /// Tags, separated by `|` in a cell.
    #[serde(default)]
    pub tags: Option<String>,
    /// The contact's language.
    #[serde(default)]
    pub locale: Option<String>,
    /// Custom merge fields: CSV header to field name.
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
}

/// What an import is told before the file is read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct ImportOptions {
    /// The owner attests they have consent to email these addresses. Must be `true`.
    pub attest_consent: bool,
    /// The list the import adds its contacts to.
    #[serde(default)]
    pub list_id: Option<ContactListId>,
    /// Which header holds which field.
    pub columns: ColumnMap,
}

/// One staged row, as the job reads it back.
#[derive(Debug, Serialize, Deserialize)]
struct StagedContact {
    email: String,
    consent_source: String,
    consent_at: Option<DateTime<Utc>>,
    first_name: Option<String>,
    last_name: Option<String>,
    tags: Option<Vec<String>>,
    locale: Option<String>,
    fields: Option<Map<String, Value>>,
}

/// The file positions of the mapped columns.
struct Columns {
    width: usize,
    email: usize,
    consent_source: usize,
    consent_at: Option<usize>,
    first_name: Option<usize>,
    last_name: Option<usize>,
    tags: Option<usize>,
    locale: Option<usize>,
    fields: Vec<(usize, String)>,
}

impl Columns {
    /// Finds every mapped header; one that is missing, or appears twice, makes the file
    /// unusable.
    fn resolve(headers: &StringRecord, map: &ColumnMap) -> Result<Self, ContactError> {
        let mut positions: HashMap<&str, Vec<usize>> = HashMap::new();
        for (index, name) in headers.iter().enumerate() {
            positions.entry(name.trim()).or_default().push(index);
        }
        let find = |name: &str| match positions.get(name.trim()).map(Vec::as_slice) {
            Some([index]) => Ok(*index),
            _ => Err(ContactError::CsvInvalid),
        };
        let find_opt = |name: Option<&String>| name.map(|name| find(name)).transpose();
        Ok(Self {
            width: headers.len(),
            email: find(&map.email)?,
            consent_source: find(&map.consent_source)?,
            consent_at: find_opt(map.consent_at.as_ref())?,
            first_name: find_opt(map.first_name.as_ref())?,
            last_name: find_opt(map.last_name.as_ref())?,
            tags: find_opt(map.tags.as_ref())?,
            locale: find_opt(map.locale.as_ref())?,
            fields: map
                .fields
                .iter()
                .map(|(header, key)| Ok((find(header)?, key.clone())))
                .collect::<Result<_, ContactError>>()?,
        })
    }
}

/// Raised inside the byte stream when the upload passes `contacts.csv_max_bytes`.
#[derive(Debug, thiserror::Error)]
#[error("the file is over the size limit")]
struct TooLarge;

/// Fails the read once more than `left` bytes have come through.
struct Limited<R> {
    inner: R,
    left: u64,
}

impl<R: AsyncRead + Unpin> AsyncRead for Limited<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = &mut *self;
        let before = buf.filled().len();
        match Pin::new(&mut this.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                let read = u64::try_from(buf.filled().len() - before).unwrap_or(u64::MAX);
                if read > this.left {
                    return Poll::Ready(Err(io::Error::other(TooLarge)));
                }
                this.left -= read;
                Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}

fn is_too_large(err: &io::Error) -> bool {
    err.get_ref()
        .is_some_and(<dyn std::error::Error + Send + Sync>::is::<TooLarge>)
}

fn read_error(err: &io::Error) -> ContactError {
    if is_too_large(err) {
        ContactError::CsvTooLarge
    } else {
        ContactError::CsvInvalid
    }
}

fn csv_error(err: &csv_async::Error) -> ContactError {
    match err.kind() {
        ErrorKind::Io(err) => read_error(err),
        _ => ContactError::CsvInvalid,
    }
}

/// `;` when the header line has more semicolons than commas outside quotes, else `,`.
fn sniff_delimiter(header: &[u8]) -> u8 {
    let (mut commas, mut semicolons, mut quoted) = (0_usize, 0_usize, false);
    for byte in header {
        match byte {
            b'"' => quoted = !quoted,
            b',' if !quoted => commas += 1,
            b';' if !quoted => semicolons += 1,
            _ => {}
        }
    }
    if semicolons > commas { b';' } else { b',' }
}

/// Undoes the export's neutralizing: a `'` before `=`, `+`, `-`, `@`, tab, or CR is removed.
fn unescape(cell: &str) -> &str {
    match cell.strip_prefix('\'') {
        Some(rest) if rest.starts_with(['=', '+', '-', '@', '\t', '\r']) => rest,
        _ => cell,
    }
}

/// RFC 3339, or a plain date as midnight UTC.
fn parse_time(text: &str) -> Option<DateTime<Utc>> {
    if let Ok(at) = DateTime::parse_from_rfc3339(text) {
        return Some(at.with_timezone(&Utc));
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|at| at.and_utc())
}

/// Maps one data row to its staged form, or the reason it is rejected. `seen` holds the
/// address hashes of the file's accepted rows.
fn parse_row(
    columns: &Columns,
    record: &StringRecord,
    key: &[u8],
    seen: &mut HashSet<[u8; 32]>,
) -> Result<Value, &'static str> {
    if record.len() != columns.width {
        return Err("row_malformed");
    }
    // A blank cell is `None`, so an existing contact keeps its value.
    let cell = |index: usize| {
        let raw = record.get(index)?.trim();
        (!raw.is_empty()).then(|| unescape(raw).to_string())
    };
    let optional = |index: Option<usize>| index.and_then(cell);

    let email = cell(columns.email)
        .as_deref()
        .and_then(parse_address)
        .ok_or("email_invalid")?;
    let consent_source = cell(columns.consent_source).ok_or("consent_missing")?;
    let consent_at = match optional(columns.consent_at) {
        Some(text) => Some(parse_time(&text).ok_or("consent_at_invalid")?),
        None => None,
    };
    if !seen.insert(email_hash(key, &email)) {
        return Err("duplicate_in_file");
    }
    let tags = optional(columns.tags)
        .map(|cell| {
            cell.split('|')
                .map(str::trim)
                .filter(|tag| !tag.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .filter(|tags| !tags.is_empty());
    let fields: Map<String, Value> = columns
        .fields
        .iter()
        .filter_map(|(index, name)| Some((name.clone(), Value::String(cell(*index)?))))
        .collect();
    let staged = StagedContact {
        email,
        consent_source,
        consent_at,
        first_name: optional(columns.first_name),
        last_name: optional(columns.last_name),
        tags,
        locale: optional(columns.locale),
        fields: (!fields.is_empty()).then_some(fields),
    };
    serde_json::to_value(staged).map_err(|_| "row_malformed")
}

/// What a staging run has read but not yet written.
#[derive(Default)]
struct Pending {
    rows: Vec<StagedRow>,
    rejects: Vec<ImportRejection>,
}

impl Pending {
    fn len(&self) -> usize {
        self.rows.len() + self.rejects.len()
    }

    /// Stages the rows and counts the rejects, inside the caller's transaction.
    async fn write(
        &mut self,
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: ContactImportId,
    ) -> Result<(), ContactError> {
        if !self.rows.is_empty() {
            ContactImportRepo::stage_rows(conn, scope, id, &self.rows).await?;
        }
        if !self.rejects.is_empty() {
            ContactImportRepo::progress(conn, scope.owner, id, 0, &self.rejects).await?;
        }
        self.rows.clear();
        self.rejects.clear();
        Ok(())
    }
}

fn job_error(err: &ContactError) -> JobError {
    let what = match err {
        ContactError::Data(_) => "a database error",
        _ => "an unexpected error",
    };
    JobError::Retry(format!("the contact import stopped on {what}"))
}

impl ContactService {
    /// Reads a CSV upload into a new import and queues the job that applies it. Owner only.
    ///
    /// The attestation and the list are checked before a byte is read, the header next. Rows
    /// the file itself condemns (`email_invalid`, `consent_missing`, `consent_at_invalid`,
    /// `row_malformed`, `duplicate_in_file`) go to the report at once. A file that fails after
    /// the import exists leaves it `failed` with no staged rows and no job.
    ///
    /// # Errors
    ///
    /// [`ContactError::Forbidden`] for a delegate, [`ContactError::AttestationRequired`],
    /// [`ContactError::NotFound`] for a list the owner does not have,
    /// [`ContactError::CsvInvalid`] for a missing or repeated mapped header or bytes that are
    /// not UTF-8, [`ContactError::CsvTooLarge`] past `csv_max_bytes` or `csv_max_rows`.
    pub async fn start_import<R>(
        &self,
        scope: &OwnerScope,
        options: ImportOptions,
        body: R,
    ) -> Result<ContactImport, ContactError>
    where
        R: AsyncRead + Unpin + Send,
    {
        require(scope, Capability::OwnerOnly)?;
        if !options.attest_consent {
            return Err(ContactError::AttestationRequired);
        }
        if let Some(list) = options.list_id {
            let mut conn = self.pool.acquire().await?;
            ContactListRepo::get(&mut conn, scope, list).await?;
        }

        let mut body = BufReader::new(Limited {
            inner: body,
            left: self.settings.csv_max_bytes,
        });
        let mut head = Vec::new();
        (&mut body)
            .take(HEADER_MAX)
            .read_until(b'\n', &mut head)
            .await
            .map_err(|err| read_error(&err))?;
        if head.is_empty()
            || (u64::try_from(head.len()).is_ok_and(|len| len == HEADER_MAX)
                && !head.ends_with(b"\n"))
        {
            return Err(ContactError::CsvInvalid);
        }
        if head.starts_with(b"\xEF\xBB\xBF") {
            head.drain(..3);
        }
        let delimiter = sniff_delimiter(&head);
        let mut reader = AsyncReaderBuilder::new()
            .delimiter(delimiter)
            .flexible(true)
            .trim(Trim::None)
            .create_reader(io::Cursor::new(head).chain(body));
        let headers = reader.headers().await.map_err(|err| csv_error(&err))?;
        let columns = Columns::resolve(headers, &options.columns)?;

        let id = ContactImportId::from_uuid(self.ids.generate());
        let mut conn = self.pool.acquire().await?;
        ContactImportRepo::create(&mut conn, scope, id, 0, options.list_id).await?;
        drop(conn);

        match self.stage(scope, id, &columns, &mut reader).await {
            Ok(()) => {
                let mut conn = self.pool.acquire().await?;
                Ok(ContactImportRepo::get(&mut conn, scope, id).await?)
            }
            Err(err) => {
                if let Ok(mut conn) = self.pool.acquire().await {
                    ContactImportRepo::abandon(&mut conn, scope.owner, id)
                        .await
                        .ok();
                }
                Err(err)
            }
        }
    }

    /// Stages every data row, then sets the total and queues the job in one transaction.
    async fn stage<R>(
        &self,
        scope: &OwnerScope,
        id: ContactImportId,
        columns: &Columns,
        reader: &mut AsyncReader<R>,
    ) -> Result<(), ContactError>
    where
        R: AsyncRead + Unpin + Send,
    {
        let mut seen = HashSet::new();
        let mut pending = Pending::default();
        let mut record = StringRecord::new();
        let mut rows: u32 = 0;
        while reader
            .read_record(&mut record)
            .await
            .map_err(|err| csv_error(&err))?
        {
            rows += 1;
            if rows > self.settings.csv_max_rows {
                return Err(ContactError::CsvTooLarge);
            }
            // Row 1 is the header, so the numbers match a spreadsheet's.
            let row_no = i32::try_from(rows).map_or(i32::MAX, |n| n.saturating_add(1));
            match parse_row(columns, &record, &self.signing_key, &mut seen) {
                Ok(data) => pending.rows.push(StagedRow { row_no, data }),
                Err(code) => pending.rejects.push(ImportRejection { row_no, code }),
            }
            if pending.len() >= CHUNK {
                let mut tx = self.pool.begin().await?;
                pending.write(&mut tx, scope, id).await?;
                tx.commit().await?;
            }
        }

        let mut tx = self.pool.begin().await?;
        pending.write(&mut tx, scope, id).await?;
        let total = i32::try_from(rows).unwrap_or(i32::MAX);
        ContactImportRepo::set_total(&mut tx, scope, id, total).await?;
        let job = ContactImportJob {
            import: id.as_uuid(),
            owner: scope.owner.as_uuid(),
        };
        self.queue.enqueue_in(&mut tx, &job, None).await?;
        tx.commit().await?;
        Ok(())
    }

    /// One import, with its live counts and report. Owner only.
    ///
    /// # Errors
    ///
    /// [`ContactError::NotFound`] when the owner has no such import, and
    /// [`ContactError::Forbidden`] for a delegate.
    pub async fn get_import(
        &self,
        scope: &OwnerScope,
        id: ContactImportId,
    ) -> Result<ContactImport, ContactError> {
        require(scope, Capability::OwnerOnly)?;
        let mut conn = self.pool.acquire().await?;
        Ok(ContactImportRepo::get(&mut conn, scope, id).await?)
    }

    /// The `contact_import` job: applies the staged rows a chunk per transaction, then completes
    /// the import and records `contacts_imported`. A run for an import that is gone or no longer
    /// processing changes nothing.
    ///
    /// # Errors
    ///
    /// [`JobError::Retry`] when a chunk fails; that chunk is rolled back and its rows stay
    /// staged for the retry.
    pub async fn run_import(&self, job: ContactImportJob) -> Result<(), JobError> {
        let scope = OwnerScope::own(UserId::from_uuid(job.owner));
        let id = ContactImportId::from_uuid(job.import);
        match self.get_import(&scope, id).await {
            Ok(import) if import.status == ImportStatus::Processing => {}
            Ok(_) | Err(ContactError::NotFound) => return Ok(()),
            Err(err) => return Err(job_error(&err)),
        }
        while self
            .apply_chunk(&scope, id)
            .await
            .map_err(|err| job_error(&err))?
        {}
        self.finish_import(&scope, id)
            .await
            .map_err(|err| job_error(&err))
    }

    /// Applies one chunk under the owner's contacts lock. `false` when nothing was left.
    async fn apply_chunk(
        &self,
        scope: &OwnerScope,
        id: ContactImportId,
    ) -> Result<bool, ContactError> {
        let owner = scope.owner;
        let mut tx = self.pool.begin().await?;
        ContactRepo::lock_owner(&mut tx, owner).await?;
        let rows = ContactImportRepo::take_chunk(&mut tx, owner, id, CHUNK_ROWS).await?;
        if rows.is_empty() {
            return Ok(false);
        }
        let import = ContactImportRepo::get(&mut tx, scope, id).await?;
        let cap = i64::from(UserLimitsRepo::get(&mut tx, owner).await?.contacts_cap);
        let mut count = ContactRepo::count(&mut tx, owner).await?;
        let mut ok: i32 = 0;
        let mut rejects = Vec::new();
        let mut members: Vec<ContactId> = Vec::new();
        for row in rows {
            let staged: StagedContact = serde_json::from_value(row.data).map_err(|_| {
                ContactError::Data(DataError::Conflict("staged_row_invalid".to_string()))
            })?;
            if cap > 0
                && count >= cap
                && !ContactRepo::exists(
                    &mut tx,
                    owner,
                    email_hash(&self.signing_key, &staged.email),
                )
                .await?
            {
                rejects.push(ImportRejection {
                    row_no: row.row_no,
                    code: "contact_limit_exceeded",
                });
                continue;
            }
            let upsert = ContactRepo::upsert(
                &mut tx,
                scope,
                &self.signing_key,
                NewContact {
                    id: ContactId::from_uuid(self.ids.generate()),
                    email: staged.email,
                    first_name: staged.first_name,
                    last_name: staged.last_name,
                    fields: staged.fields.map(Value::Object),
                    tags: staged.tags,
                    locale: staged.locale,
                    consent_source: Some(staged.consent_source),
                    consent_at: staged.consent_at,
                },
            )
            .await?;
            if matches!(upsert, ContactUpsert::Created(_)) {
                count += 1;
            }
            members.push(upsert.contact().id);
            ok += 1;
        }
        if let Some(list) = import.list_id
            && !members.is_empty()
        {
            // A list deleted since the import started adds no members.
            match ContactListRepo::add_members(&mut tx, scope, list, &members).await {
                Ok(()) | Err(DataError::NotFound) => {}
                Err(err) => return Err(err.into()),
            }
        }
        ContactImportRepo::progress(&mut tx, owner, id, ok, &rejects).await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Completes the import and records `contacts_imported` in one transaction. An import
    /// already closed is left alone.
    async fn finish_import(
        &self,
        scope: &OwnerScope,
        id: ContactImportId,
    ) -> Result<(), ContactError> {
        let mut tx = self.pool.begin().await?;
        ContactRepo::lock_owner(&mut tx, scope.owner).await?;
        let done = match ContactImportRepo::complete(&mut tx, scope.owner, id).await {
            Ok(done) => done,
            Err(DataError::Conflict(_) | DataError::NotFound) => return Ok(()),
            Err(err) => return Err(err.into()),
        };
        let mut event = AuditEvent::new(AuditEventKind::ContactsImported).owner(scope.owner);
        let mut details = vec![
            ("import_id", Value::from(id.to_string())),
            ("rows_total", Value::from(done.rows_total)),
            ("rows_ok", Value::from(done.rows_ok)),
            ("rows_rejected", Value::from(done.rows_rejected)),
            ("attested_at", Value::from(done.attested_at.to_rfc3339())),
        ];
        if let Some(list) = done.list_id {
            details.push(("list_id", Value::from(list.to_string())));
        }
        let audit_error = |_| ContactError::Data(DataError::Conflict("audit_detail".to_string()));
        if let Some(by) = done.attested_by {
            event = event
                .actor(by)
                .detail_user_id("attested_by_user_id", by)
                .map_err(audit_error)?;
        }
        for (key, value) in details {
            event = event.detail(key, value).map_err(audit_error)?;
        }
        self.record(&mut tx, event).await?;
        tx.commit().await?;
        Ok(())
    }
}
