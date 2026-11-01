#![allow(clippy::unwrap_used, clippy::panic, clippy::too_many_lines)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Arc;

use emixdb::dto::Pagination;
use postit_config::ContactsSettings;
use postit_contacts::{
    ColumnMap, ContactError, ContactImportJob, ContactService, ContactsDeps, ImportOptions,
    NewContactInput,
};
use postit_core::{ContactImportId, SystemClock, SystemIdGenerator, UserId};
use postit_data::contacts::{
    Contact, ContactFilter, ContactImport, ImportStatus, SuppressionReason, SuppressionRepo,
};
use postit_data::testkit::seed_user;
use postit_data::{Capability, OwnerScope};
use postit_jobs::JobQueue;
use serde_json::{Value, json};
use sqlx::PgPool;

const KEY: &[u8] = b"contacts-test-key";

fn settings() -> ContactsSettings {
    ContactsSettings {
        csv_max_bytes: 4 * 1024 * 1024,
        csv_max_rows: 10_000,
    }
}

fn service_with(pool: &PgPool, settings: ContactsSettings) -> ContactService {
    ContactService::new(ContactsDeps {
        pool: pool.clone(),
        clock: Arc::new(SystemClock),
        ids: Arc::new(SystemIdGenerator),
        queue: JobQueue::new(
            pool.clone(),
            Arc::new(SystemIdGenerator),
            Arc::new(SystemClock),
        ),
        signing_key: secrecy::SecretSlice::from(KEY.to_vec()),
        settings,
    })
}

fn service(pool: &PgPool) -> ContactService {
    service_with(pool, settings())
}

async fn set_cap(pool: &PgPool, user: UserId, cap: i32) {
    sqlx::query(
        "INSERT INTO user_limits (user_id, email_daily_cap, contacts_cap, storage_quota_bytes)
         VALUES ($1, 100, $2, 1000000)
         ON CONFLICT (user_id) DO UPDATE SET contacts_cap = EXCLUDED.contacts_cap",
    )
    .bind(user.as_uuid())
    .bind(cap)
    .execute(pool)
    .await
    .unwrap();
}

async fn owner(pool: &PgPool, cap: i32) -> OwnerScope {
    let owner = seed_user(pool, "active").await;
    set_cap(pool, owner, cap).await;
    OwnerScope::own(owner)
}

/// The mapping for a file with `email` and `consent` columns only.
fn columns() -> ColumnMap {
    ColumnMap {
        email: "email".to_string(),
        consent_source: "consent".to_string(),
        consent_at: None,
        first_name: None,
        last_name: None,
        tags: None,
        locale: None,
        fields: BTreeMap::new(),
    }
}

fn options(columns: ColumnMap) -> ImportOptions {
    ImportOptions {
        attest_consent: true,
        list_id: None,
        columns,
    }
}

fn job(scope: &OwnerScope, import: &ContactImport) -> ContactImportJob {
    ContactImportJob {
        import: import.id.as_uuid(),
        owner: scope.owner.as_uuid(),
    }
}

/// Stages `csv` and runs the job to the end.
async fn import(
    svc: &ContactService,
    scope: &OwnerScope,
    opts: ImportOptions,
    csv: &[u8],
) -> ContactImport {
    let started = svc.start_import(scope, opts, csv).await.unwrap();
    svc.run_import(job(scope, &started)).await.unwrap();
    svc.get_import(scope, started.id).await.unwrap()
}

async fn contacts(svc: &ContactService, scope: &OwnerScope) -> Vec<Contact> {
    let mut all = Vec::new();
    let mut page = 1;
    loop {
        let set = svc
            .list(
                scope,
                Pagination {
                    page,
                    page_size: 100,
                },
                ContactFilter::default(),
            )
            .await
            .unwrap();
        let done = set.data.len() < 100;
        all.extend(set.data);
        if done {
            return all;
        }
        page += 1;
    }
}

async fn contact(svc: &ContactService, scope: &OwnerScope, email: &str) -> Contact {
    contacts(svc, scope)
        .await
        .into_iter()
        .find(|c| c.email == email)
        .unwrap_or_else(|| panic!("no contact {email}"))
}

fn codes(import: &ContactImport) -> Vec<(i64, String)> {
    import.report["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["row"].as_i64().unwrap(),
                e["code"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

async fn staged_rows(pool: &PgPool, id: ContactImportId) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM contact_import_rows WHERE import_id = $1")
        .bind(id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn import_rows(pool: &PgPool, owner: UserId) -> Vec<(String, i64)> {
    sqlx::query_as(
        "SELECT i.status, (SELECT COUNT(*) FROM contact_import_rows r WHERE r.import_id = i.id)
         FROM contact_imports i WHERE i.owner_id = $1",
    )
    .bind(owner.as_uuid())
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn queued_jobs(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE job_type = 'contact_import'")
        .fetch_one(pool)
        .await
        .unwrap()
}

/// A file of `n` rows `u{i}@example.com` with consent.
fn many(n: usize) -> String {
    let mut csv = String::from("email,consent\n");
    for i in 1..=n {
        writeln!(csv, "u{i}@example.com,form").unwrap();
    }
    csv
}

#[sqlx::test(migrations = "../data/migrations")]
async fn maps_columns_and_custom_fields(pool: PgPool) {
    let scope = owner(&pool, 0).await;
    let svc = service(&pool);
    let mut map = ColumnMap {
        email: "E-mail".to_string(),
        consent_source: "Source".to_string(),
        consent_at: Some("When".to_string()),
        first_name: Some("First".to_string()),
        last_name: Some("Last".to_string()),
        tags: Some("Tags".to_string()),
        locale: Some("Lang".to_string()),
        fields: BTreeMap::new(),
    };
    map.fields
        .insert("Company".to_string(), "company".to_string());
    let csv = "E-mail,Source,When,First,Last,Tags,Lang,Company,Ignored\n\
               Ann@Example.com,fair,2026-01-02T03:04:05Z,Ann,Lee,vip,ar,Acme,skip me\n";

    let done = import(&svc, &scope, options(map), csv.as_bytes()).await;

    assert_eq!(done.status, ImportStatus::Completed);
    assert_eq!(
        (done.rows_total, done.rows_ok, done.rows_rejected),
        (1, 1, 0)
    );
    let ann = contact(&svc, &scope, "ann@example.com").await;
    assert_eq!(ann.first_name.as_deref(), Some("Ann"));
    assert_eq!(ann.last_name.as_deref(), Some("Lee"));
    assert_eq!(ann.locale.as_deref(), Some("ar"));
    assert_eq!(ann.tags, vec!["vip".to_string()]);
    assert_eq!(ann.consent_source.as_deref(), Some("fair"));
    assert_eq!(
        ann.consent_at.unwrap().to_rfc3339(),
        "2026-01-02T03:04:05+00:00"
    );
    assert_eq!(ann.fields, json!({"company": "Acme"}));
    assert_eq!(ann.status.as_str(), "subscribed");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn missing_consent_rejected(pool: PgPool) {
    let scope = owner(&pool, 0).await;
    let svc = service(&pool);
    let csv = "email,consent\na@example.com,form\nb@example.com,  \n";
    let done = import(&svc, &scope, options(columns()), csv.as_bytes()).await;
    assert_eq!(
        (done.rows_total, done.rows_ok, done.rows_rejected),
        (2, 1, 1)
    );
    assert_eq!(codes(&done), vec![(3, "consent_missing".to_string())]);
    assert_eq!(contacts(&svc, &scope).await.len(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn bad_consent_at_rejected(pool: PgPool) {
    let scope = owner(&pool, 0).await;
    let svc = service(&pool);
    let mut map = columns();
    map.consent_at = Some("at".to_string());
    let csv = "email,consent,at\n\
               a@example.com,form,yesterday\n\
               b@example.com,form,\n\
               c@example.com,form,2026-03-04\n";
    let done = import(&svc, &scope, options(map), csv.as_bytes()).await;
    assert_eq!(codes(&done), vec![(2, "consent_at_invalid".to_string())]);
    assert_eq!(done.rows_ok, 2);
    // An empty `consent_at` stays null; a plain date is midnight UTC.
    assert_eq!(
        contact(&svc, &scope, "b@example.com").await.consent_at,
        None
    );
    assert_eq!(
        contact(&svc, &scope, "c@example.com")
            .await
            .consent_at
            .unwrap()
            .to_rfc3339(),
        "2026-03-04T00:00:00+00:00"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn row_with_wrong_field_count_rejected(pool: PgPool) {
    let scope = owner(&pool, 0).await;
    let svc = service(&pool);
    let csv = "email,consent\na@example.com,form,extra\nb@example.com\nc@example.com,form\n";
    let done = import(&svc, &scope, options(columns()), csv.as_bytes()).await;
    assert_eq!(done.status, ImportStatus::Completed);
    assert_eq!(
        codes(&done),
        vec![
            (2, "row_malformed".to_string()),
            (3, "row_malformed".to_string())
        ]
    );
    assert_eq!(done.rows_ok, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn mapped_header_missing_answers_csv_invalid(pool: PgPool) {
    let scope = owner(&pool, 0).await;
    let svc = service(&pool);
    let mut map = columns();
    map.first_name = Some("First".to_string());
    let csv = "email,consent\na@example.com,form\n";
    assert!(matches!(
        svc.start_import(&scope, options(map), csv.as_bytes()).await,
        Err(ContactError::CsvInvalid)
    ));
    let mut custom = columns();
    custom
        .fields
        .insert("Company".to_string(), "company".to_string());
    assert!(matches!(
        svc.start_import(&scope, options(custom), csv.as_bytes())
            .await,
        Err(ContactError::CsvInvalid)
    ));
    assert!(import_rows(&pool, scope.owner).await.is_empty());
    assert_eq!(queued_jobs(&pool).await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn attestation_required(pool: PgPool) {
    let scope = owner(&pool, 0).await;
    let svc = service(&pool);
    let mut opts = options(columns());
    opts.attest_consent = false;
    assert!(matches!(
        svc.start_import(&scope, opts, &b"email,consent\na@example.com,form\n"[..])
            .await,
        Err(ContactError::AttestationRequired)
    ));
    assert!(import_rows(&pool, scope.owner).await.is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn import_is_owner_only(pool: PgPool) {
    let owner_id = seed_user(&pool, "active").await;
    set_cap(&pool, owner_id, 0).await;
    let who = postit_data::testkit::callers(&pool, owner_id, &[]).await;
    let svc = service(&pool);
    assert!(matches!(
        svc.start_import(
            &who.edit,
            options(columns()),
            &b"email,consent\na@example.com,form\n"[..]
        )
        .await,
        Err(ContactError::Forbidden(Capability::OwnerOnly))
    ));
    assert!(import_rows(&pool, owner_id).await.is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn over_max_rows_refused(pool: PgPool) {
    let scope = owner(&pool, 0).await;
    let svc = service_with(
        &pool,
        ContactsSettings {
            csv_max_rows: 3,
            ..settings()
        },
    );
    assert!(matches!(
        svc.start_import(&scope, options(columns()), many(4).as_bytes())
            .await,
        Err(ContactError::CsvTooLarge)
    ));
    assert_eq!(
        import_rows(&pool, scope.owner).await,
        vec![("failed".to_string(), 0)]
    );
    // Exactly at the limit is fine.
    let done = import(&svc, &scope, options(columns()), many(3).as_bytes()).await;
    assert_eq!(done.rows_ok, 3);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn over_max_bytes_refused(pool: PgPool) {
    let scope = owner(&pool, 0).await;
    // Longer than the reader's buffer, so the limit is hit after the import exists.
    let csv = many(2000);
    let svc = service_with(
        &pool,
        ContactsSettings {
            csv_max_bytes: u64::try_from(csv.len()).unwrap() - 1,
            ..settings()
        },
    );
    assert!(matches!(
        svc.start_import(&scope, options(columns()), csv.as_bytes())
            .await,
        Err(ContactError::CsvTooLarge)
    ));
    assert_eq!(
        import_rows(&pool, scope.owner).await,
        vec![("failed".to_string(), 0)]
    );
    assert_eq!(queued_jobs(&pool).await, 0);
    // A file over the limit within its first read is refused before an import exists.
    let tiny = service_with(
        &pool,
        ContactsSettings {
            csv_max_bytes: 10,
            ..settings()
        },
    );
    assert!(matches!(
        tiny.start_import(&scope, options(columns()), many(1).as_bytes())
            .await,
        Err(ContactError::CsvTooLarge)
    ));
    assert_eq!(import_rows(&pool, scope.owner).await.len(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn cap_rejects_new_and_updates_existing(pool: PgPool) {
    let scope = owner(&pool, 2).await;
    let svc = service(&pool);
    svc.create(
        &scope,
        NewContactInput {
            email: "a@example.com".to_string(),
            first_name: Some("Old".to_string()),
            last_name: None,
            fields: None,
            tags: None,
            locale: None,
            consent_source: "form".to_string(),
            consent_at: None,
        },
    )
    .await
    .unwrap();
    let mut map = columns();
    map.first_name = Some("first".to_string());
    let csv = "email,consent,first\n\
               b@example.com,form,Bea\n\
               c@example.com,form,Cy\n\
               a@example.com,form,Ann\n";

    let done = import(&svc, &scope, options(map), csv.as_bytes()).await;

    assert_eq!(done.status, ImportStatus::Completed);
    assert_eq!((done.rows_ok, done.rows_rejected), (2, 1));
    assert_eq!(
        codes(&done),
        vec![(3, "contact_limit_exceeded".to_string())]
    );
    assert_eq!(done.report["totals"]["contact_limit_exceeded"], 1);
    assert_eq!(
        contact(&svc, &scope, "a@example.com")
            .await
            .first_name
            .as_deref(),
        Some("Ann")
    );
    assert_eq!(contacts(&svc, &scope).await.len(), 2);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn suppressed_address_stays_unsubscribed(pool: PgPool) {
    let scope = owner(&pool, 0).await;
    let svc = service(&pool);
    let mut conn = pool.acquire().await.unwrap();
    SuppressionRepo::add(
        &mut conn,
        &scope,
        KEY,
        "gone@example.com",
        SuppressionReason::Manual,
    )
    .await
    .unwrap();
    SuppressionRepo::add(
        &mut conn,
        &scope,
        KEY,
        "bounce@example.com",
        SuppressionReason::Bounced,
    )
    .await
    .unwrap();
    drop(conn);
    let csv = "email,consent\nGone@example.com,form\nbounce@example.com,form\n";
    import(&svc, &scope, options(columns()), csv.as_bytes()).await;
    assert_eq!(
        contact(&svc, &scope, "gone@example.com")
            .await
            .status
            .as_str(),
        "unsubscribed"
    );
    assert_eq!(
        contact(&svc, &scope, "bounce@example.com")
            .await
            .status
            .as_str(),
        "bounced"
    );
    // Importing again does not resubscribe.
    import(&svc, &scope, options(columns()), csv.as_bytes()).await;
    assert_eq!(
        contact(&svc, &scope, "gone@example.com")
            .await
            .status
            .as_str(),
        "unsubscribed"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn reimport_is_idempotent(pool: PgPool) {
    let scope = owner(&pool, 0).await;
    let svc = service(&pool);
    let mut map = columns();
    map.first_name = Some("first".to_string());
    map.tags = Some("tags".to_string());
    let csv = "email,consent,first,tags\na@example.com,form,Ann,x|y\nb@example.com,form,Bo,\n";
    let first = import(&svc, &scope, options(map.clone()), csv.as_bytes()).await;
    let before = contacts(&svc, &scope).await;
    let second = import(&svc, &scope, options(map), csv.as_bytes()).await;
    let after = contacts(&svc, &scope).await;
    assert_eq!(first.rows_ok, 2);
    assert_eq!(second.rows_ok, 2);
    assert_eq!(before.len(), 2);
    let strip = |list: Vec<Contact>| {
        list.into_iter()
            .map(|c| (c.id, c.email, c.first_name, c.tags, c.fields, c.status))
            .collect::<Vec<_>>()
    };
    assert_eq!(strip(before), strip(after));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn import_without_name_column_keeps_names(pool: PgPool) {
    let scope = owner(&pool, 0).await;
    let svc = service(&pool);
    let mut map = columns();
    map.first_name = Some("first".to_string());
    map.last_name = Some("last".to_string());
    map.locale = Some("lang".to_string());
    map.tags = Some("tags".to_string());
    map.fields.insert("co".to_string(), "company".to_string());
    import(
        &svc,
        &scope,
        options(map),
        b"email,consent,first,last,lang,tags,co\na@example.com,form,Ann,Lee,ar,vip,Acme\n",
    )
    .await;

    import(
        &svc,
        &scope,
        options(columns()),
        b"email,consent\na@example.com,newer form\n",
    )
    .await;

    let ann = contact(&svc, &scope, "a@example.com").await;
    assert_eq!(ann.first_name.as_deref(), Some("Ann"));
    assert_eq!(ann.last_name.as_deref(), Some("Lee"));
    assert_eq!(ann.locale.as_deref(), Some("ar"));
    assert_eq!(ann.tags, vec!["vip".to_string()]);
    assert_eq!(ann.fields, json!({"company": "Acme"}));
    assert_eq!(ann.consent_source.as_deref(), Some("newer form"));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn empty_cell_keeps_existing_value(pool: PgPool) {
    let scope = owner(&pool, 0).await;
    let svc = service(&pool);
    let mut map = columns();
    map.first_name = Some("first".to_string());
    map.tags = Some("tags".to_string());
    map.consent_at = Some("at".to_string());
    map.fields.insert("co".to_string(), "company".to_string());
    map.fields.insert("city".to_string(), "city".to_string());
    import(
        &svc,
        &scope,
        options(map.clone()),
        b"email,consent,first,tags,at,co,city\n\
          a@example.com,form,Ann,vip,2026-01-01T00:00:00Z,Acme,Cairo\n",
    )
    .await;

    import(
        &svc,
        &scope,
        options(map),
        b"email,consent,first,tags,at,co,city\na@example.com,form,,,,,Giza\n",
    )
    .await;

    let ann = contact(&svc, &scope, "a@example.com").await;
    assert_eq!(ann.first_name.as_deref(), Some("Ann"));
    assert_eq!(ann.tags, vec!["vip".to_string()]);
    assert_eq!(
        ann.consent_at.unwrap().to_rfc3339(),
        "2026-01-01T00:00:00+00:00"
    );
    assert_eq!(ann.fields, json!({"company": "Acme", "city": "Giza"}));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn tags_split_on_pipe(pool: PgPool) {
    let scope = owner(&pool, 0).await;
    let svc = service(&pool);
    let mut map = columns();
    map.tags = Some("tags".to_string());
    import(
        &svc,
        &scope,
        options(map),
        b"email,consent,tags\na@example.com,form,\"news| vip ||events, 2026\"\n",
    )
    .await;
    assert_eq!(
        contact(&svc, &scope, "a@example.com").await.tags,
        vec![
            "news".to_string(),
            "vip".to_string(),
            "events, 2026".to_string()
        ]
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn leading_quote_before_formula_character_is_stripped(pool: PgPool) {
    let scope = owner(&pool, 0).await;
    let svc = service(&pool);
    let mut map = columns();
    map.first_name = Some("first".to_string());
    map.last_name = Some("last".to_string());
    map.fields.insert("note".to_string(), "note".to_string());
    let csv = "email,consent,first,last,note\n\
               a@example.com,form,'=SUM(A1),'-5,'@home\n\
               b@example.com,'+form,'plain,\"'\tx\",'\n";
    import(&svc, &scope, options(map), csv.as_bytes()).await;
    let a = contact(&svc, &scope, "a@example.com").await;
    assert_eq!(a.first_name.as_deref(), Some("=SUM(A1)"));
    assert_eq!(a.last_name.as_deref(), Some("-5"));
    assert_eq!(a.fields, json!({"note": "@home"}));
    let b = contact(&svc, &scope, "b@example.com").await;
    assert_eq!(b.consent_source.as_deref(), Some("+form"));
    // A quote before anything else is the contact's own text.
    assert_eq!(b.first_name.as_deref(), Some("'plain"));
    assert_eq!(b.last_name.as_deref(), Some("\tx"));
    assert_eq!(b.fields, json!({"note": "'"}));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn import_adds_to_chosen_list(pool: PgPool) {
    let scope = owner(&pool, 0).await;
    let svc = service(&pool);
    let list = svc.create_list(&scope, "News").await.unwrap();
    let mut opts = options(columns());
    opts.list_id = Some(list.id);
    let done = import(
        &svc,
        &scope,
        opts,
        b"email,consent\na@example.com,form\nb@example.com,form\nbad,form\n",
    )
    .await;
    assert_eq!(done.list_id, Some(list.id));
    let members = svc
        .list(
            &scope,
            Pagination {
                page: 1,
                page_size: 50,
            },
            ContactFilter {
                list_id: Some(list.id),
                ..ContactFilter::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(members.total, 2);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn list_deleted_during_import_still_completes(pool: PgPool) {
    let scope = owner(&pool, 0).await;
    let svc = service(&pool);
    let list = svc.create_list(&scope, "News").await.unwrap();
    let mut opts = options(columns());
    opts.list_id = Some(list.id);
    let started = svc
        .start_import(&scope, opts, many(3).as_bytes())
        .await
        .unwrap();
    svc.delete_list(&scope, list.id).await.unwrap();

    svc.run_import(job(&scope, &started)).await.unwrap();

    let done = svc.get_import(&scope, started.id).await.unwrap();
    assert_eq!(done.status, ImportStatus::Completed);
    assert_eq!(done.rows_ok, 3);
    assert_eq!(done.list_id, None);
    assert_eq!(contacts(&svc, &scope).await.len(), 3);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn foreign_list_refused(pool: PgPool) {
    let scope = owner(&pool, 0).await;
    let other = owner(&pool, 0).await;
    let svc = service(&pool);
    let theirs = svc.create_list(&other, "Theirs").await.unwrap();
    let mut opts = options(columns());
    opts.list_id = Some(theirs.id);
    assert!(matches!(
        svc.start_import(&scope, opts, &b"email,consent\na@example.com,form\n"[..])
            .await,
        Err(ContactError::NotFound)
    ));
    assert!(import_rows(&pool, scope.owner).await.is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn crash_mid_chunk_applies_no_row_twice(pool: PgPool) {
    let scope = owner(&pool, 0).await;
    let svc = service(&pool);
    let started = svc
        .start_import(&scope, options(columns()), many(2500).as_bytes())
        .await
        .unwrap();
    // Corrupt a row of the second chunk, so the job fails after committing the first.
    sqlx::query(
        "UPDATE contact_import_rows SET data = '\"broken\"'::jsonb
         WHERE import_id = $1 AND row_no = 1500",
    )
    .bind(started.id.as_uuid())
    .execute(&pool)
    .await
    .unwrap();

    assert!(svc.run_import(job(&scope, &started)).await.is_err());
    let midway = svc.get_import(&scope, started.id).await.unwrap();
    assert_eq!(midway.status, ImportStatus::Processing);
    assert_eq!(midway.rows_ok, 1000);
    assert_eq!(staged_rows(&pool, started.id).await, 1500);

    sqlx::query(
        "UPDATE contact_import_rows
         SET data = '{\"email\": \"u1499@example.com\", \"consent_source\": \"form\"}'::jsonb
         WHERE import_id = $1 AND row_no = 1500",
    )
    .bind(started.id.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    svc.run_import(job(&scope, &started)).await.unwrap();

    let done = svc.get_import(&scope, started.id).await.unwrap();
    assert_eq!(done.status, ImportStatus::Completed);
    assert_eq!(
        (done.rows_total, done.rows_ok, done.rows_rejected),
        (2500, 2500, 0)
    );
    assert_eq!(contacts(&svc, &scope).await.len(), 2500);
    assert_eq!(staged_rows(&pool, started.id).await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn retry_after_completion_changes_nothing(pool: PgPool) {
    let scope = owner(&pool, 0).await;
    let svc = service(&pool);
    let started = svc
        .start_import(&scope, options(columns()), many(3).as_bytes())
        .await
        .unwrap();
    svc.run_import(job(&scope, &started)).await.unwrap();
    let first = svc.get_import(&scope, started.id).await.unwrap();

    svc.run_import(job(&scope, &started)).await.unwrap();

    let second = svc.get_import(&scope, started.id).await.unwrap();
    assert_eq!(
        (
            first.status,
            first.rows_ok,
            first.rows_rejected,
            first.report
        ),
        (
            second.status,
            second.rows_ok,
            second.rows_rejected,
            second.report
        )
    );
    let audits: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE kind = 'contacts_imported'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(audits, 1);
    // A job for an import that no longer exists is a no-op too.
    svc.run_import(ContactImportJob {
        import: uuid::Uuid::now_v7(),
        owner: scope.owner.as_uuid(),
    })
    .await
    .unwrap();
}

#[sqlx::test(migrations = "../data/migrations")]
async fn staging_rejects_reach_the_report(pool: PgPool) {
    let scope = owner(&pool, 0).await;
    let svc = service(&pool);
    let csv = "email,consent\nnot-an-address,form\na@example.com,form\n";
    let started = svc
        .start_import(&scope, options(columns()), csv.as_bytes())
        .await
        .unwrap();
    assert_eq!(started.status, ImportStatus::Processing);
    assert_eq!(started.rows_total, 2);
    assert_eq!(started.rows_rejected, 1);
    assert_eq!(codes(&started), vec![(2, "email_invalid".to_string())]);
    assert_eq!(started.report["totals"]["email_invalid"], 1);
    assert_eq!(queued_jobs(&pool).await, 1);

    svc.run_import(job(&scope, &started)).await.unwrap();
    let done = svc.get_import(&scope, started.id).await.unwrap();
    assert_eq!((done.rows_ok, done.rows_rejected), (1, 1));
    assert_eq!(codes(&done), vec![(2, "email_invalid".to_string())]);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn report_holds_no_addresses(pool: PgPool) {
    let scope = owner(&pool, 1).await;
    let svc = service(&pool);
    let mut map = columns();
    map.first_name = Some("first".to_string());
    let csv = "email,consent,first\n\
               keep@leak.example,form,Secretname\n\
               keep@leak.example,form,Secretname\n\
               broken@,form,Secretname\n\
               nope@leak.example,,Secretname\n\
               over@leak.example,form,Secretname\n";
    let done = import(&svc, &scope, options(map), csv.as_bytes()).await;
    assert_eq!(
        codes(&done),
        vec![
            (3, "duplicate_in_file".to_string()),
            (4, "email_invalid".to_string()),
            (5, "consent_missing".to_string()),
            (6, "contact_limit_exceeded".to_string()),
        ]
    );
    let report = done.report.to_string();
    let audit: Vec<Value> = sqlx::query_scalar("SELECT details FROM audit_events")
        .fetch_all(&pool)
        .await
        .unwrap();
    let payloads: Vec<Value> = sqlx::query_scalar("SELECT payload FROM jobs")
        .fetch_all(&pool)
        .await
        .unwrap();
    for text in [
        report,
        Value::from(audit).to_string(),
        Value::from(payloads).to_string(),
    ] {
        assert!(!text.contains("leak.example"), "{text}");
        assert!(!text.contains("broken"), "{text}");
        assert!(!text.contains("Secretname"), "{text}");
    }
    let details: Value =
        sqlx::query_scalar("SELECT details FROM audit_events WHERE kind = 'contacts_imported'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(details["import_id"], done.id.to_string());
    assert_eq!(details["rows_total"], 5);
    assert_eq!(details["rows_ok"], 1);
    assert_eq!(details["rows_rejected"], 4);
    assert_eq!(details["attested_by_user_id"], scope.owner.to_string());
    assert!(details["attested_at"].is_string());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn row_with_nul_is_malformed_and_the_rest_apply(pool: PgPool) {
    let scope = owner(&pool, 0).await;
    let svc = service(&pool);
    let mut map = columns();
    map.first_name = Some("first".to_string());
    map.tags = Some("tags".to_string());
    map.fields
        .insert("company".to_string(), "company".to_string());
    // A NUL in a name, a tag, a custom field, the address, and a column no field maps.
    let csv = "email,consent,first,tags,company,note\n\
               a@example.com,form,Ann,vip,Acme,\n\
               nul.name@leak.example,form,Ad\0a,,,\n\
               nul.tag@leak.example,form,,v\0ip,,\n\
               nul.field@leak.example,form,,,Ac\0me,\n\
               nul\0addr@leak.example,form,,,,\n\
               nul.note@leak.example,form,,,,x\0\n\
               b@example.com,form,Bo,,,\n";
    let done = import(&svc, &scope, options(map), csv.as_bytes()).await;

    assert_eq!(done.status, ImportStatus::Completed);
    assert_eq!(
        (done.rows_total, done.rows_ok, done.rows_rejected),
        (7, 2, 5)
    );
    assert_eq!(
        codes(&done),
        (3..=7)
            .map(|row| (row, "row_malformed".to_string()))
            .collect::<Vec<_>>()
    );
    let emails: Vec<String> = contacts(&svc, &scope)
        .await
        .into_iter()
        .map(|c| c.email)
        .collect();
    assert_eq!(emails, vec!["a@example.com", "b@example.com"]);
    // Neither the report, the audit trail, nor the job holds an address from the file.
    let audit: Vec<Value> = sqlx::query_scalar("SELECT details FROM audit_events")
        .fetch_all(&pool)
        .await
        .unwrap();
    let payloads: Vec<Value> = sqlx::query_scalar("SELECT payload FROM jobs")
        .fetch_all(&pool)
        .await
        .unwrap();
    for text in [
        done.report.to_string(),
        Value::from(audit).to_string(),
        Value::from(payloads).to_string(),
    ] {
        assert!(!text.contains("leak.example"), "{text}");
        assert!(!text.contains("example.com"), "{text}");
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn nul_in_mapping_names_is_refused(pool: PgPool) {
    let scope = owner(&pool, 0).await;
    let svc = service(&pool);
    let csv = "email,consent,company\na@example.com,form,Acme\n";
    let mut field_name = columns();
    field_name
        .fields
        .insert("company".to_string(), "comp\0any".to_string());
    let mut header = columns();
    header.first_name = Some("fir\0st".to_string());
    for map in [field_name, header] {
        assert!(matches!(
            svc.start_import(&scope, options(map), csv.as_bytes()).await,
            Err(ContactError::InvalidText { field: "columns" })
        ));
    }
    // Refused before an import exists.
    assert!(import_rows(&pool, scope.owner).await.is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn bom_and_semicolons_import(pool: PgPool) {
    let scope = owner(&pool, 0).await;
    let svc = service(&pool);
    let mut map = columns();
    map.first_name = Some("first".to_string());
    let csv = "\u{feff}email;consent;first\r\n\
               a@example.com;form;\u{0633}\u{0627}\u{0631}\u{0629}\r\n\
               b@example.com;\"form; signed\";Bo\r\n";
    let done = import(&svc, &scope, options(map), csv.as_bytes()).await;
    assert_eq!((done.rows_ok, done.rows_rejected), (2, 0));
    assert_eq!(
        contact(&svc, &scope, "a@example.com")
            .await
            .first_name
            .as_deref(),
        Some("\u{0633}\u{0627}\u{0631}\u{0629}")
    );
    assert_eq!(
        contact(&svc, &scope, "b@example.com")
            .await
            .consent_source
            .as_deref(),
        Some("form; signed")
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn windows_1256_bytes_answer_csv_invalid(pool: PgPool) {
    let scope = owner(&pool, 0).await;
    let svc = service(&pool);
    let mut map = columns();
    map.first_name = Some("first".to_string());
    // "سارة" in Windows-1256.
    let mut csv = b"email,consent,first\na@example.com,form,".to_vec();
    csv.extend_from_slice(&[0xD3, 0xC7, 0xD1, 0xC9]);
    csv.push(b'\n');
    assert!(matches!(
        svc.start_import(&scope, options(map.clone()), &csv[..])
            .await,
        Err(ContactError::CsvInvalid)
    ));
    assert_eq!(
        import_rows(&pool, scope.owner).await,
        vec![("failed".to_string(), 0)]
    );
    // In the header too.
    let mut header = b"email,consent,".to_vec();
    header.extend_from_slice(&[0xC7, 0xE1]);
    header.extend_from_slice(b"\na@example.com,form,x\n");
    assert!(matches!(
        svc.start_import(&scope, options(columns()), &header[..])
            .await,
        Err(ContactError::CsvInvalid)
    ));
    assert!(contacts(&svc, &scope).await.is_empty());
    // The bad header refused the file before any import row was made.
    assert_eq!(
        import_rows(&pool, scope.owner).await,
        vec![("failed".to_string(), 0)]
    );
    assert_eq!(queued_jobs(&pool).await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn same_address_twice_is_duplicate_in_file(pool: PgPool) {
    let scope = owner(&pool, 0).await;
    let svc = service(&pool);
    let mut map = columns();
    map.first_name = Some("first".to_string());
    let csv = "email,consent,first\na@example.com,form,One\n A@EXAMPLE.com ,form,Two\n";
    let done = import(&svc, &scope, options(map), csv.as_bytes()).await;
    assert_eq!(codes(&done), vec![(3, "duplicate_in_file".to_string())]);
    assert_eq!(
        contact(&svc, &scope, "a@example.com")
            .await
            .first_name
            .as_deref(),
        Some("One")
    );
    assert_eq!(contacts(&svc, &scope).await.len(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn two_concurrent_imports_respect_cap(pool: PgPool) {
    let scope = owner(&pool, 1500).await;
    let svc = Arc::new(service(&pool));
    // Both files share u1..u500, then each has 1,000 addresses of its own.
    let one = many(1500);
    let mut two = String::from("email,consent\n");
    for i in 1..=500 {
        writeln!(two, "u{i}@example.com,form").unwrap();
    }
    for i in 1..=1000 {
        writeln!(two, "v{i}@example.com,form").unwrap();
    }
    let first = svc
        .start_import(&scope, options(columns()), one.as_bytes())
        .await
        .unwrap();
    let second = svc
        .start_import(&scope, options(columns()), two.as_bytes())
        .await
        .unwrap();

    let runs: Vec<_> = [first.clone(), second.clone()]
        .into_iter()
        .map(|started| {
            let svc = Arc::clone(&svc);
            let job = job(&scope, &started);
            tokio::spawn(async move { svc.run_import(job).await })
        })
        .collect();
    for run in runs {
        run.await.unwrap().unwrap();
    }

    let a = svc.get_import(&scope, first.id).await.unwrap();
    let b = svc.get_import(&scope, second.id).await.unwrap();
    assert_eq!(a.status, ImportStatus::Completed);
    assert_eq!(b.status, ImportStatus::Completed);
    assert_eq!(contacts(&svc, &scope).await.len(), 1500);
    let limited = a.report["totals"]["contact_limit_exceeded"]
        .as_i64()
        .unwrap_or(0)
        + b.report["totals"]["contact_limit_exceeded"]
            .as_i64()
            .unwrap_or(0);
    // 2,500 distinct addresses against a cap of 1,500.
    assert_eq!(limited, 1000);
    assert_eq!(
        a.rows_ok + b.rows_ok + i32::try_from(limited).unwrap(),
        3000
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn failure_mid_file_leaves_no_staged_rows(pool: PgPool) {
    let scope = owner(&pool, 0).await;
    let svc = service_with(
        &pool,
        ContactsSettings {
            csv_max_rows: 6000,
            ..settings()
        },
    );
    // A bad byte at row 5,000, after four chunks were staged.
    let mut csv = String::from("email,consent\n");
    for i in 1..5000 {
        writeln!(csv, "u{i}@example.com,form").unwrap();
    }
    let mut bytes = csv.into_bytes();
    bytes.extend_from_slice(b"u5000@example.com,form\xC7\n");
    bytes.extend_from_slice(b"u5001@example.com,form\n");
    assert!(matches!(
        svc.start_import(&scope, options(columns()), &bytes[..])
            .await,
        Err(ContactError::CsvInvalid)
    ));

    // A file past `csv_max_rows`.
    assert!(matches!(
        svc.start_import(&scope, options(columns()), many(6001).as_bytes())
            .await,
        Err(ContactError::CsvTooLarge)
    ));

    assert_eq!(
        import_rows(&pool, scope.owner).await,
        vec![("failed".to_string(), 0), ("failed".to_string(), 0)]
    );
    assert_eq!(queued_jobs(&pool).await, 0);
    assert!(contacts(&svc, &scope).await.is_empty());
}
